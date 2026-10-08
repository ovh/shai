use super::InternalAgentState;
use crate::agent::{AgentCore, AgentError, InternalAgentEvent};
use tracing::error;

impl AgentCore {
    pub async fn state_running_handle_event(
        &mut self,
        event: InternalAgentEvent,
    ) -> Result<(), AgentError> {
        let InternalAgentState::Running = &self.state else {
            return Err(AgentError::InvalidState(format!(
                "state Running expected but current state is : {:?}",
                self.state.to_public()
            )));
        };

        match event {
            InternalAgentEvent::CancelTask => {
                // Silently ignore
            }
            InternalAgentEvent::ThinkingStart => {
                self.spawn_next_step().await;
            }
            _ => {
                // Running state: Most other events should be handled by main loop or are illegal
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
}
