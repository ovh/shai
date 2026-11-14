use async_trait::async_trait;
use rmcp::{
    model::{ClientCapabilities, ClientInfo, Implementation, InitializeRequestParams},
    service::{RunningService, ServiceExt},
    transport::{
        streamable_http_client::StreamableHttpClientTransportConfig, StreamableHttpClientTransport,
    },
    RoleClient,
};

use super::mcp::{call_tool_params, content_to_string, McpClient, McpToolDescription};
use crate::tools::{ToolCall, ToolResult};

pub struct HttpClient {
    url: String,
    bearer_token: Option<String>,
    service: Option<RunningService<RoleClient, InitializeRequestParams>>,
}

impl HttpClient {
    pub fn new(url: String) -> Self {
        Self::new_with_auth(url, None)
    }

    pub fn new_with_auth(url: String, bearer_token: Option<String>) -> Self {
        Self {
            url,
            bearer_token,
            service: None,
        }
    }
}

#[async_trait]
impl McpClient for HttpClient {
    async fn connect(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Only connect if not already connected
        if self.service.is_some() {
            return Ok(());
        }

        let transport = if let Some(token) = &self.bearer_token {
            StreamableHttpClientTransport::from_config(
                StreamableHttpClientTransportConfig::with_uri(self.url.as_str())
                    .auth_header(token.as_str()),
            )
        } else {
            StreamableHttpClientTransport::from_uri(self.url.as_str())
        };

        let client_info = ClientInfo::new(
            ClientCapabilities::default(),
            Implementation::new("shai-mcp-http-client", "0.1.0"),
        );
        let service = client_info.serve(transport).await?;

        // Give the server a moment to process the initialization
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        self.service = Some(service);
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(service) = self.service.take() {
            service.cancel().await?;
        }
        Ok(())
    }

    async fn list_tools(
        &self,
    ) -> Result<Vec<McpToolDescription>, Box<dyn std::error::Error + Send + Sync>> {
        let service = self.service.as_ref().ok_or("Not connected")?;
        let tools_result = service.list_tools(None).await?;

        let tool_descriptions = tools_result
            .tools
            .into_iter()
            .map(|tool| McpToolDescription {
                name: tool.name.to_string(),
                description: tool.description.unwrap_or_default().to_string(),
                parameters_schema: serde_json::Value::Object((*tool.input_schema).clone()),
            })
            .collect();

        Ok(tool_descriptions)
    }

    async fn execute_tool(
        &self,
        tool_call: ToolCall,
    ) -> Result<ToolResult, Box<dyn std::error::Error + Send + Sync>> {
        let service = self.service.as_ref().ok_or("Not connected")?;

        let result = service.call_tool(call_tool_params(&tool_call)).await?;

        Ok(ToolResult::success(content_to_string(result.content)))
    }
}
