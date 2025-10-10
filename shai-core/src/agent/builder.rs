use shai_llm::{ChatMessage, LlmClient};
use uuid::Uuid;
use std::sync::Arc;



use crate::tools::mcp::mcp_oauth::signin_oauth;
use crate::tools::{create_mcp_client, get_mcp_tools, AnyTool, BashTool, EditTool, FetchTool, FindTool, FsOperationLog, LsTool, McpConfig, MultiEditTool, ReadTool, TodoReadTool, TodoStorage, TodoWriteTool, WriteTool};
use crate::config::agent::AgentConfig;
use crate::runners::coder::CoderBrain;
use super::Brain;
use super::AgentCore;
use super::claims::ClaimManager;
use super::AgentError;

/// Builder for AgentCore
pub struct AgentBuilder {
    pub session_id: String,
    pub brain: Box<dyn Brain>,
    pub goal: Option<String>,
    pub trace: Vec<ChatMessage>,
    pub available_tools: Vec<Box<dyn AnyTool>>,
    pub permissions: ClaimManager,
}

impl AgentBuilder {
    pub fn new(brain: Box<dyn Brain>) -> Self {
        Self {
            session_id: Uuid::new_v4().to_string(),
            brain: brain,
            goal: None,
            trace: vec![],
            available_tools: vec![],
            permissions: ClaimManager::new(),
        }
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

    /// Enable sudo mode - bypasses all permission checks
    pub fn sudo(mut self) -> Self {
        self.permissions.sudo();
        self
    }

    /// Build the AgentCore with required runtime fields
    pub fn build(mut self) -> AgentCore {        
        if let Some(goal) = self.goal {
            self.trace.push(ChatMessage::User { content: shai_llm::ChatMessageContent::Text(goal.clone()), name: None });
        }


        AgentCore::new(
            self.session_id.clone(),
            self.brain,
            self.trace,
            self.available_tools,
            self.permissions
        )
    }

    /// Create an AgentBuilder from an AgentConfig
    pub async fn from_config(mut config: AgentConfig) -> Result<Self, AgentError> {
        // Create LLM client from provider config using the utility method
        let llm_client = Arc::new(
            LlmClient::create_provider(&config.llm_provider.provider, &config.llm_provider.env_vars)
                .map_err(|e| AgentError::LlmError(e.to_string()))?
        );
        
        // Create brain with custom system prompt and temperature
        let brain = Box::new(CoderBrain::with_custom_prompt(
            llm_client.clone(),
            config.llm_provider.model.clone(),
            config.system_prompt.clone(),
            config.temperature,
        ));

        // Create tools
        let tools = Self::create_tools_from_config(&mut config).await?;
        
        // Display available tools by category
        let mut tool_groups: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
        
        for tool in &tools {
            let group_name = tool.group().unwrap_or("unknown").to_string();
            tool_groups.entry(group_name).or_insert_with(Vec::new).push(tool.name());
        }
        
        // Display builtin tools first
        if let Some(builtin_tools) = tool_groups.remove("builtin") {
            eprintln!("\x1b[2m░ builtin: {}\x1b[0m", builtin_tools.join(", "));
        }
        
        // Display MCP tools
        for (group_name, group_tools) in tool_groups {
            if group_name != "unknown" {
                eprintln!("\x1b[2m░ mcp({}): {}\x1b[0m", group_name, group_tools.join(", "));
            }
        }

        Ok(Self::new(brain)
            .tools(tools)
            .id(&format!("agent-{}", config.name)))
    }

    /// Create tools from config
    async fn create_tools_from_config(config: &mut AgentConfig) -> Result<Vec<Box<dyn AnyTool>>, AgentError> {
        let mut tools: Vec<Box<dyn AnyTool>> = Vec::new();

        // Create shared storage for todo tools
        let todo_storage = Arc::new(TodoStorage::new());
        
        // Create shared operation log for file system tools
        let fs_log = Arc::new(FsOperationLog::new());

        // Add builtin tools based on config
        let builtin_tools_to_add = if config.tools.builtin.contains(&"*".to_string()) {
            // Add all builtin tools
            vec!["bash", "edit", "multiedit", "fetch", "search", "ls", "read", "todo_read", "todo_write", "write"]
        } else {
            // Add only specified tools
            config.tools.builtin.iter().map(|s| s.as_str()).collect()
        };

        for tool_name in builtin_tools_to_add {
            // Skip if tool is in builtin excluded list
            if config.tools.builtin_excluded.contains(&tool_name.to_string()) {
                continue;
            }
            
            match tool_name {
                "bash" => tools.push(Box::new(BashTool::new())),
                "edit" => tools.push(Box::new(EditTool::new(fs_log.clone()))),
                "multiedit" => tools.push(Box::new(MultiEditTool::new(fs_log.clone()))),
                "fetch" => tools.push(Box::new(FetchTool::new())),
                "search" => tools.push(Box::new(FindTool::new())),
                "ls" => tools.push(Box::new(LsTool::new())),
                "read" => tools.push(Box::new(ReadTool::new(fs_log.clone()))),
                "todo_read" => tools.push(Box::new(TodoReadTool::new(todo_storage.clone()))),
                "todo_write" => tools.push(Box::new(TodoWriteTool::new(todo_storage.clone()))),
                "write" => tools.push(Box::new(WriteTool::new(fs_log.clone()))),
                _ => return Err(AgentError::ConfigurationError(format!("Unknown builtin tool: {}", tool_name))),
            }
        }

        // Add MCP tools
        let mut config_changed = false;
        for (mcp_name, mcp_tool_config) in &mut config.tools.mcp {
            let oauth_changed = Self::mcp_check_oauth(mcp_name, &mut mcp_tool_config.config).await?;
            if oauth_changed {
                config_changed = true;
            }

            // Get all tools from MCP client
            let mcp_client = create_mcp_client(mcp_tool_config.config.clone());
            let all_mcp_tools = get_mcp_tools(mcp_client, mcp_name).await
                .map_err(|e| AgentError::ConfigurationError(format!("Failed to get tools from MCP '{}': {}", mcp_name, e)))?;
            
            // Check if we should add all tools or filter by enabled_tools
            if mcp_tool_config.enabled_tools.contains(&"*".to_string()) {
                // Add all tools from this MCP client (except excluded ones)
                for tool in all_mcp_tools {
                    let tool_name = tool.name();
                    if !mcp_tool_config.excluded_tools.contains(&tool_name) {
                        tools.push(tool);
                    }
                }
            } else {
                // Filter and add only enabled tools (except excluded ones)
                for tool in all_mcp_tools {
                    let tool_name = tool.name();
                    if mcp_tool_config.enabled_tools.contains(&tool_name) && !mcp_tool_config.excluded_tools.contains(&tool_name) {
                        tools.push(tool);
                    }
                }
                
                // Check if all enabled tools were found (only when not using wildcard)
                for enabled_tool in &mcp_tool_config.enabled_tools {
                    let found = tools.iter().any(|t| t.name() == *enabled_tool);
                    if !found {
                        return Err(AgentError::ConfigurationError(format!("Tool '{}' not found in MCP client '{}'", enabled_tool, mcp_name)));
                    }
                }
            }
        }

        // Save config if OAuth flow added new tokens
        if config_changed {
            config.save().map_err(|e| AgentError::ConfigurationError(format!("Failed to save agent config: {}", e)))?;
        }

        Ok(tools)
    }

    /// Handle OAuth flow for MCP connections if needed
    async fn mcp_check_oauth(mcp_name: &str, mcp_config: &mut McpConfig) -> Result<bool, AgentError> {
        use crate::tools::mcp::McpConfig;
        
        let mut config_changed = false;
        
        // Only handle HTTP configs that might need OAuth
        if let McpConfig::Http { url, bearer_token } = mcp_config {
            // Test connection with current config
            let test_config = McpConfig::Http { 
                url: url.clone(), 
                bearer_token: bearer_token.clone() 
            };
            let mut test_client = create_mcp_client(test_config);
            match test_client.connect().await {
                Ok(_) => {
                    if bearer_token.is_some() {
                        eprintln!("\x1b[2m░ MCP '{}' connected (authenticated)\x1b[0m", mcp_name);
                    } else {
                        eprintln!("\x1b[2m░ MCP '{}' connected (no auth)\x1b[0m", mcp_name);
                    }
                }
                Err(_) => {
                    eprintln!("\x1b[2m░ MCP '{}' connection failed, starting OAuth flow...\x1b[0m", mcp_name);
                    let url_clone = url.clone();
                    match signin_oauth(&url_clone).await {
                        Ok(token) => {
                            eprintln!("\x1b[2m░ MCP '{}' connected (OAuth successful)\x1b[0m", mcp_name);
                            *bearer_token = Some(token);
                            config_changed = true;
                        }
                        Err(e) => {
                            return Err(AgentError::ConfigurationError(format!("OAuth failed for MCP '{}': {}", mcp_name, e)));
                        }
                    }
                }
            }
        }
        // SSE and Stdio don't need OAuth handling for now
        
        Ok(config_changed)
    }
}
