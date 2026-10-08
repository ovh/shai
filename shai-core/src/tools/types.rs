use async_trait::async_trait;
use openai_dive::v1::resources::chat::{
    ChatCompletionFunction, ChatCompletionTool, ChatCompletionToolType,
};
use schemars::JsonSchema;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use shai_llm::{ToolBox, ToolDescription};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Empty parameters struct for tools that don't need any parameters
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ToolEmptyParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _unused: Option<String>,
}

/// Agent-level Permission struct for Read/Write global permissions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToolCapability {
    Read,
    Write,
    Network,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub tool_call_id: String,
    pub tool_name: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ToolResult {
    Success {
        output: String,
        metadata: Option<HashMap<String, serde_json::Value>>,
    },
    Error {
        error: String,
        metadata: Option<HashMap<String, serde_json::Value>>,
    },
    Denied,
}

impl fmt::Display for ToolResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ToolResult::Success { output, .. } => write!(f, "{}", output),
            ToolResult::Error { error, .. } => {
                write!(f, "The tool failed with the following error: {}", error)
            }
            ToolResult::Denied => write!(f, "The tool call was rejected by the user"),
        }
    }
}

impl ToolResult {
    /// Create a successful result with output
    pub fn success(output: String) -> Self {
        Self::Success {
            output,
            metadata: None,
        }
    }

    /// Create a successful result with output and metadata
    pub fn success_with_metadata(
        output: String,
        metadata: HashMap<String, serde_json::Value>,
    ) -> Self {
        Self::Success {
            output,
            metadata: Some(metadata),
        }
    }

    /// Create an error result
    pub fn error(error: String) -> Self {
        Self::Error {
            error,
            metadata: None,
        }
    }

    /// Create an error result
    pub fn denied() -> Self {
        Self::Denied
    }

    /// Create an error result with metadata
    pub fn error_with_metadata(
        error: String,
        metadata: HashMap<String, serde_json::Value>,
    ) -> Self {
        Self::Error {
            error,
            metadata: Some(metadata),
        }
    }

    /// Check if the result is successful
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success { .. })
    }

    /// Check if the result is an error
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error { .. })
    }

    /// Get the metadata if present
    pub fn metadata(&self) -> Option<&HashMap<String, serde_json::Value>> {
        match self {
            ToolResult::Success { metadata, .. } => metadata.as_ref(),
            ToolResult::Error { metadata, .. } => metadata.as_ref(),
            ToolResult::Denied => None,
        }
    }

    /// Check if the tool was denied
    pub fn is_denied(&self) -> bool {
        matches!(self, Self::Denied)
    }
}

#[async_trait]
pub trait Tool: ToolDescription + Send + Sync {
    type Params: DeserializeOwned + JsonSchema + Send + Sync;

    fn capabilities(&self) -> &'static [ToolCapability];

    /// execute the tool.
    /// parameters are specific for each tool
    async fn execute(
        &self,
        params: Self::Params,
        cancel_token: Option<CancellationToken>,
    ) -> ToolResult;

    /// execute the tool in preview mode - shows what would happen without making changes
    /// Default implementation returns None (no preview available)
    async fn execute_preview(&self, _params: Self::Params) -> Option<ToolResult> {
        None
    }

    /// execute the tool.
    /// params are jsno-serialized then deserialized in tool specific parameter.
    async fn execute_json(
        &self,
        params: serde_json::Value,
        cancel_token: Option<CancellationToken>,
    ) -> ToolResult {
        // Deserialize JSON directly to typed parameters
        let typed_params: <Self>::Params = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => {
                return ToolResult::error(format!(
                    "Parameter deserialization failed for '{}': {}",
                    self.name(),
                    e
                ))
            }
        };

        // Call the typed execute method directly
        self.execute(typed_params, cancel_token).await
    }
}

/// Simple trait that agents can use - no associated types
#[async_trait]
pub trait AnyTool: ToolDescription + Send + Sync {
    fn capabilities(&self) -> &[ToolCapability];

    async fn execute_json(
        &self,
        params: serde_json::Value,
        cancel_token: Option<CancellationToken>,
    ) -> ToolResult;
    async fn execute_preview_json(&self, params: serde_json::Value) -> Option<ToolResult>;
}

/// Auto-implement AnyTool
#[async_trait]
impl<T> AnyTool for T
where
    T: Tool + 'static,
{
    fn capabilities(&self) -> &[ToolCapability] {
        <T as Tool>::capabilities(self)
    }

    async fn execute_json(
        &self,
        params: serde_json::Value,
        cancel_token: Option<CancellationToken>,
    ) -> ToolResult {
        self.execute_json(params, cancel_token).await
    }

    async fn execute_preview_json(&self, params: serde_json::Value) -> Option<ToolResult> {
        let typed_params: <T as Tool>::Params = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(_) => return None,
        };

        self.execute_preview(typed_params).await
    }
}

pub type ToolError = Box<dyn std::error::Error + Send + Sync>;

impl dyn AnyTool {
    pub fn to_openai(&self) -> ChatCompletionTool {
        ChatCompletionTool {
            r#type: ChatCompletionToolType::Function,
            function: ChatCompletionFunction {
                name: self.name().to_string(),
                description: Some(self.description().to_string()),
                parameters: self.parameters_schema(),
            },
        }
    }
}

/// A toolbox is a set of tool
pub type AnyToolBox = Vec<Arc<dyn AnyTool>>;

// Extension trait for Vec operations
pub trait IntoToolBox {
    fn into_toolbox(self) -> ToolBox;
}

impl IntoToolBox for AnyToolBox {
    fn into_toolbox(self) -> ToolBox {
        self.into_iter()
            .map(|tool| tool as Arc<dyn ToolDescription>)
            .collect()
    }
}

pub trait ContainsAnyTool {
    fn contains_tool(&self, name: &str) -> bool;
    fn get_tool(&self, name: &str) -> Option<Arc<dyn AnyTool>>;
}

impl ContainsAnyTool for AnyToolBox {
    fn contains_tool(&self, name: &str) -> bool {
        self.iter().any(|tool| tool.name() == name)
    }

    fn get_tool(&self, name: &str) -> Option<Arc<dyn AnyTool>> {
        self.iter().find(|tool| tool.name() == name).cloned()
    }
}
