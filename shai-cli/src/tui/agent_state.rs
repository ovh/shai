use openai_dive::v1::resources::chat::{ChatMessage, ChatMessageContent};
use shai_core::agent::events::AgentEvent;
use shai_core::tools::todo::TodoItem;

use super::mcp_manager::McpManager;
use super::perm_manager::PermissionManager;
use super::session_manager::SessionManager;
use super::token_counter::TokenCounter;
use super::tool_tracker::ToolTracker;

pub struct AgentState {
    tool_tracker: ToolTracker,
    token_counter: TokenCounter,
    permission_manager: PermissionManager,
    session_manager: SessionManager,
    mcp_manager: McpManager,
    todos: Vec<TodoItem>,
}

impl AgentState {
    pub fn new() -> Self {
        Self {
            tool_tracker: ToolTracker::new(),
            token_counter: TokenCounter::new(),
            permission_manager: PermissionManager::new(),
            session_manager: SessionManager::new(),
            mcp_manager: McpManager::new(),
            todos: Vec::new(),
        }
    }

    pub fn tool_tracker(&self) -> &ToolTracker {
        &self.tool_tracker
    }

    #[allow(dead_code)] // symmetric accessor for tool_tracker
    pub fn tool_tracker_mut(&mut self) -> &mut ToolTracker {
        &mut self.tool_tracker
    }

    pub fn token_counter(&self) -> &TokenCounter {
        &self.token_counter
    }

    pub fn permission_manager(&self) -> &PermissionManager {
        &self.permission_manager
    }

    pub fn permission_manager_mut(&mut self) -> &mut PermissionManager {
        &mut self.permission_manager
    }

    pub fn session_manager(&self) -> &SessionManager {
        &self.session_manager
    }

    pub fn session_manager_mut(&mut self) -> &mut SessionManager {
        &mut self.session_manager
    }

    pub fn mcp_manager(&self) -> &McpManager {
        &self.mcp_manager
    }

    pub fn mcp_manager_mut(&mut self) -> &mut McpManager {
        &mut self.mcp_manager
    }

    pub fn todos(&self) -> &[TodoItem] {
        &self.todos
    }
}

#[async_trait::async_trait]
impl super::handler::AgentHandler for AgentState {
    async fn handle_event(&mut self, event: &AgentEvent) {
        match event {
            AgentEvent::ToolCallStarted { call, .. } => {
                self.tool_tracker.start_tool(call.clone());
            }
            AgentEvent::ToolCallCompleted { call, result, .. } => {
                self.tool_tracker.complete_tool(call, result);
            }
            AgentEvent::PermissionRequired {
                request_id,
                request,
                ..
            } => {
                self.permission_manager
                    .push(request_id.clone(), request.clone());
            }
            AgentEvent::TokenUsage {
                input_tokens,
                output_tokens,
                cached_tokens,
            } => {
                self.token_counter
                    .add(*input_tokens, *output_tokens, *cached_tokens);
            }
            AgentEvent::TodoUpdated { todos } => {
                self.todos = todos.clone();
            }
            AgentEvent::BrainResult {
                thought:
                    Ok(ChatMessage::Assistant {
                        content: Some(ChatMessageContent::Text(text)),
                        ..
                    }),
                ..
            } if !text.trim().is_empty() => {
                self.session_manager.set_last_assistant_response(text);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::handler::AgentHandler;
    use super::*;
    use shai_core::agent::events::PermissionRequest;

    use super::super::test_utils::make_tool_call;

    #[tokio::test]
    async fn test_handle_tool_started() {
        let mut state = AgentState::new();
        let call = make_tool_call("tool1", "read");
        let event = AgentEvent::ToolCallStarted {
            timestamp: chrono::Utc::now(),
            call,
        };
        state.handle_event(&event).await;
        assert_eq!(state.tool_tracker.len(), 1);
    }

    #[tokio::test]
    async fn test_handle_token_usage() {
        let mut state = AgentState::new();
        let event = AgentEvent::TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            cached_tokens: 25,
        };
        state.handle_event(&event).await;
        assert_eq!(state.token_counter.input_tokens(), 100);
        assert_eq!(state.token_counter.output_tokens(), 50);
        assert_eq!(state.token_counter.cached_tokens(), 25);
    }

    #[tokio::test]
    async fn test_handle_permission_required() {
        let mut state = AgentState::new();
        let request = PermissionRequest {
            tool_name: "read".to_string(),
            operation: "read file".to_string(),
            call: make_tool_call("tool1", "read"),
            preview: None,
        };
        let event = AgentEvent::PermissionRequired {
            request_id: "req-1".to_string(),
            request,
        };
        state.handle_event(&event).await;
        assert_eq!(state.permission_manager.len(), 1);
    }

    #[tokio::test]
    async fn test_handle_brain_result_sets_last_assistant_response() {
        let mut state = AgentState::new();
        let event = AgentEvent::BrainResult {
            timestamp: chrono::Utc::now(),
            thought: Ok(ChatMessage::Assistant {
                content: Some(ChatMessageContent::Text("Hello world".to_string())),
                tool_calls: None,
                name: None,
                audio: None,
                reasoning: None,
                reasoning_content: None,
                refusal: None,
            }),
        };
        state.handle_event(&event).await;
        assert_eq!(
            state.session_manager.last_assistant_response(),
            "Hello world"
        );
    }

    #[tokio::test]
    async fn test_handle_todo_updated_stores_todos() {
        use shai_core::tools::todo::{TodoItem, TodoStatus};

        let mut state = AgentState::new();
        assert!(state.todos().is_empty());

        let now = chrono::Utc::now().to_rfc3339();
        let event = AgentEvent::TodoUpdated {
            todos: vec![
                TodoItem {
                    id: "1".to_string(),
                    content: "explore repo".to_string(),
                    status: TodoStatus::Completed,
                    created_at: now.clone(),
                    updated_at: now.clone(),
                },
                TodoItem {
                    id: "2".to_string(),
                    content: "implement feature".to_string(),
                    status: TodoStatus::InProgress,
                    created_at: now.clone(),
                    updated_at: now,
                },
            ],
        };
        state.handle_event(&event).await;

        assert_eq!(state.todos().len(), 2);
        assert_eq!(state.todos()[1].content, "implement feature");
    }
}
