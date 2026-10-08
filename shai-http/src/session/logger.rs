use shai_core::agent::AgentEvent;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use tracing::{debug, error, info};

fn color_for_session(session_id: &str) -> u8 {
    let mut hasher = DefaultHasher::new();
    session_id.hash(&mut hasher);
    let hash = hasher.finish();
    // pick one of 216 “cube” colors from 16–231
    16 + (hash % 216) as u8
}

pub fn colored_session_id(session_id: &str) -> String {
    let color = color_for_session(session_id);
    format!("\x1b[38;5;{}msid={}\x1b[0m", color, session_id)
}

pub fn log_event(event: &AgentEvent, session_id: &str) {
    let session_id = colored_session_id(session_id);
    match event {
        AgentEvent::ToolCallStarted { call, .. } => {
            debug!("{} - ToolCall: {}", session_id, call.tool_name);
        }
        AgentEvent::ToolCallCompleted {
            call,
            result,
            duration,
            ..
        } => {
            use shai_core::tools::ToolResult;
            match result {
                ToolResult::Success { .. } => {
                    debug!(
                        "{} - ToolResult: {} ✓ ({}ms)",
                        session_id,
                        call.tool_name,
                        duration.num_milliseconds()
                    );
                }
                ToolResult::Error { error, .. } => {
                    let error_oneline = error.lines().next().unwrap_or(error);
                    debug!(
                        "{} - ToolResult: {} ✗ {}",
                        session_id, call.tool_name, error_oneline
                    );
                }
                ToolResult::Denied => {
                    debug!("{} - ToolResult: {} ⊘ denied", session_id, call.tool_name);
                }
            }
        }
        AgentEvent::BrainResult { .. } => {
            debug!("{} - BrainResult", session_id);
        }
        AgentEvent::StatusChanged {
            old_status,
            new_status,
        } => {
            debug!(
                "{} - Status: {:?} ← {:?}",
                session_id, new_status, old_status
            );
        }
        AgentEvent::Error { error } => {
            error!("{} - Error: {}", session_id, error);
        }
        AgentEvent::Completed { success, message } => {
            info!(
                "{} - Completed: success={} msg={}",
                session_id, success, message
            );
        }
        _ => {}
    }
}
