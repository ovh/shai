use shai_core::agent::AgentController;
use tokio::sync::OwnedMutexGuard;
use tracing::{info, warn};

use crate::session::logger::colored_session_id;
use shai_core::session::SessionPersist;

pub enum RequestLifecycle {
    Background {
        controller_guard: OwnedMutexGuard<AgentController>,
        request_id: String,
        session_id: String,
    },
    Ephemeral {
        controller_guard: OwnedMutexGuard<AgentController>,
        request_id: String,
        session_id: String,
    },
}

impl RequestLifecycle {
    pub fn new(
        ephemeral: bool,
        controller_guard: OwnedMutexGuard<AgentController>,
        request_id: String,
        session_id: String,
    ) -> Self {
        match ephemeral {
            true => Self::Ephemeral {
                controller_guard,
                request_id,
                session_id,
            },
            false => Self::Background {
                controller_guard,
                request_id,
                session_id,
            },
        }
    }
}

impl Drop for RequestLifecycle {
    fn drop(&mut self) {
        match self {
            Self::Background {
                controller_guard,
                request_id,
                session_id,
            } => {
                info!(
                    "[{}] - {} Stream completed, releasing controller lock (background session)",
                    request_id,
                    colored_session_id(session_id)
                );

                // Save session to disk (async)
                let ctrl = controller_guard.clone();
                let sid = session_id.clone();
                tokio::spawn(async move {
                    match ctrl.get_trace().await {
                        Ok(trace) => {
                            if let Err(e) = SessionPersist::save_session(&sid, trace) {
                                warn!("Failed to save session {}: {}", sid, e);
                            }
                        }
                        Err(e) => {
                            warn!("Failed to get trace for session {}: {}", sid, e);
                        }
                    }
                });
            }
            Self::Ephemeral {
                controller_guard,
                request_id,
                session_id,
            } => {
                info!(
                    "[{}] - {} Stream completed, destroying agent (ephemeral session)",
                    request_id,
                    colored_session_id(session_id)
                );

                // Clone before moving into async task
                let ctrl = controller_guard.clone();
                let sid = session_id.clone();
                tokio::spawn(async move {
                    // Save session to disk
                    match ctrl.get_trace().await {
                        Ok(trace) => {
                            if let Err(e) = SessionPersist::save_session(&sid, trace) {
                                warn!("Failed to save session {}: {}", sid, e);
                            }
                        }
                        Err(e) => {
                            warn!("Failed to get trace for session {}: {}", sid, e);
                        }
                    }

                    // Terminate the agent
                    let _ = ctrl.terminate().await;
                });
            }
        }
    }
}
