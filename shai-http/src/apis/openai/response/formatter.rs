use async_trait::async_trait;
use openai_dive::v1::resources::response::{
    items::{FunctionToolCall, InputItemStatus},
    request::ResponseParameters,
    response::{
        MessageStatus, OutputContent, OutputMessage, ReasoningStatus, ResponseObject,
        ResponseOutput, Role,
    },
};
use openai_dive::v1::resources::response::shared::ResponseUsage;
use openai_dive::v1::resources::chat::{ChatMessage, ChatMessageContent};
use shai_core::agent::AgentEvent;
use uuid::Uuid;

use super::types::ResponseStreamEvent;
use crate::streaming::EventFormatter;

/// Formatter for OpenAI Response API
pub struct ResponseFormatter {
    pub model: String,
    pub created_at: u32,
    pub payload: ResponseParameters,

    // State management
    sequence: u32,
    output: Vec<ResponseOutput>,
    accumulated_text: String,
    initial_event_sent: bool,
}

impl ResponseFormatter {
    pub fn new(model: String, payload: ResponseParameters) -> Self {
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as u32;

        Self {
            model,
            created_at,
            payload,
            sequence: 0,
            output: Vec::new(),
            accumulated_text: String::new(),
            initial_event_sent: false,
        }
    }

    fn build_response_object(
        &self,
        session_id: &str,
        status: ReasoningStatus,
        output: Vec<ResponseOutput>,
    ) -> ResponseObject {
        ResponseObject {
            id: session_id.to_string(),
            object: "response".to_string(),
            created_at: self.created_at,
            model: self.model.clone(),
            status,
            output,
            instruction: self.payload.instructions.clone(),
            metadata: self.payload.metadata.clone(),
            temperature: self.payload.temperature,
            max_output_tokens: self.payload.max_output_tokens,
            parallel_tool_calls: self.payload.parallel_tool_calls,
            previous_response_id: None,
            reasoning: self.payload.reasoning.clone(),
            text: self.payload.text.clone(),
            tool_choice: self.payload.tool_choice.clone(),
            tools: self.payload.tools.clone().unwrap_or_default(),
            top_p: self.payload.top_p,
            top_logprobs: None,
            truncation: self.payload.truncation.clone(),
            user: self.payload.user.clone(),
            usage: Some(ResponseUsage {
                input_tokens: Some(0),
                input_tokens_details: None,
                output_tokens: Some(0),
                output_tokens_details: None,
                total_tokens: 0,
            }),
            incomplete_details: None,
            error: None,
        }
    }
}

#[async_trait]
impl EventFormatter for ResponseFormatter {
    type Output = ResponseStreamEvent;

    async fn format_event(
        &mut self,
        event: AgentEvent,
        session_id: &str,
    ) -> Option<Self::Output> {
        // Send initial event on first call
        if !self.initial_event_sent {
            self.initial_event_sent = true;
            let initial_response = self.build_response_object(
                session_id,
                ReasoningStatus::InProgress,
                vec![],
            );
            let evt = ResponseStreamEvent::created(self.sequence, initial_response);
            self.sequence += 1;
            return Some(evt);
        }

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
                            self.accumulated_text = text;
                        }
                    }
                    Err(err) => {
                        // Accumulate error message as text
                        self.accumulated_text = format!("Error: {}", err);
                    }
                }
                None
            }

            // Tool calls
            AgentEvent::ToolCallStarted { call, .. } => {
                let tool_output = ResponseOutput::FunctionToolCall(FunctionToolCall {
                    id: Some(call.tool_call_id.clone()),
                    call_id: call.tool_call_id.clone(),
                    name: call.tool_name.clone(),
                    arguments: call.parameters.to_string(),
                    status: Some(InputItemStatus::InProgress),
                });

                let output_index = self.output.len();
                self.output.push(tool_output.clone());

                let event = ResponseStreamEvent::output_item_added(self.sequence, output_index, tool_output);
                self.sequence += 1;

                Some(event)
            }

            AgentEvent::ToolCallCompleted { call, result, .. } => {
                use shai_core::tools::ToolResult;

                let tool_status = match &result {
                    ToolResult::Success { .. } => {
                        InputItemStatus::Completed
                    }
                    _ => {
                        InputItemStatus::Incomplete
                    }
                };

                if let Some(idx) = self.output.iter().position(|o| {
                    if let ResponseOutput::FunctionToolCall(tc) = o {
                        tc.id.as_deref() == Some(call.tool_call_id.as_str())
                    } else {
                        false
                    }
                }) {
                    self.output[idx] = ResponseOutput::FunctionToolCall(FunctionToolCall {
                        id: Some(call.tool_call_id.clone()),
                        call_id: call.tool_call_id.clone(),
                        name: call.tool_name.clone(),
                        arguments: call.parameters.to_string(),
                        status: Some(tool_status),
                    });

                    let event = ResponseStreamEvent::output_item_done(self.sequence, idx, self.output[idx].clone());
                    self.sequence += 1;

                    return Some(event);
                }

                None
            }

            AgentEvent::Completed { message, success, .. } => {
                if !message.is_empty() {
                    self.accumulated_text = message;
                }

                let msg_output = ResponseOutput::Message(OutputMessage {
                    id: Uuid::new_v4().to_string(),
                    role: Role::Assistant,
                    status: MessageStatus::Completed,
                    content: vec![OutputContent::Text {
                        text: self.accumulated_text.clone(),
                        annotations: vec![],
                    }],
                });
                self.output.push(msg_output);

                let final_status = if success {
                    ReasoningStatus::Completed
                } else {
                    ReasoningStatus::Failed
                };

                let final_response = self.build_response_object(
                    session_id,
                    final_status,
                    self.output.clone(),
                );

                let event = ResponseStreamEvent::completed(self.sequence, final_response);

                Some(event)
            }

            AgentEvent::StatusChanged { new_status, .. } => {
                use shai_core::agent::PublicAgentState;
                if matches!(new_status, PublicAgentState::Paused { .. }) {
                    let msg_output = ResponseOutput::Message(OutputMessage {
                        id: Uuid::new_v4().to_string(),
                        role: Role::Assistant,
                        status: MessageStatus::Completed,
                        content: vec![OutputContent::Text {
                            text: self.accumulated_text.clone(),
                            annotations: vec![],
                        }],
                    });
                    self.output.push(msg_output);

                    let final_response = self.build_response_object(
                        session_id,
                        ReasoningStatus::Incomplete,
                        self.output.clone(),
                    );

                    let event = ResponseStreamEvent::completed(self.sequence, final_response);

                    return Some(event);
                }
                None
            }
            _ => None,
        }
    }

    fn event_name(&self, output: &Self::Output) -> &str {
        output.event_name()
    }
}
