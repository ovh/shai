use crate::session::logger::colored_session_id;
use openai_dive::v1::resources::chat::ChatMessage;
use shai_core::agent::{AgentController, AgentError, AgentEvent};
use std::sync::Arc;
use tokio::sync::{broadcast::Receiver, Mutex};
use tokio::task::JoinHandle;
use tracing::info;

use super::RequestLifecycle;

/// Represents a single HTTP request session with automatic lifecycle management
pub struct RequestSession {
    pub controller: AgentController,
    pub event_rx: Receiver<AgentEvent>,
    pub lifecycle: RequestLifecycle,
}

/// A single agent session - represents one running agent instance
/// Can be ephemeral (destroyed after request) or persistent (kept alive)
/// Each request holds a guard against the controller so that only one query is processed per session
/// - In background mode (ephemeral=false), the session survives the request and the guard is simply drop
/// - In ephemeral mode (ephemeral=true), the entire session stops and is deleted once the query ends or the client disconnect
pub struct AgentSession {
    controller: Arc<Mutex<AgentController>>,
    event_rx: Receiver<AgentEvent>,
    logging_task: JoinHandle<()>,
    agent_task: JoinHandle<()>,

    pub session_id: String,
    pub agent_name: String,
    pub ephemeral: bool,
}

impl AgentSession {
    pub fn new(
        session_id: String,
        controller: AgentController,
        event_rx: Receiver<AgentEvent>,
        agent_task: JoinHandle<()>,
        logging_task: JoinHandle<()>,
        agent_name: Option<String>,
        ephemeral: bool,
    ) -> Self {
        let agent_name_display = agent_name.unwrap_or_else(|| "default".to_string());

        Self {
            controller: Arc::new(Mutex::new(controller)),
            event_rx,
            logging_task,
            agent_task,
            session_id,
            agent_name: agent_name_display,
            ephemeral,
        }
    }

    /// Terminate a session
    pub async fn cancel(&self, http_request_id: &String) -> Result<(), AgentError> {
        let ctrl = self.controller.clone().lock_owned().await;
        info!(
            "[{}] - {} cancelling session",
            http_request_id,
            colored_session_id(&self.session_id)
        );
        ctrl.terminate().await
    }

    /// Subscribe to events from this session (read-only, non-blocking)
    /// Used for GET /v1/responses/{response_id} to observe an ongoing session
    pub fn watch(&self) -> Receiver<AgentEvent> {
        self.event_rx.resubscribe()
    }

    /// Handle a request for this agent session
    /// Returns a RequestSession that manages the lifecycle
    pub async fn handle_request(
        &self,
        http_request_id: &String,
        trace: Vec<ChatMessage>,
    ) -> Result<RequestSession, AgentError> {
        let controller_guard = self.controller.clone().lock_owned().await;
        controller_guard.wait_turn(None).await?;
        info!(
            "[{}] - {} handling request",
            http_request_id,
            colored_session_id(&self.session_id)
        );

        controller_guard.send_trace(trace).await?;

        let event_rx = self.event_rx.resubscribe();
        let controller = controller_guard.clone();
        let lifecycle = RequestLifecycle::new(
            self.ephemeral,
            controller_guard,
            http_request_id.clone(),
            self.session_id.clone(),
        );

        Ok(RequestSession {
            controller,
            event_rx,
            lifecycle,
        })
    }

    pub fn is_ephemeral(&self) -> bool {
        self.ephemeral
    }
}

impl Drop for AgentSession {
    fn drop(&mut self) {
        self.agent_task.abort();
        self.logging_task.abort();
    }
}
