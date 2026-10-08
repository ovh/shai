use openai_dive::v1::resources::chat::{ChatMessage, ChatMessageContent};
use shai_llm::LlmClient;
use std::sync::Arc;
use uuid::Uuid;

use super::claims::ClaimManager;
use super::AgentCore;
use super::AgentError;
use super::Brain;
use crate::agent::agent::ToolContext;
use crate::config::agent::{AgentConfig, CompactionConfig, VerificationConfig};
use crate::config::config::ShaiConfig;
use crate::runners::coder::CoderBrain;
use crate::tools::mcp::mcp_oauth::signin_oauth;
use crate::tools::{
    create_mcp_client, create_tool, get_mcp_tools, AnyTool, FsOperationLog, McpConfig,
    McpServerStatus, TodoStorage, TOOL_NAMES,
};
use crate::tools::skills::SkillTool;

use tracing::{debug, warn};

/// Builder for AgentCore
pub struct AgentBuilder {
    pub session_id: String,
    pub brain: Box<dyn Brain>,
    pub goal: Option<String>,
    pub trace: Vec<ChatMessage>,
    pub available_tools: Vec<Box<dyn AnyTool>>,
    pub permissions: ClaimManager,
    pub compaction_config: CompactionConfig,
    pub verification_config: VerificationConfig,
    pub fs_operation_log: Arc<FsOperationLog>,
    pub working_dir: Option<String>,
    pub todo_storage: Option<Arc<TodoStorage>>,
    pub mcp_status: Vec<McpServerStatus>,
}

impl AgentBuilder {
    /// Create a new AgentBuilder with an optional config name
    /// If None, creates a default agent with LLM from ShaiConfig
    /// If Some(name), loads agent from config file
    pub async fn create(config_name: Option<String>) -> Result<Self, AgentError> {
        match config_name {
            Some(name) => {
                let config = AgentConfig::load(&name).map_err(|e| {
                    AgentError::ConfigurationError(format!(
                        "Failed to load agent '{}': {}",
                        name, e
                    ))
                })?;
                Self::from_config(config).await
            }
            None => Self::default().await,
        }
    }

    /// Create a default AgentBuilder using ShaiConfig LLM and default tools
    pub async fn default() -> Result<Self, AgentError> {
        // Get LLM from ShaiConfig
        let (llm_client, model) = ShaiConfig::get_llm().await.map_err(|e| {
            AgentError::ConfigurationError(format!("Failed to get LLM from config: {}", e))
        })?;

        // Create default brain
        let brain = Box::new(CoderBrain::new(Arc::new(llm_client), model));

        // Create default toolbox (using ToolConfig from shai-cli)
        // For now, create basic tools - we can expand this later
        let fs_log = Arc::new(FsOperationLog::new());
        let (tools, todo_storage) = Self::create_default_tools(fs_log.clone());

        Ok(Self::with_brain(brain)
            .tools(tools)
            .set_todo_storage(todo_storage))
    }

    /// Create AgentBuilder with a specific brain
    pub fn with_brain(brain: Box<dyn Brain>) -> Self {
        Self {
            session_id: Uuid::new_v4().to_string(),
            brain,
            goal: None,
            trace: vec![],
            available_tools: vec![],
            permissions: ClaimManager::new(),
            compaction_config: CompactionConfig::default(),
            verification_config: VerificationConfig::default(),
            fs_operation_log: Arc::new(FsOperationLog::new()),
            working_dir: None,
            todo_storage: None,
            mcp_status: Vec::new(),
        }
    }

    /// Create a builder for a sub-agent that inherits shared state from a parent `ToolContext`.
    ///
    /// Sub-agents get their own `trace`, `tool_cache`, and `tool_budget` (fresh copies),
    /// but share `claims`, `todo_storage`, and `fs_operation_log` with the parent.
    pub async fn for_sub_agent(parent: &ToolContext, brain: Box<dyn Brain>) -> Self {
        let permissions = parent.claims.read().await.clone();
        Self {
            session_id: Uuid::new_v4().to_string(),
            brain,
            goal: None,
            trace: vec![],
            available_tools: vec![],
            permissions,
            compaction_config: parent.compaction_config.clone(),
            verification_config: parent.verification_config.clone(),
            fs_operation_log: parent.fs_operation_log.clone(),
            working_dir: parent.working_dir.clone(),
            todo_storage: Some(parent.todo_storage.clone()),
            mcp_status: Vec::new(),
        }
    }

