use async_trait::async_trait;
use rmcp::{
    model::{
        CallToolRequestParam, ClientCapabilities, ClientInfo, Implementation,
        InitializeRequestParam,
    },
    service::{RunningService, ServiceExt},
    transport::SseClientTransport,
    RoleClient,
};
use std::borrow::Cow;

use super::mcp::{McpClient, McpToolDescription};
use crate::tools::{ToolCall, ToolResult};

pub struct SseClient {
    url: String,
    service: Option<RunningService<RoleClient, InitializeRequestParam>>,
}

impl SseClient {
    pub fn new(url: String) -> Self {
        Self { url, service: None }
    }
}

#[async_trait]
impl McpClient for SseClient {
    async fn connect(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Only connect if not already connected
        if self.service.is_some() {
            return Ok(());
        }

        let transport = SseClientTransport::start(self.url.as_str()).await?;
        let client_info = ClientInfo {
            protocol_version: Default::default(),
            capabilities: ClientCapabilities::default(),
            client_info: Implementation {
                name: "shai-mcp-sse-client".to_string(),
                version: "0.1.0".to_string(),
            },
        };
        let service = client_info.serve(transport).await?;
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
        let tools_result = service.list_tools(Default::default()).await?;

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

        let result = service
            .call_tool(CallToolRequestParam {
                name: Cow::Owned(tool_call.tool_name.clone()),
                arguments: tool_call.parameters.as_object().cloned(),
            })
            .await?;

        let content = result
            .content
            .into_iter()
            .map(|c| match c.raw {
                rmcp::model::RawContent::Text(text_content) => text_content.text,
                rmcp::model::RawContent::Image(image_data) => {
                    format!("[Image: {} bytes]", image_data.data.len())
                }
                rmcp::model::RawContent::Resource(_) => "[Resource]".to_string(),
                rmcp::model::RawContent::Audio(audio_data) => {
                    format!("[Audio: {} bytes]", audio_data.data.len())
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        Ok(ToolResult::success(content))
    }
}
