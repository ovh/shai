use axum::{
    extract::State,
    response::{IntoResponse, Response, Sse, Json},
};
use futures::StreamExt;
use openai_dive::v1::resources::chat::{
    ChatCompletionParameters, ChatCompletionResponse, ChatCompletionChoice,
    ChatMessage, ChatMessageContent,
};
use openai_dive::v1::resources::shared::{Usage, FinishReason};
use shai_core::agent::AgentEvent;
use tokio_stream::wrappers::BroadcastStream;
use tracing::info;
use uuid::Uuid;

use super::formatter::ChatCompletionFormatter;
use crate::{ApiJson, ServerState, ErrorResponse, session_to_sse_stream};

/// Handle OpenAI chat completion - supports both streaming and non-streaming
pub async fn handle_chat_completion(
    State(state): State<ServerState>,
    ApiJson(payload): ApiJson<ChatCompletionParameters>,
) -> Result<Response, ErrorResponse> {
    let request_id = Uuid::new_v4();
    let session_id = Uuid::new_v4().to_string();

    let is_streaming = payload.stream.unwrap_or(false);
    info!("[{}] POST /v1/chat/completions model={} stream={} (ephemeral)",
        request_id, payload.model, is_streaming);

    // Check if streaming is requested
    if is_streaming {
        handle_chat_completion_stream(state, payload, request_id, session_id).await
    } else {
        handle_chat_completion_non_stream(state, payload, request_id, session_id).await
    }
}

/// Handle streaming chat completion
async fn handle_chat_completion_stream(
    state: ServerState,
    payload: ChatCompletionParameters,
    request_id: Uuid,
    session_id: String,
) -> Result<Response, ErrorResponse> {
    let trace = build_message_trace(&payload);
    let model = payload.model.clone();

    // Create ephemeral session
    let agent_session = state.session_manager
        .create_new_session(&request_id.to_string(), &session_id, Some(model.clone()), true)
        .await
        .map_err(|e| ErrorResponse::internal_error(format!("Failed to create session: {}", e)))?;

    // Create request session
    let request_session = agent_session
        .handle_request(&request_id.to_string(), trace)
        .await
        .map_err(|e| ErrorResponse::internal_error(format!("Failed to handle request: {}", e)))?;

    // Create the formatter for OpenAI Chat Completion API
    let formatter = ChatCompletionFormatter::new(model);

    // Create SSE stream
    let stream = session_to_sse_stream(request_session, formatter, session_id, true);

    Ok(Sse::new(stream).into_response())
}

