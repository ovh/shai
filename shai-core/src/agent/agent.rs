use crate::agent::ClaimManager;
use crate::tools::fs::operation_log::FsOperationLog;
use crate::tools::AnyTool;
use async_trait::async_trait;
use openai_dive::v1::resources::chat::{ChatMessage, ChatMessageContent};
use serde::{Deserialize, Serialize};
use shai_llm::ToolCallMethod;
use std::boxed::Box;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, oneshot, RwLock};

// Helper functions to make the main loop more readable

use crate::agent::AgentError;
use crate::agent::InternalAgentState;
use crate::agent::{AgentEvent, AgentRequest};
use crate::agent::{Brain, InternalAgentEvent};
use tracing::debug;

use super::protocol::{AgentController, SentCommand};
use super::{AgentEventHandler, AgentResponse};
use crate::config::agent::{CompactionConfig, VerificationConfig};

/// Trait defining the public interface for agents
#[async_trait]
pub trait Agent: Send + Sync {
    /// Start the agent execution (blocking until completion)
    async fn run(&mut self) -> Result<AgentResult, AgentError>;

    /// Get a controller to send commands to the agent
    fn controller(&mut self) -> AgentController;

    /// Get an event watcher to subscribe to agent events
    fn watch(&mut self) -> broadcast::Receiver<AgentEvent>;

    /// List all available tools (name, description) registered with the agent
    fn list_tools(&self) -> Vec<(String, String)>;

    /// Register an event handler closure
    fn on_event<F>(self, handler: F) -> Self
    where
        F: Fn(AgentEvent) + Send + Sync + 'static,
        Self: Sized;

    /// Register an event handler that implements AgentEventHandler
    fn with_event_handler<H>(self, handler: H) -> Self
    where
        H: AgentEventHandler + 'static,
        Self: Sized;
}

/// Result of a completed agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResult {
    pub success: bool,
    pub message: String,
    pub trace: Vec<ChatMessage>,
}

/// Metadata stored for each tool call, used during trace compaction
/// to provide context about what was compacted.
#[derive(Debug, Clone)]
pub struct ToolCallInfo {
    pub tool_name: String,
    pub primary_param: Option<String>,
}

/// Caches tool execution results for duplicate detection and smart compaction.
/// All fields are behind Arc so the struct is cheaply cloneable for sub-agents.
#[derive(Clone)]
pub struct ToolCache {
    /// Cache of recently executed bash commands for duplicate detection
    pub command_cache: Arc<RwLock<Vec<(String, String)>>>,
    /// Cache of recently read files for duplicate detection
    pub read_cache: Arc<RwLock<Vec<(String, String)>>>,
    /// Maps tool_call_id → tool call metadata for smart compaction messages
    pub tool_call_metadata: Arc<RwLock<std::collections::HashMap<String, ToolCallInfo>>>,
    pub max_cached_commands: usize,
    pub max_cached_reads: usize,
}

impl ToolCache {
    pub fn new(compaction_config: &CompactionConfig) -> Self {
        Self {
            command_cache: Arc::new(RwLock::new(Vec::new())),
            read_cache: Arc::new(RwLock::new(Vec::new())),
            tool_call_metadata: Arc::new(RwLock::new(std::collections::HashMap::new())),
            max_cached_commands: compaction_config.max_cached_commands,
            max_cached_reads: compaction_config.max_cached_reads,
        }
    }
}

/// Tracks tool call counts and limits for the agent loop.
/// All fields are behind Arc so the struct is cheaply cloneable for sub-agents.
#[derive(Clone)]
pub struct ToolBudget {
    /// Number of tool calls made in the current turn
    pub count: Arc<RwLock<usize>>,
    /// Maximum tool calls per turn (None = unlimited)
    pub max_calls: Option<usize>,
    /// Soft budget threshold (max_calls / 2). Warnings and critical notices are based on this.
    pub soft_limit: Option<usize>,
}

