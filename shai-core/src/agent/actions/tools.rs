use std::sync::Arc;

use chrono::{TimeDelta, Utc};
use shai_llm::{ChatMessage, ToolCall as LlmToolCall};
use tokio::sync::{broadcast, RwLock};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::info;
use serde_json::from_str;
use uuid::Uuid;
use crate::agent::{AgentCore, AgentEvent, ClaimManager, InternalAgentEvent, InternalAgentState, PermissionRequest, PermissionResponse};
use crate::tools::{AnyTool, ToolCall, ToolCapability, ToolResult};
use tracing::debug;

impl AgentCore {

    /// Spawn a cancellable coroutine that runs all tool call in parrallel and waits for them to finish
    pub async fn spawn_tools(&mut self, tool_calls: Vec<LlmToolCall>) {
        let cancellation_token = CancellationToken::new();
        let cancel_clone = cancellation_token.clone();
        let internal_tx = self.internal_tx.clone();

        // Clone all needed data from self before spawning
        let public_event_tx = self.socket.tx_event.clone();
        let available_tools = self.available_tools.clone();
        let claims = self.permissions.clone();
        let trace = self.trace.clone();
        let full_trace = self.full_trace.clone();

        // Spawn a task to wait for all tool executions
        let mut join_handles = Vec::new();

        // Spawn all tool executions
        for tc in tool_calls {
            let handle = Self::spawn_tool_static(
                tc,
                cancel_clone.clone(),
                public_event_tx.clone(),
                available_tools.clone(),
                claims.clone(),
                internal_tx.clone(),
                trace.clone(),
                full_trace.clone(),
            );
            join_handles.push(handle);
        }
            
        // Wait for all tools to complete or be cancelled
        tokio::spawn(async move {
            tokio::select! {
                _ = cancel_clone.cancelled() => {
                    // Tools were cancelled, no need to send completion event
                }
                any_denied = async {
                    // wait for all tools completion and collect denial status
                    let mut result = false;
                    for handle in join_handles {
                        if let Ok(was_denied) = handle.await {
                            result = result || was_denied;
                        }
                    }
                    result
                } => {
                    // All tools completed, move to Running state
                    let _ = internal_tx.send(InternalAgentEvent::ToolsCompleted { any_denied });
                }
            }
        });
        
        // Set state to Processing with cancellation token
        self.set_state(InternalAgentState::Processing { 
            task_name: "tools".to_string(), 
            tools_exec_at: Utc::now(), 
            cancellation_token
        }).await;
    }

    /// Spawn a cancellable coroutine that runs a single tool call
    /// coordinating the appropriate tool specific event (start/completed)
    fn spawn_tool_static(
        tc: LlmToolCall,
        cancel_token: CancellationToken,
        public_event_tx: Option<broadcast::Sender<AgentEvent>>,
        available_tools: Vec<Arc<dyn AnyTool>>,
        claims: Arc<RwLock<ClaimManager>>,
        internal_tx: broadcast::Sender<InternalAgentEvent>,
        trace: Arc<RwLock<Vec<ChatMessage>>>,
        full_trace: Arc<RwLock<Vec<ChatMessage>>>,
    ) -> tokio::task::JoinHandle<bool> {
        tokio::spawn(async move {
            let tc_for_error = tc.clone();
            match Self::tool_exist(available_tools, tc) {
                // tool does not exist, we fail immediately
                Err(tool_result) => {
                    if let Some(tx) = public_event_tx.clone() {
                        let _ = tx.send(AgentEvent::ToolCallCompleted { 
                            duration: TimeDelta::zero(), 
                            call: ToolCall {
                                tool_call_id: tc_for_error.id.clone(),
                                tool_name: tc_for_error.function.name.clone(),
                                parameters: serde_json::Value::Null
                            }, 
                            result: tool_result
                        });
                    }
                    false
                }

                // emit tool call
                // execute tool
                // emit tool result
                Ok((tool, call)) => {
                    let start = Utc::now();

                    // Emit tool call started event
                    if let Some(tx) = public_event_tx.clone() {
                        let _ = tx.send(AgentEvent::ToolCallStarted { 
                            timestamp: start.clone(), 
                            call: call.clone(), 
                        });
                    }
                    
                    // execute tool
                    let tool_handle = Self::spawn_tool_exec(
                        tool, call.clone(), 
                        cancel_token.clone(), 
                        claims, 
                        public_event_tx.clone(), 
                        internal_tx.subscribe());

                    // wait for result (or for cancellation)
                    let result: ToolResult = tokio::select! {
                        join_result = tool_handle => {
                            match join_result {
                                Ok(tool_result) => tool_result,
                                Err(join_error) => {
                                    debug!(target: "agent::tool_completed", "tool execution task failed: {}", join_error);
                                    ToolResult::error(format!("tool execution task failed: {}", join_error))
                                }
                            }
                         },
                        _ = cancel_token.cancelled() => {
                            debug!(target: "agent::tool_completed", "cancelled by user");
                            ToolResult::error("tool call was cancelled by the user".to_string())
                        }
                    };

                    // let's first add tool result to trace
                    let _ = {
                        let tool_message = ChatMessage::Tool {
                            tool_call_id: call.tool_call_id.clone(),
                            content: result.to_string()
                        };
                        trace.write().await.push(tool_message.clone());
                        full_trace.write().await.push(tool_message);
                    };

                    // Emit tool call finish event
                    let tool_was_denied = result.is_denied();
                    info!(target: "agent::tool_completed", call = ?tc_for_error.function.name.clone(), result = ?result);
                    if let Some(tx) = public_event_tx.clone() {
                        let _ = tx.send(AgentEvent::ToolCallCompleted { 
                            duration: Utc::now() - start, 
                            call: call, 
                            result 
                        });   
                    }

                    tool_was_denied                    
                }
            }
        })
    }

