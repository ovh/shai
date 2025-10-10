use crate::agent::{AgentCore, InternalAgentEvent, AgentError};
use tracing::error;

impl AgentCore {
    pub async fn state_pause_handle_event(&mut self, event: InternalAgentEvent) -> Result<(), AgentError> {
        match event {
            InternalAgentEvent::CancelTask => {
                // Silently ignore
                Ok(())
            }
            InternalAgentEvent::ManualCompressionRequested => {
                // Trigger manual context compression
                self.check_and_compress_context_manual().await
            }
            _ => {
                // Paused state: All other events are illegal until user send something
                // ignore all events but log error
                error!("event {:?} unexpected in state {:?}", event, self.state.to_public());
                Ok(())
            }
        }
    }
}