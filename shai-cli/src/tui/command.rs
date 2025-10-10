use std::{collections::HashMap, io, time::Duration};
use shai_llm::ToolCallMethod;

use crate::tui::App;

impl App<'_> {
    pub(crate) fn list_command() -> HashMap<(String, String),Vec<String>> {
        HashMap::from([
            (("/exit","exit from the tui"), vec![]),
            (("/auth","select a provider"), vec![]),
            (("/tc","set the tool call method: [fc | fc2 | so]"), vec!["method"]),
            (("/tokens","display token usage (input/output)"), vec![]),
            (("/compact","trigger context compaction"), vec![]),
        ])
        .into_iter()
        .map(|((cmd,desc),args)|((cmd.to_string(),desc.to_string()),args.into_iter().map(|s|s.to_string()).collect()))
        .collect()
    }

    pub(crate) async fn handle_app_command(&mut self, command: &str) -> io::Result<()> {
        let mut parts = command.split_whitespace();
        let cmd = parts.next().unwrap();
        let args: Vec<&str> = parts.collect();

        match cmd {
            "/exit" => {
                self.exit = true;
            }
            "/tc" => {
                if let Some(ref agent) = self.agent {
                    match args.into_iter().next() {
                        Some("auto") => {
                            if let Ok(method) = agent.controller.set_method(Some(ToolCallMethod::Auto)).await {
                                self.input.alert_msg("llm will now try all method for tool calls", Duration::from_secs(3));
                                self.input.set_tool_call_method(method);
                            }
                        }
                        Some("fc") => {
                            if let Ok(method) = agent.controller.set_method(Some(ToolCallMethod::FunctionCall)).await {
                                self.input.alert_msg("llm will now use function calling api for tool calls", Duration::from_secs(3));
                                self.input.set_tool_call_method(method);
                            }
                        }
                        Some("fc2") => {
                            if let Ok(method) = agent.controller.set_method(Some(ToolCallMethod::FunctionCallRequired)).await {
                                self.input.alert_msg("llm will now use function calling in required mode for tool calls", Duration::from_secs(3));
                                self.input.set_tool_call_method(method);
                            }
                        }
                        Some("so") => {
                            if let Ok(method) = agent.controller.set_method(Some(ToolCallMethod::StructuredOutput)).await {
                                self.input.alert_msg("llm will now use structured output for tool calls", Duration::from_secs(3));
                                self.input.set_tool_call_method(method);
                            }
                        }
                        _ => {}
                    }
                }
            }
            "/tokens" => {
                let msg = format!(
                    "Token Usage - Input: {}, Output: {}, Total: {}",
                    self.total_input_tokens,
                    self.total_output_tokens,
                    self.total_input_tokens + self.total_output_tokens
                );
                self.input.alert_msg(&msg, Duration::from_secs(5));
            }
            "/compact" => {
                if let Some(ref agent) = self.agent {
                    let _ = agent.controller.trigger_context_compression().await;
                    self.input.alert_msg("Context compression triggered", Duration::from_secs(2));
                }
            }
            _ => {
                self.input.alert_msg("command unknown", Duration::from_secs(1));
            }
        }
        Ok(())
    }
}