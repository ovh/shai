use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

pub struct TodoStorage {
    store: RwLock<Vec<TodoItem>>,
}

impl Default for TodoStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl TodoStorage {
    pub fn new() -> Self {
        Self {
            store: RwLock::new(Vec::new()),
        }
    }

    pub async fn get_all(&self) -> Vec<TodoItem> {
        self.store.read().await.clone()
    }

    pub async fn replace_all(&self, items: Vec<TodoItem>) {
        *self.store.write().await = items;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TodoItem {
    pub id: String,
    pub content: String,
    pub status: TodoStatus,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(inline)]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}

impl TodoItem {
    pub fn format_for_display(&self) -> String {
        let (checkbox, color_code) = match self.status {
            TodoStatus::Pending => ("☐", ""),
            TodoStatus::InProgress => ("☐", "\x1b[1;34m"),
            TodoStatus::Completed => ("☑", "\x1b[32m"),
        };

        format!("{}{} {}\x1b[0m", color_code, checkbox, self.content)
    }
}

impl TodoStorage {
    pub fn format_all(&self, todos: &[TodoItem]) -> String {
        if todos.is_empty() {
            "No todos found. The todo list is empty.".to_string()
        } else {
            todos
                .iter()
                .map(|todo| todo.format_for_display())
                .collect::<Vec<_>>()
                .join("\n")
        }
    }
}
