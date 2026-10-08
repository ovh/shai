use super::brain::ThinkerDecision;
use super::AgentError;
use crate::agent::PublicAgentState;
use crate::tools::{ToolCall, ToolResult};
use async_trait::async_trait;
use chrono::{DateTime, TimeDelta, Utc};
use futures::future::BoxFuture;
use openai_dive::v1::resources::chat::ChatMessage;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::sync::Arc;

/// Internal events for agent state machine communication
/// These events are used internally between agent components and state handlers
#[derive(Debug, Clone)]
pub enum InternalAgentEvent {
    /// Agent initialization completed
    AgentInitialized,
    /// Request to start thinking operation
    CancelTask,
    /// Request to start thinking operation
    ThinkingStart,
    /// Brain completed and returned a result for the next step
    BrainResult {
        result: Result<ThinkerDecision, AgentError>,
    },
    /// Agent started executing a tool
    ToolCallStarted {
        timestamp: DateTime<Utc>,
        call: ToolCall,
    },
    /// Tool execution completed and returned a result
    ToolCallCompleted {
        duration: TimeDelta,
        call: ToolCall,
        result: ToolResult,
    },
    /// All tools completed execution
    ToolsCompleted { any_denied: bool },
    /// User response received from controller
    UserResponseReceived {
        request_id: String,
        response: UserResponse,
    },
    /// Permission response received from controller
    PermissionResponseReceived {
        request_id: String,
        response: PermissionResponse,
    },
}

/// Public events emitted to external controllers/UI
/// These events are what external consumers receive and can respond to
#[derive(Clone)]
pub enum AgentEvent {
    /// Agent status has changed
    StatusChanged {
        old_status: PublicAgentState,
        new_status: PublicAgentState,
    },
    /// Thinking Start
    ThinkingStart,
    /// Agent is thinking - provides the thought content to display to user
    BrainResult {
        timestamp: DateTime<Utc>,
        thought: Result<ChatMessage, AgentError>,
    },
    /// Agent started executing a tool
    ToolCallStarted {
        timestamp: DateTime<Utc>,
        call: ToolCall,
    },
    /// Tool execution completed and returned a result
    ToolCallCompleted {
        duration: TimeDelta,
        call: ToolCall,
        result: ToolResult,
        original_bytes: usize,
        compacted_bytes: usize,
    },
    /// User provided input to the agent
    UserInput { input: String },
    /// Agent requires user input to continue
    UserInputRequired {
        request_id: String,
        request: UserRequest,
    },
    /// Agent requires permission to perform an action
    PermissionRequired {
        request_id: String,
        request: PermissionRequest,
    },
    /// Agent encountered an error
    Error { error: String },
    /// Agent execution completed
    Completed { success: bool, message: String },
    /// Token usage information from LLM response
    TokenUsage {
        input_tokens: u32,
        output_tokens: u32,
        cached_tokens: u32,
    },
    /// Todo list was updated
    TodoUpdated {
        todos: Vec<crate::tools::todo::TodoItem>,
    },
}

/// Types of user input that an agent can request
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum UserRequest {
    /// Free text input
    Text { prompt: String },
    /// Multiple choice selection
    Choice {
        prompt: String,
        options: Vec<String>,
    },
    /// Yes/No confirmation
    Confirmation { prompt: String },
}

/// User's response to an input request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UserResponse {
    /// Text input response
    Text(String),
    /// Choice selection (index into options)
    Choice(usize),
    /// Confirmation response
    Confirmation(bool),
    /// User cancelled the input
    Cancel,
    /// No user available (agent is not interactive)
    NoUser,
}

/// Request for permission to perform an action
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PermissionRequest {
    /// Name of the tool requesting permission
    pub tool_name: String,
    /// Description of the operation
    pub operation: String,
    /// Additional details about the request
    pub call: ToolCall,
    /// Preview of what the tool would do (if available)
    pub preview: Option<ToolResult>,
}

/// Response to a permission request
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PermissionResponse {
    /// Allow this specific operation
    Allow,
    /// Allow this type of operation always
    AllowAlways,
    /// Operation Forbidden
    Forbidden,
    /// Operation was denied
    Deny,
    /// No permission system available (auto-deny for safety)
    NoPermissionSystem,
}

