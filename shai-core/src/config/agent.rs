use super::config::ShaiConfig;
use crate::tools::mcp::McpConfig;
use json_comments::StripComments;
use serde::{Deserialize, Serialize};
use shai_llm::ToolCallMethod;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProviderConfig {
    pub provider: String,
    pub env_vars: HashMap<String, String>,
    pub model: String,
    pub tool_method: ToolCallMethod,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolConfig {
    pub config: McpConfig,
    #[serde(default = "default_enabled_tools")]
    pub enabled_tools: Vec<String>,
    #[serde(default)]
    pub excluded_tools: Vec<String>,
    /// Whether this MCP server is required to start successfully (default: false)
    /// If false, connection errors will be logged as warnings and agent will continue
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTools {
    #[serde(default)]
    pub builtin: Vec<String>,
    #[serde(default)]
    pub builtin_excluded: Vec<String>,
    #[serde(default)]
    pub mcp: HashMap<String, McpToolConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub name: String,
    pub description: String,
    #[serde(default = "default_llm_provider")]
    pub llm_provider: AgentProviderConfig,
    #[serde(default)]
    pub tools: AgentTools,
    #[serde(default = "default_system_prompt")]
    pub system_prompt: String,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default)]
    pub compaction: CompactionConfig,
    #[serde(default)]
    pub verification: VerificationConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_max_output_chars")]
    pub max_output_chars: usize,
    #[serde(default = "default_max_tool_calls_per_turn")]
    pub max_tool_calls_per_turn: Option<usize>,
    #[serde(default)]
    pub max_cached_commands: usize,
    #[serde(default = "default_max_trace_chars")]
    pub max_trace_chars: usize,
    /// Maximum number of cached read results (per file path + line range)
    #[serde(default = "default_max_cached_reads")]
    pub max_cached_reads: usize,
    /// Default exclusion patterns for the find tool (substrings matched against file paths)
    #[serde(default = "default_find_exclude_patterns")]
    pub find_exclude_patterns: Vec<String>,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_output_chars: 8000,
            max_tool_calls_per_turn: Some(100),
            max_cached_commands: 50,
            max_trace_chars: 85000,
            max_cached_reads: 100,
            find_exclude_patterns: default_find_exclude_patterns(),
        }
    }
}

fn default_max_cached_reads() -> usize {
    100
}

fn default_find_exclude_patterns() -> Vec<String> {
    vec![
        ".git".to_string(),
        "target".to_string(),
        "node_modules".to_string(),
        ".next".to_string(),
        "dist".to_string(),
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationConfig {
    /// Whether post-edit verification is enabled (default: false)
    #[serde(default)]
    pub enabled: bool,
    /// Maximum time in seconds to wait for the verification command to complete
    #[serde(default = "default_verification_timeout_secs")]
    pub timeout_secs: u64,
    /// Mapping of language name to verification command.
    /// Defaults use stdlib-only tools. Users can override per-language.
    #[serde(default = "default_verification_commands")]
    pub commands: HashMap<String, Vec<String>>,
}

impl Default for VerificationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            timeout_secs: 30,
            commands: default_verification_commands(),
        }
    }
}

fn default_verification_timeout_secs() -> u64 {
    30
}

fn default_verification_commands() -> HashMap<String, Vec<String>> {
    let mut commands = HashMap::new();
    commands.insert("rust".to_string(), vec!["cargo".into(), "check".into()]);
    commands.insert(
        "go".to_string(),
        vec!["go".into(), "build".into(), "./...".into()],
    );
    commands.insert(
        "python".to_string(),
        vec!["python".into(), "-m".into(), "py_compile".into()],
    );
    commands.insert(
        "typescript".to_string(),
        vec!["node".into(), "--check".into()],
    );
    commands.insert(
        "javascript".to_string(),
        vec!["node".into(), "--check".into()],
    );
    commands.insert("perl".to_string(), vec!["perl".into(), "-c".into()]);
    commands.insert("ruby".to_string(), vec!["ruby".into(), "-c".into()]);
    commands.insert("bash".to_string(), vec!["bash".into(), "-n".into()]);
    commands.insert("php".to_string(), vec!["php".into(), "-l".into()]);
    commands.insert("lua".to_string(), vec!["luac".into(), "-p".into()]);
    commands
}

fn default_true() -> bool {
    true
}

fn default_max_output_chars() -> usize {
    8000
}

fn default_max_tool_calls_per_turn() -> Option<usize> {
    Some(100)
}

fn default_max_trace_chars() -> usize {
    85000
}

fn default_llm_provider() -> AgentProviderConfig {
    let shai_config = ShaiConfig::load().unwrap_or_else(|e| {
        tracing::warn!("Failed to load config, using default: {}", e);
        ShaiConfig::default()
    });

    let provider_config = shai_config
        .get_selected_provider()
        .expect("No provider configured in default config");

    AgentProviderConfig {
        provider: provider_config.provider.clone(),
        env_vars: provider_config.env_vars.clone(),
        model: provider_config.model.clone(),
        tool_method: provider_config.tool_method,
    }
}

