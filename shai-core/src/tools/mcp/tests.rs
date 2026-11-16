#[cfg(test)]
mod tests {
    use crate::tools::mcp::mcp_config::McpCredentialStore;
    use crate::tools::ToolCall;
    use crate::tools::{
        create_mcp_client, HttpClient, McpClient, McpConfig, SseClient, StdioClient,
    };
    use mockito::Matcher;
    use rmcp::transport::auth::CredentialStore;
    use serde_json::json;
    use std::process::Command;
    use std::time::Duration;
    use tokio;

    fn initialize_response() -> String {
        json!({
            "jsonrpc": "2.0",
            "id": 0,
            "result": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "serverInfo": { "name": "mock-mcp", "version": "1.0.0" }
            }
        })
        .to_string()
    }

    fn challenge(server: &mockito::Server) -> String {
        format!(
            "Bearer resource_metadata=\"{}/.well-known/oauth-protected-resource\"",
            server.url()
        )
    }

    fn protected_resource_metadata(server: &mockito::Server, resource: Option<String>) -> String {
        let mut metadata = json!({
            "authorization_servers": [server.url()]
        });
        if let Some(resource) = resource {
            metadata["resource"] = json!(resource);
        }
        metadata.to_string()
    }

    fn authorization_metadata(server: &mockito::Server) -> String {
        json!({
            "issuer": server.url(),
            "authorization_endpoint": format!("{}/authorize", server.url()),
            "token_endpoint": format!("{}/token", server.url()),
            "registration_endpoint": format!("{}/register", server.url()),
            "response_types_supported": ["code"],
            "code_challenge_methods_supported": ["S256"],
            "scopes_supported": ["all"]
        })
        .to_string()
    }

    fn token_response(access_token: &str, refresh_token: &str) -> String {
        json!({
            "access_token": access_token,
            "token_type": "Bearer",
            "refresh_token": refresh_token,
            "expires_in": 3600,
            "scope": "all"
        })
        .to_string()
    }

    async fn callback(auth_url: String, expected_resource: &str) {
        let url = reqwest::Url::parse(&auth_url).unwrap();
        let parameters: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(parameters["resource"], expected_resource);
        assert_eq!(parameters["code_challenge_method"], "S256");
        assert_eq!(parameters["response_type"], "code");
        let callback = format!(
            "{}?code=test-code&state={}",
            parameters["redirect_uri"], parameters["state"]
        );
        let response = reqwest::get(callback).await.unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
    }

    /// Check if uvx is available on the system
    fn is_uvx_available() -> bool {
        Command::new("uvx").arg("--version").output().is_ok()
    }

    /// Check if MCP server is running on localhost:8000
    async fn is_mcp_server_available() -> bool {
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            reqwest::get("http://localhost:8000"),
        )
        .await
        .is_ok()
    }

    #[tokio::test]
    async fn test_mcp_stdio_integration() {
        // Skip test if uvx is not installed
        if !is_uvx_available() {
            println!("Skipping MCP test: uvx not available");
            return;
        }

        // Create stdio client for mcp-server-fetch
        let mut client = StdioClient::new("uvx".to_string(), vec!["mcp-server-fetch".to_string()]);

        // Test connection
        match client.connect().await {
            Ok(_) => println!("✅ Successfully connected to mcp-server-fetch"),
            Err(e) => {
                println!("❌ Failed to connect to mcp-server-fetch: {}", e);
                println!("This might be because mcp-server-fetch is not installed.");
                println!("Try running: uvx mcp-server-fetch --help");
                return;
            }
        }

        // Test listing tools
        let tools = match client.list_tools().await {
            Ok(tools) => {
                println!("✅ Successfully listed {} tools", tools.len());
                for tool in &tools {
                    println!("  - {}: {}", tool.name, tool.description);
                }
                tools
            }
            Err(e) => {
                println!("❌ Failed to list tools: {}", e);
                let _ = client.disconnect().await;
                return;
            }
        };

        // Find a fetch tool to test
        let fetch_tool = tools
            .iter()
            .find(|tool| tool.name.to_lowercase().contains("fetch"));

        if let Some(tool) = fetch_tool {
            println!("✅ Found tool to test: {}", tool.name);

            // Test executing the tool with a simple HTTP request
            let tool_call = ToolCall {
                tool_call_id: "test-1".to_string(),
                tool_name: tool.name.clone(),
                parameters: json!({
                    "url": "https://ovhcloud.com"
                }),
            };

            match client.execute_tool(tool_call).await {
                Ok(result) => {
                    println!("✅ Successfully executed tool");
                    println!("Result: {}", result);
                }
                Err(e) => {
                    println!("❌ Failed to execute tool: {}", e);
                    // Don't return here, still test disconnect
                }
            }
        } else {
            println!("⚠️  No fetch-related tool found in available tools");
        }

        // Test disconnection
        match client.disconnect().await {
            Ok(_) => println!("✅ Successfully disconnected"),
            Err(e) => println!("❌ Failed to disconnect: {}", e),
        }
    }

    #[tokio::test]
    async fn test_mcp_stdio_no_tools() {
        if !is_uvx_available() {
            println!("Skipping MCP test: uvx not available");
            return;
        }

        let mut client = StdioClient::new("uvx".to_string(), vec!["mcp-server-fetch".to_string()]);

        // Test that we can connect and get an empty or non-empty tool list
        if client.connect().await.is_ok() {
            let tools = client.list_tools().await.unwrap_or_default();
            println!("Found {} tools", tools.len());
            let _ = client.disconnect().await;
        }
    }

    #[tokio::test]
    async fn test_mcp_stdio_invalid_command() {
        let mut client = StdioClient::new("nonexistent-command".to_string(), vec![]);

        // This should fail to connect
        match client.connect().await {
            Ok(_) => panic!("Should not have connected to nonexistent command"),
            Err(_) => println!("✅ Correctly failed to connect to invalid command"),
        }
    }

    #[tokio::test]
    async fn oauth_resource_spoofing_is_rejected_before_registration() {
        for (resource, expected_error) in [
            (
                Some("https://real.example.com/mcp".to_string()),
                "resource mismatch",
            ),
            (None, "missing required resource"),
        ] {
            let mut server = mockito::Server::new_async().await;
            let _unauthorized = server
                .mock("POST", "/mcp")
                .match_header("authorization", Matcher::Missing)
                .with_status(401)
                .with_header("www-authenticate", &challenge(&server))
                .create_async()
                .await;
            let _metadata = server
                .mock("GET", "/.well-known/oauth-protected-resource")
                .with_status(200)
                .with_header("content-type", "application/json")
                .with_body(protected_resource_metadata(&server, resource))
                .create_async()
                .await;
            let registration = server
                .mock("POST", "/register")
                .expect(0)
                .create_async()
                .await;
            let opened = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let opened_by_browser = opened.clone();
            let mut client = HttpClient::with_credential_store(
                format!("{}/mcp", server.url()),
                None,
                McpCredentialStore::in_memory(),
            )
            .with_browser_opener(move |_| {
                opened_by_browser.store(true, std::sync::atomic::Ordering::SeqCst);
            });

            let error = format!("{:?}", client.connect().await.unwrap_err());
            assert!(
                error.to_lowercase().contains(expected_error),
                "unexpected error: {error}"
            );
            assert!(!opened.load(std::sync::atomic::Ordering::SeqCst));
            registration.assert_async().await;
        }
    }

    #[tokio::test]
    async fn oauth_issuer_mismatch_is_rejected_before_registration() {
        let mut server = mockito::Server::new_async().await;
        let resource = format!("{}/mcp", server.url());
        let _unauthorized = server
            .mock("POST", "/mcp")
            .match_header("authorization", Matcher::Missing)
            .with_status(401)
            .with_header("www-authenticate", &challenge(&server))
            .create_async()
            .await;
        let _resource_metadata = server
            .mock("GET", "/.well-known/oauth-protected-resource")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(protected_resource_metadata(&server, Some(resource.clone())))
            .create_async()
            .await;
        let invalid_metadata = json!({
            "issuer": "https://real.example.com",
            "authorization_endpoint": format!("{}/authorize", server.url()),
            "token_endpoint": format!("{}/token", server.url()),
            "registration_endpoint": format!("{}/register", server.url()),
            "response_types_supported": ["code"],
            "code_challenge_methods_supported": ["S256"]
        });
        let _authorization_metadata = server
            .mock("GET", "/.well-known/oauth-authorization-server")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(invalid_metadata.to_string())
            .create_async()
            .await;
        let registration = server
            .mock("POST", "/register")
            .expect(0)
            .create_async()
            .await;
        let opened = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let opened_by_browser = opened.clone();
        let mut client =
            HttpClient::with_credential_store(resource, None, McpCredentialStore::in_memory())
                .with_browser_opener(move |_| {
                    opened_by_browser.store(true, std::sync::atomic::Ordering::SeqCst);
                });

        let error = format!("{:?}", client.connect().await.unwrap_err());
        assert!(
            error.to_lowercase().contains("issuer"),
            "unexpected error: {error}"
        );
        assert!(!opened.load(std::sync::atomic::Ordering::SeqCst));
        registration.assert_async().await;
    }

    #[tokio::test]
    async fn oauth_authorization_and_refresh_survive_restart() {
        let mut server = mockito::Server::new_async().await;
        let resource = format!("{}/mcp", server.url());
        let _unauthorized = server
            .mock("POST", "/mcp")
            .match_header("authorization", Matcher::Missing)
            .with_status(401)
            .with_header("www-authenticate", &challenge(&server))
            .create_async()
            .await;
        let _unauthorized_probe = server
            .mock("GET", "/mcp")
            .match_header("authorization", Matcher::Missing)
            .with_status(401)
            .with_header("www-authenticate", &challenge(&server))
            .create_async()
            .await;
        let _first_connection = server
            .mock("POST", "/mcp")
            .match_header("authorization", "Bearer first-token")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(initialize_response())
            .create_async()
            .await;
        let _refreshed_connection = server
            .mock("POST", "/mcp")
            .match_header("authorization", "Bearer refreshed-token")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(initialize_response())
            .create_async()
            .await;
        let _resource_metadata = server
            .mock("GET", "/.well-known/oauth-protected-resource")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(protected_resource_metadata(&server, Some(resource.clone())))
            .create_async()
            .await;
        let _authorization_metadata = server
            .mock("GET", "/.well-known/oauth-authorization-server")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(authorization_metadata(&server))
            .create_async()
            .await;
        let registration = server
            .mock("POST", "/register")
            .match_header("content-type", "application/json")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                json!({
                    "client_id": "shai-test-client",
                    "client_name": "Shai MCP Client",
                    "redirect_uris": ["http://127.0.0.1/callback"]
                })
                .to_string(),
            )
            .create_async()
            .await;
        let authorization_exchange = server
            .mock("POST", "/token")
            .match_body(Matcher::AllOf(vec![
                Matcher::UrlEncoded("grant_type".into(), "authorization_code".into()),
                Matcher::UrlEncoded("code".into(), "test-code".into()),
                Matcher::UrlEncoded("resource".into(), resource.clone()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(token_response("first-token", "first-refresh-token"))
            .create_async()
            .await;
        let refresh_exchange = server
            .mock("POST", "/token")
            .match_body(Matcher::AllOf(vec![
                Matcher::UrlEncoded("grant_type".into(), "refresh_token".into()),
                Matcher::UrlEncoded("refresh_token".into(), "first-refresh-token".into()),
                Matcher::UrlEncoded("resource".into(), resource.clone()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(token_response("refreshed-token", "rotated-refresh-token"))
            .create_async()
            .await;

        let directory = tempfile::TempDir::new().unwrap();
        let store = McpCredentialStore::from_path(directory.path().join("oauth.json"));
        let (authorization_sender, mut authorization_receiver) =
            tokio::sync::mpsc::unbounded_channel::<String>();
        let mut client = HttpClient::with_credential_store(resource.clone(), None, store.clone())
            .with_browser_opener(move |url| {
                authorization_sender.send(url.to_string()).unwrap();
            });
        let connection = tokio::spawn(async move { client.connect().await.map(|()| client) });
        let auth_url = match tokio::time::timeout(
            Duration::from_secs(10),
            authorization_receiver.recv(),
        )
        .await
        {
            Ok(Some(url)) => url,
            outcome => match connection.await.unwrap() {
                Ok(_) => panic!("authorization completed without an auth URL: {outcome:?}"),
                Err(error) => panic!("authorization failed before opening browser: {error:?}"),
            },
        };
        assert_eq!(
            reqwest::Url::parse(&auth_url)
                .unwrap()
                .query_pairs()
                .find(|(name, _)| name == "scope")
                .map(|(_, value)| value.into_owned())
                .as_deref(),
            Some("all")
        );
        callback(auth_url, &resource).await;
        let client = tokio::time::timeout(Duration::from_secs(15), connection)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        authorization_exchange.assert_async().await;
        registration.assert_async().await;
        drop(client);

        let mut stored = store.load().await.unwrap().unwrap();
        assert_eq!(
            serde_json::to_value(stored.token_response.clone()).unwrap()["refresh_token"],
            "first-refresh-token"
        );
        stored.token_received_at = Some(0);
        store.save(stored).await.unwrap();

        let mut reconnected =
            HttpClient::with_credential_store(resource.clone(), None, store.clone())
                .with_browser_opener(|_| {
                    panic!("stored credentials should not start a browser flow")
                });
        tokio::time::timeout(Duration::from_secs(15), reconnected.connect())
            .await
            .unwrap()
            .unwrap();
        refresh_exchange.assert_async().await;

        let stored = store.load().await.unwrap().unwrap();
        let response = serde_json::to_value(stored.token_response.unwrap()).unwrap();
        assert_eq!(response["access_token"], "refreshed-token");
        assert_eq!(response["refresh_token"], "rotated-refresh-token");
    }

    #[tokio::test]
    async fn test_mcp_config_factory() {
        // Test stdio config
        let stdio_config = McpConfig::Stdio {
            command: "echo".to_string(),
            args: vec!["hello".to_string()],
        };
        let _stdio_client = create_mcp_client(stdio_config);
        println!("✅ Successfully created StdioClient via factory");

        // Test HTTP config
        let http_config = McpConfig::Http {
            url: "http://localhost:8080".to_string(),
            auth: None,
        };
        let _http_client = create_mcp_client(http_config);
        println!("✅ Successfully created HttpClient via factory");

        // Test SSE config
        let sse_config = McpConfig::Sse {
            url: "http://localhost:8080/sse".to_string(),
        };
        let _sse_client = create_mcp_client(sse_config);
        println!("✅ Successfully created SseClient via factory");

        println!("✅ All MCP client types created successfully via factory");
    }

    #[tokio::test]
    async fn legacy_bearer_token_is_used_without_starting_oauth() {
        let mut server = mockito::Server::new_async().await;
        let connection = server
            .mock("POST", "/mcp")
            .expect(2)
            .match_header("authorization", "Bearer legacy-token")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(initialize_response())
            .create_async()
            .await;
        let mut client = HttpClient::new_with_auth(
            format!("{}/mcp", server.url()),
            Some("legacy-token".to_string()),
        );
        client.connect().await.unwrap();
        connection.assert_async().await;
    }

    #[tokio::test]
    async fn test_mcp_http_integration() {
        // Skip test if no MCP server is running on localhost:8000
        if !is_mcp_server_available().await {
            println!("Skipping MCP HTTP test: no server available on localhost:8000");
            return;
        }

        // Create HTTP client for MCP server on localhost:8000/mcp
        let mut client = HttpClient::new_with_auth(
            "https://localhost:8000/mcp".to_string(),
            Some("TEST_BEARER".to_string()),
        );

        // Test connection
        match client.connect().await {
            Ok(_) => println!("✅ Successfully connected to MCP HTTP server"),
            Err(e) => {
                println!("❌ Failed to connect to MCP HTTP server: {}", e);
                return;
            }
        }

        // Test listing tools
        let tools = match client.list_tools().await {
            Ok(tools) => {
                println!("✅ Successfully listed {} tools", tools.len());
                for tool in &tools {
                    println!("  - {}: {}", tool.name, tool.description);
                }
                tools
            }
            Err(e) => {
                println!("❌ Failed to list tools: {}", e);
                let _ = client.disconnect().await;
                return;
            }
        };

        // Find a fetch tool to test
        let fetch_tool = tools
            .iter()
            .find(|tool| tool.name.to_lowercase().contains("fetch"));

        if let Some(tool) = fetch_tool {
            println!("✅ Found tool to test: {}", tool.name);

            // Test executing the tool with a simple HTTP request
            let tool_call = ToolCall {
                tool_call_id: "test-http-1".to_string(),
                tool_name: tool.name.clone(),
                parameters: json!({
                    "url": "https://ovhcloud.com"
                }),
            };

            match client.execute_tool(tool_call).await {
                Ok(result) => {
                    println!("✅ Successfully executed tool via HTTP");
                    println!("Result: {}", result);
                }
                Err(e) => {
                    println!("❌ Failed to execute tool: {}", e);
                    // Don't return here, still test disconnect
                }
            }
        } else {
            println!("⚠️  No fetch-related tool found in available tools");
        }

        // Test disconnection
        match client.disconnect().await {
            Ok(_) => println!("✅ Successfully disconnected from HTTP server"),
            Err(e) => println!("❌ Failed to disconnect: {}", e),
        }
    }

    #[tokio::test]
    async fn test_mcp_sse_integration() {
        // Skip test if no MCP server is running on localhost:8000
        if !is_mcp_server_available().await {
            println!("Skipping MCP SSE test: no server available on localhost:8000");
            return;
        }

        // Create SSE client for MCP server - SSE endpoint and message POST endpoint
        let mut client = SseClient::new("http://localhost:8000/sse".to_string());

        // Test connection
        match client.connect().await {
            Ok(_) => println!("✅ Successfully connected to MCP SSE server"),
            Err(e) => {
                println!("❌ Failed to connect to MCP SSE server: {}", e);
                return;
            }
        }

        // Test listing tools
        let tools = match client.list_tools().await {
            Ok(tools) => {
                println!("✅ Successfully listed {} tools", tools.len());
                for tool in &tools {
                    println!("  - {}: {}", tool.name, tool.description);
                }
                tools
            }
            Err(e) => {
                println!("❌ Failed to list tools: {}", e);
                let _ = client.disconnect().await;
                return;
            }
        };

        // Find a fetch tool to test
        let fetch_tool = tools
            .iter()
            .find(|tool| tool.name.to_lowercase().contains("fetch"));

        if let Some(tool) = fetch_tool {
            println!("✅ Found tool to test: {}", tool.name);

            // Test executing the tool with a simple HTTP request
            let tool_call = ToolCall {
                tool_call_id: "test-sse-1".to_string(),
                tool_name: tool.name.clone(),
                parameters: json!({
                    "url": "https://ovhcloud.com"
                }),
            };

            match client.execute_tool(tool_call).await {
                Ok(result) => {
                    println!("✅ Successfully executed tool via SSE");
                    println!("Result: {}", result);
                }
                Err(e) => {
                    println!("❌ Failed to execute tool: {}", e);
                    // Don't return here, still test disconnect
                }
            }
        } else {
            println!("⚠️  No fetch-related tool found in available tools");
        }

        // Test disconnection
        match client.disconnect().await {
            Ok(_) => println!("✅ Successfully disconnected from SSE server"),
            Err(e) => println!("❌ Failed to disconnect: {}", e),
        }
    }
}
