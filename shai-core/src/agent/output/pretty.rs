use crate::agent::{AgentError, AgentEvent};
use crate::tools::highlight::highlight_content;
use crate::tools::{ToolCall, ToolResult};
use chrono::Utc;
use openai_dive::v1::resources::chat::{ChatMessage, ChatMessageContent};
use termimad::crossterm::style::Color;
use termimad::{rgb, MadSkin};

/// Pretty formatter that formats agent events into strings for display
pub struct PrettyFormatter {
    skin: MadSkin,
    max_preview_lines: usize,
}

impl PrettyFormatter {
    pub fn new() -> Self {
        Self::with_max_preview_lines(50)
    }

    pub fn with_max_preview_lines(max_preview_lines: usize) -> Self {
        let mut skin = MadSkin::default_dark();
        skin.code_block.set_fgbg(Color::DarkGrey, Color::Reset);
        Self {
            skin,
            max_preview_lines,
        }
    }

    /// Format an agent event into a displayable string
    pub fn format_event(&self, event: &AgentEvent) -> Option<String> {
        match event {
            AgentEvent::ThinkingStart => None,
            AgentEvent::BrainResult { thought, .. } => self.format_thinking(thought),
            AgentEvent::ToolCallStarted { .. } => {
                // do nothing because tool can be call in parallel, we only display the result
                None
            }
            AgentEvent::ToolCallCompleted { call, result, .. } => {
                Some(self.format_tool_result(call, result))
            }
            AgentEvent::StatusChanged { .. } => {
                // Don't format state changes - only show brain results and tool calls
                None
            }
            AgentEvent::UserInput { input } => {
                // Display > literally, then process the content as markdown
                let lines: Vec<&str> = input.lines().collect();
                let mut output = String::new();

                if lines.len() == 1 {
                    // Single line: ANSI prefix + markdown content
                    output.push_str("\x1b[36m>\x1b[0m ");
                    let mut user_skin = self.skin.clone();
                    user_skin.paragraph.set_fg(rgb(180, 180, 180)); // Light grey
                    output.push_str(&user_skin.term_text(input).to_string());
                } else {
                    // Multi-line: ANSI prefix for first line, then markdown for rest
                    output.push_str(&format!("\x1b[36m>\x1b[0m {}", lines[0]));

                    if lines.len() > 1 {
                        let remaining_content = lines[1..].join("\n");
                        if !remaining_content.trim().is_empty() {
                            output.push('\n');
                            let mut user_skin = self.skin.clone();
                            user_skin.paragraph.set_fg(rgb(180, 180, 180)); // Light grey
                            let formatted_content =
                                user_skin.term_text(&remaining_content).to_string();
                            // Add 2-space indent to each line
                            for line in formatted_content.lines() {
                                output.push_str(&format!("  {}\n", line));
                            }
                            output.pop(); // Remove last newline
                        }
                    }
                }

                Some(output)
            }
            AgentEvent::UserInputRequired { .. } => {
                //let markdown = format!("🤔 **User input required:** {:?}", request);
                //Some(self.skin.term_text(&markdown).to_string())
                None
            }
            AgentEvent::PermissionRequired { .. } => {
                //let markdown = format!("🔐 **Permission required:** {}", request.operation);
                //Some(self.skin.term_text(&markdown).to_string())
                None
            }
            AgentEvent::Error { error } => {
                let markdown = format!("❌ **Error:** {}", error);
                let mut error_skin = self.skin.clone();
                error_skin.paragraph.set_fg(rgb(255, 100, 100)); // Red for errors
                error_skin.bold.set_fg(rgb(255, 150, 150)); // Light red for bold
                Some(error_skin.term_text(&markdown).to_string())
            }
            AgentEvent::Completed { success, message } => {
                let markdown = if *success {
                    format!("✅ **Completed:** {}", message)
                } else {
                    format!("❌ **Failed:** {}", message)
                };

                let mut completion_skin = self.skin.clone();
                if *success {
                    completion_skin.paragraph.set_fg(rgb(100, 255, 100)); // Green for success
                    completion_skin.bold.set_fg(rgb(150, 255, 150)); // Light green for bold
                } else {
                    completion_skin.paragraph.set_fg(rgb(255, 100, 100)); // Red for failure
                    completion_skin.bold.set_fg(rgb(255, 150, 150)); // Light red for bold
                }

                Some(completion_skin.term_text(&markdown).to_string())
            }
            AgentEvent::TokenUsage { .. } => {
                // Don't display token usage in the main output - it's handled by /tokens command
                None
            }
            AgentEvent::TodoUpdated { .. } => {
                // Todo updates are displayed in the sidebar, not in the main output
                None
            }
        }
        .map(|s| format!("\n{}", s))
    }

