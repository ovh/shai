use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub tool: String,
    #[serde(default)]
    pub args: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_stream: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speech: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub other: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviousCall {
    pub call: ToolCall,
    pub result: ToolCallResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserMessage {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attached_files: Option<HashMap<String, String>>, // { filename: base64file, ... }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantMessage {
    pub assistant: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
#[allow(clippy::large_enum_variant)]
pub enum Message {
    User(UserMessage),
    Assistant(AssistantMessage),
    PreviousCall(PreviousCall),
}

/// Agent capability configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCapability {
    #[serde(rename = "type")]
    pub tool_type: String, // "capability"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub internet: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speech: Option<bool>,
}

/// OpenAI API tool configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAiApi {
    #[serde(rename = "type")]
    pub tool_type: String, // "openai"
    pub url: String,
    pub description: String,
    pub model: String,
}

/// MCP tool configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    #[serde(rename = "type")]
    pub tool_type: String, // "mcp"
    pub url: String,
}

/// Discriminated union of tool types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AgentTool {
    #[serde(rename = "capability")]
    Capability {
        #[serde(skip_serializing_if = "Option::is_none")]
        thinking: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        internet: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        image: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        speech: Option<bool>,
    },
    #[serde(rename = "openai")]
    OpenAi {
        url: String,
        description: String,
        model: String,
    },
    #[serde(rename = "mcp")]
    Mcp { url: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiModalQuery {
    pub model: String,
    #[serde(default)]
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub messages: Option<Vec<Message>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<AgentTool>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiModalStreamingResponse {
    pub id: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assistant: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call: Option<ToolCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<ToolCallResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
#[allow(clippy::large_enum_variant)]
pub enum ResponseMessage {
    Assistant(AssistantMessage),
    PreviousCall(PreviousCall),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiModalResponse {
    pub id: String,
    pub model: String,
    pub result: Vec<ResponseMessage>,
}
