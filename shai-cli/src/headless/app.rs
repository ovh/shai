use std::sync::Arc;

use crate::headless::tools::ToolConfig;

use super::tools::{list_all_tools, parse_tools_list, ToolName};
use openai_dive::v1::resources::chat::{ChatMessage, ChatMessageContent};
use shai_core::agent::{
    Agent, AgentBuilder, AgentError, AgentResult, Brain, LoggingConfig, StdoutEventManager,
};
use shai_core::config::agent::AgentConfig;
use shai_core::config::config::ShaiConfig;
use shai_core::runners::coder::coder::CoderBrain;
use shai_core::runners::searcher::searcher::SearcherBrain;
use shai_llm::LlmClient;

pub enum AgentKind {
    Coder,
    #[allow(dead_code)] // TODO: reserved for future "searcher" agent mode
    Searcher,
}

pub struct AppHeadless {
    kind: AgentKind,
    temperature: Option<f32>,
}

impl AppHeadless {
    pub fn new() -> Self {
        Self {
            kind: AgentKind::Coder,
            temperature: None,
        }
    }

    pub fn set_temperature(&mut self, temperature: f32) {
        self.temperature = Some(temperature);
    }

    pub async fn run(
        &self,
        initial_trace: Vec<ChatMessage>,
        tools: Option<String>,
        remove: Option<String>,
        trace: bool,
        agent_name: Option<String>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Configure internal debug logging to file
        /*
        let _ = LoggingConfig::default()
            .level("debug")
            .file_path("agent_debug.log")
            .init();
        */

        // Validate that we have some input
        if initial_trace.is_empty() {
            eprintln!("Error: Please provide a prompt for the coder agent");
            eprintln!(
                "Usage: shai \"your prompt here\" or using pipe echo \"your prompt here\" | shai"
            );
            return Ok(());
        }

        let agent = if let Some(agent_name) = agent_name {
            // Use custom agent from config
            AgentBuilder::create(Some(agent_name))
                .await
                .map_err(|e| format!("Failed to create agent: {}", e))?
                .with_traces(initial_trace)
                .sudo()
                .build()
        } else {
            // Use default agent with provided tools
            let (llm_client, model) = ShaiConfig::get_llm().await?;
            eprintln!(
                "\x1b[2m░ {} on {}\x1b[0m",
                model,
                llm_client.provider().name()
            );

            // Handle tool selection if needed
            if tools.is_some() || remove.is_some() {
                // Need custom tool configuration
                let tools = match (tools, remove) {
                    (Some(tools_str), _) => {
                        let selected_tools = parse_tools_list(&tools_str)?;
                        ToolConfig::new().add_tools(selected_tools)
                    }
                    (None, Some(remove_str)) => {
                        let tools_to_remove = parse_tools_list(&remove_str)?;
                        ToolConfig::new().remove_tools(tools_to_remove)
                    }
                    (None, None) => ToolConfig::new(),
                };

                let toolbox = tools.build_toolbox();
                let brain: Box<dyn Brain> = match self.kind {
                    AgentKind::Coder => Box::new(CoderBrain::new(Arc::new(llm_client), model)),
                    AgentKind::Searcher => {
                        Box::new(SearcherBrain::new(Arc::new(llm_client), model))
                    }
                };

                AgentBuilder::with_brain(brain)
                    .tools(toolbox)
                    .with_traces(initial_trace)
                    .sudo()
                    .build()
            } else {
                // Use default agent
                AgentBuilder::default()
                    .await
                    .map_err(|e| format!("Failed to create default agent: {}", e))?
                    .with_traces(initial_trace)
                    .sudo()
                    .build()
            }
        };

        if let Some(temp) = self.temperature {
            *agent.temperature.write().await = temp;
        }

        let result = agent
            .with_event_handler(StdoutEventManager::new())
            .run()
            .await;

        match result {
            Ok(AgentResult {
                success: _success,
                message: _message,
                trace: agent_trace,
            }) => {
                // TODO: use `success` to set exit code and `message` for summary output
                if trace {
                    println!("{}", serde_json::to_string_pretty(&agent_trace)?);
                } else {
                    if let Some(message) = agent_trace.last() {
                        match message {
                            ChatMessage::Assistant {
                                content: Some(ChatMessageContent::Text(content)),
                                ..
                            } => {
                                println!("{}", content);
                            }
                            ChatMessage::Tool { content, .. } => {
                                println!("{}", content);
                            }
                            _ => {}
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("Agent failed: {}", e);
            }
        }
        Ok(())
    }
}
