use super::InternalAgentState;
use crate::agent::{AgentCore, AgentError, InternalAgentEvent};
use crate::tools::fs::verification::run_verification;
use openai_dive::v1::resources::chat::{ChatMessage, ChatMessageContent};

impl AgentCore {
    pub async fn state_processing_handle_event(
        &mut self,
        event: InternalAgentEvent,
    ) -> Result<(), AgentError> {
        match event {
            InternalAgentEvent::CancelTask => self.cancel_task().await,
            InternalAgentEvent::BrainResult { result } => self.process_next_step(result).await,
            InternalAgentEvent::ToolsCompleted { any_denied } => {
                if !any_denied {
                    let edited_files = self.tool_ctx.fs_operation_log.drain_edited_files().await;
                    if !edited_files.is_empty() {
                        if let Some(diagnostics) = run_verification(
                            &edited_files,
                            &self.tool_ctx.working_dir,
                            &self.tool_ctx.verification_config,
                        )
                        .await
                        {
                            self.tool_ctx.trace.write().await.push(ChatMessage::Tool {
                                tool_call_id: "verification".to_string(),
                                content: ChatMessageContent::Text(diagnostics),
                            });
                        }
                    }
                }
                if any_denied {
                    self.set_state(InternalAgentState::Paused).await;
                } else {
                    self.set_state(InternalAgentState::Running).await;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// cancel all pending tasks
    async fn cancel_task(&mut self) -> Result<(), AgentError> {
        let InternalAgentState::Processing {
            cancellation_token, ..
        } = &self.state
        else {
            return Err(AgentError::InvalidState(format!(
                "state Processing expected but current state is : {:?}",
                self.state.to_public()
            )));
        };

        cancellation_token.cancel();
        Ok(())
    }
}
