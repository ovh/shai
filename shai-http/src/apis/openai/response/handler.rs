use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response, Sse},
    Json,
};
use openai_dive::v1::resources::response::request::ResponseParameters;
use tracing::info;
use uuid::Uuid;

use super::formatter::ResponseFormatter;
use super::types::build_message_trace;
use crate::{event_to_sse_stream, session_to_sse_stream, ApiJson, ErrorResponse, ServerState};

/// POST /v1/responses - Create a model response
/// Supports both stateful (store=true, previous_response_id) and stateless (store=false) modes
pub async fn handle_response(
    State(state): State<ServerState>,
    ApiJson(payload): ApiJson<ResponseParameters>,
) -> Result<Response, ErrorResponse> {
    let request_id = Uuid::new_v4();
    let store = payload.store.unwrap_or(true);
    let session_id = payload
        .previous_response_id
        .clone()
        .unwrap_or_else(|| format!("resp_{}", Uuid::new_v4()));

    info!(
        "[{}] POST /v1/responses session={} store={} stream={}",
        request_id,
        session_id,
        store,
        payload.stream.unwrap_or(false)
    );

    // Check if streaming is requested
    if payload.stream.unwrap_or(false) {
        handle_response_stream(state, payload, request_id, session_id, !store).await
    } else {
        handle_response_non_stream(state, payload, request_id, session_id, !store).await
    }
}

/// Handle streaming response
async fn handle_response_stream(
    state: ServerState,
    payload: ResponseParameters,
    request_id: Uuid,
    session_id: String,
    is_ephemeral: bool,
) -> Result<Response, ErrorResponse> {
    let trace = build_message_trace(&payload);
    let model = payload.model.clone();

    // Get or create session agent based on whether previous_response_id was provided
    let agent_session = if payload.previous_response_id.is_some() {
        // previous_response_id provided -> must exist (in memory or disk), error if not
        state
            .session_manager
            .get_session(&request_id.to_string(), &session_id, model.clone())
            .await
            .map_err(|e| {
                ErrorResponse::invalid_request(format!("Previous response not found: {}", e))
            })?
    } else {
        // No previous_response_id -> create new session
        state
            .session_manager
            .create_new_session(
                &request_id.to_string(),
                &session_id,
                Some(model.clone()),
                is_ephemeral,
            )
            .await
            .map_err(|e| {
                ErrorResponse::internal_error(format!("Failed to create session: {}", e))
            })?
    };

    // Create request session
    let request_session = agent_session
        .handle_request(&request_id.to_string(), trace)
        .await
        .map_err(|e| ErrorResponse::internal_error(format!("Failed to handle request: {}", e)))?;

    // Create the formatter for OpenAI Response API
    let formatter = ResponseFormatter::new(model, payload);

    // Create SSE stream
    let stream = session_to_sse_stream(request_session, formatter, session_id, true);

    Ok(Sse::new(stream).into_response())
}

/// Handle non-streaming response
async fn handle_response_non_stream(
    _state: ServerState,
    _payload: ResponseParameters,
    _request_id: Uuid,
    _session_id: String,
    _is_ephemeral: bool,
) -> Result<Response, ErrorResponse> {
    Err(ErrorResponse::internal_error(
        "Response API (non-stream) not yet implemented".to_string(),
    ))
}

/// GET /v1/responses/{response_id} - Retrieve a model response
/// Read-only access to an ongoing or completed session
pub async fn handle_get_response(
    State(state): State<ServerState>,
    Path(response_id): Path<String>,
) -> Result<Response, ErrorResponse> {
    let request_id = Uuid::new_v4();
    info!("[{}] GET /v1/responses/{}", request_id, response_id);

    // Get the existing session (note: without agent_name, will only check memory, not disk)
    // For GET we don't have the model from request, so we use the session's agent_name
    // This means GET can only access in-memory sessions
    let agent_session = state
        .session_manager
        .get_session(&request_id.to_string(), &response_id, "default".to_string())
        .await
        .map_err(|e| ErrorResponse::invalid_request(format!("Response not found: {}", e)))?;

    // Subscribe to events (non-blocking, read-only)
    let event_rx = agent_session.watch();

    // Create a minimal payload for the formatter
    let placeholder_payload = ResponseParameters {
        model: agent_session.agent_name.clone(),
        stream: Some(true),
        ..Default::default()
    };

    // Create the formatter
    let formatter = ResponseFormatter::new(agent_session.agent_name.clone(), placeholder_payload);

    // Create SSE stream using the simple sse_stream (no lifecycle needed for read-only)
    // stop_on_pause = false means stream stops on Completed OR Paused
    let stream = event_to_sse_stream(event_rx, formatter, response_id, false);

    Ok(Sse::new(stream).into_response())
}

/// POST /v1/responses/{response_id}/cancel - Cancel a model response
pub async fn handle_cancel_response(
    State(state): State<ServerState>,
    Path(response_id): Path<String>,
) -> Result<Response, ErrorResponse> {
    let request_id = Uuid::new_v4();
    info!("[{}] POST /v1/responses/{}/cancel", request_id, response_id);

    // Cancel the session
    state
        .session_manager
        .cancel_session(&request_id.to_string(), &response_id)
        .await
        .map_err(|e| ErrorResponse::internal_error(format!("Failed to cancel session: {}", e)))?;

    // Return success response
    Ok(Json(serde_json::json!({
        "id": response_id,
        "object": "response",
        "status": "cancelled"
    }))
    .into_response())
}
