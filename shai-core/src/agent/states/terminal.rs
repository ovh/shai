use crate::agent::{AgentCore, AgentError, InternalAgentEvent};
use tracing::error;

impl AgentCore {
    pub async fn state_terminal_handle_event(
        &mut self,
        event: InternalAgentEvent,
    ) -> Result<(), AgentError> {
        // ignore all events but log error
        error!(
            "event {:?} unexpected in state {:?}",
            event,
            self.state.to_public()
        );
        Ok(())
    }
}
