use chrono::Utc;
use shai_llm::ChatMessage;
use tracing::info;
use tokio_util::sync::CancellationToken;
use crate::agent::{AgentCore, AgentError, AgentEvent, InternalAgentEvent, InternalAgentState, ThinkerContext, ThinkerDecision, ThinkerFlowControl};

impl AgentCore {
    /// Launch a brain task to decide next step
    pub async fn spawn_next_step(&mut self) {         
        let cancellation_token = CancellationToken::new();
        let cancel_token_clone = cancellation_token.clone();
        let trace = self.trace.clone();
        let tx_clone = self.internal_tx.clone();
        let available_tools = self.available_tools.clone();
        let method = self.method.clone();
        let context = ThinkerContext {
            trace,
            available_tools,
            method
        };
        let brain = self.brain.clone();
        
        //////////////////////// TOKIO SPAWN
        tokio::spawn(async move {
            tokio::select! {
                result = async {
                    brain.write().await.next_step(context).await
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
            cancellation_token
        }).await;
    }


    /// Process a brain task result
    pub async fn process_next_step(&mut self, result: Result<ThinkerDecision, AgentError>) -> Result<(), AgentError> {
        let _ = self.check_and_compress_context().await?;
        let ThinkerDecision{message, flow, token_usage, compression_info} = self.handle_brain_error(result).await?;
        let ChatMessage::Assistant { content, reasoning_content, tool_calls, .. } = message.clone() else {
            return self.handle_brain_error::<ThinkerDecision>(
                Err(AgentError::InvalidResponse(format!("ChatMessage::Assistant expected, but got {:?} instead", message)))).await.map(|_| ()
            );
        };
    
        // Add the message to trace
        info!(target: "agent::think", reasoning_content = ?reasoning_content, content = ?content);
        let trace = self.trace.clone();
        let full_trace = self.full_trace.clone();
        trace.write().await.push(message.clone());
        full_trace.write().await.push(message.clone());

        // Emit event to external consumers
        let _ = self.emit_event(AgentEvent::BrainResult {
            timestamp: Utc::now(),
            thought: Ok(message.clone())
        }).await;

        // Emit token usage event if available
        if let Some((input_tokens, output_tokens)) = token_usage {
            let _ = self.emit_event(AgentEvent::TokenUsage {
                input_tokens,
                output_tokens
            }).await;
        }

        // Emit context compression event if available
        if let Some(compression_info) = compression_info {
            let _ = self.emit_event(AgentEvent::ContextCompressed {
                original_message_count: compression_info.original_message_count,
                compressed_message_count: compression_info.compressed_message_count,
                tokens_before: compression_info.tokens_before,
                current_tokens: compression_info.current_tokens,
                max_tokens: compression_info.max_tokens,
                ai_summary: compression_info.ai_summary,
            }).await;
        }
    
        // run tool call if any
        let tool_calls_from_brain = tool_calls.unwrap_or(vec![]);
        if !tool_calls_from_brain.is_empty() {
            self.spawn_tools(tool_calls_from_brain).await;
            return Ok(())
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

    /// Trigger manual context compression regardless of threshold
    pub async fn check_and_compress_context_manual(&mut self) -> Result<(), AgentError> {
        // Set state to Processing to block new messages
        self.set_state(InternalAgentState::Processing {
            task_name: "context_compression".to_string(),
            tools_exec_at: Utc::now(),
            cancellation_token: CancellationToken::new(),
        }).await;

        let brain = self.brain.clone();
        let brain_read = brain.read().await;

        use std::any::Any;

        if let Some(coder_brain) = (&**brain_read as &dyn Any).downcast_ref::<crate::runners::coder::coder::CoderBrain>() {
            if let Some(compressor) = &coder_brain.context_compressor {
                let compressor_clone = compressor.clone();
                drop(brain_read);

                let trace = self.trace.read().await.clone();
                let full_trace = self.full_trace.read().await.clone();
                let mut compressor_clone = compressor_clone;

                // Force compression - manually call compress_messages_force
                let (compressed_trace, compression_info) = compressor_clone.compress_messages_force(trace, full_trace).await;

                // Update the trace with compressed version
                {
                    let mut trace_write = self.trace.write().await;
                    *trace_write = compressed_trace;
                }

                // Update the compressor in the brain
                {
                    let mut brain_write = brain.write().await;
                    if let Some(coder_brain_mut) = (&mut **brain_write as &mut dyn Any).downcast_mut::<crate::runners::coder::coder::CoderBrain>() {
                        coder_brain_mut.context_compressor = Some(compressor_clone);
                    }
                }

                // Emit compression event if compression occurred
                if let Some(compression_info) = compression_info {
                    let _ = self.emit_event(AgentEvent::ContextCompressed {
                        original_message_count: compression_info.original_message_count,
                        compressed_message_count: compression_info.compressed_message_count,
                        tokens_before: compression_info.tokens_before,
                        current_tokens: compression_info.current_tokens,
                        max_tokens: compression_info.max_tokens,
                        ai_summary: compression_info.ai_summary,
                    }).await;
                }
            }
        }

        // Return to Paused state after compression
        self.set_state(InternalAgentState::Paused).await;
        Ok(())
    }

    /// Check if context compression is needed and apply it when task is complete
    async fn check_and_compress_context(&mut self) -> Result<(), AgentError> {
        // Extract compression logic from the brain if it's a CoderBrain
        let brain = self.brain.clone();
        let brain_read = brain.read().await;

        // This is a bit hacky but we need to check if the brain has a compressor
        // We'll use Any trait to downcast to CoderBrain
        use std::any::Any;

        if let Some(coder_brain) = (&**brain_read as &dyn Any).downcast_ref::<crate::runners::coder::coder::CoderBrain>() {
            if let Some(compressor) = &coder_brain.context_compressor {
                let compressor_clone = compressor.clone();
                drop(brain_read); // Release the read lock

                let trace = self.trace.read().await.clone();
                let mut compressor_clone = compressor_clone;

                if compressor_clone.should_compress_conversation(&trace) {
                    // Set state to Processing to block new messages during compression
                    self.set_state(InternalAgentState::Processing {
                        task_name: "context_compression".to_string(),
                        tools_exec_at: Utc::now(),
                        cancellation_token: CancellationToken::new(),
                    }).await;

                    let full_trace = self.full_trace.read().await.clone();
                    let (compressed_trace, compression_info) = compressor_clone.compress_messages(trace, full_trace).await;

                    // Update the trace with compressed version
                    {
                        let mut trace_write = self.trace.write().await;
                        *trace_write = compressed_trace;
                    }

                    // Update the compressor in the brain
                    {
                        let mut brain_write = brain.write().await;
                        if let Some(coder_brain_mut) = (&mut **brain_write as &mut dyn Any).downcast_mut::<crate::runners::coder::coder::CoderBrain>() {
                            coder_brain_mut.context_compressor = Some(compressor_clone);
                        }
                    }

                    // Emit compression event if compression occurred
                    if let Some(compression_info) = compression_info {
                        let _ = self.emit_event(AgentEvent::ContextCompressed {
                            original_message_count: compression_info.original_message_count,
                            compressed_message_count: compression_info.compressed_message_count,
                            tokens_before: compression_info.tokens_before,
                            current_tokens: compression_info.current_tokens,
                            max_tokens: compression_info.max_tokens,
                            ai_summary: compression_info.ai_summary,
                        }).await;
                    }
                }
            }
        }

        Ok(())
    }

    // Helper method that emits error events before returning the error
    async fn handle_brain_error<T>(&mut self, result: Result<T, AgentError>) -> Result<T, AgentError> {
        match result {
            Ok(value) => Ok(value),
            Err(error) => {
                self.set_state(InternalAgentState::Paused).await;
                let _ = self.emit_event(AgentEvent::BrainResult {
                    timestamp: Utc::now(),
                    thought: Err(error.clone())
                }).await;
                Err(error)
            }
        }
    }
}