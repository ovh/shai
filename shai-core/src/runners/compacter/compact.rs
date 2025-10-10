use shai_llm::{ChatMessage, ChatMessageContent, client::LlmClient};
use tracing::{debug, info, warn};
use std::sync::Arc;
use openai_dive::v1::resources::chat::ChatCompletionParametersBuilder;

use super::prompt::get_compression_summary_prompt;

/// Information about a compression operation
#[derive(Debug, Clone)]
pub struct CompressionInfo {
    pub original_message_count: usize,
    pub compressed_message_count: usize,
    pub tokens_before: Option<u32>,
    pub current_tokens: Option<u32>,
    pub max_tokens: u32,
    pub ai_summary: Option<String>,
}

/// Context compression utilities for managing conversation history within token limits
#[derive(Clone)]
pub struct ContextCompressor {
    max_tokens: u32,
    current_tokens: u32,
    llm_client: Option<Arc<LlmClient>>,
    model: Option<String>,
}

impl ContextCompressor {
    pub fn new(max_tokens: u32) -> Self {
        Self {
            max_tokens,
            current_tokens: 0,
            llm_client: None,
            model: None,
        }
    }

    pub fn new_with_llm(max_tokens: u32, llm_client: Arc<LlmClient>, model: String) -> Self {
        Self {
            max_tokens,
            current_tokens: 0,
            llm_client: Some(llm_client),
            model: Some(model),
        }
    }

    /// Update the current token count
    pub fn update_token_count(&mut self, input_tokens: u32, output_tokens: u32) {
        self.current_tokens += input_tokens + output_tokens;
        debug!(
            target: "context_compression",
            current_tokens = self.current_tokens,
            max_tokens = self.max_tokens,
            "Updated token count"
        );
    }

    /// Check if we're approaching the context limit with dynamic threshold
    /// Uses different thresholds based on context size to avoid premature compression
    pub fn should_compress(&self) -> bool {
        let threshold_percentage = 0.90;
        let threshold = (self.max_tokens as f64 * threshold_percentage) as u32;
        self.current_tokens >= threshold
    }

    /// Check if compression is actually beneficial given the current conversation
    pub fn should_compress_conversation(&self, messages: &[ChatMessage]) -> bool {
        // Don't compress if we're not near the limit
        if !self.should_compress() {
            return false;
        }

        // Count non-system messages
        let non_system_count = messages.iter()
            .filter(|msg| !matches!(msg, ChatMessage::System { .. }))
            .count();

        // Only compress if we have at least 2 messages (1 user-assistant pair)
        // This ensures we have something meaningful to summarize
        let should_compress = non_system_count > 2;

        let threshold_percentage = 80;

        debug!(
            target: "context_compression",
            non_system_count = non_system_count,
            current_tokens = self.current_tokens,
            max_tokens = self.max_tokens,
            should_compress = should_compress,
            threshold_percentage = threshold_percentage,
            "Compression decision"
        );

        should_compress
    }

    /// Force compress the conversation history regardless of thresholds
    /// Keeps the system message and recent messages while summarizing middle conversation
    /// Returns (compressed_messages, compression_info)
    pub async fn compress_messages_force(&mut self, messages: Vec<ChatMessage>, full_trace: Vec<ChatMessage>) -> (Vec<ChatMessage>, Option<CompressionInfo>) {
        // Count non-system messages to ensure we have something to compress
        let non_system_count = messages.iter()
            .filter(|msg| !matches!(msg, ChatMessage::System { .. }))
            .count();

        // Only compress if we have at least 2 messages (1 user-assistant pair)
        if non_system_count <= 2 {
            info!(target: "context_compression", "Not enough messages to compress (need > 2 non-system messages)");
            return (messages, None);
        }

        self.compress_messages_internal(messages, full_trace).await
    }

    /// Compress the conversation history by removing older messages and replacing with AI summary
    /// Keeps the system message and recent messages while summarizing middle conversation
    /// Returns (compressed_messages, compression_info)
    pub async fn compress_messages(&mut self, messages: Vec<ChatMessage>, full_trace: Vec<ChatMessage>) -> (Vec<ChatMessage>, Option<CompressionInfo>) {
        if !self.should_compress_conversation(&messages) {
            return (messages, None);
        }

        self.compress_messages_internal(messages, full_trace).await
    }

