use std::collections::HashMap;
use std::time::{Duration, Instant};

use shai_core::agent::output::PrettyFormatter;
use shai_core::tools::{ToolCall, ToolResult};

pub struct ToolTracker {
    running: HashMap<String, ToolCall>,
    start_times: HashMap<String, Instant>,
    last_output: Option<String>,
    last_file_path: Option<String>,
}

impl ToolTracker {
    pub fn new() -> Self {
        Self {
            running: HashMap::new(),
            start_times: HashMap::new(),
            last_output: None,
            last_file_path: None,
        }
    }

    pub fn start_tool(&mut self, call: ToolCall) {
        self.start_times
            .insert(call.tool_call_id.clone(), Instant::now());
        self.running.insert(call.tool_call_id.clone(), call);
    }

    pub fn complete_tool(&mut self, call: &ToolCall, result: &ToolResult) {
        self.running.remove(&call.tool_call_id);
        self.start_times.remove(&call.tool_call_id);
        if let ToolResult::Success { output, .. } = result {
            let file_path =
                PrettyFormatter::extract_primary_param(&call.parameters, &call.tool_name)
                    .map(|(_, path)| path);
            self.last_output = Some(output.clone());
            self.last_file_path = file_path;
        }
    }

    pub fn is_empty(&self) -> bool {
        self.running.is_empty()
    }

    pub fn len(&self) -> usize {
        self.running.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &ToolCall)> {
        self.running.iter()
    }

    pub fn elapsed(&self, call_id: &str) -> Option<Duration> {
        self.start_times.get(call_id).map(|t| t.elapsed())
    }

    pub fn last_output(&self) -> Option<&str> {
        self.last_output.as_deref()
    }

    pub fn last_file_path(&self) -> Option<&str> {
        self.last_file_path.as_deref()
    }
}

impl Default for ToolTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::test_utils::make_tool_call;

    #[test]
    fn test_start_and_complete_tool() {
        let mut tracker = ToolTracker::new();
        let call = make_tool_call("tool1", "read");
        tracker.start_tool(call.clone());

        assert_eq!(tracker.len(), 1);
        assert!(!tracker.is_empty());

        let result = ToolResult::Success {
            output: "file contents".to_string(),
            metadata: None,
        };
        tracker.complete_tool(&call, &result);

        assert_eq!(tracker.len(), 0);
        assert!(tracker.is_empty());
        assert_eq!(tracker.last_output(), Some("file contents"));
        assert_eq!(tracker.last_file_path(), Some("/tmp/test.txt"));
    }

    #[test]
    fn test_complete_with_error_result() {
        let mut tracker = ToolTracker::new();
        let call = make_tool_call("tool1", "read");
        tracker.start_tool(call.clone());

        let result = ToolResult::Error {
            error: "permission denied".to_string(),
            metadata: None,
        };
        tracker.complete_tool(&call, &result);

        assert_eq!(tracker.len(), 0);
        assert_eq!(tracker.last_output(), None);
        assert_eq!(tracker.last_file_path(), None);
    }

    #[test]
    fn test_elapsed_returns_none_after_complete() {
        let mut tracker = ToolTracker::new();
        let call = make_tool_call("tool1", "read");
        tracker.start_tool(call.clone());

        assert!(tracker.elapsed("tool1").is_some());

        let result = ToolResult::Success {
            output: "ok".to_string(),
            metadata: None,
        };
        tracker.complete_tool(&call, &result);

        assert!(tracker.elapsed("tool1").is_none());
    }

    #[test]
    fn test_iter() {
        let mut tracker = ToolTracker::new();
        tracker.start_tool(make_tool_call("tool1", "read"));
        tracker.start_tool(make_tool_call("tool2", "write"));

        let ids: Vec<&str> = tracker.iter().map(|(id, _)| id.as_str()).collect();
        assert!(ids.contains(&"tool1"));
        assert!(ids.contains(&"tool2"));
        assert_eq!(ids.len(), 2);
    }
}
