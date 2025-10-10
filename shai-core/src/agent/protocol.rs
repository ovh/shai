use shai_llm::ToolCallMethod;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{timeout, Duration};
use crate::agent::AgentError;

use super::{PermissionResponse, PublicAgentState, UserResponse};

/// Commands that can be sent to a running agent
#[derive(Debug, Clone)]
pub enum AgentRequest {
    /// Cancel agent execution
    Cancel,
    /// Stop the currently executing task
    StopCurrentTask,    
    /// Send user input (cancels current task, adds to trace, resumes agent)
    GetState,
    /// Send user input (cancels current task, adds to trace, resumes agent)
    SendUserInput{
        input: String
    },
    /// Switch method for tool call
    SwitchToolCallMethod {
        method: Option<ToolCallMethod>
    },
    /// Send user input (cancels current task, adds to trace, resumes agent)
    UserQueryResponse{
        request_id: String,
        response: UserResponse
    },
    /// Send user input (cancels current task, adds to trace, resumes agent)
    UserPermissionResponse{
        request_id: String,
        response: PermissionResponse
    },
    /// Wait until the agent reaches the Paused state
    WaitTurn,
    /// Manage sudo mode: Some(true) = enable, Some(false) = disable, None = get status
    /// Always returns current sudo status after operation
    Sudo(Option<bool>),
    /// Drop controller IO, this closes it for all controller.
    /// Once this is done, it cannot be reopen!
    Droping,
    /// Trigger context compression manually
    TriggerContextCompression,
}

/// Commands that can be sent to a running agent
#[derive(Debug, Clone)]
pub enum AgentResponse {
    Ack,
    Method {
        method: ToolCallMethod
    },
    State {
        state: PublicAgentState
    },
    SudoStatus {
        enabled: bool
    },
    Error {
        error: String
    }
}

/// Commands that can be sent to a running agent
#[derive(Debug)]
pub struct SentCommand {
    pub command:     AgentRequest,
    pub backchannel: oneshot::Sender<AgentResponse>
}

/// Controller for sending commands to an agent
#[derive(Clone)]
pub struct AgentController {
    pub txcmd: mpsc::UnboundedSender<SentCommand>,
}

impl AgentController {
    /// Send a command to the agent
    pub async fn send(&self, command: AgentRequest) -> Result<AgentResponse, AgentError> {
        let (tx, rx) = oneshot::channel();
        self.txcmd.send(SentCommand{command, backchannel: tx})
            .map_err(|_| AgentError::SessionClosed)?;

        let result = timeout(
            Duration::from_millis(1000), 
            rx
        ).await;

        match result {
            Ok(value) => value.map_err(|_| AgentError::ExecutionError("Command response channel closed".to_string())),
            Err(_) => Err(AgentError::TimeoutError)
        }
    }

    pub async fn drop(&mut self) -> Result<(), AgentError> {
        self.send(AgentRequest::Droping).await?;
        Ok(())
    }

    pub async fn cancel(&self) -> Result<(), AgentError> {
        self.send(AgentRequest::Cancel).await.map(|_| Ok(()))?
    }

    pub async fn test_stop_current_task(&self) -> Result<(), AgentError> {
        self.send(AgentRequest::StopCurrentTask).await.map(|_| Ok(()))?
    }

    pub async fn set_method(&self, method:Option<ToolCallMethod>) -> Result<ToolCallMethod, AgentError> {
        match self.send(AgentRequest::SwitchToolCallMethod { method }).await? {
            AgentResponse::Method{method} => Ok(method),
            _ => Err(AgentError::InvalidResponse("Expected Method response".to_string()))
        }
    }

    pub async fn send_user_input(&self, input: String) -> Result<(), AgentError> {
        self.send(AgentRequest::SendUserInput { input: input }).await.map(|_| Ok(()))?
    }

    pub async fn response_user_query(&self,  request_id: String, response: UserResponse) -> Result<(), AgentError> {
        self.send(AgentRequest::UserQueryResponse { request_id, response }).await.map(|_| Ok(()))?
    }

    pub async fn response_permission_request(&self,  request_id: String, response: PermissionResponse) -> Result<(), AgentError> {
        self.send(AgentRequest::UserPermissionResponse { request_id, response }).await.map(|_| Ok(()))?
    }

    pub async fn get_state(&self) -> Result<PublicAgentState, AgentError> {
        match self.send(AgentRequest::GetState).await? {
            AgentResponse::State{state} => Ok(state),
            _ => Err(AgentError::InvalidResponse("Expected State response".to_string()))
        }
    }

    /// Wait until the agent reaches the Paused state
    pub async fn wait_turn(&self, timeout_ms: Option<u64>) -> Result<(), AgentError> {
        let (tx, rx) = oneshot::channel();
        self.txcmd.send(SentCommand{command: AgentRequest::WaitTurn, backchannel: tx})
            .map_err(|_| AgentError::SessionClosed)?;

        let response = if let Some(ms) = timeout_ms {
            timeout(Duration::from_millis(ms), rx).await
                .map_err(|_| AgentError::TimeoutError)?
        } else {
            rx.await
        }
        .map_err(|_| AgentError::ExecutionError("Command response channel closed".to_string()))?;

        match response {
            AgentResponse::Ack => Ok(()),
            AgentResponse::Error { error } => Err(AgentError::ExecutionError(error)),
            _ => Err(AgentError::InvalidResponse("Expected Ack response for WaitTurn".to_string()))
        }
    }

    /// Enable sudo mode - bypasses all permission checks
    pub async fn sudo(&self) -> Result<bool, AgentError> {
        match self.send(AgentRequest::Sudo(Some(true))).await? {
            AgentResponse::SudoStatus { enabled } => Ok(enabled),
            _ => Err(AgentError::InvalidResponse("Expected SudoStatus response".to_string()))
        }
    }

    /// Disable sudo mode - re-enables permission checks
    pub async fn no_sudo(&self) -> Result<bool, AgentError> {
        match self.send(AgentRequest::Sudo(Some(false))).await? {
            AgentResponse::SudoStatus { enabled } => Ok(enabled),
            _ => Err(AgentError::InvalidResponse("Expected SudoStatus response".to_string()))
        }
    }

    /// Check if sudo mode is enabled
    pub async fn is_sudo(&self) -> Result<bool, AgentError> {
        match self.send(AgentRequest::Sudo(None)).await? {
            AgentResponse::SudoStatus { enabled } => Ok(enabled),
            _ => Err(AgentError::InvalidResponse("Expected SudoStatus response".to_string()))
        }
    }

    /// Trigger context compression manually
    pub async fn trigger_context_compression(&self) -> Result<(), AgentError> {
        self.send(AgentRequest::TriggerContextCompression).await.map(|_| Ok(()))?
    }
}