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
    MemoryRemove,
    MemoryWrite,
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
            ToolName::MemoryRemove => "memory_remove",
            ToolName::MemoryWrite => "memory_write",
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
            "memory_remove" => Some(ToolName::MemoryRemove),
            "memory_write" => Some(ToolName::MemoryWrite),
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

    /// Keep only the given tools (in the default order).
    pub fn only_tools(mut self, tools_to_keep: Vec<ToolName>) -> Self {
        self.tools.retain(|tool| tools_to_keep.contains(tool));
        self
    }

    /// Build the tool selection from the `--tools` and `--remove` arguments:
    /// `--tools` restricts the default set to the listed tools, then
    /// `--remove` drops tools from what is left.
    pub fn from_args(tools: Option<&str>, remove: Option<&str>) -> Result<Self, String> {
        let mut config = Self::new();
        if let Some(tools_str) = tools {
            config = config.only_tools(parse_tools_list(tools_str)?);
        }
        if let Some(remove_str) = remove {
            config = config.remove_tools(parse_tools_list(remove_str)?);
        }
        Ok(config)
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
                &tools::BashToolConfig::default(),
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

/// Restricted toolbox, plus the requested tools it did not provide.
pub type RestrictedToolbox = (Vec<Box<dyn AnyTool>>, Vec<ToolName>);

/// Apply the `--tools` / `--remove` arguments to an already built toolbox,
/// e.g. the tools of a custom agent loaded from its config.
///
/// `--tools` can only narrow the toolbox: tools not listed are dropped
/// (including MCP tools), and listed tools the toolbox does not provide are
/// not added. Their names are returned so the caller can warn about them.
/// `--remove` drops the listed tools and keeps everything else.
pub fn restrict_toolbox(
    toolbox: Vec<Box<dyn AnyTool>>,
    tools: Option<&str>,
    remove: Option<&str>,
) -> Result<RestrictedToolbox, String> {
    let keep = tools.map(parse_tools_list).transpose()?;
    let drop = remove
        .map(parse_tools_list)
        .transpose()?
        .unwrap_or_default();

    // CLI names can differ from the tool's runtime name ("skills" is "skill")
    let runtime_name = |tool: &ToolName| -> String {
        ToolConfig {
            tools: vec![tool.clone()],
        }
        .build_toolbox()
        .first()
        .map(|t| t.name())
        .unwrap_or_else(|| tool.name().to_string())
    };
    let keep: Option<Vec<(ToolName, String)>> = keep.map(|keep| {
        keep.into_iter()
            .map(|tool| {
                let name = runtime_name(&tool);
                (tool, name)
            })
            .collect()
    });
    let drop: Vec<String> = drop.iter().map(runtime_name).collect();

    let missing = keep
        .iter()
        .flatten()
        .filter(|(_, name)| !toolbox.iter().any(|tool| tool.name() == *name))
        .map(|(tool, _)| tool.clone())
        .collect();

    let toolbox = toolbox
        .into_iter()
        .filter(|tool| {
            let name = tool.name();
            let kept = keep
                .as_ref()
                .is_none_or(|keep| keep.iter().any(|(_, n)| *n == name));
            kept && !drop.contains(&name)
        })
        .collect();

    Ok((toolbox, missing))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(toolbox: &[Box<dyn AnyTool>]) -> Vec<String> {
        toolbox.iter().map(|tool| tool.name()).collect()
    }

    fn toolbox_of(tools: &[ToolName]) -> Vec<Box<dyn AnyTool>> {
        ToolConfig {
            tools: tools.to_vec(),
        }
        .build_toolbox()
    }

    #[test]
    fn tools_arg_restricts_default_set() {
        let config = ToolConfig::from_args(Some("read,find"), None).unwrap();
        assert_eq!(config.tools, vec![ToolName::Find, ToolName::Read]);
        assert_eq!(names(&config.build_toolbox()), vec!["find", "read"]);
    }

    #[test]
    fn remove_arg_drops_from_default_set() {
        let config = ToolConfig::from_args(None, Some("bash,write")).unwrap();
        assert!(!config.tools.contains(&ToolName::Bash));
        assert!(!config.tools.contains(&ToolName::Write));
        assert_eq!(config.tools.len(), ToolName::all().len() - 2);
    }

    #[test]
    fn tools_and_remove_args_combine() {
        let config = ToolConfig::from_args(Some("read,find,bash"), Some("bash")).unwrap();
        assert_eq!(config.tools, vec![ToolName::Find, ToolName::Read]);
    }

    #[test]
    fn unknown_tool_is_an_error() {
        assert!(ToolConfig::from_args(Some("read,nope"), None).is_err());
        assert!(restrict_toolbox(Vec::new(), None, Some("nope")).is_err());
    }

    #[test]
    fn restrict_toolbox_keeps_only_listed_tools() {
        let agent_tools = toolbox_of(&[ToolName::Bash, ToolName::Read, ToolName::Write]);
        let (toolbox, missing) = restrict_toolbox(agent_tools, Some("read"), None).unwrap();
        assert_eq!(names(&toolbox), vec!["read"]);
        assert!(missing.is_empty());
    }

    #[test]
    fn restrict_toolbox_never_adds_tools() {
        // An agent configured without bash must not gain it from --tools
        let agent_tools = toolbox_of(&[ToolName::Read, ToolName::Find]);
        let (toolbox, missing) = restrict_toolbox(agent_tools, Some("read,bash"), None).unwrap();
        assert_eq!(names(&toolbox), vec!["read"]);
        assert_eq!(missing, vec![ToolName::Bash]);
    }

    #[test]
    fn restrict_toolbox_remove_keeps_the_rest() {
        let agent_tools = toolbox_of(&[ToolName::Bash, ToolName::Read, ToolName::Write]);
        let (toolbox, missing) = restrict_toolbox(agent_tools, None, Some("bash")).unwrap();
        assert_eq!(names(&toolbox), vec!["read", "write"]);
        assert!(missing.is_empty());
    }

    #[test]
    fn restrict_toolbox_matches_runtime_tool_names() {
        // `skills` on the command line is the tool named `skill` at runtime
        let agent_tools = toolbox_of(&[ToolName::Read, ToolName::Skills]);
        let skill = names(&toolbox_of(&[ToolName::Skills]));

        let (toolbox, missing) = restrict_toolbox(agent_tools, Some("skills"), None).unwrap();
        assert_eq!(names(&toolbox), skill);
        assert!(missing.is_empty());

        let agent_tools = toolbox_of(&[ToolName::Read, ToolName::Skills]);
        let (toolbox, _) = restrict_toolbox(agent_tools, None, Some("skills")).unwrap();
        assert_eq!(names(&toolbox), vec!["read"]);
    }

    #[test]
    fn restrict_toolbox_without_args_is_unchanged() {
        let agent_tools = toolbox_of(&[ToolName::Bash, ToolName::Read]);
        let (toolbox, missing) = restrict_toolbox(agent_tools, None, None).unwrap();
        assert_eq!(names(&toolbox), vec!["bash", "read"]);
        assert!(missing.is_empty());
    }
}
