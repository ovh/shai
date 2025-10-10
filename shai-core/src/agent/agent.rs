use std::sync::Arc;
use std::boxed::Box;
use shai_llm::{ChatMessage, ChatMessageContent, ToolCallMethod};
use tokio::sync::{mpsc, broadcast, RwLock, oneshot};
use serde::{Serialize, Deserialize};
use async_trait::async_trait;
use crate::tools::AnyTool;
use crate::agent::ClaimManager;

// Helper functions to make the main loop more readable

use crate::agent::{Brain, InternalAgentEvent};
use crate::agent::AgentError;
use crate::agent::{AgentRequest, AgentEvent};
use crate::agent::InternalAgentState;
use tracing::debug;

use super::protocol::{AgentController, SentCommand};
use super::{AgentResponse, AgentEventHandler};

/// Trait defining the public interface for agents
#[async_trait]
pub trait Agent: Send + Sync {    
    /// Start the agent execution (blocking until completion)
    async fn run(&mut self) -> Result<AgentResult, AgentError>;
    
    /// Get a controller to send commands to the agent
    fn controller(&mut self) -> AgentController;
    
    /// Get an event watcher to subscribe to agent events
    fn watch(&mut self) -> broadcast::Receiver<AgentEvent>;
    
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
    pub trace:   Vec<ChatMessage>, // Full uncompressed trace
    pub compressed_trace: Vec<ChatMessage>, // Compressed trace (after compression)
}

/// Core agent implementation that orchestrates any Thinker implementation
pub struct AgentCore {
    pub session_id: String,

    /// public controler and event watcher
    pub socket: AgentSocket,
    
    /// big brain
    pub brain: Arc<RwLock<Box<dyn Brain>>>,
    pub method: ToolCallMethod,

    /// agent state (manipulated by main looper + brain/tool coroutines)
    pub trace:           Arc<RwLock<Vec<ChatMessage>>>,
    pub full_trace:      Arc<RwLock<Vec<ChatMessage>>>, // Complete history before compression
    pub available_tools: Vec<Arc<dyn AnyTool>>,
    pub permissions:     Arc<RwLock<ClaimManager>>,
    pub state:           InternalAgentState,

    /// internal event
    pub internal_tx: broadcast::Sender<InternalAgentEvent>,   // event may be produced from many part of the agent
    pub internal_rx: broadcast::Receiver<InternalAgentEvent>, // events are mostly consumed by the main event loop, but also in spawn tool to monitor permissions
}

pub struct AgentSocket {
    pub tx_command:    Option<mpsc::UnboundedSender<SentCommand>>,   // might have multiple commander
    pub rx_command:    Option<mpsc::UnboundedReceiver<SentCommand>>, // self is single consumer of command from main agent loop
    pub tx_event:      Option<broadcast::Sender<AgentEvent>>,        // multiple producer of event from multiple thread within self
    pub rx_event:      Option<broadcast::Receiver<AgentEvent>>,      // multiple event watcher
}

impl AgentCore {
    pub fn new(
        session_id: String,
        brain: Box<dyn Brain>,
        trace: Vec<ChatMessage>,
        available_tools: Vec<Box<dyn AnyTool>>,
        permissions: ClaimManager,
    ) -> Self {
        let (internal_tx, internal_rx) = broadcast::channel(1024);
        Self {
            session_id: session_id.clone(),
            socket: AgentSocket{
                tx_command: None,
                rx_command: None,
                tx_event: None,
                rx_event: None,
            },
            brain: Arc::new(RwLock::new(brain)),
            method: ToolCallMethod::FunctionCall,
            trace: Arc::new(RwLock::new(trace.clone())),
            full_trace: Arc::new(RwLock::new(trace)),
            available_tools: available_tools.into_iter().map(|t| Arc::from(t) as Arc<dyn AnyTool>).collect(),
            permissions: Arc::new(RwLock::new(permissions)),
            state: InternalAgentState::Starting,
            internal_tx,
            internal_rx,
        }
    }

    /// Enable sudo mode - bypasses all permission checks
    pub async fn sudo(&mut self) {
        let mut guard = self.permissions.write().await;
        guard.sudo();
    }

