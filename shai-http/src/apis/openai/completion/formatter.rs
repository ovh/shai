use async_trait::async_trait;
use openai_dive::v1::resources::chat::{
    ChatCompletionChunkResponse, ChatCompletionChunkChoice, DeltaChatMessage,
    ChatMessageContent, ChatMessage,
};
use openai_dive::v1::resources::shared::FinishReason;
use shai_core::agent::AgentEvent;
use uuid::Uuid;

use crate::streaming::EventFormatter;

/// Formatter for OpenAI Chat Completion API (streaming)
/// Tool calls are converted to "thinking" reasoning_content deltas
pub struct ChatCompletionFormatter {
    pub model: String,
    pub created: u32,
    accumulated_text: String,
}

impl ChatCompletionFormatter {
    pub fn new(model: String) -> Self {
        let created = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as u32;

        Self {
            model,
            created,
            accumulated_text: String::new(),
        }
    }

    fn create_chunk(&self, delta: DeltaChatMessage, finish_reason: Option<FinishReason>) -> ChatCompletionChunkResponse {
        ChatCompletionChunkResponse {
            id: Some(format!("chatcmpl-{}", Uuid::new_v4())),
            object: "chat.completion.chunk".to_string(),
            created: self.created,
            model: self.model.clone(),
            choices: vec![ChatCompletionChunkChoice {
                index: Some(0),
                delta,
                finish_reason,
                logprobs: None,
            }],
            usage: None,
            system_fingerprint: None,
        }
    }
}

#[async_trait]
impl EventFormatter for ChatCompletionFormatter {
    type Output = ChatCompletionChunkResponse;

    async fn format_event(
        &mut self,
        event: AgentEvent,
        _session_id: &str,
    ) -> Option<Self::Output> {
        match event {
            // Capture assistant messages from brain results
            AgentEvent::BrainResult { thought, .. } => {
                match thought {
                    Ok(msg) => {
                        if let ChatMessage::Assistant {
                            content: Some(ChatMessageContent::Text(text)),
                            ..
                        } = msg
                        {
                            // Accumulate the text for final response
                            self.accumulated_text = text;
                        }
                        None
                    }
                    Err(err) => {
                        // Stream error as assistant message
                        let delta = DeltaChatMessage::Assistant {
                            content: Some(ChatMessageContent::Text(format!("Error: {}", err))),
                            reasoning: None,
                            reasoning_content: None,
                            refusal: None,
                            name: None,
                            tool_calls: None,
                        };
                        Some(self.create_chunk(delta, Some(FinishReason::StopSequenceReached)))
                    }
                }
            }

            // Tool call started - stream as thinking delta
            AgentEvent::ToolCallStarted { call, .. } => {
                let thinking_text = format!("[toolcall: {}]", call.tool_name);
                let delta = DeltaChatMessage::Assistant {
                    content: None,
                    reasoning: None,
                    reasoning_content: Some(thinking_text),
                    refusal: None,
                    name: None,
                    tool_calls: None,
                };

                Some(self.create_chunk(delta, None))
            }

            // Tool call completed - stream result as thinking delta
            AgentEvent::ToolCallCompleted { call, result, .. } => {
                use shai_core::tools::ToolResult;

                let thinking_text = match &result {
                    ToolResult::Success { .. } => {
                        format!("[tool succeeded: {}]", call.tool_name)
                    }
                    ToolResult::Error { error, .. } => {
                        let error_oneline = error.lines().next().unwrap_or(error);
                        format!("[tool failed: {} - {}]", call.tool_name, error_oneline)
                    }
                    ToolResult::Denied => {
                        format!("[tool denied: {}]", call.tool_name)
                    }
                };

                let delta = DeltaChatMessage::Assistant {
                    content: None,
                    reasoning: None,
                    reasoning_content: Some(thinking_text),
                    refusal: None,
                    name: None,
                    tool_calls: None,
                };

                Some(self.create_chunk(delta, None))
            }

            // Agent completed - stream final content as delta
            AgentEvent::Completed { message, .. } => {
                if !message.is_empty() {
                    self.accumulated_text = message;
                }

                // Send the final content delta
                let content_delta = DeltaChatMessage::Assistant {
                    content: Some(ChatMessageContent::Text(self.accumulated_text.clone())),
                    reasoning: None,
                    reasoning_content: None,
                    refusal: None,
                    name: None,
                    tool_calls: None,
                };

                // Always use StopSequenceReached for completion
                // Success/failure is indicated in the content
                let finish_reason = Some(FinishReason::StopSequenceReached);

                Some(self.create_chunk(content_delta, finish_reason))
            }

            AgentEvent::Error { error } => {
                // Stream error as content delta
                let delta = DeltaChatMessage::Assistant {
                    content: Some(ChatMessageContent::Text(format!("Error: {}", error))),
                    reasoning: None,
                    reasoning_content: None,
                    refusal: None,
                    name: None,
                    tool_calls: None,
                };

                Some(self.create_chunk(delta, Some(FinishReason::StopSequenceReached)))
            }

            _ => None,
        }
    }
}
