use async_trait::async_trait;
use shai_llm::ToolDescription;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::tools::{AnyTool, ToolCall, ToolCapability, ToolResult};

#[derive(Debug, Clone)]
pub struct McpToolDescription {
    pub name: String,
    pub description: String,
    pub parameters_schema: serde_json::Value,
}

#[async_trait]
pub trait McpClient: Send + Sync {
    async fn connect(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    async fn disconnect(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    async fn list_tools(
        &self,
    ) -> Result<Vec<McpToolDescription>, Box<dyn std::error::Error + Send + Sync>>;
    async fn execute_tool(
        &self,
        tool_call: ToolCall,
    ) -> Result<ToolResult, Box<dyn std::error::Error + Send + Sync>>;
}

pub struct WrappedMcpTool {
    pub desc: McpToolDescription,
    pub client: Arc<Mutex<Box<dyn McpClient>>>,
    pub mcp_name: String,
}

impl ToolDescription for WrappedMcpTool {
    fn name(&self) -> String {
        self.desc.name.clone()
    }

    fn description(&self) -> String {
        self.desc.description.clone()
    }

    fn parameters_schema(&self) -> serde_json::Value {
        self.desc.parameters_schema.clone()
    }

    fn group(&self) -> Option<&str> {
        Some(&self.mcp_name)
    }
}

#[async_trait]
impl AnyTool for WrappedMcpTool {
    fn capabilities(&self) -> &[ToolCapability] {
        &[ToolCapability::Network]
    }

    async fn execute_json(
        &self,
        params: serde_json::Value,
        _cancel_token: Option<tokio_util::sync::CancellationToken>,
    ) -> ToolResult {
        let tool_call = ToolCall {
            tool_call_id: format!("mcp-{}", uuid::Uuid::new_v4()),
            tool_name: self.desc.name.clone(),
            parameters: params,
        };

        // Lock the client for execution
        // right now we only do one call at a time per mcp server to avoid race condition
        let client = self.client.lock().await;

        match client.execute_tool(tool_call).await {
            Ok(result) => result,
            Err(e) => ToolResult::error(format!("MCP tool execution failed: {}", e)),
        }
    }

    async fn execute_preview_json(&self, _params: serde_json::Value) -> Option<ToolResult> {
        None // MCP tools don't support preview mode
    }
}

/// Create AnyTool instances from an MCP client
pub async fn get_mcp_tools(
    mut client: Box<dyn McpClient>,
    mcp_name: &str,
) -> Result<Vec<Box<dyn AnyTool>>, Box<dyn std::error::Error + Send + Sync>> {
    // Auto-connect if not already connected
    client.connect().await?;

    let tool_descriptions = client.list_tools().await?;
    let client_ref = Arc::new(Mutex::new(client));

    let wrapped_tools: Vec<Box<dyn AnyTool>> = tool_descriptions
        .into_iter()
        .map(|desc| {
            Box::new(WrappedMcpTool {
                desc,
                client: client_ref.clone(),
                mcp_name: mcp_name.to_string(),
            }) as Box<dyn AnyTool>
        })
        .collect();

    Ok(wrapped_tools)
}