    /// Disable sudo mode - re-enables permission checks  
    pub async fn no_sudo(&mut self) {
        let mut guard = self.permissions.write().await;
        guard.no_sudo();
    }

    /// Check if sudo mode is enabled
    pub async fn is_sudo(&self) -> bool {
        let guard = self.permissions.read().await;
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
        F: Fn(AgentEvent) + Send + Sync + 'static
    {
        self.on_event(handler)
    }
    
    /// Register an event handler that implements AgentEventHandler
    fn with_event_handler<H>(self, handler: H) -> Self
    where 
        H: AgentEventHandler + 'static
    {
        self.with_event_handler(handler)
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
            txcmd: self.socket.tx_command.as_ref().unwrap().clone()
        }
    }

    fn assert_socket_created(&mut self) {
        if self.socket.tx_event.is_none() {
            let (tx_event, rx_event) = broadcast::channel(1024);
            self.socket.tx_event = Some(tx_event);
            self.socket.rx_event = Some(rx_event);
        }
    }

    /// Get a new event channel
    pub fn watch(&mut self) ->  broadcast::Receiver<AgentEvent> {
        self.assert_socket_created();
        self.socket.tx_event.as_ref().unwrap().subscribe()
    }

    /// Register an anonymous closure to process event
    pub fn on_event<F>(mut self, handler: F) -> Self 
    where 
        F: Fn(AgentEvent) + Send + Sync + 'static
    {
        self.assert_socket_created();
        let mut rx =  self.socket.tx_event.as_ref().unwrap().subscribe();
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
        H: AgentEventHandler + 'static
    {
        self.assert_socket_created();
        let mut rx = self.socket.tx_event.as_ref().unwrap().subscribe();
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
        let mut rx = self.socket.tx_event.as_ref().unwrap().subscribe();
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
                        if matches!(new_status, 
                            super::states::PublicAgentState::Completed { .. } |
                            super::states::PublicAgentState::Failed { .. } |
                            super::states::PublicAgentState::Cancelled
                        ) {
                            break AgentResponse::Error { 
                                error: "Agent finished before reaching Paused state".to_string() 
                            };
                        }
                    }
                    Err(_) => {
                        break AgentResponse::Error { 
                            error: "Event channel closed".to_string() 
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
        self.handle_event(InternalAgentEvent::AgentInitialized).await?;
        
        loop {
            if matches!(self.state, InternalAgentState::Paused) && !self.has_io() {
                debug!(target: "agent::loop", "state is paused but has no more controller, moving to completion");
                self.set_state(InternalAgentState::Completed { success: true }).await;
            }

            // Handle terminal states - exit immediately
            match &self.state {
                InternalAgentState::Completed { success } => {
                    debug!(target: "agent::terminated", "completed");
                    let full_trace = self.full_trace.clone();
                    let compressed_trace = self.trace.clone();
                    let full_guard = full_trace.read().await;
                    let compressed_guard = compressed_trace.read().await;
                    return Ok(AgentResult {
                        success: success.clone(),
                        message: "Agent completed".to_string(),
                        trace: full_guard.clone(),
                        compressed_trace: compressed_guard.clone(),
                    });
                },
                InternalAgentState::Failed { error } => {
                    debug!(target: "agent::terminated", "failed");
                    return Err(AgentError::ExecutionError(error.clone()));
                },
                _ => {}
            }
            
            // Special handling for Running state - check for commands but don't automatically start thinking
            if matches!(self.state, InternalAgentState::Running) {
                // Check for pending commands (non-blocking)                
                if let Some(ref mut rx_command) = self.socket.rx_command {
                    if let Ok(command) = rx_command.try_recv() {
                        _ = self.handle_command(command).await;
                        continue
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
        let SentCommand{command, backchannel} = command;

        let res = match command {
            AgentRequest::Droping => {
                if let Some(ref mut rx_command) = self.socket.rx_command {
                    debug!(target: "agent::command", "droping IO controller");
                    rx_command.close();
                }
                Ok(AgentResponse::Ack)
            }
            AgentRequest::GetState => {
                Ok(AgentResponse::State { state: self.state.to_public()})
            }
            AgentRequest::Sudo(operation) => {
                let mut guard = self.permissions.write().await;
                match operation {
                    Some(true) => guard.sudo(),
                    Some(false) => guard.no_sudo(),
                    None => {} // Just get status
                }
                let enabled = guard.is_sudo();
                Ok(AgentResponse::SudoStatus { enabled })
            }
            AgentRequest::Cancel=> {
                self.handle_event(InternalAgentEvent::CancelTask).await
                .and({
                    self.set_state(InternalAgentState::Failed { error: "The agent was cancelled".to_string() }).await;
                    Ok(AgentResponse::Ack)
                })
            }
            AgentRequest::StopCurrentTask => {
                self.handle_event(InternalAgentEvent::CancelTask).await
                .and({
                    self.set_state(InternalAgentState::Paused).await;
                    Ok(AgentResponse::Ack)
                })
            }
            AgentRequest::SwitchToolCallMethod { method } => {
                if let Some(method) = method {
                    self.method = method;   
                }
                Ok(AgentResponse::Method { method: self.method })
            }
            AgentRequest::SendUserInput{ input } => {
                self.handle_event(InternalAgentEvent::CancelTask).await
                .and({
                    // Emit UserInput event
                    let _ = self.emit_event(AgentEvent::UserInput {
                        input: input.clone()
                    }).await;

                    let user_message = ChatMessage::User {
                        content: ChatMessageContent::Text(input),
                        name: None
                    };
                    self.trace.write().await.push(user_message.clone());
                    self.full_trace.write().await.push(user_message);

                    self.set_state(InternalAgentState::Running).await;
                    Ok(AgentResponse::Ack)
                })
            }
            AgentRequest::UserQueryResponse{ request_id: query_id, response } => {
                // This event is managed by the spawn thread directly, thus sending to the broadcast internal event channel
                let _ = self.internal_tx.send(InternalAgentEvent::UserResponseReceived{
                    request_id: query_id,
                    response: response
                }).map_err(|_| AgentError::SessionClosed)?;
                Ok(AgentResponse::Ack)
            }
            AgentRequest::UserPermissionResponse{ request_id, response } => {
                // This event is managed by the spawn thread directly, thus sending to the broadcast internal event channel
                let _ = self.internal_tx.send(InternalAgentEvent::PermissionResponseReceived {
                    request_id: request_id,
                    response: response
                }).map_err(|_| AgentError::SessionClosed)?;
                Ok(AgentResponse::Ack)
            }
            AgentRequest::WaitTurn => {
                self.handle_wait_turn(backchannel).await;
                return Ok(()); // We handle the response in the spawned task
            }
            AgentRequest::TriggerContextCompression => {
                // Trigger context compression by calling the check_and_compress_context method
                // We'll send a special internal event to trigger the compression
                let _ = self.internal_tx.send(InternalAgentEvent::ManualCompressionRequested).map_err(|_| AgentError::SessionClosed)?;
                Ok(AgentResponse::Ack)
            }
        }.unwrap_or_else(|e| AgentResponse::Error { error: e.to_string() });

        // ignore if channel is closed
        let _ = backchannel.send(res);
        Ok(())
    }


    /// Handle an event
    async fn handle_event(&mut self, event: InternalAgentEvent) -> Result<(), AgentError> {
        debug!(target: "agent::internal_event", event = ?event);
        match self.state {
            InternalAgentState::Starting => {
                self.state_starting_handle_event(event).await
            }
            InternalAgentState::Running => {
                self.state_running_handle_event(event).await
            }
            InternalAgentState::Processing { .. } => {
                self.state_processing_handle_event(event).await
            }
            InternalAgentState::Paused => {
                self.state_pause_handle_event(event).await
            }
            _ => {
                self.state_terminal_handle_event(event).await
            }
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
        let _ = self.emit_event(AgentEvent::StatusChanged {
            old_status: old_state,
            new_status: new_state,
        }).await;
        
        self.state = to_state;
    }
    
    /// Emit an event to the controller
    pub async fn emit_event(&self, event: AgentEvent) -> Result<(), AgentError> {
        // ignore if no receiver or if all receiver are dropped
        if let Some(tx) = &self.socket.tx_event {
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