    /// Set the todo_storage
    pub fn set_todo_storage(mut self, todo_storage: Arc<TodoStorage>) -> Self {
        self.todo_storage = Some(todo_storage);
        self
    }

    /// Create default set of tools
    pub fn create_default_tools(
        fs_log: Arc<FsOperationLog>,
    ) -> (Vec<Box<dyn AnyTool>>, Arc<TodoStorage>) {
        let todo_storage = Arc::new(TodoStorage::new());
        let exclude_patterns = CompactionConfig::default().find_exclude_patterns.clone();

        let tools: Vec<Box<dyn AnyTool>> = TOOL_NAMES
            .iter()
            .filter_map(|name| create_tool(name, fs_log.clone(), todo_storage.clone(), &exclude_patterns))
            .collect();

        (tools, todo_storage)
    }
}

impl AgentBuilder {
    pub fn id(mut self, session_id: &str) -> Self {
        self.session_id = session_id.to_string();
        self
    }

    pub fn brain(mut self, brain: Box<dyn Brain>) -> Self {
        self.brain = brain;
        self
    }

    pub fn goal(mut self, goal: &str) -> Self {
        self.goal = Some(goal.to_string());
        self
    }

    pub fn with_traces(mut self, trace: Vec<ChatMessage>) -> Self {
        self.trace = trace;
        self
    }

    pub fn tools(mut self, available_tools: Vec<Box<dyn AnyTool>>) -> Self {
        self.available_tools = available_tools;
        self
    }

    pub fn permissions(mut self, permissions: ClaimManager) -> Self {
        self.permissions = permissions;
        self
    }

    pub fn working_dir(mut self, working_dir: impl Into<String>) -> Self {
        self.working_dir = Some(working_dir.into());
        self
    }

    /// Enable sudo mode - bypasses all permission checks
    pub fn sudo(mut self) -> Self {
        self.permissions.sudo();
        self
    }

    /// Build the AgentCore with required runtime fields
    pub fn build(mut self) -> AgentCore {
        if let Some(goal) = self.goal {
            self.trace.push(ChatMessage::User {
                content: ChatMessageContent::Text(goal.clone()),
                name: None,
            });
        }

        let todo_storage = self
            .todo_storage
            .unwrap_or_else(|| Arc::new(TodoStorage::new()));

        AgentCore::new(
            self.session_id.clone(),
            self.brain,
            self.trace,
            self.available_tools,
            self.permissions,
            self.compaction_config,
            self.verification_config,
            self.fs_operation_log,
            self.working_dir,
            todo_storage,
        )
    }

    /// Create an AgentBuilder from an AgentConfig
    pub async fn from_config(mut config: AgentConfig) -> Result<Self, AgentError> {
        // Create LLM client from provider config using the utility method
        let llm_client = Arc::new(
            LlmClient::create_provider(
                &config.llm_provider.provider,
                &config.llm_provider.env_vars,
            )
            .map_err(|e| AgentError::LlmError(e.to_string()))?,
        );

        // Create brain with custom system prompt and temperature
        let brain = Box::new(CoderBrain::with_custom_prompt(
            llm_client.clone(),
            config.llm_provider.model.clone(),
            config.system_prompt.clone(),
            config.temperature,
        ));

        // Create tools
        let fs_log = Arc::new(FsOperationLog::new());
        let (tools, todo_storage, mcp_status) =
            Self::create_tools_from_config(&mut config, fs_log.clone()).await?;

        // Display available tools by category
        let mut tool_groups: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();

        for tool in &tools {
            let group_name = tool.group().unwrap_or("unknown").to_string();
            tool_groups
                .entry(group_name)
                .or_insert_with(Vec::new)
                .push(tool.name());
        }

        // Display builtin tools first
        if let Some(builtin_tools) = tool_groups.remove("builtin") {
            debug!(target: "agent::builder", "builtin: {}", builtin_tools.join(", "));
        }

        // Display MCP tools
        for (group_name, group_tools) in tool_groups {
            if group_name != "unknown" {
                debug!(target: "agent::builder", "mcp({}): {}", group_name, group_tools.join(", "));
            }
        }

        let mut builder = Self::with_brain(brain);
        builder.compaction_config = config.compaction.clone();
        builder.verification_config = config.verification.clone();
        builder.fs_operation_log = fs_log;
        builder.mcp_status = mcp_status;
        Ok(builder
            .tools(tools)
            .set_todo_storage(todo_storage)
            .id(&format!("agent-{}", config.name)))
    }