    /// Format a thinking message
    fn format_thinking(&self, thought: &Result<ChatMessage, AgentError>) -> Option<String> {
        match thought {
            Ok(ChatMessage::Assistant {
                content,
                reasoning_content,
                ..
            }) => {
                let content_empty = content.as_ref().is_none_or(
                    |c| matches!(c, ChatMessageContent::Text(t) if t.trim().is_empty()),
                );
                let reasoning_empty = reasoning_content
                    .as_deref()
                    .is_none_or(|r| r.trim().is_empty());
                if content_empty && reasoning_empty {
                    return None;
                }

                let parts: Vec<_> = [
                    reasoning_content
                        .as_deref()
                        .filter(|r| !r.trim().is_empty())
                        .map(|r| {
                            let mut reasoning_skin = self.skin.clone();
                            reasoning_skin.paragraph.set_fg(rgb(120, 120, 120)); // Dim text
                            format!("\x1b[2m✻ {}\x1b[0m", reasoning_skin.term_text(r))
                        }),
                    content.as_ref().and_then(|c| match c {
                        ChatMessageContent::Text(text) if !text.trim().is_empty() => {
                            Some(format!("● {}\x1b[0m", self.skin.term_text(text)))
                        }
                        _ => None,
                    }),
                ]
                .into_iter()
                .flatten()
                .collect();
                (!parts.is_empty()).then(|| parts.join("\n"))
            }
            Err(err) => {
                let mut error_skin = self.skin.clone();
                error_skin.paragraph.set_fg(rgb(255, 100, 100));
                error_skin.bold.set_fg(rgb(255, 150, 150));
                Some(
                    error_skin
                        .text(&format!("● **Error:** {}", err), None)
                        .to_string(),
                )
            }
            _ => None,
        }
    }

    /// Format tool started
    pub fn format_tool_started(&self, call: &ToolCall) -> String {
        let tool_name = match call.tool_name.as_str() {
            "ls" => "List".to_string(),
            _ => Self::capitalize_first(&call.tool_name),
        };
        let context = Self::extract_primary_param(&call.parameters, &call.tool_name);

        let mut output = String::new();
        if let Some((_, ctx)) = context {
            output.push_str(&format!(
                "\x1b[36m→\x1b[0m \x1b[1m{}\x1b[0m {}",
                tool_name, ctx
            ));
        } else {
            output.push_str(&format!("\x1b[36m→\x1b[0m \x1b[1m{}\x1b[0m", tool_name));
        }
        output
    }

    /// Format tool started
    pub fn format_tool_running(&self, call: &ToolCall) -> String {
        let tool_name = match call.tool_name.as_str() {
            "ls" => "List".to_string(),
            _ => Self::capitalize_first(&call.tool_name),
        };
        let context = Self::extract_primary_param(&call.parameters, &call.tool_name);

        let mut output = String::new();
        let bullet = if (Utc::now().timestamp_millis() / 500) % 2 == 0 {
            "→"
        } else {
            "➔"
        };
        if let Some((_, ctx)) = context {
            output.push_str(&format!(
                "\x1b[36m{}\x1b[0m \x1b[1m{}\x1b[0m {}",
                bullet, tool_name, ctx
            ));
        } else {
            output.push_str(&format!(
                "\x1b[36m{}\x1b[0m \x1b[1m{}\x1b[0m",
                bullet, tool_name
            ));
        }
        output
    }