/// Handle non-streaming chat completion
/// Directly processes events and returns a single complete response
async fn handle_chat_completion_non_stream(
    state: ServerState,
    payload: ChatCompletionParameters,
    request_id: Uuid,
    session_id: String,
) -> Result<Response, ErrorResponse> {
    let trace = build_message_trace(&payload);

    // Create ephemeral session
    let agent_session = state.session_manager
        .create_new_session(&request_id.to_string(), &session_id, Some(payload.model.clone()), true)
        .await
        .map_err(|e| ErrorResponse::internal_error(format!("Failed to create session: {}", e)))?;

    // Send messages and get event stream
    let request_session = agent_session
        .handle_request(&request_id.to_string(), trace)
        .await
        .map_err(|e| ErrorResponse::internal_error(format!("Failed to handle request: {}", e)))?;

    // Collect events - accumulate both content and reasoning (tool calls)
    let mut event_stream = BroadcastStream::new(request_session.event_rx);
    let mut final_message = String::new();
    let mut reasoning_steps = Vec::new();

    while let Some(result) = event_stream.next().await {
        match result {
            Ok(event) => {
                // Check if this is a terminal event
                let is_terminal = matches!(
                    event,
                    AgentEvent::Completed { .. }
                        | AgentEvent::StatusChanged {
                            new_status: shai_core::agent::PublicAgentState::Paused,
                            ..
                        }
                );

                match event {
                    AgentEvent::Completed { message, .. } => {
                        final_message = message;
                    }
                    AgentEvent::BrainResult { thought, .. } => {
                        if let Ok(msg) = thought {
                            if let ChatMessage::Assistant {
                                content: Some(ChatMessageContent::Text(text)),
                                ..
                            } = msg
                            {
                                final_message = text;
                            }
                        }
                    }
                    AgentEvent::ToolCallStarted { call, .. } => {
                        reasoning_steps.push(format!("[toolcall: {}]", call.tool_name));
                    }
                    AgentEvent::ToolCallCompleted { call, result: tool_result, .. } => {
                        use shai_core::tools::ToolResult;
                        let step = match &tool_result {
                            ToolResult::Success { .. } => format!("[tool succeeded: {}]", call.tool_name),
                            ToolResult::Error { error, .. } => {
                                let error_oneline = error.lines().next().unwrap_or(error);
                                format!("[tool failed: {} - {}]", call.tool_name, error_oneline)
                            }
                            ToolResult::Denied => format!("[tool denied: {}]", call.tool_name),
                        };
                        reasoning_steps.push(step);
                    }
                    _ => {}
                }

                if is_terminal {
                    break;
                }
            }
            Err(e) => {
                return Err(ErrorResponse::internal_error(format!("Event stream error: {}", e)));
            }
        }
    }

    // Build OpenAI-compatible response
    let response = ChatCompletionResponse {
        id: Some(format!("chatcmpl-{}", Uuid::new_v4())),
        object: "chat.completion".to_string(),
        created: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as u32,
        model: payload.model.clone(),
        choices: vec![ChatCompletionChoice {
            index: 0,
            message: ChatMessage::Assistant {
                content: Some(ChatMessageContent::Text(final_message)),
                name: None,
                tool_calls: None,
                audio: None,
                reasoning: None,
                reasoning_content: if reasoning_steps.is_empty() {
                    None
                } else {
                    Some(reasoning_steps.join("\n"))
                },
                refusal: None,
            },
            finish_reason: Some(FinishReason::StopSequenceReached),
            logprobs: None,
        }],
        usage: Some(Usage {
            prompt_tokens: Some(0),
            completion_tokens: Some(0),
            total_tokens: 0,
            completion_tokens_details: None,
            prompt_tokens_details: None,
        }),
        system_fingerprint: None,
        service_tier: None,
    };

    Ok(Json(response).into_response())
}

/// Build message trace from OpenAI chat completion parameters
fn build_message_trace(params: &ChatCompletionParameters) -> Vec<ChatMessage> {
    let mut trace = Vec::new();

    for msg in &params.messages {
        match msg {
            ChatMessage::System { content, name } => {
                if let ChatMessageContent::Text(text) = content {
                    trace.push(ChatMessage::System {
                        content: ChatMessageContent::Text(text.clone()),
                        name: name.clone(),
                    });
                }
            }
            ChatMessage::User { content, name, .. } => {
                let text = match content {
                    ChatMessageContent::Text(t) => t.clone(),
                    ChatMessageContent::ContentPart(parts) => {
                        parts
                            .iter()
                            .filter_map(|p| match p {
                                openai_dive::v1::resources::chat::ChatMessageContentPart::Text(t) => Some(t.text.as_str()),
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                            .join("\n")
                    }
                    ChatMessageContent::None => String::new(),
                };
                if !text.is_empty() {
                    trace.push(ChatMessage::User {
                        content: ChatMessageContent::Text(text),
                        name: name.clone(),
                    });
                }
            }
            ChatMessage::Assistant { content, name, .. } => {
                if let Some(ChatMessageContent::Text(text)) = content {
                    trace.push(ChatMessage::Assistant {
                        content: Some(ChatMessageContent::Text(text.clone())),
                        tool_calls: None,
                        name: name.clone(),
                        audio: None,
                        reasoning: None,
                        reasoning_content: None,
                        refusal: None,
                    });
                }
            }
            _ => {}
        }
    }

    trace
}