    /// Create tools from config
async fn create_tools_from_config(
        config: &mut AgentConfig,
        fs_log: Arc<FsOperationLog>,
    ) -> Result<(Vec<Box<dyn AnyTool>>, Arc<TodoStorage>, Vec<McpServerStatus>), AgentError> {
        let mut tools: Vec<Box<dyn AnyTool>> = Vec::new();
        let mut mcp_status: Vec<McpServerStatus> = Vec::new();

        // Create shared storage for todo tools
        let todo_storage = Arc::new(TodoStorage::new());

        // Add builtin tools based on config
        let builtin_tools_to_add = if config.tools.builtin.contains(&"*".to_string()) {
            TOOL_NAMES.iter().map(|s| *s).collect::<Vec<_>>()
        } else {
            // Add only specified tools
            config.tools.builtin.iter().map(|s| s.as_str()).collect()
        };

        for tool_name in builtin_tools_to_add {
            // Skip if tool is in builtin excluded list
            if config
                .tools
                .builtin_excluded
                .contains(&tool_name.to_string())
            {
                continue;
            }

            match create_tool(
                tool_name,
                fs_log.clone(),
                todo_storage.clone(),
                &config.compaction.find_exclude_patterns.clone(),
            ) {
                Some(tool) => tools.push(tool),
                None => {
                    return Err(AgentError::ConfigurationError(format!(
                        "Unknown builtin tool: {}",
                        tool_name
                    )))
                }
            }
        }

        // Add MCP tools
        let mut config_changed = false;
        for (mcp_name, mcp_tool_config) in &mut config.tools.mcp {
            // Try to check OAuth and connect
            let oauth_result = Self::mcp_check_oauth(mcp_name, &mut mcp_tool_config.config).await;

            match oauth_result {
                Ok(oauth_changed) => {
                    if oauth_changed {
                        config_changed = true;
                    }
                }
                Err(e) => {
                    if mcp_tool_config.required {
                        return Err(e);
                    } else {
                        warn!(target: "agent::builder", "MCP '{}' failed to connect: {}. Skipping (not required).", mcp_name, e);
                        mcp_status.push(McpServerStatus {
                            name: mcp_name.clone(),
                            connected: false,
                            tool_count: 0,
                            error: Some(e.to_string()),
                        });
                        continue;
                    }
                }
            }

            // Get all tools from MCP client
            let mcp_client = create_mcp_client(mcp_tool_config.config.clone());
            let mcp_tools_result = get_mcp_tools(mcp_client, mcp_name).await;

            let all_mcp_tools = match mcp_tools_result {
                Ok(tools) => tools,
                Err(e) => {
                    if mcp_tool_config.required {
                        return Err(AgentError::ConfigurationError(format!(
                            "Failed to get tools from MCP '{}': {}",
                            mcp_name, e
                        )));
                    } else {
                        warn!(target: "agent::builder", "MCP '{}' failed to get tools: {}. Skipping (not required).", mcp_name, e);
                        mcp_status.push(McpServerStatus {
                            name: mcp_name.clone(),
                            connected: false,
                            tool_count: 0,
                            error: Some(e.to_string()),
                        });
                        continue;
                    }
                }
            };

            // Count tools before filtering
            let mut added_count = 0;

            // Check if we should add all tools or filter by enabled_tools
            if mcp_tool_config.enabled_tools.contains(&"*".to_string()) {
                // Add all tools from this MCP client (except excluded ones)
                for tool in all_mcp_tools {
                    let tool_name = tool.name();
                    if !mcp_tool_config.excluded_tools.contains(&tool_name) {
                        tools.push(tool);
                        added_count += 1;
                    }
                }
            } else {
                // Filter and add only enabled tools (except excluded ones)
                for tool in all_mcp_tools {
                    let tool_name = tool.name();
                    if mcp_tool_config.enabled_tools.contains(&tool_name)
                        && !mcp_tool_config.excluded_tools.contains(&tool_name)
                    {
                        tools.push(tool);
                        added_count += 1;
                    }
                }

                // Check if all enabled tools were found (only when not using wildcard)
                for enabled_tool in &mcp_tool_config.enabled_tools {
                    let found = tools.iter().any(|t| t.name() == *enabled_tool);
                    if !found {
                        if mcp_tool_config.required {
                            return Err(AgentError::ConfigurationError(format!(
                                "Tool '{}' not found in MCP client '{}'",
                                enabled_tool, mcp_name
                            )));
                        } else {
                            warn!(target: "agent::builder", "Tool '{}' not found in MCP client '{}'. Skipping (not required).", enabled_tool, mcp_name);
                        }
                    }
                }
            }

            mcp_status.push(McpServerStatus {
                name: mcp_name.clone(),
                connected: true,
                tool_count: added_count,
                error: None,
            });
        }

        // Save config if OAuth flow added new tokens
        if config_changed {
            config.save().map_err(|e| {
                AgentError::ConfigurationError(format!("Failed to save agent config: {}", e))
            })?;
        }

        Ok((tools, todo_storage, mcp_status))
    }

