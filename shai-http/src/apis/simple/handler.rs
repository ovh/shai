use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response, Sse},
};
use openai_dive::v1::resources::chat::{ChatMessage, ChatMessageContent, ToolCall as LlmToolCall, Function};
use tracing::info;
use uuid::Uuid;

use super::types::{MultiModalQuery, Message};
use super::formatter::SimpleFormatter;
use crate::{session_to_sse_stream, ApiJson, ErrorResponse, ServerState};

/// Handle multimodal query without explicit session id (ephemeral session)
pub async fn handle_multimodal_query_stream(
    State(state): State<ServerState>,
    ApiJson(payload): ApiJson<MultiModalQuery>,
) -> Result<Response, ErrorResponse> {
    handle_multimodal_query_stream_internal(state, None, payload).await
}

/// Handle multimodal query with provided session id (persistent session)
pub async fn handle_multimodal_query_stream_with_session(
    State(state): State<ServerState>,
    Path(session_id): Path<String>,
    ApiJson(payload): ApiJson<MultiModalQuery>,
) -> Result<Response, ErrorResponse> {
    handle_multimodal_query_stream_internal(state, Some(session_id), payload).await
}

/// Shared implementation for multimodal query handlers
async fn handle_multimodal_query_stream_internal(
    state: ServerState,
    session_id_param: Option<String>,
    payload: MultiModalQuery,
) -> Result<Response, ErrorResponse> {
    let request_id = Uuid::new_v4();

    // Determine session_id: use provided, or generate ephemeral
    let is_ephemeral = session_id_param.is_none();
    let session_id = session_id_param
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    info!(
        "[{}] POST /v1/multimodal/{} model={} ephemeral={}",
        request_id, session_id, payload.model, is_ephemeral
    );

    // Build trace from query
    let trace = build_message_trace(&payload);

    // Get or create session agent
    let agent_session = if is_ephemeral {
        // Ephemeral -> create new session
        state.session_manager
            .create_new_session(&request_id.to_string(), &session_id, Some(payload.model.clone()), is_ephemeral)
            .await
            .map_err(|e| ErrorResponse::internal_error(format!("Failed to create session: {}", e)))?
    } else {
        // Persistent -> get existing (from memory or disk) or create new
        match state.session_manager.get_session(&request_id.to_string(), &session_id, payload.model.clone()).await {
            Ok(session) => session,
            Err(_) => {
                // Doesn't exist in memory or disk, create it
                state.session_manager
                    .create_new_session(&request_id.to_string(), &session_id, Some(payload.model.clone()), is_ephemeral)
                    .await
                    .map_err(|e| ErrorResponse::internal_error(format!("Failed to create session: {}", e)))?
            }
        }
    };

    // Create request session
    let request_session = agent_session
        .handle_request(&request_id.to_string(), trace)
        .await
        .map_err(|e| ErrorResponse::internal_error(format!("Failed to handle request: {}", e)))?;

    // Create the formatter for Simple Multimodal API
    let formatter = SimpleFormatter::new(payload.model.clone());

    // Create SSE stream
    let stream = session_to_sse_stream(request_session, formatter, session_id, true);

    Ok(Sse::new(stream).into_response())
}


/// Build message trace from query
fn build_message_trace(query: &MultiModalQuery) -> Vec<ChatMessage> {
    let mut trace = Vec::new();

    if let Some(messages) = &query.messages {
        for msg in messages.iter() {
            match msg {
                Message::User(user_msg) => {
                    trace.push(ChatMessage::User {
                        content: ChatMessageContent::Text(user_msg.message.clone()),
                        name: None,
                    });
                }
                Message::Assistant(assistant_msg) => {
                    trace.push(ChatMessage::Assistant {
                        content: Some(ChatMessageContent::Text(assistant_msg.assistant.clone())),
                        tool_calls: None,
                        name: None,
                        audio: None,
                        reasoning: None,
                        reasoning_content: None,
                        refusal: None,
                    });
                }
                Message::PreviousCall(prev_call) => {
                    // Convert args HashMap back to JSON for parameters
                    let parameters = serde_json::to_value(&prev_call.call.args)
                        .unwrap_or(serde_json::Value::Object(Default::default()));
                    let tool_call_id = format!("call_{}", Uuid::new_v4());

                    // Create the assistant message with tool call
                    trace.push(ChatMessage::Assistant {
                        content: None,
                        tool_calls: Some(vec![LlmToolCall {
                            id: tool_call_id.clone(),
                            r#type: "function".to_string(),
                            function: Function {
                                name: prev_call.call.tool.clone(),
                                arguments: serde_json::to_string(&parameters).unwrap_or_default(),
                            },
                        }]),
                        name: None,
                        audio: None,
                        reasoning: None,
                        reasoning_content: None,
                        refusal: None,
                    });

                    // Create the tool response message
                    let tool_result_text = prev_call
                        .result
                        .text
                        .clone()
                        .or(prev_call.result.error.clone())
                        .unwrap_or_else(|| "No result".to_string());

                    trace.push(ChatMessage::Tool {
                        content: ChatMessageContent::Text(tool_result_text),
                        tool_call_id,
                    });
                }
            }
        }
    }

    trace
}
