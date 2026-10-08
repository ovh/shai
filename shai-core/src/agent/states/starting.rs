use super::InternalAgentState;
use crate::agent::{AgentCore, AgentError, InternalAgentEvent};
use openai_dive::v1::resources::chat::ChatMessage;
use tracing::error;

impl AgentCore {
    pub async fn state_starting_handle_event(
        &mut self,
        event: InternalAgentEvent,
    ) -> Result<(), AgentError> {
        let InternalAgentState::Starting = &self.state else {
            return Err(AgentError::InvalidState(format!(
                "state Starting expected but current state is : {:?}",
                self.state.to_public()
            )));
        };

        match event {
            InternalAgentEvent::AgentInitialized => {
                self.handle_agent_initialized().await;
            }
            _ => {
                // ignore all events but log error
                error!(
                    "event {:?} unexpected in state {:?}",
                    event,
                    self.state.to_public()
                );
            }
        }
        Ok(())
    }

    /// Handle agent initialization - move from Starting to Running or Paused based on goal
    async fn handle_agent_initialized(&mut self) {
        let trace = self.tool_ctx.trace.clone();
        let guard = trace.read().await;
        if let Some(ChatMessage::User { .. }) = guard.last() {
            self.set_state(InternalAgentState::Running).await;
        } else {
            self.set_state(InternalAgentState::Paused).await;
        }
    }
}
