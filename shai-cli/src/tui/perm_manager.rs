use std::collections::VecDeque;

use shai_core::agent::events::PermissionRequest;

pub struct PermissionManager {
    queue: VecDeque<(String, PermissionRequest)>,
}

impl PermissionManager {
    pub fn new() -> Self {
        Self {
            queue: VecDeque::new(),
        }
    }

    pub fn push(&mut self, request_id: String, request: PermissionRequest) {
        self.queue.push_back((request_id, request));
    }

    pub fn pop(&mut self) -> Option<(String, PermissionRequest)> {
        self.queue.pop_front()
    }

    pub fn front(&self) -> Option<&(String, PermissionRequest)> {
        self.queue.front()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }
}

impl Default for PermissionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_request() -> PermissionRequest {
        PermissionRequest {
            tool_name: "read".to_string(),
            operation: "read file".to_string(),
            call: shai_core::tools::ToolCall {
                tool_call_id: "test-id".to_string(),
                tool_name: "read".to_string(),
                parameters: serde_json::json!({"path": "/tmp/test.txt"}),
            },
            preview: None,
        }
    }

    #[test]
    fn test_push_and_pop() {
        let mut pm = PermissionManager::new();
        assert!(pm.is_empty());

        pm.push("req-1".to_string(), make_request());
        assert_eq!(pm.len(), 1);
        assert!(!pm.is_empty());

        let (id, _) = pm.pop().unwrap();
        assert_eq!(id, "req-1");
        assert!(pm.is_empty());
    }

    #[test]
    fn test_front_does_not_remove() {
        let mut pm = PermissionManager::new();
        pm.push("req-1".to_string(), make_request());
        pm.push("req-2".to_string(), make_request());

        assert_eq!(pm.len(), 2);
        let front = pm.front().unwrap();
        assert_eq!(front.0, "req-1");
        assert_eq!(pm.len(), 2);
    }

    #[test]
    fn test_pop_empty_returns_none() {
        let mut pm = PermissionManager::new();
        assert!(pm.pop().is_none());
    }
}