impl ToolBudget {
    pub fn new(max_calls: Option<usize>) -> Self {
        Self {
            count: Arc::new(RwLock::new(0)),
            max_calls,
            soft_limit: max_calls.map(|m| m / 2),
        }
    }

    pub async fn increment(&self, n: usize) {
        *self.count.write().await += n;
    }

    pub async fn reset(&self) {
        *self.count.write().await = 0;
    }

    pub async fn is_at_limit(&self) -> bool {
        match self.max_calls {
            Some(max) => *self.count.read().await >= max,
            None => false,
        }
    }

    /// Atomically checks the limit and increments by `n` if not at limit.
    /// Returns `false` if at limit (count unchanged), `true` if increment succeeded.
    pub async fn try_increment(&self, n: usize) -> bool {
        let mut count = self.count.write().await;
        if let Some(max) = self.max_calls {
            if *count >= max {
                return false;
            }
        }
        *count += n;
        true
    }
}

/// Bundles shared state passed to tool coroutines.
/// All fields are behind Arc/Vec so the struct is cheaply cloneable for sub-agents.
#[derive(Clone)]
pub struct ToolContext {
    // Communication
    pub public_event_tx: Option<broadcast::Sender<AgentEvent>>,
    pub internal_tx: broadcast::Sender<InternalAgentEvent>,
    // Shared state
    pub available_tools: Vec<Arc<dyn AnyTool>>,
    pub trace: Arc<RwLock<Vec<ChatMessage>>>,
    pub claims: Arc<RwLock<ClaimManager>>,
    pub compaction_config: CompactionConfig,
    pub verification_config: VerificationConfig,
    pub working_dir: Option<String>,
    pub todo_storage: Arc<crate::tools::todo::TodoStorage>,
    pub fs_operation_log: Arc<FsOperationLog>,
    // Caching & budget
    pub tool_cache: ToolCache,
    pub tool_budget: ToolBudget,
}

/// Core agent implementation that orchestrates any Thinker implementation
pub struct AgentCore {
    pub session_id: String,

    /// public controler and event watcher
    pub socket: AgentSocket,

    /// big brain
    pub brain: Arc<RwLock<Box<dyn Brain>>>,
    pub method: ToolCallMethod,
    pub temperature: Arc<RwLock<f32>>,

    pub state: InternalAgentState,

    /// internal event
    pub internal_rx: broadcast::Receiver<InternalAgentEvent>,

    /// shared tool execution context
    pub tool_ctx: ToolContext,
}

pub struct AgentSocket {
    pub tx_command: Option<mpsc::UnboundedSender<SentCommand>>, // might have multiple commander
    pub rx_command: Option<mpsc::UnboundedReceiver<SentCommand>>, // self is single consumer of command from main agent loop
}

impl AgentCore {
    pub fn new(
        session_id: String,
        brain: Box<dyn Brain>,
        trace: Vec<ChatMessage>,
        available_tools: Vec<Box<dyn AnyTool>>,
        permissions: ClaimManager,
        compaction_config: CompactionConfig,
        verification_config: VerificationConfig,
        fs_operation_log: Arc<FsOperationLog>,
        working_dir: Option<String>,
        todo_storage: Arc<crate::tools::todo::TodoStorage>,
    ) -> Self {
        let (internal_tx, internal_rx) = broadcast::channel(1024);
        let tool_cache = ToolCache::new(&compaction_config);
        let tool_budget = ToolBudget::new(compaction_config.max_tool_calls_per_turn);
        Self {
            session_id: session_id.clone(),
            socket: AgentSocket {
                tx_command: None,
                rx_command: None,
            },
            brain: Arc::new(RwLock::new(brain)),
            method: ToolCallMethod::FunctionCall,
            temperature: Arc::new(RwLock::new(0.0)),
            state: InternalAgentState::Starting,
            internal_rx,
            tool_ctx: ToolContext {
                public_event_tx: None,
                internal_tx,
                available_tools: available_tools
                    .into_iter()
                    .map(|t| Arc::from(t) as Arc<dyn AnyTool>)
                    .collect(),
                trace: Arc::new(RwLock::new(trace)),
                claims: Arc::new(RwLock::new(permissions)),
                compaction_config,
                verification_config,
                working_dir,
                todo_storage,
                fs_operation_log,
                tool_cache,
                tool_budget,
            },
        }
    }

