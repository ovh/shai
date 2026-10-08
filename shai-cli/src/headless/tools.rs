use shai_core::tools::{self, AnyTool};
use std::sync::Arc;

/// Available tools for the coder agent
#[derive(Debug, Clone, PartialEq)]
pub enum ToolName {
    Bash,
    Edit,
    Fetch,
    Find,
    Ls,
    Read,
    TodoRead,
    TodoWrite,
    Write,
    Skills,
}

impl ToolName {
    pub fn all() -> Vec<ToolName> {
        let mut all = Vec::new();
        for name in tools::TOOL_NAMES {
            all.push(ToolName::from_str(name).expect("unknown tool name in TOOL_NAMES"));
        }
        all
    }

    pub fn name(&self) -> &'static str {
        match self {
            ToolName::Bash => "bash",
            ToolName::Edit => "edit",
            ToolName::Fetch => "fetch",
            ToolName::Find => "find",
            ToolName::Ls => "ls",
            ToolName::Read => "read",
            ToolName::TodoRead => "todo_read",
            ToolName::TodoWrite => "todo_write",
            ToolName::Write => "write",
            ToolName::Skills => "skills",
        }
    }

    pub fn from_str(s: &str) -> Option<ToolName> {
        match s {
            "bash" => Some(ToolName::Bash),
            "edit" => Some(ToolName::Edit),
            "fetch" => Some(ToolName::Fetch),
            "find" => Some(ToolName::Find),
            "ls" => Some(ToolName::Ls),
            "read" => Some(ToolName::Read),
            "todo_read" => Some(ToolName::TodoRead),
            "todo_write" => Some(ToolName::TodoWrite),
            "write" => Some(ToolName::Write),
            "skills" => Some(ToolName::Skills),
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

    pub fn build_toolbox(&self) -> Vec<Box<dyn AnyTool>> {
        let todo_storage = Arc::new(tools::TodoStorage::new());
        let fs_log = Arc::new(tools::FsOperationLog::new());
        let mut toolbox: Vec<Box<dyn AnyTool>> = Vec::new();
        for tool_name in &self.tools {
            if let Some(tool) = tools::create_tool(
                tool_name.name(),
                fs_log.clone(),
                todo_storage.clone(),
                &[],
            ) {
                toolbox.push(tool);
            }
        }
        toolbox
    }
}

pub fn list_all_tools() {
    eprintln!("Available tools:");
    for name in tools::TOOL_NAMES {
        eprintln!("  {}", name);
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
