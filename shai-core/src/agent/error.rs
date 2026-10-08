use shai_llm::provider::LlmError;
use thiserror::Error;

#[derive(Error, Debug, Clone)]
pub enum AgentError {
    #[error("Agent execution error: {0}")]
    ExecutionError(String),
    #[error("LLM error: {0}")]
    LlmError(String),
    #[error("Tool error: {0}")]
    ToolError(String),
    #[error("Agent session has been closed")]
    SessionClosed,
    #[error("Invalid response: {0}")]
    InvalidResponse(String),
    #[error("User interaction timeout")]
    UserTimeout,
    #[error("Permission denied")]
    PermissionDenied,
    #[error("User input cancelled")]
    UserInputCancelled,
    #[error("Configuration error: {0}")]
    ConfigurationError(String),
    #[error("Agent execution timed out")]
    TimeoutError,
    #[error("Maximum iterations reached")]
    MaxIterationsReached,
    #[error("Invalid state: {0}")]
    InvalidState(String),
    #[error("Invalid state transition: {0}")]
    InvalidStateTransition(String),
}

#[derive(Debug)]
pub enum AgentExecutionError {
    LlmError(LlmError),
    ToolError(String),
    TimeoutError,
    MaxIterationsReached,
    ConfigurationError(String),
}

impl std::fmt::Display for AgentExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentExecutionError::LlmError(e) => write!(f, "LLM error: {}", e),
            AgentExecutionError::ToolError(e) => write!(f, "Tool error: {}", e),
            AgentExecutionError::TimeoutError => write!(f, "Agent execution timed out"),
            AgentExecutionError::MaxIterationsReached => write!(f, "Maximum iterations reached"),
            AgentExecutionError::ConfigurationError(e) => write!(f, "Configuration error: {}", e),
        }
    }
}

impl std::error::Error for AgentExecutionError {}

impl From<LlmError> for AgentExecutionError {
    fn from(error: LlmError) -> Self {
        AgentExecutionError::LlmError(error)
    }
}
