use crate::provider::{LlmProvider, LlmError, LlmStream, ProviderInfo, EnvVar};
use super::api::*;
use async_trait::async_trait;
use reqwest::Client;
use serde_json::json;
use futures::{StreamExt, stream};
use openai_dive::v1::resources::{
    chat::{ChatCompletionParameters, ChatCompletionResponse, ChatCompletionChunkResponse, ChatMessage, DeltaChatMessage, ChatMessageContent, ChatCompletionChoice, ChatCompletionChunkChoice, ToolCall, Function},
    model::ListModelResponse,
    shared::{FinishReason, Usage},
};

pub struct AnthropicProvider {
    api_key: String,
    client: Client,
}

impl AnthropicProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: Client::new(),
        }
    }

    /// Create Anthropic provider from environment variables
    /// Returns None if required environment variables are not set
    pub fn from_env() -> Option<Self> {
        std::env::var("ANTHROPIC_API_KEY").ok().map(|api_key| {
            Self::new(api_key)
        })
    }

    async fn parse_anthropic_stream(
        response: reqwest::Response,
    ) -> Result<LlmStream, LlmError> {
        let stream = response.bytes_stream();
        
        let parsed_stream = stream
            .map(|chunk_result| {
                match chunk_result {
                    Ok(chunk) => {
                        let chunk_str = String::from_utf8_lossy(&chunk);
                        Self::parse_sse_chunk(&chunk_str)
                    }
                    Err(e) => vec![Err(Box::new(e) as LlmError)],
                }
            })
            .flat_map(|results| stream::iter(results));

        Ok(Box::new(Box::pin(parsed_stream)))
    }

    fn parse_sse_chunk(chunk: &str) -> Vec<Result<ChatCompletionChunkResponse, LlmError>> {
        let mut results = Vec::new();
        let mut current_event_type: Option<String> = None;
        let mut current_data = String::new();
        
        for line in chunk.lines() {
            let line = line.trim();
            
            if line.starts_with("event: ") {
                current_event_type = Some(line[7..].to_string());
            } else if line.starts_with("data: ") {
                current_data = line[6..].to_string();
            } else if line.is_empty() && current_event_type.is_some() {
                // End of SSE event
                if let Some(event_type) = current_event_type.take() {
                    match Self::process_anthropic_event(&event_type, &current_data) {
                        Ok(Some(response)) => results.push(Ok(response)),
                        Ok(None) => {}, // Non-content events like ping
                        Err(e) => results.push(Err(e)),
                    }
                    current_data.clear();
                }
            }
        }
        
        results
    }

    fn process_anthropic_event(event_type: &str, data: &str) -> Result<Option<ChatCompletionChunkResponse>, LlmError> {
        match serde_json::from_str::<AnthropicStreamEvent>(data) {
            Ok(event) => Self::convert_anthropic_event_to_stream_response(event),
            Err(e) => {
                // Don't fail on unknown events, just skip them
                if event_type == "ping" {
                    Ok(None)
                } else {
                    Err(Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("Failed to parse Anthropic event {}: {}. Error: {}", event_type, data, e)
                    )) as LlmError)
                }
            }
        }
    }

    fn convert_anthropic_event_to_stream_response(event: AnthropicStreamEvent) -> Result<Option<ChatCompletionChunkResponse>, LlmError> {
        match event {
            AnthropicStreamEvent::ContentBlockDelta { delta, .. } => {
                let content = match delta {
                    AnthropicDelta::TextDelta { text } => Some(ChatMessageContent::Text(text)),
                    AnthropicDelta::ThinkingDelta { thinking: _ } => return Ok(None), // Skip thinking content
                    AnthropicDelta::InputJsonDelta { partial_json } => Some(ChatMessageContent::Text(partial_json)),
                };

                Ok(Some(ChatCompletionChunkResponse {
                    id: Some(format!("anthropic-{}", uuid::Uuid::new_v4())),
                    object: "chat.completion.chunk".to_string(),
                    created: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs() as u32,
                    model: "claude".to_string(),
                    choices: vec![ChatCompletionChunkChoice {
                        index: Some(0),
                        delta: DeltaChatMessage::Assistant {
                            content,
                            reasoning: None,
                            reasoning_content: None,
                            refusal: None,
                            name: None,
                            tool_calls: None,
                        },
                        finish_reason: None,
                        logprobs: None,
                    }],
                    usage: None,
                    system_fingerprint: None,
                }))
            }
            AnthropicStreamEvent::MessageDelta { delta, .. } => {
                let finish_reason = delta.stop_reason.map(|_| FinishReason::StopSequenceReached);

                Ok(Some(ChatCompletionChunkResponse {
                    id: Some(format!("anthropic-{}", uuid::Uuid::new_v4())),
                    object: "chat.completion.chunk".to_string(),
                    created: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs() as u32,
                    model: "claude".to_string(),
                    choices: vec![ChatCompletionChunkChoice {
                        index: Some(0),
                        delta: DeltaChatMessage::Assistant {
                            content: None,
                            reasoning: None,
                            reasoning_content: None,
                            refusal: None,
                            name: None,
                            tool_calls: None,
                        },
                        finish_reason,
                        logprobs: None,
                    }],
                    usage: None,
                    system_fingerprint: None,
                }))
            }
            AnthropicStreamEvent::MessageStop => {
                Ok(Some(ChatCompletionChunkResponse {
                    id: Some(format!("anthropic-{}", uuid::Uuid::new_v4())),
                    object: "chat.completion.chunk".to_string(),
                    created: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs() as u32,
                    model: "claude".to_string(),
                    choices: vec![ChatCompletionChunkChoice {
                        index: Some(0),
                        delta: DeltaChatMessage::Assistant {
                            content: None,
                            reasoning: None,
                            reasoning_content: None,
                            refusal: None,
                            name: None,
                            tool_calls: None,
                        },
                        finish_reason: Some(FinishReason::StopSequenceReached),
                        logprobs: None,
                    }],
                    usage: None,
                    system_fingerprint: None,
                }))
            }
            _ => Ok(None), // Skip other events like message_start, content_block_start, ping, etc.
        }
    }

    pub(crate) fn convert_to_anthropic_format(&self, request: &ChatCompletionParameters) -> serde_json::Value {
        let (system_messages, messages) = self.convert_messages(&request.messages);

        let mut anthropic_request = json!({
            "model": request.model,
            "max_tokens": request.max_tokens.unwrap_or(1000),
            "messages": messages
        });

        if !system_messages.is_empty() {
            anthropic_request["system"] = json!(system_messages.join("\n\n"));
        }

        if let Some(tools) = &request.tools {
            anthropic_request["tools"] = json!(self.convert_tools(tools));
        }

        anthropic_request
    }

    fn convert_messages(&self, messages: &[ChatMessage]) -> (Vec<String>, Vec<serde_json::Value>) {
        let mut system_messages = Vec::new();
        let mut converted_messages = Vec::new();

        for (i, msg) in messages.iter().enumerate() {
            match msg {
                ChatMessage::System { content, .. } => {
                    system_messages.push(self.extract_content_text(content));
                }
                ChatMessage::User { content, .. } => {
                    converted_messages.push(json!({
                        "role": "user",
                        "content": self.extract_content_text(content)
                    }));
                }
                ChatMessage::Assistant { content, tool_calls, .. } => {
                    let is_final = i == messages.len() - 1;
                    if let Some(assistant_content) = self.build_assistant_content(content, tool_calls, is_final) {
                        converted_messages.push(json!({
                            "role": "assistant",
                            "content": assistant_content
                        }));
                    }
                }
                ChatMessage::Developer { content, .. } => {
                    converted_messages.push(json!({
                        "role": "user",
                        "content": self.extract_content_text(content)
                    }));
                }
                ChatMessage::Tool { content, tool_call_id, .. } => {
                    converted_messages.push(json!({
                        "role": "user",
                        "content": [{
                            "type": "tool_result",
                            "tool_use_id": tool_call_id,
                            "content": content.clone()
                        }]
                    }));
                }
            }
        }

        (system_messages, converted_messages)
    }

    fn build_assistant_content(&self, content: &Option<ChatMessageContent>, tool_calls: &Option<Vec<ToolCall>>, is_final: bool) -> Option<serde_json::Value> {
        match tool_calls {
            Some(calls) => {
                let mut blocks = Vec::new();
                
                // Add text content if present
                if let Some(text_content) = content {
                    let text = self.extract_content_text(text_content);
                    if !text.is_empty() {
                        blocks.push(json!({"type": "text", "text": text}));
                    }
                }
                
                // Add tool_use blocks
                for call in calls {
                    let input = serde_json::from_str(&call.function.arguments).unwrap_or_else(|_| json!({}));
                    blocks.push(json!({
                        "type": "tool_use",
                        "id": call.id,
                        "name": call.function.name,
                        "input": input
                    }));
                }
                
                Some(json!(blocks))
            }
            None => {
                let text = content.as_ref().map(|c| self.extract_content_text(c)).unwrap_or_default();
                
                // Only allow empty content if this is the final assistant message
                if text.is_empty() && !is_final {
                    None // Skip this empty assistant message
                } else {
                    Some(json!(text))
                }
            }
        }
    }

    fn convert_tools(&self, tools: &[openai_dive::v1::resources::chat::ChatCompletionTool]) -> Vec<serde_json::Value> {
        tools.iter().map(|tool| {
            json!({
                "name": tool.function.name,
                "description": tool.function.description.as_ref().unwrap_or(&tool.function.name),
                "input_schema": tool.function.parameters
            })
        }).collect()
    }

    fn extract_content_text(&self, content: &ChatMessageContent) -> String {
        match content {
            ChatMessageContent::Text(text) => text.clone(),
            ChatMessageContent::ContentPart(parts) => {
                parts.iter().filter_map(|part| {
                    match part {
                        openai_dive::v1::resources::chat::ChatMessageContentPart::Text(text_part) => {
                            Some(text_part.text.clone())
                        }
                        _ => None, // Skip images, audio, etc.
                    }
                }).collect::<Vec<_>>().join(" ")
            }
            ChatMessageContent::None => String::new(),
        }
    }

    fn convert_from_anthropic_format(&self, response: serde_json::Value) -> Result<ChatCompletionResponse, LlmError> {
        let mut text_content = Vec::new();
        let mut tool_calls = Vec::new();
        
        // Parse content array
        if let Some(content_array) = response["content"].as_array() {
            for content_block in content_array {
                match content_block["type"].as_str() {
                    Some("text") => {
                        if let Some(text) = content_block["text"].as_str() {
                            text_content.push(text.to_string());
                        }
                    }
                    Some("tool_use") => {
                        if let (Some(id), Some(name), Some(input)) = (
                            content_block["id"].as_str(),
                            content_block["name"].as_str(),
                            content_block["input"].as_object()
                        ) {
                            tool_calls.push(ToolCall {
                                id: id.to_string(),
                                r#type: "function".to_string(),
                                function: Function {
                                    name: name.to_string(),
                                    arguments: serde_json::to_string(input).unwrap_or_default(),
                                }
                            });
                        }
                    }
                    _ => {} // Skip other content types
                }
            }
        }
        
        // Combine text content
        let combined_text = text_content.join(" ").trim().to_string();
        let content = if combined_text.is_empty() { 
            None 
        } else { 
            Some(ChatMessageContent::Text(combined_text))
        };
        
        // Convert tool_calls to Option
        let tool_calls_option = if tool_calls.is_empty() { None } else { Some(tool_calls) };
        
        Ok(ChatCompletionResponse {
            id: Some(response["id"].as_str().unwrap_or("").to_string()),
            object: "chat.completion".to_string(),
            created: 0,
            model: response["model"].as_str().unwrap_or("").to_string(),
            choices: vec![ChatCompletionChoice {
                index: 0,
                message: ChatMessage::Assistant {
                    content,
                    reasoning: None,
                    reasoning_content: None,
                    refusal: None,
                    name: None,
                    audio: None,
                    tool_calls: tool_calls_option,
                },
                finish_reason: Some(FinishReason::StopSequenceReached),
                logprobs: None,
            }],
            usage: Some(Usage {
                prompt_tokens: Some(response["usage"]["input_tokens"].as_u64().unwrap_or(0) as u32),
                completion_tokens: Some(response["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32),
                total_tokens: response["usage"]["input_tokens"].as_u64().unwrap_or(0) as u32 + response["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32,
                prompt_tokens_details: None,
                completion_tokens_details: None,
            }),
            service_tier: None,
            system_fingerprint: None,
        })
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    async fn models(&self) -> Result<ListModelResponse, LlmError> {
        // Anthropic doesn't have a models endpoint, so we return a hardcoded list
        use openai_dive::v1::resources::model::Model;
        
        let models = vec![
            Model {
                id: "claude-3-5-sonnet-20241022".to_string(),
                object: "model".to_string(),
                created: Some(1640995200), // Dummy timestamp
                owned_by: "anthropic".to_string(),
            },
            Model {
                id: "claude-3-5-haiku-20241022".to_string(),
                object: "model".to_string(),
                created: Some(1640995200),
                owned_by: "anthropic".to_string(),
            },
            Model {
                id: "claude-3-opus-20240229".to_string(),
                object: "model".to_string(),
                created: Some(1640995200),
                owned_by: "anthropic".to_string(),
            },
            Model {
                id: "claude-3-sonnet-20240229".to_string(),
                object: "model".to_string(),
                created: Some(1640995200),
                owned_by: "anthropic".to_string(),
            },
            Model {
                id: "claude-3-haiku-20240307".to_string(),
                object: "model".to_string(),
                created: Some(1640995200),
                owned_by: "anthropic".to_string(),
            },
        ];

        Ok(ListModelResponse {
            object: "list".to_string(),
            data: models,
        })
    }

    async fn chat(&self, request: ChatCompletionParameters) -> Result<ChatCompletionResponse, LlmError> {
        let anthropic_request = self.convert_to_anthropic_format(&request);
        
        let response = self.client
            .post(&format!("{}/messages", ANTHROPIC_API_BASE))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&anthropic_request)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(format!("Anthropic API error: {}", error_text).into());
        }

        let anthropic_response: serde_json::Value = response.json().await?;
        self.convert_from_anthropic_format(anthropic_response)
    }

    async fn chat_stream(&self, request: ChatCompletionParameters) -> Result<LlmStream, LlmError> {
        let mut anthropic_request = self.convert_to_anthropic_format(&request);
        // Add streaming parameter
        anthropic_request["stream"] = json!(true);
        
        let response = self.client
            .post(&format!("{}/messages", ANTHROPIC_API_BASE))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&anthropic_request)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(format!("Anthropic API streaming error: {}", error_text).into());
        }

        Self::parse_anthropic_stream(response).await
    }

    fn supports_functions(&self, model: String) -> bool {
        true // Anthropic supports function calling via tool use
    }

    fn supports_structured_output(&self, model: String) -> bool {
        false
    }

    fn name(&self) -> &'static str {
        "anthropic"
    }
    
    fn info() -> ProviderInfo {
        ProviderInfo {
            name: "anthropic",
            display_name: "Anthropic (Claude 3.5 Sonnet, Claude 3 Opus)",
            env_vars: vec![
                EnvVar::required("ANTHROPIC_API_KEY", "Anthropic API key"),
            ],
        }
    }
    
}