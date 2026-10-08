pub mod actions;
pub mod agent;
pub mod brain;
pub mod builder;
pub mod claims;
pub mod error;
pub mod events;
pub mod output;
pub mod protocol;
pub mod states;

#[cfg(test)]
mod tests;

pub use agent::{Agent, AgentCore, AgentResult, TaskAgentResponse};
pub use agent::{ToolBudget, ToolCache, ToolCallInfo};
pub use states::{InternalAgentState, PublicAgentState};

pub use protocol::{AgentController, AgentRequest, AgentResponse};

pub use events::{
    closure_handler, AgentEvent, AgentEventHandler, ClosureHandler, DynEventHandler,
    InternalAgentEvent, PermissionRequest, PermissionResponse, UserRequest, UserResponse,
};
pub use output::StdoutEventManager;

pub use crate::logging::LoggingConfig;
pub use brain::{Brain, ThinkerContext, ThinkerDecision, ThinkerFlowControl, ToolBudgetRef};
pub use builder::AgentBuilder;
pub use claims::{ClaimManager, PermissionError};
pub use error::{AgentError, AgentExecutionError};