    /// Internal method that performs the actual compression
    async fn compress_messages_internal(&mut self, messages: Vec<ChatMessage>, full_trace: Vec<ChatMessage>) -> (Vec<ChatMessage>, Option<CompressionInfo>) {

        let original_count = messages.len();
        let tokens_before_compression = self.current_tokens;

        info!(
            target: "context_compression",
            total_tokens_before = self.current_tokens,
            max_tokens = self.max_tokens,
            original_message_count = messages.len(),
            "Compressing context due to token limit"
        );

        // Extract the most recent user message from the full conversation history (full_trace)
        let first_user_message = full_trace.iter()
            .rev()
            .find_map(|msg| {
                if let ChatMessage::User { content, .. } = msg {
                    if let ChatMessageContent::Text(text) = content {
                        return Some(text.clone());
                    }
                }
                None
            })
            .unwrap_or_else(|| "[No user message found]".to_string());

        let mut compressed = Vec::new();
        let mut system_messages = Vec::new();
        let mut middle_messages = Vec::new();
        let mut recent_messages = Vec::new();

        // First pass: filter out old summary messages and collect non-system messages
        let non_summary_messages: Vec<ChatMessage> = messages.iter()
            .filter(|msg| {
                // Filter out old summary messages
                !matches!(msg, ChatMessage::System { name: Some(name), .. } if name == "summary")
            })
            .cloned()
            .collect();

        // Second pass: categorize messages
        let non_system_count = non_summary_messages.iter()
            .filter(|msg| !matches!(msg, ChatMessage::System { .. }))
            .count();

        let mut non_system_index = 0;
        for message in &non_summary_messages {
            match message {
                ChatMessage::System { .. } => {
                    // Keep non-summary system messages (like the original system prompt)
                    system_messages.push(message.clone());
                }
                _ => {
                    // Keep the last 6 non-system messages (2-3 complete interaction cycles) as recent
                    // This ensures we preserve enough context for the agent to understand
                    // what it was doing and avoid repeating actions
                    if non_system_index >= non_system_count.saturating_sub(6) {
                        recent_messages.push(message.clone());
                    } else {
                        middle_messages.push(message.clone());
                    }
                    non_system_index += 1;
                }
            }
        }

        // Add system messages first (excluding old summaries)
        compressed.extend(system_messages);

        // Try to generate AI summary of middle conversation
        // Pass all non-summary messages and the first user message from full_trace
        let (ai_summary, summary_tokens) = if !middle_messages.is_empty() {
            match self.summarize_conversation(&non_summary_messages, &first_user_message).await {
                Ok((summary, tokens)) => {
                    info!(target: "context_compression", "Successfully generated AI summary");
                    compressed.push(ChatMessage::System {
                        content: ChatMessageContent::Text(format!(
                            "Previous conversation summary: {}",
                            summary
                        )),
                        name: Some("summary".to_string()),
                    });
                    (Some(summary), tokens)
                }
                Err(e) => {
                    warn!(target: "context_compression", error = e, "Failed to generate AI summary, using fallback");
                    compressed.push(ChatMessage::System {
                        content: ChatMessageContent::Text(
                            "[Previous conversation history compressed - AI summary unavailable]".to_string()
                        ),
                        name: Some("system".to_string()),
                    });
                    (None, 50) // Estimate for fallback message
                }
            }
        } else {
            (None, 0)
        };

        // Add recent messages
        compressed.extend(recent_messages);

        self.current_tokens = summary_tokens;

        // Safely create compression info with validation
        let compression_info = CompressionInfo {
            original_message_count: original_count,
            compressed_message_count: compressed.len(),
            tokens_before: Some(tokens_before_compression),
            // Only include token info if we have valid data (summary_tokens > 0)
            current_tokens: if summary_tokens > 0 { Some(summary_tokens) } else { None },
            max_tokens: self.max_tokens,
            ai_summary: ai_summary.clone(),
        };

        info!(
            target: "context_compression",
            compressed_message_count = compressed.len(),
            estimated_tokens_after_compression = self.current_tokens,
            output_tokens = self.current_tokens,
            middle_messages_summarized = middle_messages.len(),
            "Context compression with AI summary completed"
        );

        (compressed, Some(compression_info))
    }

    /// Get the current token count
    pub fn get_current_tokens(&self) -> u32 {
        self.current_tokens
    }

    /// Get the maximum token limit
    pub fn get_max_tokens(&self) -> u32 {
        self.max_tokens
    }

    /// Check if we're near the absolute limit (95% threshold)
    pub fn is_near_limit(&self) -> bool {
        let threshold = (self.max_tokens as f64 * 0.95) as u32;
        self.current_tokens >= threshold
    }

