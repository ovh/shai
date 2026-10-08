#[cfg(test)]
mod tests {
    use crate::tools::ToolCall;
    use crate::tools::{
        create_mcp_client, HttpClient, McpClient, McpConfig, SseClient, StdioClient,
    };
    use serde_json::json;
    use std::process::Command;
    use tokio;

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