    /// Format tool result
    fn format_tool_result(&self, call: &ToolCall, result: &ToolResult) -> String {
        let tool_name = Self::capitalize_first(&call.tool_name);
        let context = Self::extract_primary_param(&call.parameters, &call.tool_name);

        let color = if matches!(result, ToolResult::Success { .. }) {
            "\x1b[32m"
        } else {
            "\x1b[31m"
        };

        // Compact format for read/find/ls tools
        if matches!(call.tool_name.as_str(), "read" | "find" | "ls") {
            let mut parts = Vec::new();
            if call.tool_name == "read" {
                if let Some(files) = call.parameters.get("files").and_then(|f| f.as_array()) {
                    if let Some(first_file) = files.first().and_then(|f| f.as_object()) {
                        if let Some(offset) = first_file.get("offset").and_then(|v| v.as_u64()) {
                            if offset > 1 {
                                parts.push(format!("offset={}", offset));
                            }
                        }
                        if let Some(limit) = first_file.get("limit").and_then(|v| v.as_u64()) {
                            parts.push(format!("limit={}", limit));
                        }
                    }
                }
            }
            let params_str = if parts.is_empty() {
                String::new()
            } else {
                format!(" [{}]", parts.join(", "))
            };

            let mut output = String::new();
            if let Some((_, ctx)) = &context {
                output.push_str(&format!(
                    "{}→\x1b[0m \x1b[1m{}\x1b[0m {}{}\n",
                    color, tool_name, ctx, params_str
                ));
            } else {
                output.push_str(&format!(
                    "{}→\x1b[0m \x1b[1m{}\x1b[0m{}\n",
                    color, tool_name, params_str
                ));
            }

            // Show error/denied message if applicable
            match result {
                ToolResult::Error { error, .. } => {
                    output.push_str(&format!("  \x1b[2;31mError: {}\x1b[0m", error));
                }
                ToolResult::Denied => {
                    output.push_str(
                        "  \x1b[2;31mDenied: The tool call was rejected by the user\x1b[0m",
                    );
                }
                _ => {}
            }

            return output;
        }

        let mut output = String::new();
        if let Some((_, ctx)) = context {
            output.push_str(&format!(
                "{}→\x1b[0m \x1b[1m{}\x1b[0m {}\n",
                color, tool_name, ctx
            ));
        } else {
            output.push_str(&format!("{}→\x1b[0m \x1b[1m{}\x1b[0m\n", color, tool_name));
        }

        match result {
            ToolResult::Success {
                output: tool_output,
                ..
            } => {
                if tool_output.trim().is_empty() {
                    // Use ANSI codes: bold "Completed"
                    output.push_str("  → \x1b[1mCompleted\x1b[0m");
                } else {
                    let lines = tool_output.lines().count();
                    let chars = tool_output.len();

                    // Use ANSI codes: bold numbers, normal text
                    if lines == 1 {
                        output.push_str(&format!("  → \x1b[1m{}\x1b[0m chars", chars));
                    } else {
                        output.push_str(&format!(
                            "  → \x1b[1m{}\x1b[0m lines, \x1b[1m{}\x1b[0m chars",
                            lines, chars
                        ));
                    }

                    // Show first N lines for user display only for specific tools
                    if matches!(
                        call.tool_name.as_str(),
                        "bash" | "edit" | "multiedit" | "write" | "todo_read" | "todo_write"
                    ) {
                        let preview_lines: Vec<&str> =
                            tool_output.lines().take(self.max_preview_lines).collect();
                        if !preview_lines.is_empty() {
                            // Determine if we should syntax-highlight this output
                            let should_highlight = matches!(call.tool_name.as_str(), "write");
                            let file_path = if should_highlight {
                                Self::extract_primary_param(&call.parameters, &call.tool_name)
                                    .map(|(_, path)| path)
                            } else {
                                None
                            };

                            let mut markdown_content = String::new();
                            markdown_content.push('\n');
                            for line in &preview_lines {
                                if should_highlight {
                                    // Syntax-highlighted output — no diff coloring
                                    markdown_content.push_str(&format!("      {}\n", line));
                                } else if line.starts_with("+") && !line.starts_with("++") {
                                    markdown_content
                                        .push_str(&format!("      \x1b[32m{}\x1b[0m\n", line));
                                } else if line.starts_with("-") && !line.starts_with("--") {
                                    markdown_content
                                        .push_str(&format!("      \x1b[31m{}\x1b[0m\n", line));
                                } else {
                                    markdown_content.push_str(&format!("      {}\n", line));
                                }
                            }
                            if lines > self.max_preview_lines {
                                markdown_content.push_str(&format!(
                                    "      ... {} more lines\n",
                                    lines - self.max_preview_lines
                                ));
                            }

                            // Apply syntax highlighting if we have a file path
                            let final_content = if let Some(path) = file_path {
                                // Reconstruct the preview content without leading newlines/spaces
                                let raw_preview: String = preview_lines.join("\n");
                                let highlighted = highlight_content(&raw_preview, &path);
                                let mut result = String::new();
                                result.push('\n');
                                for line in highlighted.lines() {
                                    result.push_str(&format!("      {}\n", line));
                                }
                                if lines > self.max_preview_lines {
                                    result.push_str(&format!(
                                        "      ... {} more lines\n",
                                        lines - self.max_preview_lines
                                    ));
                                }
                                result
                            } else {
                                markdown_content
                            };

                            // Render markdown content and append to output
                            output.push_str(&self.skin.term_text(&final_content).to_string());
                        }
                    }
                }
            }
            ToolResult::Error { error, .. } => {
                // Use ANSI codes: entire line dim red
                output.push_str(&format!("  → \x1b[2;31mError: {}\x1b[0m", error));
            }
            ToolResult::Denied => {
                // Use ANSI codes: entire line dim red
                output.push_str(
                    "  → \x1b[2;31mDenied: The tool call was rejected by the user\x1b[0m",
                );
            }
        }

        output
    }

