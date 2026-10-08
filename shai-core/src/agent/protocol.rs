use crate::agent::AgentError;
use openai_dive::v1::resources::chat::ChatMessage;
use shai_llm::ToolCallMethod;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{timeout, Duration};

use super::{PermissionResponse, PublicAgentState, UserResponse};

/// Commands that can be sent to a running agent
#[derive(Debug, Clone)]
pub enum AgentRequest {
    /// Stop the Agent
    Terminate,
    /// Stop the currently executing task
    StopCurrentTask,
    /// Regenerate the last response (removes last assistant message and re-thinks)
    Regenerate,
    /// Get current agent state
    GetState,
    /// Get the conversation trace
    GetTrace,
    /// Send user input (cancels current task, adds to trace, resumes agent)
    SendUserInput { input: String },
    /// Send multiple messages as a trace (cancels current task, adds all to trace, resumes agent)
    SendTrace { messages: Vec<ChatMessage> },
    /// Load a trace without starting to think (stays paused)
    LoadTrace { messages: Vec<ChatMessage> },
    /// Switch method for tool call
    SwitchToolCallMethod { method: Option<ToolCallMethod> },
    /// Set the LLM sampling temperature
    SetTemperature { temperature: f32 },
    /// Send user input (cancels current task, adds to trace, resumes agent)
    UserQueryResponse {
        request_id: String,
        response: UserResponse,
    },
    /// Send user input (cancels current task, adds to trace, resumes agent)
    UserPermissionResponse {
        request_id: String,
        response: PermissionResponse,
    },
    /// Wait until the agent reaches the Paused state
    WaitTurn,
    /// Manage sudo mode: Some(true) = enable, Some(false) = disable, None = get status
    /// Always returns current sudo status after operation
    Sudo(Option<bool>),
    /// Manage plan mode: Some(true) = enable, Some(false) = disable, None = get status
    PlanMode(Option<bool>),
    /// Set active system prompts by name
    SetActivePrompts(Vec<String>),
    /// Drop controller IO, this closes it for all controller.
    /// Once this is done, it cannot be reopen!
    Droping,
}

/// Commands that can be sent to a running agent
#[derive(Debug, Clone)]
pub enum AgentResponse {
    Ack,
    Method { method: ToolCallMethod },
    Temperature { temperature: f32 },
    State { state: PublicAgentState },
    Trace { trace: Vec<ChatMessage> },
    SudoStatus { enabled: bool },
    PlanModeStatus { enabled: bool },
    PromptsStatus { prompts: Vec<String> },
    Error { error: String },
}

/// Commands that can be sent to a running agent
#[derive(Debug)]
pub struct SentCommand {
    pub command: AgentRequest,
    pub backchannel: oneshot::Sender<AgentResponse>,
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
        self.txcmd
            .send(SentCommand {
                command,
                backchannel: tx,
            })
            .map_err(|_| AgentError::SessionClosed)?;

        let result = timeout(Duration::from_millis(1000), rx).await;

