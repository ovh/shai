use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ToolCallMethod {
    /// let the system decide what technique to use
    Auto,
    /// use function call api with tool choice set to auto
    FunctionCall,
    /// use function call api with tool choice set to required (gave special tool for "no tool")
    FunctionCallRequired,
    /// use response_format to force structured output, add tool documentation in system prompt
    StructuredOutput,
    /// instruct llm to use special tag and parse the response from content, add tool documentation in system prompt
    Parsing,
}

/// A tool must be able to describe its parameter as a json schema
pub trait ToolDescription: Send + Sync {
    fn name(&self) -> String;

    fn description(&self) -> String;

    fn parameters_schema(&self) -> serde_json::Value;

    /// Return the group name for this tool (e.g., "builtin", "mcp_ovh")
    fn group(&self) -> Option<&str> {
        None
    }
}

/// A toolbox is a set of tool
pub type ToolBox = Vec<Arc<dyn ToolDescription>>;

pub trait ContainsTool {
    fn contains_tool(&self, name: &str) -> bool;
}

impl ContainsTool for ToolBox {
    fn contains_tool(&self, name: &str) -> bool {
        self.iter().any(|tool| tool.name() == name)
    }
}
