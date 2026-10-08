use super::structs::{FetchToolParams, HttpMethod};
use crate::tools::{tool, ToolResult};
use reqwest;
use serde_json::json;
use std::collections::HashMap;
use std::time::Duration;

pub struct FetchTool;

impl Default for FetchTool {
    fn default() -> Self {
        Self::new()
    }
}

impl FetchTool {
    pub fn new() -> Self {
        Self
    }
}

#[tool(name = "fetch", description = r#"Retrieves content from a URL. This tool is ideal for accessing web pages, APIs, or other online resources.

**Functionality:**
- Supports `GET`, `POST`, `PUT`, and `DELETE` HTTP methods.
- Allows for custom headers and request bodies, making it suitable for interacting with REST APIs.
- Includes a timeout to prevent indefinite hangs on unresponsive servers.

**Usage Notes:**
- Provide a fully-qualified URL.
- For API interactions, you can set the `Content-Type` header to `application/json` and provide a JSON string as the `body`.
- The tool will return the raw response body, which you can then parse or analyze.

**Examples:**
- **Get a web page:** `fetch(url='https://example.com')`
- **Get JSON data from an API:** `fetch(url='https://api.example.com/data')`
- **Post JSON data to an API:** `fetch(url='https://api.example.com/users', method='POST', headers={'Content-Type': 'application/json'}, body='{"name": "John Doe"}')`
"#, capabilities = [ToolCapability::Network])]
impl FetchTool {
    async fn execute(&self, params: FetchToolParams) -> ToolResult {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(params.timeout))
            .build();

        let client = match client {
            Ok(c) => c,
            Err(e) => return ToolResult::error(format!("Failed to create HTTP client: {}", e)),
        };

        // Build the request
        let mut request_builder = match params.method {
            HttpMethod::Get => client.get(&params.url),
            HttpMethod::Post => client.post(&params.url),
            HttpMethod::Put => client.put(&params.url),
            HttpMethod::Delete => client.delete(&params.url),
        };

        // Add headers if provided
        if let Some(headers) = &params.headers {
            for (key, value) in headers {
                request_builder = request_builder.header(key, value);
            }
        }

        // Add body for POST/PUT requests
        if let Some(body) = &params.body {
            request_builder = request_builder.body(body.clone());
        }

        // Execute the request
        match request_builder.send().await {
            Ok(response) => {
                let status = response.status();
                let headers: HashMap<String, String> = response
                    .headers()
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
                    .collect();

                match response.text().await {
                    Ok(body) => {
                        let mut meta = HashMap::new();
                        meta.insert("url".to_string(), json!(params.url));
                        meta.insert(
                            "method".to_string(),
                            json!(match params.method {
                                HttpMethod::Get => "GET",
                                HttpMethod::Post => "POST",
                                HttpMethod::Put => "PUT",
                                HttpMethod::Delete => "DELETE",
                            }),
                        );
                        meta.insert("status_code".to_string(), json!(status.as_u16()));
                        meta.insert("response_headers".to_string(), json!(headers));
                        meta.insert("content_length".to_string(), json!(body.len()));

                        if status.is_success() {
                            ToolResult::Success {
                                output: body,
                                metadata: Some(meta),
                            }
                        } else {
                            ToolResult::Error {
                                error: format!("HTTP request failed with status: {}", status),
                                metadata: Some(meta),
                            }
                        }
                    }
                    Err(e) => ToolResult::error(format!("Failed to read response body: {}", e)),
                }
            }
            Err(e) => ToolResult::error(format!("HTTP request failed: {}", e)),
        }
    }
}