    /// Enable sudo mode - bypasses all permission checks
    pub async fn sudo(&mut self) {
        let mut guard = self.tool_ctx.claims.write().await;
        guard.sudo();
    }

    /// Disable sudo mode - re-enables permission checks  
    pub async fn no_sudo(&mut self) {
        let mut guard = self.tool_ctx.claims.write().await;
        guard.no_sudo();
    }

    /// Check if sudo mode is enabled
    pub async fn is_sudo(&self) -> bool {
        let guard = self.tool_ctx.claims.read().await;
        guard.is_sudo()
    }
}

#[async_trait]
impl Agent for AgentCore {
    /// Start the agent execution (blocking until completion)
    async fn run(&mut self) -> Result<AgentResult, AgentError> {
        self.start().await
    }

    /// Get a controller to send commands to the agent
    fn controller(&mut self) -> AgentController {
        self.controller()
    }

    /// Get an event watcher to subscribe to agent events
    fn watch(&mut self) -> broadcast::Receiver<AgentEvent> {
        self.watch()
    }

    /// Register an event handler closure
    fn on_event<F>(self, handler: F) -> Self
    where
        F: Fn(AgentEvent) + Send + Sync + 'static,
    {
        self.on_event(handler)
    }

    /// Register an event handler that implements AgentEventHandler
    fn with_event_handler<H>(self, handler: H) -> Self
    where
        H: AgentEventHandler + 'static,
    {
        self.with_event_handler(handler)
    }

    fn list_tools(&self) -> Vec<(String, String)> {
        self.tool_ctx
            .available_tools
            .iter()
            .map(|t| (t.name(), t.description()))
            .collect()
    }
}

impl AgentCore {
    /// Get a new controller
    pub fn controller(&mut self) -> AgentController {
        if self.socket.rx_command.is_none() {
            let (tx_command, rx_command) = mpsc::unbounded_channel();
            self.socket.tx_command = Some(tx_command);
            self.socket.rx_command = Some(rx_command);
        }
        AgentController {
            txcmd: self.socket.tx_command.as_ref().unwrap().clone(),
        }
    }

    fn assert_socket_created(&mut self) {
        if self.tool_ctx.public_event_tx.is_none() {
            let (tx_event, _rx_event) = broadcast::channel(1024);
            self.tool_ctx.public_event_tx = Some(tx_event);
        }
    }

    /// Get a new event channel
    pub fn watch(&mut self) -> broadcast::Receiver<AgentEvent> {
        self.assert_socket_created();
        self.tool_ctx.public_event_tx.as_ref().unwrap().subscribe()
    }

    /// Register an anonymous closure to process event
    pub fn on_event<F>(mut self, handler: F) -> Self
    where
        F: Fn(AgentEvent) + Send + Sync + 'static,
    {
        self.assert_socket_created();
        let mut rx = self.tool_ctx.public_event_tx.as_ref().unwrap().subscribe();
        _ = tokio::spawn(async move {
            while let Ok(e) = rx.recv().await {
                handler(e);
            }
        });
        self
    }

    /// Register an event handler that implements AgentEventHandler
    pub fn with_event_handler<H>(mut self, handler: H) -> Self
    where
        H: AgentEventHandler + 'static,
    {
        self.assert_socket_created();
        let mut rx = self.tool_ctx.public_event_tx.as_ref().unwrap().subscribe();
        _ = tokio::spawn(async move {
            while let Ok(event) = rx.recv().await {
                handler.handle_event(event).await;
            }
        });
        self
    }