    /// Extract the most relevant parameter for display context
    pub fn extract_primary_param(
        args: &serde_json::Value,
        tool_name: &str,
    ) -> Option<(String, String)> {
        if let Some(obj) = args.as_object() {
            // Common parameter names to look for, in order of preference
            let param_names = match tool_name {
                "read" | "write" | "edit" | "multiedit" => vec!["path"],
                "ls" | "glob" => vec!["path", "directory", "pattern"],
                "find" | "grep" => vec!["pattern", "path"],
                "bash" => vec!["command"],
                _ => vec!["path", "pattern", "command", "query", "input"],
            };

            for param in param_names {
                if let Some(value) = obj.get(param).and_then(|v| v.as_str()) {
                    return Some((param.to_string(), Self::format_param_value(value)));
                }
            }

            // If no top-level match, check inside the "files" array (read/write/edit tools)
            if let Some(files) = obj.get("files").and_then(|f| f.as_array()) {
                if let Some(first_file) = files.first().and_then(|f| f.as_object()) {
                    if let Some(value) = first_file.get("path").and_then(|v| v.as_str()) {
                        return Some(("path".to_string(), Self::format_param_value(value)));
                    }
                }
            }

            // If no specific param found, take the first string value
            for (param, value) in obj {
                if let Some(s) = value.as_str() {
                    return Some((param.to_string(), Self::format_param_value(s)));
                }
            }
        }
        None
    }

    /// Format parameter value for display
    pub fn format_param_value(value: &str) -> String {
        Self::format_path(value)
    }

    /// Format file path to be more readable
    pub fn format_path(path: &str) -> String {
        // Remove common prefixes to make paths shorter
        if let Ok(current_dir) = std::env::current_dir() {
            if let Some(current_path) = current_dir.to_str() {
                let prefix = format!("{}/", current_path);
                return path.replace(&prefix, "");
            }
        }
        path.to_string()
    }

    /// Capitalize first letter of string
    pub fn capitalize_first(s: &str) -> String {
        let mut chars = s.chars();
        match chars.next() {
            None => String::new(),
            Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        }
    }

    pub fn format_toolcall(&self, call: &ToolCall, preview: Option<&ToolResult>) -> String {
        // If preview is available, use it instead of env variables
        if let Some(preview_result) = preview {
            return preview_result.to_string();
        }

        // Fall back to original logic (env variables)
        let mut output = String::new();
        let removed =
            Self::extract_primary_param(&call.parameters, &call.tool_name).map(|(param, _)| param);
        if !call.parameters.is_null()
            && !call.parameters.as_object().is_none_or(|obj| obj.is_empty())
        {
            match &call.parameters {
                serde_json::Value::Object(map) => {
                    for (key, value) in map {
                        if matches!(removed.as_ref(), Some(rm) if rm == key) {
                            continue;
                        }
                        output.push_str(&format!(
                            "{}: {}\n",
                            key,
                            self.format_tool_parameter(value)
                        ));
                    }
                }
                _ => {
                    output.push_str(&self.format_tool_parameter(&call.parameters));
                }
            }
        }

        output
    }

    pub fn format_tool_parameter(&self, param: &serde_json::Value) -> String {
        match &param {
            serde_json::Value::String(s) => s.to_string(),
            serde_json::Value::Number(n) => {
                format!("{}", n)
            }
            serde_json::Value::Bool(b) => {
                format!("{}", b)
            }
            serde_json::Value::Null => "null\n".to_string(),
            serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
                serde_json::to_string_pretty(&param).unwrap_or_else(|_| "Invalid JSON".to_string())
            }
        }
    }
}

impl Default for PrettyFormatter {
    fn default() -> Self {
        Self::new()
    }
}