    /// Create a summary of the conversation history using AI
    /// Returns (summary_text, summary_tokens_used)
    async fn summarize_conversation(&mut self, messages: &[ChatMessage], first_user_message: &str) -> Result<(String, u32), String> {
        let Some(ref llm_client) = self.llm_client else {
            return Err("No LLM client available for summarization".to_string());
        };

        let Some(ref model) = self.model else {
            return Err("No model specified for summarization".to_string());
        };

        // Create a conversation string from messages
        let mut conversation_text = String::new();
        for message in messages {
            match message {
                ChatMessage::User { content, .. } => {
                    if let ChatMessageContent::Text(text) = content {
                        conversation_text.push_str(&format!("User: {}\n", text));
                    }
                }
                ChatMessage::Assistant { content: Some(content), .. } => {
                    if let ChatMessageContent::Text(text) = content {
                        conversation_text.push_str(&format!("Assistant: {}\n", text));
                    }
                }
                ChatMessage::Tool { content, .. } => {
                    conversation_text.push_str(&format!("Tool: {}\n", content));
                }
                ChatMessage::System { content, .. } => {
                    if let ChatMessageContent::Text(text) = content {
                        conversation_text.push_str(&format!("System: {}\n", text));
                    }
                }
                _ => {} // Skip other message types
            }
        }

        let summary_prompt = get_compression_summary_prompt();

        let summary_request = ChatCompletionParametersBuilder::default()
            .model(model)
            .messages(vec![
                ChatMessage::System {
                    content: ChatMessageContent::Text(summary_prompt.to_string()),
                    name: None,
                },
                ChatMessage::User {
                    content: ChatMessageContent::Text(format!("Original user request: \"{}\"\n\nFull conversation:\n{}", first_user_message, conversation_text)),
                    name: None,
                },
            ])
            .temperature(0.1)
            .build()
            .map_err(|e| format!("Failed to build summary request: {}", e))?;

        match llm_client.chat(summary_request).await {
            Ok(response) => {
                // Safely extract token usage from the summary generation
                let summary_tokens = if let Some(usage) = &response.usage {
                    // Use completion_tokens if available, otherwise fall back to 0
                    usage.completion_tokens.unwrap_or(0)
                } else {
                    // No usage information available - return error instead of crashing
                    warn!(target: "context_compression", "No usage information available in LLM response");
                    return Err("No token usage information available from LLM".to_string());
                };

                // Safely extract the summary content
                if let Some(choice) = response.choices.first() {
                    if let ChatMessage::Assistant { content: Some(content), .. } = &choice.message {
                        if let ChatMessageContent::Text(summary) = content {
                            // Only proceed if we have both valid summary and token count
                            if !summary.trim().is_empty() && summary_tokens > 0 {
                                debug!(target: "context_compression",
                                    summary_length = summary.len(),
                                    summary_tokens = summary_tokens,
                                    "Successfully generated AI summary with token count"
                                );
                                return Ok((summary.clone(), summary_tokens));
                            } else {
                                warn!(target: "context_compression", "Received empty summary or zero tokens");
                                return Err("Received empty summary or invalid token count".to_string());
                            }
                        }
                    }
                }
                Err("No valid summary content found in LLM response".to_string())
            }
            Err(e) => {
                warn!(target: "context_compression", error = ?e, "Failed to generate summary, falling back to simple compression");
                Err(format!("LLM summarization failed: {}", e))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compression_threshold() {
        let compressor = ContextCompressor::new(1000);
        assert!(!compressor.should_compress());

        let mut compressor = ContextCompressor::new(1000);
        compressor.update_token_count(800, 0);
        assert!(compressor.should_compress());
    }

    #[tokio::test]
    async fn test_message_compression() {
        let mut compressor = ContextCompressor::new(1000);
        compressor.current_tokens = 850; // Above 80% threshold

        let messages = vec![
            ChatMessage::System {
                content: ChatMessageContent::Text("System prompt".to_string()),
                name: None,
            },
            ChatMessage::User {
                content: ChatMessageContent::Text("Old message 1".to_string()),
                name: None,
            },
            ChatMessage::Assistant {
                content: Some(ChatMessageContent::Text("Old response 1".to_string())),
                reasoning_content: None,
                tool_calls: None,
                refusal: None,
                name: None,
                audio: None,
            },
            ChatMessage::User {
                content: ChatMessageContent::Text("Recent message".to_string()),
                name: None,
            },
            ChatMessage::Assistant {
                content: Some(ChatMessageContent::Text("Recent response".to_string())),
                reasoning_content: None,
                tool_calls: None,
                refusal: None,
                name: None,
                audio: None,
            },
        ];

        let (compressed, _info) = compressor.compress_messages(messages).await;

        // Should contain: system message, compression notice, recent messages
        assert!(compressed.len() >= 4);

        // First message should be system
        assert!(matches!(compressed[0], ChatMessage::System { .. }));

        // Should contain compression notice
        let has_compression_notice = compressed.iter().any(|msg| {
            if let ChatMessage::System { content, .. } = msg {
                if let ChatMessageContent::Text(text) = content {
                    text.contains("compressed")
                } else {
                    false
                }
            } else {
                false
            }
        });
        assert!(has_compression_notice);
    }
}