    /// Handle WaitTurn command by spawning a task that waits for Paused state
    async fn handle_wait_turn(&mut self, response_channel: oneshot::Sender<AgentResponse>) {
        self.assert_socket_created();
        let mut rx = self.tool_ctx.public_event_tx.as_ref().unwrap().subscribe();
        let current_state = self.state.to_public();

        // Check if already paused
        if matches!(current_state, super::states::PublicAgentState::Paused) {
            let _ = response_channel.send(AgentResponse::Ack);
            return;
        }

        tokio::spawn(async move {
            let response = loop {
                match rx.recv().await {
                    Ok(AgentEvent::StatusChanged { new_status, .. }) => {
                        if matches!(new_status, super::states::PublicAgentState::Paused) {
                            break AgentResponse::Ack;
                        }
                        // If agent completed, failed, or was cancelled, it won't reach Paused
                        if matches!(
                            new_status,
                            super::states::PublicAgentState::Completed { .. }
                                | super::states::PublicAgentState::Failed { .. }
                                | super::states::PublicAgentState::Cancelled
                        ) {
                            break AgentResponse::Error {
                                error: "Agent finished before reaching Paused state".to_string(),
                            };
                        }
                    }
                    Err(_) => {
                        break AgentResponse::Error {
                            error: "Event channel closed".to_string(),
                        };
                    }
                    _ => {} // Ignore other events
                }
            };

            let _ = response_channel.send(response);
        });
    }

    /// Returns true if there's a controller
    pub fn has_io(&self) -> bool {
        match &self.socket.rx_command {
            Some(rx) => !rx.is_closed(),
            None => false,
        }
    }

    /// Main execution loop with single command receiver
    async fn start(&mut self) -> Result<AgentResult, AgentError> {
        self.handle_event(InternalAgentEvent::AgentInitialized)
            .await?;

        loop {
            if matches!(self.state, InternalAgentState::Paused) && !self.has_io() {
                debug!(target: "agent::loop", "state is paused but has no more controller, moving to completion");
                self.set_state(InternalAgentState::Completed { success: true })
                    .await;
            }

            // Handle terminal states - exit immediately
            match &self.state {
                InternalAgentState::Completed { success } => {
                    debug!(target: "agent::terminated", "completed");
                    let trace = self.tool_ctx.trace.clone();
                    let guard = trace.read().await;
                    return Ok(AgentResult {
                        success: success.clone(),
                        message: "Agent completed".to_string(),
                        trace: guard.clone(),
                    });
                }
                InternalAgentState::Failed { error } => {
                    debug!(target: "agent::terminated", "failed");
                    return Err(AgentError::ExecutionError(error.clone()));
                }
                _ => {}
            }

            // Special handling for Running state - check for commands but don't automatically start thinking
            if matches!(self.state, InternalAgentState::Running) {
                // Check for pending commands (non-blocking)
                if let Some(ref mut rx_command) = self.socket.rx_command {
                    if let Ok(command) = rx_command.try_recv() {
                        _ = self.handle_command(command).await;
                        continue;
                    }
                }

                // If no commands and running, start thinking
                if matches!(self.state, InternalAgentState::Running) {
                    _ = self.handle_event(InternalAgentEvent::ThinkingStart).await;
                    continue;
                }
            }

            ///////////// MAIN LOOP SLEEPER - LISTEN FOR COMMAND AND INTERNAL EVENTS
            debug!(target: "agent::loop", status = ?self.state.to_public(), "Entering event loop");
            tokio::select! {
                // only listen to command if there's a controller
                command_result = async {
                    match &mut self.socket.rx_command {
                        Some(ref mut rx) => rx.recv().await,
                        None => {
                            debug!(target: "agent::command", "command channel is closed.");
                            std::future::pending().await // Never resolves
                        }
                    }
                } => {
                    if let Some(command) = command_result {
                        _ = self.handle_command(command).await;
                    }
                    // if channel is closed it means there's no more controller, we ignore silently.
                }

                // always listen to internal events
                internal_event = self.internal_rx.recv() => {
                    if let Ok(event) = internal_event {
                        _ = self.handle_event(event).await;
                    } else {
                        return Err(AgentError::InvalidState("internal event bus should not be closed".to_string()));
                    }
                }
            }
        }
    }