fn default_system_prompt() -> String {
    "{{CODER_BASE_PROMPT}}".to_string()
}

fn default_max_tokens() -> u32 {
    4096
}

fn default_temperature() -> f32 {
    0.0
}

fn default_enabled_tools() -> Vec<String> {
    vec!["*".to_string()]
}

impl Default for AgentTools {
    fn default() -> Self {
        Self {
            builtin: vec!["*".to_string()],
            builtin_excluded: Vec::new(),
            mcp: HashMap::new(),
        }
    }
}

impl AgentConfig {
    /// Get the agents directory path
    pub fn agents_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
        let config_dir = std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|_| {
                dirs::home_dir()
                    .map(|home| home.join(".config"))
                    .ok_or("Could not find home directory")
            })?;

        let agents_dir = config_dir.join("shai").join("agents");
        std::fs::create_dir_all(&agents_dir)?;
        Ok(agents_dir)
    }

    /// Get the path for a specific agent config file
    pub fn agent_config_path(agent_name: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let agents_dir = Self::agents_dir()?;
        Ok(agents_dir.join(format!("{}.config.json", agent_name)))
    }

    /// Get the legacy path for a specific agent config file
    fn legacy_agent_config_path(agent_name: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let agents_dir = Self::agents_dir()?;
        Ok(agents_dir.join(format!("{}.config", agent_name)))
    }

    /// Load an agent config from file
    pub fn load(agent_name: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let config_path = Self::agent_config_path(agent_name)?;

        if config_path.exists() {
            let content_bytes = std::fs::read(config_path)?;
            let content_stripped = StripComments::new(&content_bytes[..]);
            let config: AgentConfig = serde_json::from_reader(content_stripped)?;
            return Ok(config);
        }

        let legacy_path = Self::legacy_agent_config_path(agent_name)?;
        if legacy_path.exists() {
            let content_bytes = std::fs::read(legacy_path)?;
            let content_stripped = StripComments::new(&content_bytes[..]);
            let config: AgentConfig = serde_json::from_reader(content_stripped)?;
            return Ok(config);
        }

        Err(format!("Agent config '{}' does not exist", agent_name).into())
    }

    /// Save the agent config to file
    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let config_path = Self::agent_config_path(&self.name)?;
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&config_path, content)?;
        Ok(())
    }

    /// List all available agents
    pub fn list_agents() -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let agents_dir = Self::agents_dir()?;
        let mut agents = Vec::new();

        if agents_dir.exists() {
            for entry in std::fs::read_dir(agents_dir)? {
                let entry = entry?;
                let path = entry.path();

                if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
                    if let Some(agent_name) = filename
                        .strip_suffix(".config.json")
                        .or_else(|| filename.strip_suffix(".config"))
                    {
                        agents.push(agent_name.to_string());
                    }
                }
            }
        }

        agents.sort();
        agents.dedup();
        Ok(agents)
    }

    /// Check if an agent config exists
    pub fn exists(agent_name: &str) -> bool {
        Self::agent_config_path(agent_name)
            .map(|p| p.exists())
            .unwrap_or(false)
            || Self::legacy_agent_config_path(agent_name)
                .map(|p| p.exists())
                .unwrap_or(false)
    }

    /// Delete an agent config
    pub fn delete(agent_name: &str) -> Result<(), Box<dyn std::error::Error>> {
        let config_path = Self::agent_config_path(agent_name)?;
        let legacy_path = Self::legacy_agent_config_path(agent_name)?;

        if config_path.exists() {
            std::fs::remove_file(config_path)?;
            return Ok(());
        }

        if legacy_path.exists() {
            std::fs::remove_file(legacy_path)?;
            return Ok(());
        }

        Err(format!("Agent config '{}' does not exist", agent_name).into())
    }

    /// Check if a builtin tool is enabled
    pub fn is_builtin_tool_enabled(&self, tool_name: &str) -> bool {
        self.tools.builtin.contains(&tool_name.to_string())
    }

    /// Check if a specific MCP tool is enabled
    pub fn is_mcp_tool_enabled(&self, mcp_name: &str, tool_name: &str) -> bool {
        self.tools
            .mcp
            .get(mcp_name)
            .map(|mcp_tool| mcp_tool.enabled_tools.contains(&tool_name.to_string()))
            .unwrap_or(false)
    }

    /// Get all enabled MCP tool names across all MCP configs
    pub fn get_all_enabled_mcp_tools(&self) -> Vec<String> {
        self.tools
            .mcp
            .values()
            .flat_map(|mcp_tool| &mcp_tool.enabled_tools)
            .cloned()
            .collect()
    }
}
