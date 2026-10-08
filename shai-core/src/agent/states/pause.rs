use crate::agent::{AgentCore, AgentError, InternalAgentEvent};
use tracing::error;

impl AgentCore {
    pub async fn state_pause_handle_event(
        &mut self,
        event: InternalAgentEvent,
    ) -> Result<(), AgentError> {
        match event {
            InternalAgentEvent::CancelTask => {
                // Silently ignore
                Ok(())
            }
            _ => {
                // Paused state: All other events are illegal until user send something
                // ignore all events but log error
                error!(
                    "event {:?} unexpected in state {:?}",
                    event,
                    self.state.to_public()
                );
                Ok(())
            }
        }
    }
}