    /// Handle a command
    async fn handle_command(&mut self, command: SentCommand) -> Result<(), AgentError> {
        debug!(target: "agent::command", event = ?command);
        let SentCommand {
            command,
            backchannel,
        } = command;

        let res = match command {
            AgentRequest::Droping => {
                if let Some(ref mut rx_command) = self.socket.rx_command {
                    debug!(target: "agent::command", "droping IO controller");
                    rx_command.close();
                }
                Ok(AgentResponse::Ack)
            }
            AgentRequest::GetState => Ok(AgentResponse::State {
                state: self.state.to_public(),
            }),
            AgentRequest::GetTrace => {
                let trace = self.tool_ctx.trace.read().await.clone();
                Ok(AgentResponse::Trace { trace })
            }
            AgentRequest::Sudo(operation) => {
                let mut guard = self.tool_ctx.claims.write().await;
                match operation {
                    Some(true) => guard.sudo(),
                    Some(false) => guard.no_sudo(),
                    None => {} // Just get status
                }
                let enabled = guard.is_sudo();
                Ok(AgentResponse::SudoStatus { enabled })
            }
            AgentRequest::PlanMode(operation) => {
                let mut guard = self.tool_ctx.claims.write().await;
                match operation {
                    Some(true) => guard.plan_mode(),
                    Some(false) => guard.no_plan_mode(),
                    None => {} // Just get status
                }
                let enabled = guard.is_plan_mode();
                Ok(AgentResponse::PlanModeStatus { enabled })
            }
            AgentRequest::SetActivePrompts(prompts) => {
                let mut guard = self.tool_ctx.claims.write().await;
                guard.set_active_prompts(prompts);
                let prompts = guard.active_prompts().to_vec();
                Ok(AgentResponse::PromptsStatus { prompts })
            }
            AgentRequest::Terminate => {
                self.handle_event(InternalAgentEvent::CancelTask)
                    .await
                    .and({
                        self.set_state(InternalAgentState::Completed { success: false })
                            .await;
                        Ok(AgentResponse::Ack)
                    })
            }
            AgentRequest::StopCurrentTask => self
                .handle_event(InternalAgentEvent::CancelTask)
                .await
                .and({
                    self.set_state(InternalAgentState::Paused).await;
                    Ok(AgentResponse::Ack)
                }),
            AgentRequest::Regenerate => {
                self.handle_event(InternalAgentEvent::CancelTask)
                    .await
                    .and({
                        // Remove trailing assistant messages and tool calls to re-think
                        // from the last user message
                        {
                            let mut trace = self.tool_ctx.trace.write().await;
                            while let Some(msg) = trace.last() {
                                match msg {
                                    ChatMessage::User { .. } => break,
                                    _ => {
                                        trace.pop();
                                    }
                                }
                            }
                        }
                        self.set_state(InternalAgentState::Running).await;
                        Ok(AgentResponse::Ack)
                    })
            }
            AgentRequest::SwitchToolCallMethod { method } => {
                if let Some(method) = method {
                    self.method = method;
                }
                Ok(AgentResponse::Method {
                    method: self.method,
                })
            }
            AgentRequest::SetTemperature { temperature } => {
                *self.temperature.write().await = temperature;
                Ok(AgentResponse::Temperature { temperature })
            }
            AgentRequest::SendUserInput { input } => {
                self.handle_event(InternalAgentEvent::CancelTask)
                    .await
                    .and({
                        // Emit UserInput event
                        let _ = self
                            .emit_event(AgentEvent::UserInput {
                                input: input.clone(),
                            })
                            .await;

                        self.tool_ctx.trace.write().await.push(ChatMessage::User {
                            content: ChatMessageContent::Text(input),
                            name: None,
                        });

                        // Reset tool call counter for the new turn
                        self.tool_ctx.tool_budget.reset().await;

                        self.set_state(InternalAgentState::Running).await;
                        Ok(AgentResponse::Ack)
                    })
            }
            AgentRequest::SendTrace { messages } => {
                self.handle_event(InternalAgentEvent::CancelTask)
                    .await
                    .and({
                        // Add all messages to trace at once
                        self.tool_ctx.trace.write().await.extend(messages);

                        // Reset tool call counter for the new turn
                        self.tool_ctx.tool_budget.reset().await;

                        self.set_state(InternalAgentState::Running).await;
                        Ok(AgentResponse::Ack)
                    })
            }
            AgentRequest::LoadTrace { messages } => {
                self.handle_event(InternalAgentEvent::CancelTask)
                    .await
                    .and({
                        // Add all messages to trace at once
                        self.tool_ctx.trace.write().await.extend(messages);

                        // Stay paused - don't start thinking
                        Ok(AgentResponse::Ack)
                    })
            }
            AgentRequest::UserQueryResponse {
                request_id: query_id,
                response,
            } => {
                // This event is managed by the spawn thread directly, thus sending to the broadcast internal event channel
                let _ = self
                    .tool_ctx
                    .internal_tx
                    .send(InternalAgentEvent::UserResponseReceived {
                        request_id: query_id,
                        response: response,
                    })
                    .map_err(|_| AgentError::SessionClosed)?;
                Ok(AgentResponse::Ack)
            }
            AgentRequest::UserPermissionResponse {
                request_id,
                response,
            } => {
                // This event is managed by the spawn thread directly, thus sending to the broadcast internal event channel
                let _ = self
                    .tool_ctx
                    .internal_tx
                    .send(InternalAgentEvent::PermissionResponseReceived {
                        request_id: request_id,
                        response: response,
                    })
                    .map_err(|_| AgentError::SessionClosed)?;
                Ok(AgentResponse::Ack)
            }
            AgentRequest::WaitTurn => {
                self.handle_wait_turn(backchannel).await;
                return Ok(()); // We handle the response in the spawned task
            }
        }
        .unwrap_or_else(|e| AgentResponse::Error {
            error: e.to_string(),
        });

        // ignore if channel is closed
        let _ = backchannel.send(res);
        Ok(())
    }

