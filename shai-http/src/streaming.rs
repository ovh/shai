use async_trait::async_trait;
use axum::response::sse::Event;
use futures::stream::{Stream, StreamExt};
use serde::Serialize;
use shai_core::agent::{AgentEvent, PublicAgentState};
use std::convert::Infallible;
use tokio::sync::broadcast::Receiver;
use tokio_stream::wrappers::BroadcastStream;
use tracing::error;

use crate::session::RequestSession;

/// Trait for formatting AgentEvents into API-specific response formats
#[async_trait]
pub trait EventFormatter: Send {
    type Output: Serialize + Send;

    /// Convert an AgentEvent to API-specific format
    /// Returns None if the event should be filtered out
    async fn format_event(&mut self, event: AgentEvent, session_id: &str) -> Option<Self::Output>;

    /// Get the SSE event name for this output
    /// Default is "message"
    fn event_name(&self, _output: &Self::Output) -> &str {
        "message"
    }
}

/// Internal helper to create SSE stream with optional lifecycle
fn sse_stream_internal<F, L>(
    event_rx: Receiver<AgentEvent>,
    formatter: F,
    session_id: String,
    lifecycle: Option<L>,
    stop_on_pause: bool,
) -> impl Stream<Item = Result<Event, Infallible>>
where
    F: EventFormatter + 'static,
    L: Send + 'static,
{
    futures::stream::unfold(
        (BroadcastStream::new(event_rx), formatter, false, lifecycle),
        move |state| {
            let session_id = session_id.clone();
            async move {
                let (mut rx, mut fmt, done, lifecycle) = state;

                if done {
                    return None;
                }

                loop {
                    match rx.next().await {
                        Some(Ok(event)) => {
                            let is_terminal = is_terminal_event(&event, stop_on_pause);
                            let formatted = fmt.format_event(event, &session_id).await;
                            let new_done = if is_terminal { true } else { done };

                            if let Some(output) = formatted {
                                match serde_json::to_string(&output) {
                                    Ok(json) => {
                                        let sse_event = Event::default().data(json);
                                        return Some((
                                            Ok(sse_event),
                                            (rx, fmt, new_done, lifecycle),
                                        ));
                                    }
                                    Err(e) => {
                                        error!("[{}] Failed to serialize event: {}", session_id, e);
                                        continue;
                                    }
                                }
                            } else {
                                if new_done {
                                    return None;
                                }
                                continue;
                            }
                        }
                        Some(Err(e)) => {
                            error!("[{}] Error receiving event: {}", session_id, e);
                            return None;
                        }
                        None => {
                            return None;
                        }
                    }
                }
            }
        },
    )
}

/// Core SSE stream creation from event receiver
/// Watches events, formats them, and stops on completion or client disconnect
///
/// # Parameters
/// * `stop_on_pause` - If true, only stops on Completed. If false, stops on Completed or StatusChanged to Paused.
pub fn event_to_sse_stream<F>(
    event_rx: Receiver<AgentEvent>,
    formatter: F,
    session_id: String,
    stop_on_pause: bool,
) -> impl Stream<Item = Result<Event, Infallible>>
where
    F: EventFormatter + 'static,
{
    sse_stream_internal(event_rx, formatter, session_id, None::<()>, stop_on_pause)
}

/// Create an SSE stream from a RequestSession
/// Same as sse_stream but keeps lifecycle in scope for session cleanup
///
/// # Parameters
/// * `stop_on_pause` - If true, only stops on Completed. If false, stops on Completed or StatusChanged to Paused.
pub fn session_to_sse_stream<F>(
    request_session: RequestSession,
    formatter: F,
    session_id: String,
    stop_on_pause: bool,
) -> impl Stream<Item = Result<Event, Infallible>>
where
    F: EventFormatter + 'static,
{
    let event_rx = request_session.event_rx;
    let _controller = request_session.controller;
    let lifecycle = request_session.lifecycle;

    sse_stream_internal(
        event_rx,
        formatter,
        session_id,
        Some(lifecycle),
        stop_on_pause,
    )
}

/// Check if an event signals the end of the stream
///
/// # Parameters
/// * `stop_on_pause` - If true, only Completed is terminal. If false, both Completed and Paused are terminal.
fn is_terminal_event(event: &AgentEvent, stop_on_pause: bool) -> bool {
    match event {
        AgentEvent::Completed { .. } => true,
        AgentEvent::StatusChanged {
            new_status: PublicAgentState::Paused,
            ..
        } => stop_on_pause,
        _ => false,
    }
}