/// Type alias for event handlers (for backwards compatibility)
pub type DynEventHandler = Arc<dyn Fn(AgentEvent) -> BoxFuture<'static, ()> + Send + Sync>;

#[async_trait]
pub trait AgentEventHandler: Send + Sync {
    async fn handle_event(&self, event: AgentEvent);
}

// Generic adapter to allow closures to implement AgentEventHandler
pub struct ClosureHandler<F>
where
    F: Fn(AgentEvent) -> BoxFuture<'static, ()> + Send + Sync + 'static,
{
    handler: F,
}

impl<F> ClosureHandler<F>
where
    F: Fn(AgentEvent) -> BoxFuture<'static, ()> + Send + Sync + 'static,
{
    pub fn new(handler: F) -> Self {
        Self { handler }
    }
}

#[async_trait]
impl<F> AgentEventHandler for ClosureHandler<F>
where
    F: Fn(AgentEvent) -> BoxFuture<'static, ()> + Send + Sync + 'static,
{
    async fn handle_event(&self, event: AgentEvent) {
        (self.handler)(event).await;
    }
}

// Helper function to convert Fn(...) -> impl Future into BoxFuture
pub fn closure_handler<F, Fut>(
    handler: F,
) -> ClosureHandler<impl Fn(AgentEvent) -> BoxFuture<'static, ()> + Send + Sync + 'static>
where
    F: Fn(AgentEvent) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    ClosureHandler::new(move |event: AgentEvent| Box::pin(handler(event)))
}

impl std::fmt::Debug for AgentEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentEvent::StatusChanged {
                old_status,
                new_status,
            } => f
                .debug_struct("StatusChanged")
                .field("old_status", old_status)
                .field("new_status", new_status)
                .finish(),
            AgentEvent::ThinkingStart => f.debug_struct("ThinkingStart").finish(),
            AgentEvent::BrainResult { timestamp, thought } => f
                .debug_struct("BrainResult")
                .field("timestamp", timestamp)
                .field("thought", thought)
                .finish(),
            AgentEvent::ToolCallStarted { timestamp, call } => f
                .debug_struct("ToolCallStarted")
                .field("timestamp", timestamp)
                .field("call", call)
                .finish(),
            AgentEvent::ToolCallCompleted {
                duration,
                call,
                result,
                original_bytes,
                compacted_bytes,
            } => f
                .debug_struct("ToolCallCompleted")
                .field("timestamp", duration)
                .field("call", call)
                .field("result", result)
                .field("original_bytes", original_bytes)
                .field("compacted_bytes", compacted_bytes)
                .finish(),
            AgentEvent::UserInput { input } => {
                f.debug_struct("UserInput").field("input", input).finish()
            }
            AgentEvent::UserInputRequired {
                request_id: input_id,
                request: input_type,
                ..
            } => {
                f.debug_struct("UserInputRequired")
                    .field("input_id", input_id)
                    .field("input_type", input_type)
                    //.field("response_channel", &"<oneshot::Sender>")
                    .finish()
            }
            AgentEvent::PermissionRequired {
                request_id,
                request,
                ..
            } => {
                f.debug_struct("PermissionRequired")
                    .field("request_id", request_id)
                    .field("request", request)
                    //.field("response_channel", &"<oneshot::Sender>")
                    .finish()
            }
            AgentEvent::Error { error } => f.debug_struct("Error").field("error", error).finish(),
            AgentEvent::Completed { success, message } => f
                .debug_struct("Completed")
                .field("success", success)
                .field("message", message)
                .finish(),
            AgentEvent::TokenUsage {
                input_tokens,
                output_tokens,
                cached_tokens,
            } => f
                .debug_struct("TokenUsage")
                .field("input_tokens", input_tokens)
                .field("output_tokens", output_tokens)
                .field("cached_tokens", cached_tokens)
                .finish(),
            AgentEvent::TodoUpdated { todos } => {
                f.debug_struct("TodoUpdated").field("todos", todos).finish()
            }
        }
    }
}
