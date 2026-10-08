use chrono::{DateTime, Utc};
use tokio_util::sync::CancellationToken;

/// Internal agent status (contains channels and sync primitives)
#[derive(Debug)]
pub enum InternalAgentState {
    /// Agent is starting up
    Starting,
    /// Agent is actively running,
    Running,
    /// Executing, might be doing multiple things at once
    Processing {
        task_name: String,
        tools_exec_at: DateTime<Utc>,
        cancellation_token: CancellationToken,
    },
    /// Agent execution is paused
    Paused,
    /// Agent completed successfully
    Completed { success: bool },
    /// Agent failed with error
    Failed { error: String },
}

/// Public agent status (clean version without internal channels/sync primitives)
#[derive(Debug, Clone)]
pub enum PublicAgentState {
    /// Agent is starting up
    Starting,
    /// Agent is actively running
    Running,
    /// Agent is thinking
    Processing {
        task_name: String,
        tools_exec_at: DateTime<Utc>,
    },
    /// Agent execution is paused
    Paused,
    /// Agent completed successfully
    Completed { success: bool },
    /// Agent was cancelled
    Cancelled,
    /// Agent failed with error
    Failed { error: String },
}

impl std::fmt::Display for PublicAgentState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PublicAgentState::Starting => write!(f, "starting"),
            PublicAgentState::Running => write!(f, "running"),
            PublicAgentState::Processing { .. } => write!(f, "processing"),
            PublicAgentState::Paused => write!(f, "paused"),
            PublicAgentState::Completed { success } => {
                write!(f, "{}", if *success { "completed" } else { "failed" })
            }
            PublicAgentState::Cancelled => write!(f, "cancelled"),
            PublicAgentState::Failed { .. } => write!(f, "failed"),
        }
    }
}

impl InternalAgentState {
    /// Convert internal status to public status (removing channels and sync primitives)
    pub fn to_public(&self) -> PublicAgentState {
        match self {
            InternalAgentState::Starting => PublicAgentState::Starting,
            InternalAgentState::Running => PublicAgentState::Running,
            InternalAgentState::Processing {
                task_name,
                tools_exec_at,
                ..
            } => PublicAgentState::Processing {
                task_name: task_name.clone(),
                tools_exec_at: *tools_exec_at,
            },
            InternalAgentState::Paused => PublicAgentState::Paused,
            InternalAgentState::Completed { success } => {
                PublicAgentState::Completed { success: *success }
            }
            InternalAgentState::Failed { error } => PublicAgentState::Failed {
                error: error.clone(),
            },
        }
    }
}
