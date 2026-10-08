use super::{TodoItem, TodoStatus, TodoStorage};
use crate::tools::ToolEmptyParams;
use crate::tools::{tool, ToolResult};
use chrono::Utc;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

// Input struct for creating todos
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(inline)]
pub struct TodoItemInput {
    pub content: String,
    pub status: TodoStatus,
}

impl From<TodoItemInput> for TodoItem {
    fn from(input: TodoItemInput) -> Self {
        let now = Utc::now().to_rfc3339();
        Self {
            id: Uuid::new_v4().to_string(),
            content: input.content,
            status: input.status,
            created_at: now.clone(),
            updated_at: now,
        }
    }
}

// Read Tool
#[derive(Clone)]
pub struct TodoReadTool {
    storage: Arc<TodoStorage>,
}

#[tool(
    name = "todo_read",
    description = "Fetches the current to-do list for the session. Use this proactively to stay informed about the status of ongoing tasks."
)]
impl TodoReadTool {
    pub fn new(storage: Arc<TodoStorage>) -> Self {
        Self { storage }
    }

    async fn execute(&self, _params: ToolEmptyParams) -> ToolResult {
        let todos = self.storage.get_all().await;

        let output = self.storage.format_all(&todos);

        ToolResult::Success {
            output,
            metadata: Some({
                let mut meta = HashMap::new();
                meta.insert("todo_count".to_string(), json!(todos.len()));
                meta
            }),
        }
    }
}

// Write Tool Parameters
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TodoWriteParams {
    /// List of todos to write (replaces entire list)
    pub todos: Vec<TodoItemInput>,
}

// Write Tool
#[derive(Clone)]
pub struct TodoWriteTool {
    storage: Arc<TodoStorage>,
}

#[tool(
    name = "todo_write",
    description = r#"Creates and manages a structured task list for the coding session. This is vital for organizing complex work, tracking progress, and showing a clear plan.

**Examples:**
Create tasks:
```json
{"todos": [{"content": "Implement the feature", "status": "in_progress"}, {"content": "Write tests", "status": "pending"}]}
```
Mark a task as completed:
```json
{"todos": [{"content": "Implement the feature", "status": "completed"}, {"content": "Write tests", "status": "in_progress"}]}
```
"#
)]
impl TodoWriteTool {
    pub fn new(storage: Arc<TodoStorage>) -> Self {
        Self { storage }
    }

    async fn execute(&self, params: TodoWriteParams) -> ToolResult {
        // Convert input todos to full TodoItems
        let todo_items: Vec<TodoItem> =
            params.todos.into_iter().map(|input| input.into()).collect();

        // Replace entire list
        self.storage.replace_all(todo_items.clone()).await;

        let output = self.storage.format_all(&todo_items);
        let summary = format!("Updated {} todo items\n{}", todo_items.len(), output);

        ToolResult::Success {
            output: summary,
            metadata: Some({
                let mut meta = HashMap::new();
                meta.insert("todo_count".to_string(), json!(todo_items.len()));
                meta
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shai_llm::ToolDescription;

    #[test]
    fn test_todo_read_json_schema() {
        let store = TodoStorage::new();
        let tool = TodoReadTool::new(Arc::new(store));
        let schema = tool.parameters_schema();
        println!("{}", serde_json::to_string_pretty(&schema).unwrap());
    }

    #[test]
    fn test_todo_write_json_schema() {
        let store = TodoStorage::new();
        let tool = TodoWriteTool::new(Arc::new(store));
        let schema = tool.parameters_schema();
        println!("{}", serde_json::to_string_pretty(&schema).unwrap());
    }
}
