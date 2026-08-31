/// Streaming event types for OpenAI Response API
/// These types are not yet available in openai_dive, so we define them locally.
///
/// Reference: https://platform.openai.com/docs/api-reference/responses-streaming

use serde::{Deserialize, Serialize};
use openai_dive::v1::resources::response::{
    request::{ContentInput, ContentItem, ResponseInput, ResponseInputItem, ResponseParameters},
    response::{ResponseObject, ResponseOutput, Role},
};
use openai_dive::v1::resources::chat::{ChatMessage, ChatMessageContent};

/// Base streaming event structure
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ResponseStreamEvent {
    #[serde(rename = "type")]
    pub event_type: ResponseEventType,
    #[serde(flatten)]
    pub data: ResponseEventData,
}

/// Event types for Response API streaming
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ResponseEventType {
    #[serde(rename = "response.created")]
    ResponseCreated,
    #[serde(rename = "response.in_progress")]
    ResponseInProgress,
    #[serde(rename = "response.output_item.added")]
    ResponseOutputItemAdded,
    #[serde(rename = "response.output_item.done")]
    ResponseOutputItemDone,
    #[serde(rename = "response.output_text.delta")]
    ResponseOutputTextDelta,
    #[serde(rename = "response.completed")]
    ResponseCompleted,
}

/// Event data for streaming events
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum ResponseEventData {
    /// response.created, response.in_progress, response.completed
    Response {
        sequence_number: u32,
        response: ResponseObject,
    },
    /// response.output_item.added, response.output_item.done
    OutputItem {
        sequence_number: u32,
        output_index: usize,
        item: ResponseOutput,
    },
    /// response.output_text.delta
    TextDelta {
        sequence_number: u32,
        item_id: String,
        output_index: usize,
        content_index: usize,
        delta: String,
    },
}

impl ResponseStreamEvent {
    /// Create a response.created event
    pub fn created(sequence_number: u32, response: ResponseObject) -> Self {
        Self {
            event_type: ResponseEventType::ResponseCreated,
            data: ResponseEventData::Response {
                sequence_number,
                response,
            },
        }
    }

    /// Create a response.in_progress event
    pub fn in_progress(sequence_number: u32, response: ResponseObject) -> Self {
        Self {
            event_type: ResponseEventType::ResponseInProgress,
            data: ResponseEventData::Response {
                sequence_number,
                response,
            },
        }
    }

    /// Create a response.output_item.added event
    pub fn output_item_added(sequence_number: u32, output_index: usize, item: ResponseOutput) -> Self {
        Self {
            event_type: ResponseEventType::ResponseOutputItemAdded,
            data: ResponseEventData::OutputItem {
                sequence_number,
                output_index,
                item,
            },
        }
    }

    /// Create a response.output_item.done event
    pub fn output_item_done(sequence_number: u32, output_index: usize, item: ResponseOutput) -> Self {
        Self {
            event_type: ResponseEventType::ResponseOutputItemDone,
            data: ResponseEventData::OutputItem {
                sequence_number,
                output_index,
                item,
            },
        }
    }

    /// Create a response.output_text.delta event
    pub fn output_text_delta(
        sequence_number: u32,
        item_id: String,
        output_index: usize,
        content_index: usize,
        delta: String,
    ) -> Self {
        Self {
            event_type: ResponseEventType::ResponseOutputTextDelta,
            data: ResponseEventData::TextDelta {
                sequence_number,
                item_id,
                output_index,
                content_index,
                delta,
            },
        }
    }

    /// Create a response.completed event
    pub fn completed(sequence_number: u32, response: ResponseObject) -> Self {
        Self {
            event_type: ResponseEventType::ResponseCompleted,
            data: ResponseEventData::Response {
                sequence_number,
                response,
            },
        }
    }

    /// Get the SSE event name for this event
    pub fn event_name(&self) -> &'static str {
        match self.event_type {
            ResponseEventType::ResponseCreated => "response.created",
            ResponseEventType::ResponseInProgress => "response.in_progress",
            ResponseEventType::ResponseOutputItemAdded => "response.output_item.added",
            ResponseEventType::ResponseOutputItemDone => "response.output_item.done",
            ResponseEventType::ResponseOutputTextDelta => "response.output_text.delta",
            ResponseEventType::ResponseCompleted => "response.completed",
        }
    }
}

/// Convert OpenAI Response API input to ChatMessage trace
pub fn build_message_trace(params: &ResponseParameters) -> Vec<ChatMessage> {
    let mut trace = Vec::new();

    // Add instructions as system message if present
    if let Some(instructions) = &params.instructions {
        trace.push(ChatMessage::System {
            content: ChatMessageContent::Text(instructions.clone()),
            name: None,
        });
    }

    // Convert input messages
    match &params.input {
        ResponseInput::Text(text) => {
            trace.push(ChatMessage::User {
                content: ChatMessageContent::Text(text.clone()),
                name: None,
            });
        }
        ResponseInput::List(items) => {
            for item in items {
                if let ResponseInputItem::Message(msg) = item {
                    match &msg.role {
                        Role::User => {
                            // Convert content to text (simplified for now)
                            let text = match &msg.content {
                                ContentInput::Text(t) => t.clone(),
                                ContentInput::List(items) => {
                                    // For now, just extract text items
                                    items
                                        .iter()
                                        .filter_map(|item| {
                                            if let ContentItem::Text { text } = item {
                                                Some(text.clone())
                                            } else {
                                                None
                                            }
                                        })
                                        .collect::<Vec<_>>()
                                        .join("\n")
                                }
                            };
                            trace.push(ChatMessage::User {
                                content: ChatMessageContent::Text(text),
                                name: None,
                            });
                        }
                        Role::Assistant => {
                            let text = match &msg.content {
                                ContentInput::Text(t) => t.clone(),
                                ContentInput::List(items) => {
                                    items
                                        .iter()
                                        .filter_map(|item| {
                                            if let ContentItem::Text { text } = item {
                                                Some(text.clone())
                                            } else {
                                                None
                                            }
                                        })
                                        .collect::<Vec<_>>()
                                        .join("\n")
                                }
                            };
                            trace.push(ChatMessage::Assistant {
                                content: Some(ChatMessageContent::Text(text)),
                                tool_calls: None,
                                name: None,
                                audio: None,
                                reasoning: None,
                                reasoning_content: None,
                                refusal: None,
                            });
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    trace
}