use schemars::JsonSchema;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct BashToolParams {
    /// The bash command to execute
    pub command: String,
    /// Timeout in seconds (optional, None = no timeout)
    pub timeout: Option<u32>,
    /// Working directory for command execution (optional)
    pub working_dir: Option<String>,
    /// Environment variables to set (optional)
    #[serde(default)]
    pub env: HashMap<String, String>,
}
