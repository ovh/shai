use std::fs::OpenOptions;
use std::io::Write;
use async_trait::async_trait;
use chrono::Utc;
use crate::agent::{AgentEvent, AgentEventHandler};

/// File logger that writes all agent events to a debug log file
pub struct FileEventLogger {
    log_path: String,
}

impl FileEventLogger {
    pub fn new(log_path: impl Into<String>) -> Self {
        Self {
            log_path: log_path.into(),
        }
    }

    pub fn default() -> Self {
        Self::new("agent_events.log")
    }

    fn write_event(&self, event: &AgentEvent) {
        let timestamp = Utc::now();
        let event_str = match event {
            AgentEvent::StatusChanged { old_status, new_status } => {
                format!("StatusChanged: {:?} -> {:?}", old_status, new_status)
            }
            AgentEvent::ThinkingStart => {
                format!("ThinkingStart")
            }
            AgentEvent::BrainResult { timestamp: event_time, thought } => {
                format!("BrainResult: {:?} - {:?}", event_time, thought)
            }
            AgentEvent::ToolCallStarted { timestamp: event_time, call } => {
                format!("ToolCallStarted: {:?} - {}", event_time, call.tool_name)
            }
            AgentEvent::ToolCallCompleted { duration, call, result } => {
                format!("ToolCallCompleted: {} in {:?} - {:?}", call.tool_name, duration, result)
            }
            AgentEvent::UserInput { input } => {
                format!("UserInput: {}", input)
            }
            AgentEvent::UserInputRequired { request_id, request } => {
                format!("UserInputRequired: {} - {:?}", request_id, request)
            }
            AgentEvent::PermissionRequired { request_id, request } => {
                format!("PermissionRequired: {} - {}", request_id, request.operation)
            }
            AgentEvent::Error { error } => {
                format!("Error: {}", error)
            }
            AgentEvent::Completed { success, message } => {
                format!("Completed: success={} - {}", success, message)
            }
            AgentEvent::TokenUsage { input_tokens, output_tokens } => {
                format!("Token Usage: input={} output={} total={}", input_tokens, output_tokens, input_tokens + output_tokens)
            }
            AgentEvent::ContextCompressed { original_message_count, compressed_message_count, tokens_before, current_tokens, max_tokens, ai_summary } => {
                let summary_text = if let Some(summary) = ai_summary {
                    format!(" | Summary: {}", summary)
                } else {
                    "".to_string()
                };

                let token_info = match (tokens_before, current_tokens) {
                    (Some(before), Some(after)) => format!(", tokens: {} → {}", before, after),
                    (Some(before), None) => format!(", tokens before: {}", before),
                    _ => "".to_string(),
                };

                format!("Context Compressed with AI Summary: {} → {} messages{}{}",
                    original_message_count, compressed_message_count, token_info, summary_text)
            }
        };

        let log_line = format!("[{}] {}\n", timestamp.format("%Y-%m-%d %H:%M:%S%.3f"), event_str);

        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
        {
            let _ = file.write_all(log_line.as_bytes());
            let _ = file.flush();
        }
    }
}

#[async_trait]
impl AgentEventHandler for FileEventLogger {
    async fn handle_event(&self, event: AgentEvent) {
        self.write_event(&event);
    }
}

impl Default for FileEventLogger {
    fn default() -> Self {
        Self::default()
    }
}