    /// Handle OAuth flow for MCP connections if needed
    async fn mcp_check_oauth(
        mcp_name: &str,
        mcp_config: &mut McpConfig,
    ) -> Result<bool, AgentError> {
        use crate::tools::mcp::McpConfig;
        let mut config_changed = false;

        // Only handle HTTP configs that might need OAuth
        if let McpConfig::Http { url, auth } = mcp_config {
            let needs_new_token = match auth {
                Some(token) if token.is_expired() => {
                    debug!(target: "agent::builder", "MCP '{}' token expired, refreshing...", mcp_name);
                    true
                }
                Some(_) => {
                    // Test connection with existing token
                    let test_config = McpConfig::Http {
                        url: url.clone(),
                        auth: auth.clone(),
                    };
                    let mut test_client = create_mcp_client(test_config);
                    if test_client.connect().await.is_ok() {
                        debug!(target: "agent::builder", "MCP '{}' connected (authenticated)", mcp_name);
                        false
                    } else {
                        debug!(target: "agent::builder", "MCP '{}' authentication failed, refreshing token...", mcp_name);
                        true
                    }
                }
                None => {
                    // Test connection without auth
                    let test_config = McpConfig::Http {
                        url: url.clone(),
                        auth: None,
                    };
                    let mut test_client = create_mcp_client(test_config);
                    if test_client.connect().await.is_ok() {
                        debug!(target: "agent::builder", "MCP '{}' connected (no auth required)", mcp_name);
                        false
                    } else {
                        debug!(target: "agent::builder", "MCP '{}' requires authentication, starting OAuth flow...", mcp_name);
                        true
                    }
                }
            };

            if needs_new_token {
                let url_clone = url.clone();
                match signin_oauth(&url_clone).await {
                    Ok(token) => {
                        debug!(target: "agent::builder", "MCP '{}' OAuth successful", mcp_name);
                        *auth = Some(token);
                        config_changed = true;
                    }
                    Err(e) => {
                        return Err(AgentError::ConfigurationError(format!(
                            "OAuth failed for MCP '{}': {}",
                            mcp_name, e
                        )));
                    }
                }
            }
        }
        // SSE and Stdio don't need OAuth handling for now

        Ok(config_changed)
    }
}
