use crate::agent::brain::ToolBudgetRef;
use crate::agent::{
    AgentCore, AgentError, AgentEvent, InternalAgentEvent, InternalAgentState, ThinkerContext,
    ThinkerDecision, ThinkerFlowControl,
};
use chrono::Utc;
use openai_dive::v1::resources::chat::{ChatMessage, ChatMessageContent};
use tokio_util::sync::CancellationToken;
use tracing::info;

impl AgentCore {
    /// Launch a brain task to decide next step
    pub async fn spawn_next_step(&mut self) {
        let cancellation_token = CancellationToken::new();
        let cancel_token_clone = cancellation_token.clone();
        let trace = self.tool_ctx.trace.read().await.clone();
        let tx_clone = self.tool_ctx.internal_tx.clone();
        let available_tools = self.tool_ctx.available_tools.clone();
        let method = self.method.clone();
        let max_trace_chars = self.tool_ctx.compaction_config.max_trace_chars;
        let temperature = *self.temperature.read().await;
        let is_plan_mode = self.tool_ctx.claims.read().await.is_plan_mode();
        let active_prompts = self.tool_ctx.claims.read().await.active_prompts().to_vec();
        let tool_call_count = *self.tool_ctx.tool_budget.count.read().await;
        let max_tool_calls = self.tool_ctx.tool_budget.max_calls;
        let soft_tool_calls = self.tool_ctx.tool_budget.soft_limit;
        let budget = ToolBudgetRef {
            count: tool_call_count,
            max_calls: max_tool_calls,
            soft_limit: soft_tool_calls,
        };
        let context = ThinkerContext {
            trace,
            available_tools,
            method,
            max_trace_chars,
            temperature,
            is_plan_mode,
            active_prompts,
            tool_call_metadata: self.tool_ctx.tool_cache.tool_call_metadata.clone(),
        };
        let brain = self.brain.clone();

        //////////////////////// TOKIO SPAWN
        tokio::spawn(async move {
            tokio::select! {
                result = async {
                    brain.write().await.next_step(context, budget).await
                } => {
                    let _ = tx_clone.send(InternalAgentEvent::BrainResult {
                        result
                    });
                }
                _ = cancel_token_clone.cancelled() => {
                    // Brain thinking was cancelled, no need to send result
                }
            }
        });
        //////////////////////// TOKIO SPAWN

        self.set_state(InternalAgentState::Processing {
            task_name: "next_step".to_string(),
            tools_exec_at: Utc::now(),
            cancellation_token,
        })
        .await;
    }

    /// Process a brain task result
    pub async fn process_next_step(
        &mut self,
        result: Result<ThinkerDecision, AgentError>,
    ) -> Result<(), AgentError> {
        let ThinkerDecision {
            message,
            flow,
            token_usage,
        } = self.handle_brain_error(result).await?;
        let ChatMessage::Assistant {
            content,
            reasoning_content,
            tool_calls,
            ..
        } = message.clone()
        else {
            return self
                .handle_brain_error::<ThinkerDecision>(Err(AgentError::InvalidResponse(format!(
                    "ChatMessage::Assistant expected, but got {:?} instead",
                    message
                ))))
                .await
                .map(|_| ());
        };

        // Add the message to trace
        info!(target: "agent::think", reasoning_content = ?reasoning_content, content = ?content);
        let trace = self.tool_ctx.trace.clone();
        trace.write().await.push(message.clone());

        // Emit event to external consumers
        let _ = self
            .emit_event(AgentEvent::BrainResult {
                timestamp: Utc::now(),
                thought: Ok(message.clone()),
            })
            .await;

        // Emit token usage event if available
        if let Some((input_tokens, output_tokens, cached_tokens)) = token_usage {
            let _ = self
                .emit_event(AgentEvent::TokenUsage {
                    input_tokens,
                    output_tokens,
                    cached_tokens,
                })
                .await;
        }

        // run tool call if any
        let tool_calls_from_brain = tool_calls.unwrap_or(vec![]);
        if !tool_calls_from_brain.is_empty() {
            // Check max tool calls per turn limit
            if !self.tool_ctx.tool_budget.try_increment(tool_calls_from_brain.len()).await {
                let max_tool_calls = self.tool_ctx.tool_budget.max_calls.unwrap();
                // Inject a wrap-up message for each tool call to satisfy the LLM's tool_call_id requirements
                let wrap_up = format!(
                    "You have reached the maximum number of tool calls ({}) for this turn. Please summarize what you've accomplished and provide your final answer.",
                    max_tool_calls
                );
                for tc in &tool_calls_from_brain {
                    self.tool_ctx.trace.write().await.push(ChatMessage::Tool {
                        tool_call_id: tc.id.clone(),
                        content: ChatMessageContent::Text(wrap_up.clone()),
                    });
                }
                self.set_state(InternalAgentState::Running).await;
                return Ok(());
            }
            self.spawn_tools(tool_calls_from_brain).await;
            return Ok(());
        }

        // no tool call, thus we rely on flow control
        match flow {
            ThinkerFlowControl::AgentContinue => {
                self.set_state(InternalAgentState::Running).await;
            }
            ThinkerFlowControl::AgentPause => {
                self.set_state(InternalAgentState::Paused).await;
            }
        }
        Ok(())
    }

    // Helper method that emits error events before returning the error
    async fn handle_brain_error<T>(
        &mut self,
        result: Result<T, AgentError>,
    ) -> Result<T, AgentError> {
        match result {
            Ok(value) => Ok(value),
            Err(error) => {
                self.set_state(InternalAgentState::Paused).await;
                let _ = self
                    .emit_event(AgentEvent::BrainResult {
                        timestamp: Utc::now(),
                        thought: Err(error.clone()),
                    })
                    .await;
                Err(error)
            }
        }
    }
}
