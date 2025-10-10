use std::sync::Arc;
use shai_core::tools::{AnyTool, BashTool, EditTool, FetchTool, FindTool, LsTool, 
                     MultiEditTool, ReadTool, TodoReadTool, TodoWriteTool, WriteTool,
                     TodoStorage, FsOperationLog};

/// Available tools for the coder agent
#[derive(Debug, Clone, PartialEq)]
pub enum ToolName {
    Bash,
    Edit,
    Fetch,
    Find,
    Ls,
    MultiEdit,
    Read,
    TodoRead,
    TodoWrite,
    Write,
}

impl ToolName {
    pub fn all() -> Vec<ToolName> {
        vec![
            ToolName::Bash,
            ToolName::Edit,
            ToolName::Fetch,
            ToolName::Find,
            ToolName::Ls,
            ToolName::MultiEdit,
            ToolName::Read,
            ToolName::TodoRead,
            ToolName::TodoWrite,
            ToolName::Write,
        ]
    }

    pub fn name(&self) -> &'static str {
        match self {
            ToolName::Bash => "bash",
            ToolName::Edit => "edit",
            ToolName::Fetch => "fetch",
            ToolName::Find => "search",
            ToolName::Ls => "ls",
            ToolName::MultiEdit => "multiedit",
            ToolName::Read => "read",
            ToolName::TodoRead => "todoread",
            ToolName::TodoWrite => "todowrite",
            ToolName::Write => "write",
        }
    }

    pub fn from_str(s: &str) -> Option<ToolName> {
        match s.to_lowercase().as_str() {
            "bash" => Some(ToolName::Bash),
            "edit" => Some(ToolName::Edit),
            "fetch" => Some(ToolName::Fetch),
            "search" => Some(ToolName::Find),
            "ls" => Some(ToolName::Ls),
            "multiedit" => Some(ToolName::MultiEdit),
            "read" => Some(ToolName::Read),
            "todoread" => Some(ToolName::TodoRead),
            "todowrite" => Some(ToolName::TodoWrite),
            "write" => Some(ToolName::Write),
            _ => None,
        }
    }
}

impl std::fmt::Display for ToolName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Tool configuration and manipulation
pub struct ToolConfig {
    pub tools: Vec<ToolName>,
}

impl Default for ToolConfig {
    fn default() -> Self {
        Self {
            tools: ToolName::all(),
        }
    }
}

impl ToolConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_tools(tools: Vec<ToolName>) -> Self {
        Self { tools }
    }

    pub fn remove_tools(mut self, tools_to_remove: Vec<ToolName>) -> Self {
        self.tools.retain(|tool| !tools_to_remove.contains(tool));
        self
    }

    pub fn add_tools(mut self, tools_to_add: Vec<ToolName>) -> Self {
        for tool in tools_to_add {
            if !self.tools.contains(&tool) {
                self.tools.push(tool);
            }
        }
        self
    }

    pub fn list_tools(&self) -> Vec<String> {
        self.tools.iter().map(|t| t.name().to_string()).collect()
    }

    pub fn build_toolbox(&self) -> Vec<Box<dyn AnyTool>> {
        let todo_storage = Arc::new(TodoStorage::new());
        let fs_log = Arc::new(FsOperationLog::new());
        let mut toolbox: Vec<Box<dyn AnyTool>> = Vec::new();
        for tool_name in &self.tools {
            match tool_name {
                ToolName::Bash => toolbox.push(Box::new(BashTool::new())),
                ToolName::Edit => toolbox.push(Box::new(EditTool::new(fs_log.clone()))),
                ToolName::Fetch => toolbox.push(Box::new(FetchTool::new())),
                ToolName::Find => toolbox.push(Box::new(FindTool::new())),
                ToolName::Ls => toolbox.push(Box::new(LsTool::new())),
                ToolName::MultiEdit => toolbox.push(Box::new(MultiEditTool::new(fs_log.clone()))),
                ToolName::Read => toolbox.push(Box::new(ReadTool::new(fs_log.clone()))),
                ToolName::TodoRead => toolbox.push(Box::new(TodoReadTool::new(todo_storage.clone()))),
                ToolName::TodoWrite => toolbox.push(Box::new(TodoWriteTool::new(todo_storage.clone()))),
                ToolName::Write => toolbox.push(Box::new(WriteTool::new(fs_log.clone()))),
            }
        }
        toolbox
    }
}


pub fn list_all_tools() {
    eprintln!("Available tools:");
    for tool in ToolName::all() {
        eprintln!("  {}", tool.name());
    }
}

pub fn parse_tools_list(tools_str: &str) -> Result<Vec<ToolName>, String> {
    tools_str
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| ToolName::from_str(s).ok_or_else(|| format!("Unknown tool: {}", s)))
        .collect()
}