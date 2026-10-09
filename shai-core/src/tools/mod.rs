pub mod bash;
pub mod fetch;
pub mod frontmatter;
pub mod fs;
pub mod highlight;
pub mod mcp;
pub mod memory;
pub mod prompts;
pub mod skills;
pub mod todo;
pub mod types;

#[cfg(test)]
mod tests_llm;

pub use shai_macros::tool;
pub use types::{
    AnyTool, AnyToolBox, Tool, ToolCall, ToolCapability, ToolEmptyParams, ToolError, ToolResult,
};

// Re-export all tools
pub use bash::BashTool;
pub use fetch::FetchTool;
pub use fs::{
    EditTool, FindTool, FsOperation, FsOperationLog, FsOperationSummary, FsOperationType, LsTool,
    ReadTool, WriteTool,
};
pub use mcp::{
    create_mcp_client, get_mcp_tools, HttpClient, McpClient, McpConfig, McpToolDescription,
    SseClient, StdioClient,
};
pub use memory::{MemoryRemoveTool, MemoryWriteTool};
pub use skills::SkillTool;
pub use todo::{
    TodoItem, TodoItemInput, TodoReadTool, TodoStatus, TodoStorage, TodoWriteParams, TodoWriteTool,
};

/// Names of all built-in tools, in deterministic order.
/// To add a new tool, add its name here and add a match arm in `create_tool()`.
pub const TOOL_NAMES: &[&str] = &[
    "bash",
    "edit",
    "fetch",
    "find",
    "ls",
    "read",
    "write",
    "todo_read",
    "todo_write",
    "skills",
    "memory_write",
    "memory_remove",
];

/// Factory function to create a built-in tool by name.
/// Returns `None` if the tool name is not recognized.
pub fn create_tool(
    name: &str,
    fs_log: std::sync::Arc<FsOperationLog>,
    todo_storage: std::sync::Arc<TodoStorage>,
    exclude_patterns: &[String],
) -> Option<Box<dyn AnyTool>> {
    match name {
        "bash" => Some(Box::new(BashTool::new())),
        "edit" => Some(Box::new(EditTool::new(fs_log))),
        "fetch" => Some(Box::new(FetchTool::new())),
        "find" => Some(Box::new(
            FindTool::new().with_exclude_patterns(exclude_patterns.to_vec()),
        )),
        "ls" => Some(Box::new(LsTool::new())),
        "read" => Some(Box::new(ReadTool::new(fs_log))),
        "write" => Some(Box::new(WriteTool::new(fs_log))),
        "todo_read" => Some(Box::new(TodoReadTool::new(todo_storage))),
        "todo_write" => Some(Box::new(TodoWriteTool::new(todo_storage))),
        "skills" => Some(Box::new(SkillTool::new())),
        "memory_write" => Some(Box::new(MemoryWriteTool::new())),
        "memory_remove" => Some(Box::new(MemoryRemoveTool::new())),
        _ => None,
    }
}

/// Status of an MCP server connection attempt.
#[derive(Debug, Clone)]
pub struct McpServerStatus {
    pub name: String,
    pub connected: bool,
    pub tool_count: usize,
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn make_tool(name: &str) -> Box<dyn AnyTool> {
        let fs_log = Arc::new(FsOperationLog::new());
        let todo_storage = Arc::new(TodoStorage::new());
        create_tool(name, fs_log, todo_storage, &[])
            .unwrap_or_else(|| panic!("create_tool returned None for '{}'", name))
    }

    #[test]
    fn test_tool_names_are_registered() {
        for name in ["memory_write", "memory_remove"] {
            assert!(
                TOOL_NAMES.contains(&name),
                "'{}' missing from TOOL_NAMES",
                name
            );
        }
    }

    #[test]
    fn test_create_memory_tools() {
        let write = make_tool("memory_write");
        assert_eq!(write.name(), "memory_write");
        assert_eq!(write.capabilities(), &[ToolCapability::Write]);

        let remove = make_tool("memory_remove");
        assert_eq!(remove.name(), "memory_remove");
        assert_eq!(remove.capabilities(), &[ToolCapability::Write]);
    }
}