    /// execute a single tool call
    /// checking for permission, requesting it, executing the tool
    fn spawn_tool_exec(
        tool: Arc<dyn AnyTool>, 
        call: ToolCall, 
        cancel_token: CancellationToken,
        claims: Arc<RwLock<ClaimManager>>, 
        public_event_tx: Option<broadcast::Sender<AgentEvent>>, 
        mut internal_rx: broadcast::Receiver<InternalAgentEvent>) -> JoinHandle<ToolResult> {
        tokio::spawn(async move {
            // check permission, we allow all Read Tool
            let can_run = tool.capabilities().is_empty()  
            || tool.capabilities() == &[ToolCapability::Read]
            || claims.read().await.is_permitted(&tool.name(), &call.parameters);

            // request permission if needed (|| is short-circuiting, so won't call if can_run is true)
            let can_run = can_run || match Self::request_permission_if_needed(&call, &tool, &public_event_tx, &mut internal_rx, &cancel_token).await {
                Ok(permission_granted) => permission_granted,
                Err(preview_error) => return preview_error, // Return preview error immediately
            };

            if !can_run {
                return ToolResult::denied()
            }
            
            // Execute tool with cancellation support
            tokio::select! {
                result = tool.execute_json(call.parameters.clone(), Some(cancel_token.clone())) => result,
                _ = cancel_token.cancelled() => {
                    ToolResult::error("tool call was cancelled by the user".to_string())
                }
            }
        })
    }

    /// send a permission request (if necessary) and wait for the answer
    /// Returns Ok(true) if permission granted, Ok(false) if denied, Err(ToolResult) if preview failed
    async fn request_permission_if_needed(
        call: &ToolCall,
        tool: &Arc<dyn AnyTool>,
        public_event_tx: &Option<broadcast::Sender<AgentEvent>>,
        internal_rx: &mut broadcast::Receiver<InternalAgentEvent>,
        cancel_token: &CancellationToken,
    ) -> Result<bool, ToolResult> {
        // Session is not interactive so we cannot ask for permission
        let Some(tx) = public_event_tx.as_ref() else {
            return Ok(false); 
        };
        
        // Try to get preview from tool
        let preview = tool.execute_preview_json(call.parameters.clone()).await;
        
        // If preview returned an error, return that error immediately
        if let Some(error_result) = &preview {
            if let ToolResult::Error { .. } = error_result {
                return Err(error_result.clone());
            }
        }
        
        // Send permission request
        let req_id = Uuid::new_v4().to_string();
        let _ = tx.send(AgentEvent::PermissionRequired {
            request_id: req_id.clone(),
            request: PermissionRequest {
                tool_name: call.tool_name.clone(),
                operation: "do you want to run this tool?".to_string(),
                call: call.clone(),
                preview,
            }
        });

        // Wait for permission response
        loop {
            tokio::select! {
                recv_result = internal_rx.recv() => {
                    match recv_result {
                        Ok(InternalAgentEvent::PermissionResponseReceived { request_id, response }) if request_id == req_id => {
                            return Ok(matches!(response, PermissionResponse::Allow | PermissionResponse::AllowAlways));
                        }
                        Ok(_) => continue,
                        Err(_) => return Ok(false), // Channel closed
                    }
                }
                _ = cancel_token.cancelled() => {
                    return Ok(false); // Cancelled during permission wait
                }
            }
        }
    }

    // utility method
    fn tool_exist(
        tools: Vec<Arc<dyn AnyTool>>, 
        tc: LlmToolCall
    ) -> Result<(Arc<dyn AnyTool>, ToolCall), ToolResult>{
        from_str(&tc.function.arguments)
        .map_err(|_e| 
            ToolResult::error("failed to parse tool parameters".to_string())
        )
        .and_then(|params| {
            let tool_call = ToolCall {
                tool_call_id: tc.id.clone(),
                tool_name: tc.function.name.clone(),
                parameters: params
            };
            
            // Find the tool
            tools.iter()
                .find(|t| t.name() == tool_call.tool_name)
                .cloned()
                .ok_or_else(||
                    ToolResult::error(format!("tool not found: {}", tool_call.tool_name))
                )
                .map(|tool| (tool, tool_call))
        })
    }
}