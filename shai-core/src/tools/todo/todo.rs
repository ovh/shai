use super::{TodoItem, TodoStatus, TodoStorage};
use crate::tools::ToolEmptyParams;
use crate::tools::{ToolResult, tool};
use std::sync::Arc;
use serde_json::json;
use serde::{Deserialize, Serialize};
use schemars::JsonSchema;
use std::collections::HashMap;
use chrono::Utc;
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
    storage: Arc<TodoStorage>
}

#[tool(name = "todo_read", description = "Fetches the current to-do list for the session. Use this proactively to stay informed about the status of ongoing tasks.")]
impl TodoReadTool {
    pub fn new(storage: Arc<TodoStorage>) -> Self {
        Self { storage }
    }
    
    async fn execute(&self, params: ToolEmptyParams) -> ToolResult {
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
    storage: Arc<TodoStorage>
}

#[tool(name = "todo_write", description = "Creates and manages a structured task list for the coding session. This is vital for organizing complex work, tracking progress, and showing a clear plan.")]
impl TodoWriteTool {
    pub fn new(storage: Arc<TodoStorage>) -> Self {
        Self { storage }
    }
    
    async fn execute(&self, params: TodoWriteParams) -> ToolResult {
        // Convert input todos to full TodoItems
        let todo_items: Vec<TodoItem> = params.todos.into_iter().map(|input| input.into()).collect();
        let count = todo_items.len();
        
        // Replace entire list
        self.storage.replace_all(todo_items.clone()).await;
        
        let formatted_todos = self.storage.format_all(&todo_items);
        let output = format!("Updated {} todo items.\n\n{}", count, formatted_todos);
        
        ToolResult::Success {
            output,
            metadata: Some({
                let mut meta = HashMap::new();
                meta.insert("todo_count".to_string(), json!(count));
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
