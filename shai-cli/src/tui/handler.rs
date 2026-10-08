use shai_core::agent::events::AgentEvent;

#[async_trait::async_trait]
pub trait AgentHandler {
    async fn handle_event(&mut self, event: &AgentEvent);
}