    /// Handle an event
    async fn handle_event(&mut self, event: InternalAgentEvent) -> Result<(), AgentError> {
        debug!(target: "agent::internal_event", event = ?event);
        match self.state {
            InternalAgentState::Starting => self.state_starting_handle_event(event).await,
            InternalAgentState::Running => self.state_running_handle_event(event).await,
            InternalAgentState::Processing { .. } => {
                self.state_processing_handle_event(event).await
            }
            InternalAgentState::Paused => self.state_pause_handle_event(event).await,
            _ => self.state_terminal_handle_event(event).await,
        }
    }

    /// Set agent status and emit event
    pub async fn set_state(&mut self, to_state: InternalAgentState) {
        let old_state = self.state.to_public();
        let new_state = to_state.to_public();

        debug!(
            target: "agent::status",
            "{:?} <<--- {:?}", new_state, old_state
        );

        // Emit event
        let _ = self
            .emit_event(AgentEvent::StatusChanged {
                old_status: old_state,
                new_status: new_state,
            })
            .await;

        self.state = to_state;
    }

    /// Emit an event to the controller
    pub async fn emit_event(&self, event: AgentEvent) -> Result<(), AgentError> {
        // ignore if no receiver or if all receiver are dropped
        if let Some(tx) = &self.tool_ctx.public_event_tx {
            debug!(target: "agent::public_event", event = ?event);
            let _ = tx.send(event).map_err(|_| AgentError::SessionClosed)?;
        }
        Ok(())
    }
}

/// Response from a completed task agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskAgentResponse {
    pub success: bool,
    pub message: String,
}