        match result {
            Ok(value) => value.map_err(|_| {
                AgentError::ExecutionError("Command response channel closed".to_string())
            }),
            Err(_) => Err(AgentError::TimeoutError),
        }
    }

    pub async fn drop(&mut self) -> Result<(), AgentError> {
        self.send(AgentRequest::Droping).await?;
        Ok(())
    }

    pub async fn terminate(&self) -> Result<(), AgentError> {
        self.send(AgentRequest::Terminate).await.map(|_| Ok(()))?
    }

    pub async fn stop_current_task(&self) -> Result<(), AgentError> {
        self.send(AgentRequest::StopCurrentTask)
            .await
            .map(|_| Ok(()))?
    }

    /// Remove the last assistant message(s) and re-think from the last user input
    pub async fn regenerate(&self) -> Result<(), AgentError> {
        self.send(AgentRequest::Regenerate).await.map(|_| Ok(()))?
    }

    pub async fn set_method(
        &self,
        method: Option<ToolCallMethod>,
    ) -> Result<ToolCallMethod, AgentError> {
        match self
            .send(AgentRequest::SwitchToolCallMethod { method })
            .await?
        {
            AgentResponse::Method { method } => Ok(method),
            _ => Err(AgentError::InvalidResponse(
                "Expected Method response".to_string(),
            )),
        }
    }

    pub async fn set_temperature(&self, temperature: f32) -> Result<f32, AgentError> {
        match self
            .send(AgentRequest::SetTemperature { temperature })
            .await?
        {
            AgentResponse::Temperature { temperature } => Ok(temperature),
            _ => Err(AgentError::InvalidResponse(
                "Expected Temperature response".to_string(),
            )),
        }
    }

    pub async fn send_user_input(&self, input: String) -> Result<(), AgentError> {
        self.send(AgentRequest::SendUserInput { input })
            .await
            .map(|_| Ok(()))?
    }

    pub async fn send_trace(&self, messages: Vec<ChatMessage>) -> Result<(), AgentError> {
        self.send(AgentRequest::SendTrace { messages })
            .await
            .map(|_| Ok(()))?
    }

    /// Load a trace without starting to think (stays paused)
    pub async fn load_trace(&self, messages: Vec<ChatMessage>) -> Result<(), AgentError> {
        self.send(AgentRequest::LoadTrace { messages })
            .await
            .map(|_| Ok(()))?
    }

    pub async fn response_user_query(
        &self,
        request_id: String,
        response: UserResponse,
    ) -> Result<(), AgentError> {
        self.send(AgentRequest::UserQueryResponse {
            request_id,
            response,
        })
        .await
        .map(|_| Ok(()))?
    }

    pub async fn response_permission_request(
        &self,
        request_id: String,
        response: PermissionResponse,
    ) -> Result<(), AgentError> {
        self.send(AgentRequest::UserPermissionResponse {
            request_id,
            response,
        })
        .await
        .map(|_| Ok(()))?
    }

    pub async fn get_state(&self) -> Result<PublicAgentState, AgentError> {
        match self.send(AgentRequest::GetState).await? {
            AgentResponse::State { state } => Ok(state),
            _ => Err(AgentError::InvalidResponse(
                "Expected State response".to_string(),
            )),
        }
    }

    pub async fn get_trace(&self) -> Result<Vec<ChatMessage>, AgentError> {
        match self.send(AgentRequest::GetTrace).await? {
            AgentResponse::Trace { trace } => Ok(trace),
            _ => Err(AgentError::InvalidResponse(
                "Expected Trace response".to_string(),
            )),
        }
    }

    /// Wait until the agent reaches the Paused state
    pub async fn wait_turn(&self, timeout_ms: Option<u64>) -> Result<(), AgentError> {
        let (tx, rx) = oneshot::channel();
        self.txcmd
            .send(SentCommand {
                command: AgentRequest::WaitTurn,
                backchannel: tx,
            })
            .map_err(|_| AgentError::SessionClosed)?;

        let response = if let Some(ms) = timeout_ms {
            timeout(Duration::from_millis(ms), rx)
                .await
                .map_err(|_| AgentError::TimeoutError)?
        } else {
            rx.await
        }
        .map_err(|_| AgentError::ExecutionError("Command response channel closed".to_string()))?;

        match response {
            AgentResponse::Ack => Ok(()),
            AgentResponse::Error { error } => Err(AgentError::ExecutionError(error)),
            _ => Err(AgentError::InvalidResponse(
                "Expected Ack response for WaitTurn".to_string(),
            )),
        }
    }

    /// Enable sudo mode - bypasses all permission checks
    pub async fn sudo(&self) -> Result<bool, AgentError> {
        match self.send(AgentRequest::Sudo(Some(true))).await? {
            AgentResponse::SudoStatus { enabled } => Ok(enabled),
            _ => Err(AgentError::InvalidResponse(
                "Expected SudoStatus response".to_string(),
            )),
        }
    }

    /// Disable sudo mode - re-enables permission checks
    pub async fn no_sudo(&self) -> Result<bool, AgentError> {
        match self.send(AgentRequest::Sudo(Some(false))).await? {
            AgentResponse::SudoStatus { enabled } => Ok(enabled),
            _ => Err(AgentError::InvalidResponse(
                "Expected SudoStatus response".to_string(),
            )),
        }
    }

    /// Check if sudo mode is enabled
    pub async fn is_sudo(&self) -> Result<bool, AgentError> {
        match self.send(AgentRequest::Sudo(None)).await? {
            AgentResponse::SudoStatus { enabled } => Ok(enabled),
            _ => Err(AgentError::InvalidResponse(
                "Expected SudoStatus response".to_string(),
            )),
        }
    }

    /// Enable plan mode - denies all tool execution (read-only)
    pub async fn plan_mode(&self) -> Result<bool, AgentError> {
        match self.send(AgentRequest::PlanMode(Some(true))).await? {
            AgentResponse::PlanModeStatus { enabled } => Ok(enabled),
            _ => Err(AgentError::InvalidResponse(
                "Expected PlanModeStatus response".to_string(),
            )),
        }
    }

    /// Disable plan mode - re-enables tool execution
    pub async fn no_plan_mode(&self) -> Result<bool, AgentError> {
        match self.send(AgentRequest::PlanMode(Some(false))).await? {
            AgentResponse::PlanModeStatus { enabled } => Ok(enabled),
            _ => Err(AgentError::InvalidResponse(
                "Expected PlanModeStatus response".to_string(),
            )),
        }
    }

    /// Check if plan mode is enabled
    pub async fn is_plan_mode(&self) -> Result<bool, AgentError> {
        match self.send(AgentRequest::PlanMode(None)).await? {
            AgentResponse::PlanModeStatus { enabled } => Ok(enabled),
            _ => Err(AgentError::InvalidResponse(
                "Expected PlanModeStatus response".to_string(),
            )),
        }
    }

    /// Set active system prompts by name
    pub async fn set_active_prompts(
        &self,
        prompts: Vec<String>,
    ) -> Result<Vec<String>, AgentError> {
        match self.send(AgentRequest::SetActivePrompts(prompts)).await? {
            AgentResponse::PromptsStatus { prompts } => Ok(prompts),
            _ => Err(AgentError::InvalidResponse(
                "Expected PromptsStatus response".to_string(),
            )),
        }
    }

    /// Get active system prompts
    pub async fn get_active_prompts(&self) -> Result<Vec<String>, AgentError> {
        match self.send(AgentRequest::SetActivePrompts(vec![])).await? {
            AgentResponse::PromptsStatus { prompts } => Ok(prompts),
            _ => Err(AgentError::InvalidResponse(
                "Expected PromptsStatus response".to_string(),
            )),
        }
    }
}
