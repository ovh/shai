#[cfg(test)]
mod tests {
    use crate::tools::{
        TodoItem, TodoItemInput, TodoReadTool, TodoStatus, TodoStorage, TodoWriteParams,
        TodoWriteTool, Tool, ToolEmptyParams, ToolResult,
    };
    use serde_json::json;
    use std::sync::Arc;

    // Helper function to create test storage
    fn create_test_storage() -> Arc<TodoStorage> {
        Arc::new(TodoStorage::new())
    }

    // Helper function to create sample todo input
    fn create_sample_todo_input(content: &str, status: TodoStatus) -> TodoItemInput {
        TodoItemInput {
            content: content.to_string(),
            status,
        }
    }

    #[tokio::test]
    async fn test_todo_storage_new() {
        let storage = TodoStorage::new();
        assert_eq!(storage.get_all().await.len(), 0);
    }

    #[tokio::test]
    async fn test_todo_storage_replace_all() {
        let storage = TodoStorage::new();

        let items = vec![TodoItem {
            id: "1".to_string(),
            content: "Test task".to_string(),
            status: TodoStatus::Pending,
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: "2024-01-01T00:00:00Z".to_string(),
        }];

        storage.replace_all(items.clone()).await;
        let todos = storage.get_all().await;
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].content, "Test task");
    }

    #[tokio::test]
    async fn test_todo_item_input_from_conversion() {
        let input = create_sample_todo_input("Test conversion", TodoStatus::InProgress);
        let item: TodoItem = input.into();

        assert_eq!(item.content, "Test conversion");
        assert!(matches!(item.status, TodoStatus::InProgress));
        assert!(!item.id.is_empty());
        assert!(!item.created_at.is_empty());
        assert!(!item.updated_at.is_empty());
        assert_eq!(item.created_at, item.updated_at);
    }

    #[tokio::test]
    async fn test_todo_read_tool_empty_storage() {
        let storage = create_test_storage();
        let read_tool = TodoReadTool::new(storage);

        let result = read_tool.execute(ToolEmptyParams::default(), None).await;

        assert!(result.is_success());
        if let ToolResult::Success { output, metadata } = result {
            assert!(output.contains("No todos found"));
            assert!(metadata.is_some());
            if let Some(meta) = metadata {
                assert_eq!(meta.get("todo_count"), Some(&json!(0)));
            }
        }
    }

    #[tokio::test]
    async fn test_todo_write_tool_basic() {
        let storage = create_test_storage();
        let write_tool = TodoWriteTool::new(storage.clone());

        let params = TodoWriteParams {
            todos: vec![
                create_sample_todo_input("Task 1", TodoStatus::Pending),
                create_sample_todo_input("Task 2", TodoStatus::InProgress),
            ],
        };

        let result = write_tool.execute(params, None).await;

        assert!(result.is_success());
        if let ToolResult::Success { output, metadata } = result {
            assert!(output.contains("Updated 2 todo items"));
            assert!(metadata.is_some());
            if let Some(meta) = metadata {
                assert_eq!(meta.get("todo_count"), Some(&json!(2)));
            }
        }

        // Verify items were stored
        let todos = storage.get_all().await;
        assert_eq!(todos.len(), 2);
    }

    #[tokio::test]
    async fn test_todo_write_tool_empty() {
        let storage = create_test_storage();
        let write_tool = TodoWriteTool::new(storage.clone());

        let params = TodoWriteParams { todos: vec![] };

        let result = write_tool.execute(params, None).await;

        assert!(result.is_success());
        if let ToolResult::Success { output, metadata } = result {
            assert!(output.contains("Updated 0 todo items"));
            if let Some(meta) = metadata {
                assert_eq!(meta.get("todo_count"), Some(&json!(0)));
            }
        }

        // Verify storage is still empty
        let todos = storage.get_all().await;
        assert_eq!(todos.len(), 0);
    }

    #[tokio::test]
    async fn test_todo_write_tool_replace_existing() {
        let storage = create_test_storage();
        let write_tool = TodoWriteTool::new(storage.clone());

        // Add initial todos
        let initial_params = TodoWriteParams {
            todos: vec![create_sample_todo_input(
                "Initial task",
                TodoStatus::Pending,
            )],
        };
        write_tool.execute(initial_params, None).await;

        // Replace with new todos
        let new_params = TodoWriteParams {
            todos: vec![
                create_sample_todo_input("New task 1", TodoStatus::InProgress),
                create_sample_todo_input("New task 2", TodoStatus::Completed),
            ],
        };

        let result = write_tool.execute(new_params, None).await;
        assert!(result.is_success());

        // Verify old todos were replaced
        let todos = storage.get_all().await;
        assert_eq!(todos.len(), 2);
        assert_eq!(todos[0].content, "New task 1");
        assert_eq!(todos[1].content, "New task 2");
        assert!(matches!(todos[0].status, TodoStatus::InProgress));
        assert!(matches!(todos[1].status, TodoStatus::Completed));
    }

    #[tokio::test]
    async fn test_todo_read_write_integration() {
        let storage = create_test_storage();
        let read_tool = TodoReadTool::new(storage.clone());
        let write_tool = TodoWriteTool::new(storage.clone());

        // Initially empty
        let empty_result = read_tool.execute(ToolEmptyParams::default(), None).await;
        assert!(empty_result.is_success());
        if let ToolResult::Success { output, .. } = empty_result {
            assert!(output.contains("No todos found"));
        }

        // Add todos
        let write_params = TodoWriteParams {
            todos: vec![
                create_sample_todo_input("Read test task", TodoStatus::Pending),
                create_sample_todo_input("Write test task", TodoStatus::InProgress),
            ],
        };

        let write_result = write_tool.execute(write_params, None).await;
        assert!(write_result.is_success());

        // Read todos back
        let read_result = read_tool.execute(ToolEmptyParams::default(), None).await;
        assert!(read_result.is_success());

        if let ToolResult::Success { output, metadata } = read_result {
            assert!(output.contains("Read test task"));
            assert!(output.contains("Write test task"));
            if let Some(meta) = metadata {
                assert_eq!(meta.get("todo_count"), Some(&json!(2)));
            }
        }

        // Direct verification through storage
        let todos = storage.get_all().await;
        assert_eq!(todos.len(), 2);
    }

    #[tokio::test]
    async fn test_shared_storage_between_tools() {
        let storage = create_test_storage();
        let read_tool = TodoReadTool::new(storage.clone());
        let write_tool = TodoWriteTool::new(storage.clone());

        // Test that changes made by write tool are visible to read tool
        let write_params = TodoWriteParams {
            todos: vec![
                create_sample_todo_input("Shared task 1", TodoStatus::Pending),
                create_sample_todo_input("Shared task 2", TodoStatus::Completed),
            ],
        };

        write_tool.execute(write_params, None).await;

        let read_result = read_tool.execute(ToolEmptyParams::default(), None).await;
        assert!(read_result.is_success());

        if let ToolResult::Success { output, .. } = read_result {
            assert!(output.contains("Shared task 1"));
            assert!(output.contains("Shared task 2"));
        }
    }
}
