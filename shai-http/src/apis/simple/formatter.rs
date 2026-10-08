use async_trait::async_trait;
use openai_dive::v1::resources::chat::{ChatMessage, ChatMessageContent};
use shai_core::agent::AgentEvent;
use std::collections::HashMap;

use super::types::{MultiModalStreamingResponse, ToolCall, ToolCallResult};
use crate::streaming::EventFormatter;

/// Formatter for Simple API multimodal responses
pub struct SimpleFormatter {
    pub model: String,
}

impl SimpleFormatter {
    pub fn new(model: String) -> Self {
        Self { model }
    }
}

#[async_trait]
impl EventFormatter for SimpleFormatter {
    type Output = MultiModalStreamingResponse;

    async fn format_event(&mut self, event: AgentEvent, session_id: &str) -> Option<Self::Output> {
        match event {
            AgentEvent::BrainResult { thought, .. } => {
                match thought {
                    Ok(msg) => {
                        // Extract text content from the ChatMessage
                        let text_content = match &msg {
                            ChatMessage::Assistant {
                                content: Some(ChatMessageContent::Text(text)),
                                ..
                            } => Some(text.clone()),
                            _ => None,
                        };

                        if let Some(text) = text_content {
                            return Some(MultiModalStreamingResponse {
                                id: session_id.to_string(),
                                model: self.model.clone(),
                                assistant: Some(text),
                                call: None,
                                result: None,
                            });
                        }
                        None
                    }
                    Err(err) => {
                        // Format error as assistant message
                        Some(MultiModalStreamingResponse {
                            id: session_id.to_string(),
                            model: self.model.clone(),
                            assistant: Some(format!("Error: {}", err)),
                            call: None,
                            result: None,
                        })
                    }
                }
            }
            AgentEvent::ToolCallStarted { call, .. } => Some(MultiModalStreamingResponse {
                id: session_id.to_string(),
                model: self.model.clone(),
                assistant: None,
                call: Some(ToolCall {
                    tool: call.tool_name.clone(),
                    args: parameters_to_args(&call.parameters),
                    output: None,
                }),
                result: None,
            }),
            AgentEvent::ToolCallCompleted { call, result, .. } => {
                use shai_core::tools::ToolResult;

                let (tool_result, output_str) = match &result {
                    ToolResult::Success { output, .. } => (
                        ToolCallResult {
                            text: Some(output.clone()),
                            text_stream: None,
                            image: None,
                            speech: None,
                            other: None,
                            error: None,
                            extra: None,
                        },
                        output.clone(),
                    ),
                    ToolResult::Error { error, .. } => (
                        ToolCallResult {
                            text: None,
                            text_stream: None,
                            image: None,
                            speech: None,
                            other: None,
                            error: Some(error.clone()),
                            extra: None,
                        },
                        String::new(),
                    ),
                    ToolResult::Denied => (
                        ToolCallResult {
                            text: None,
                            text_stream: None,
                            image: None,
                            speech: None,
                            other: None,
                            error: Some("Tool call denied".to_string()),
                            extra: None,
                        },
                        String::new(),
                    ),
                };

                Some(MultiModalStreamingResponse {
                    id: session_id.to_string(),
                    model: self.model.clone(),
                    assistant: None,
                    call: Some(ToolCall {
                        tool: call.tool_name.clone(),
                        args: parameters_to_args(&call.parameters),
                        output: Some(output_str),
                    }),
                    result: Some(tool_result),
                })
            }
            AgentEvent::Completed { message, .. } => Some(MultiModalStreamingResponse {
                id: session_id.to_string(),
                model: self.model.clone(),
                assistant: Some(message),
                call: None,
                result: None,
            }),
            AgentEvent::Error { error } => Some(MultiModalStreamingResponse {
                id: session_id.to_string(),
                model: self.model.clone(),
                assistant: None,
                call: None,
                result: Some(ToolCallResult {
                    text: None,
                    text_stream: None,
                    image: None,
                    speech: None,
                    other: None,
                    error: Some(error),
                    extra: None,
                }),
            }),
            _ => None,
        }
    }
}

/// Convert serde_json::Value parameters to HashMap<String, String>
fn parameters_to_args(params: &serde_json::Value) -> HashMap<String, String> {
    let mut args = HashMap::new();

    match params {
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                let value_str = match value {
                    serde_json::Value::String(s) => s.clone(),
                    other => serde_json::to_string(other).unwrap_or_default(),
                };
                args.insert(key.clone(), value_str);
            }
        }
        _ => {
            // If it's not an object, store the entire thing as a single "params" entry
            args.insert("params".to_string(), params.to_string());
        }
    }

    args
}
