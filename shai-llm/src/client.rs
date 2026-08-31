use crate::tool::ToolBox;
use crate::ToolCallMethod;

// llm/client.rs
use super::provider::{LlmProvider, LlmError, LlmStream, ProviderInfo};
use super::providers::{
    openai::OpenAIProvider,
    openai_compatible::OpenAICompatibleProvider,
    openrouter::OpenRouterProvider,
    ovhcloud::OvhCloudProvider,
    anthropic::AnthropicProvider,
    ollama::OllamaProvider,
    mistral::MistralProvider
};
use openai_dive::v1::resources::chat::ChatCompletionParametersBuilder;
use openai_dive::v1::resources::{
    chat::{ChatCompletionParameters, ChatCompletionResponse, ChatMessage, ChatMessageContent},
    model::ListModelResponse,
};
use regex::Regex;

#[derive(Debug)]
pub struct LlmClient {
    provider: Box<dyn LlmProvider>,
}

/// Provider Factory related method
impl LlmClient {
    /// Create an OpenAI provider from environment variables
    /// Returns None if required environment variables are not set
    pub fn from_env_openai() -> Option<Self> {
        OpenAIProvider::from_env().map(|provider| Self {
            provider: Box::new(provider),
        })
    }

    /// Create an Anthropic provider from environment variables
    /// Returns None if required environment variables are not set
    pub fn from_env_anthropic() -> Option<Self> {
        AnthropicProvider::from_env().map(|provider| Self {
            provider: Box::new(provider),
        })
    }

    /// Create an Ollama provider from environment variables
    /// Always returns Some since Ollama has a default base URL
    pub fn from_env_ollama() -> Option<Self> {
        OllamaProvider::from_env().map(|provider| Self {
            provider: Box::new(provider),
        })
    }

    /// Create an OpenRouter provider from environment variables
    /// Returns None if required environment variables are not set
    pub fn from_env_openrouter() -> Option<Self> {
        OpenRouterProvider::from_env().map(|provider| Self {
            provider: Box::new(provider),
        })
    }

    /// Create an OpenAI Compatible provider from environment variables
    /// Returns None if required environment variables are not set
    pub fn from_env_openai_compatible() -> Option<Self> {
        OpenAICompatibleProvider::from_env().map(|provider| Self {
            provider: Box::new(provider),
        })
    }

    /// Create an OVH Cloud provider from environment variables
    /// Returns None if required environment variables are not set
    pub fn from_env_ovhcloud() -> Option<Self> {
        OvhCloudProvider::from_env().map(|provider| Self {
            provider: Box::new(provider),
        })
    }

    /// Create a Mistral provider from environment variables
    /// Returns None if required environment variables are not set
    pub fn from_env_mistral() -> Option<Self> {
        MistralProvider::from_env().map(|provider| Self {
            provider: Box::new(provider),
        })
    }

    pub fn openai(api_key: String) -> Self {
        Self {
            provider: Box::new(OpenAIProvider::new(api_key)),
        }
    }

    pub fn compatible(api_key: String, base_url: String) -> Self {
        Self {
            provider: Box::new(OpenAICompatibleProvider::new(api_key, base_url)),
        }
    }

    pub fn openrouter(api_key: String) -> Self {
        Self {
            provider: Box::new(OpenRouterProvider::new(api_key)),
        }
    }

    pub fn ovhcloud(api_key: String, base_url: Option<String>) -> Self {
        Self {
            provider: Box::new(OvhCloudProvider::new(api_key, base_url)),
        }
    }

    pub fn anthropic(api_key: String) -> Self {
        Self {
            provider: Box::new(AnthropicProvider::new(api_key)),
        }
    }

    pub fn ollama(base_url: String) -> Self {
        Self {
            provider: Box::new(OllamaProvider::new(Some(base_url))),
        }
    }

    pub fn mistral(api_key: String) -> Self {
        Self {
            provider: Box::new(MistralProvider::new(api_key)),
        }
    }


    /// Get all available LLM clients from environment variables
    /// Returns clients in order of preference for testing
    pub fn first_from_env() -> Option<Self> {
        if let Ok(provider) = std::env::var("SHAI_PROVIDER") {
            match provider.as_str() {
                "ovhcloud" => return Self::from_env_ovhcloud(),
                "openai" => return Self::from_env_openai(),
                "mistral" => return Self::from_env_mistral(),
                "anthropic" => return Self::from_env_anthropic(),
                "openrouter" => return Self::from_env_openrouter(),
                "openai_compatible" => return Self::from_env_openai_compatible(),
                "ollama" => return Self::from_env_ollama(),
                _ => {} // Fall through to default behavior
            }
        }
        
        if let Some(client) = Self::from_env_ovhcloud() {
            return Some(client);
        }
        if let Some(client) = Self::from_env_openai() {
            return Some(client);
        }
        if let Some(client) = Self::from_env_mistral() {
            return Some(client);
        }
        if let Some(client) = Self::from_env_anthropic() {
            return Some(client);
        }
        if let Some(client) = Self::from_env_openrouter() {
            return Some(client);
        }
        if let Some(client) = Self::from_env_openai_compatible() {
            return Some(client);
        }
        if let Some(client) = Self::from_env_ollama() {
            return Some(client);
        }
        None
    }

    /// Get information about all available providers
    pub fn list_providers() -> Vec<ProviderInfo> {
        vec![
            OvhCloudProvider::info(),
            MistralProvider::info(),
            OllamaProvider::info(),
            OpenAICompatibleProvider::info(),
            OpenRouterProvider::info(),
            AnthropicProvider::info(),
            OpenAIProvider::info(),
        ]
    }

    /// Helper function to get a value from config or fall back to environment variable
    fn get_or_env(env_values: &std::collections::HashMap<String, String>, key: &str) -> Option<String> {
        env_values.get(key).cloned().or_else(|| {
            std::env::var(key).ok().map(|val| {
                //eprintln!("\x1b[2m[llm] Using {} from environment variable\x1b[0m", key);
                val
            })
        })
    }

    /// Create a provider dynamically based on name and environment values
    /// Falls back to actual environment variables if not found in config
    pub fn create_provider(provider_name: &str, env_values: &std::collections::HashMap<String, String>) -> Result<Self, LlmError> {
        match provider_name {
            "openai" => {
                let api_key = Self::get_or_env(env_values, "OPENAI_API_KEY")
                    .ok_or("OPENAI_API_KEY not found in config or environment")?;
                Ok(Self::openai(api_key))
            },
            "anthropic" => {
                let api_key = Self::get_or_env(env_values, "ANTHROPIC_API_KEY")
                    .ok_or("ANTHROPIC_API_KEY not found in config or environment")?;
                Ok(Self::anthropic(api_key))
            },
            "ollama" => {
                let base_url = Self::get_or_env(env_values, "OLLAMA_BASE_URL")
                    .unwrap_or_else(|| "http://localhost:11434/v1".to_string());
                Ok(Self::ollama(base_url))
            },
            "mistral" => {
                let api_key = Self::get_or_env(env_values, "MISTRAL_API_KEY")
                    .ok_or("MISTRAL_API_KEY not found in config or environment")?;
                Ok(Self::mistral(api_key))
            },
            "ovhcloud" => {
                let api_key = Self::get_or_env(env_values, "OVH_API_KEY")
                    .unwrap_or_else(|| String::new());
                let base_url = Self::get_or_env(env_values, "OVH_BASE_URL");
                Ok(Self::ovhcloud(api_key, base_url))
            },
            "openrouter" => {
                let api_key = Self::get_or_env(env_values, "OPENROUTER_API_KEY")
                    .ok_or("OPENROUTER_API_KEY not found in config or environment")?;
                Ok(Self::openrouter(api_key))
            },
            "openai_compatible" => {
                let api_key = Self::get_or_env(env_values, "OPENAI_COMPATIBLE_API_KEY")
                    .ok_or("OPENAI_COMPATIBLE_API_KEY not found in config or environment")?;
                let base_url = Self::get_or_env(env_values, "OPENAI_COMPATIBLE_BASE_URL")
                    .ok_or("OPENAI_COMPATIBLE_BASE_URL not found in config or environment")?;
                Ok(Self::compatible(api_key, base_url))
            },
            _ => Err(format!("Unknown provider: {}", provider_name).into())
        }
    }
}


/// Provider Delegate
impl LlmClient {
    pub async fn models(&self) -> Result<ListModelResponse, LlmError> {
        self.provider.models().await
    }

    pub async fn default_model(&self) -> Result<String, LlmError> {
        if let Ok(model) = std::env::var("SHAI_MODEL") {
            Ok(model)
        } else {
            self.provider.default_model().await
        }
    }

    pub fn provider_name(&self) -> &'static str {
        self.provider.name()
    }

    /// Get a reference to the underlying provider (for testing)
    pub fn provider(&self) -> &dyn LlmProvider {
        &*self.provider
    }
}

/// Higher level chat client
impl LlmClient {
    pub async fn chat(&self, request: ChatCompletionParameters) -> Result<ChatCompletionResponse, LlmError> {
        let request = request
            .fix_mistral_alternating();

        let response = self.provider
            .chat(request.clone())
            .await
            .inspect_err(|error| {
                crate::logging::log_llm_error(&request, error, self.provider_name());
            })?
            .extract_think_content();

        Ok(response)
    }

    pub async fn chat_stream(&self, request: ChatCompletionParameters) -> Result<LlmStream, LlmError> {
        let request = request
            .fix_mistral_alternating();

        self.provider.chat_stream(request).await
    }


}

pub trait ExtractThinkContent {
    /// Extract <think> content from assistant messages and move it to reasoning_content
    fn extract_think_content(self) -> ChatCompletionResponse;
}

impl ExtractThinkContent for ChatCompletionResponse {
    fn extract_think_content(mut self) -> ChatCompletionResponse {
        for choice in &mut self.choices {
            if let ChatMessage::Assistant { reasoning, reasoning_content, content, .. } = &mut choice.message {
                // Providers are split on the field name: some send `reasoning`, some
                // `reasoning_content`. Normalize onto reasoning_content, which is what
                // the rest of shai reads.
                if reasoning_content.is_none() {
                    if let Some(text) = reasoning.take().filter(|t| !t.trim().is_empty()) {
                        *reasoning_content = Some(text);
                    }
                }

                if let Some(ChatMessageContent::Text(content_text)) = content {
                    let think_regex = Regex::new(r"(?s)<think>(.*?)</think>").unwrap();
                    if let Some(reasoning) = think_regex.captures(content_text).map(|c| c.get(1).unwrap().as_str().trim()) {
                        *reasoning_content = Some(reasoning.to_string());
                        let cleaned = think_regex.replace_all(content_text, "").trim().to_string();
                        *content = if cleaned.is_empty() { None } else { Some(ChatMessageContent::Text(cleaned)) };
                    }
                }
            }
        }
        self
    }
}

pub trait FixMistralAlternating {
    /// Mistral enforces alternating of user/assistant which is problematic in multiturn 
    /// conversation where assistant or toolcall can be cancelled by the user...
    fn fix_mistral_alternating(self) -> ChatCompletionParameters;
}

impl FixMistralAlternating for ChatCompletionParameters {
    fn fix_mistral_alternating(self) -> ChatCompletionParameters {
        if !self.model.to_lowercase().contains("mistral")  {
            return self;
        }

        let mut res = self.clone();
        let (mut i, mut pos) = (0, 0);
        while i < res.messages.len() {
            match &res.messages[i] {
                ChatMessage::User { .. } => {
                    if pos % 2 != 0 {
                        res.messages.insert(i, ChatMessage::Assistant {
                            content: Some(ChatMessageContent::Text("I understand.".to_string())),
                            reasoning: None,
                            reasoning_content: None, tool_calls: None, refusal: None, name: None, audio: None,
                        });
                    }
                    pos += 1;
                }
                ChatMessage::Assistant { tool_calls, .. } => {
                    if tool_calls.as_ref().map_or(true, |calls| calls.is_empty()) {
                        if pos % 2 == 0 {
                            res.messages.insert(i, ChatMessage::User {
                                content: ChatMessageContent::Text("Go ahead.".to_string()),
                                name: None, 
                            });
                        }
                        pos += 1;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        res
    }
}
#[cfg(test)]
mod openai_dive_compat {
    use openai_dive::v1::resources::chat::{
        ChatCompletionResponse, ChatCompletionTool, ChatCompletionToolChoice,
        ChatCompletionToolType, ChatMessage, ChatMessageContent, ChatCompletionFunction,
        ImageUrlDetail,
    };
    use crate::client::ExtractThinkContent;

    /// 1.4.x switched `rename_all` from "lowercase" to "snake_case" on these enums.
    /// Every variant we rely on is a single word, so the wire format must be unchanged.
    #[test]
    fn role_tags_and_enums_serialize_unchanged() {
        let cases = vec![
            (ChatMessage::System { content: ChatMessageContent::Text("s".into()), name: None }, "system"),
            (ChatMessage::User { content: ChatMessageContent::Text("u".into()), name: None }, "user"),
            (ChatMessage::Developer { content: ChatMessageContent::Text("d".into()), name: None }, "developer"),
            (ChatMessage::Tool { content: ChatMessageContent::Text("t".into()), tool_call_id: "1".into() }, "tool"),
            (ChatMessage::Assistant {
                content: Some(ChatMessageContent::Text("a".into())),
                reasoning: None, reasoning_content: None, refusal: None, name: None,
                audio: None, tool_calls: None,
            }, "assistant"),
        ];

        for (msg, expected_role) in cases {
            let json = serde_json::to_value(&msg).unwrap();
            assert_eq!(json["role"], expected_role, "role tag changed for {expected_role}");
        }

        assert_eq!(serde_json::to_value(ChatCompletionToolType::Function).unwrap(), "function");
        assert_eq!(serde_json::to_value(ChatCompletionToolChoice::Auto).unwrap(), "auto");
        assert_eq!(serde_json::to_value(ChatCompletionToolChoice::None).unwrap(), "none");
        assert_eq!(serde_json::to_value(ChatCompletionToolChoice::Required).unwrap(), "required");
        assert_eq!(serde_json::to_value(ImageUrlDetail::Auto).unwrap(), "auto");
    }

    /// An assistant message must not serialize `reasoning` / `reasoning_content` when unset.
    #[test]
    fn unset_reasoning_is_not_serialized() {
        let msg = ChatMessage::Assistant {
            content: Some(ChatMessageContent::Text("hello".into())),
            reasoning: None, reasoning_content: None, refusal: None, name: None,
            audio: None, tool_calls: None,
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert!(json.get("reasoning").is_none());
        assert!(json.get("reasoning_content").is_none());
    }

    /// Tool definitions must still round-trip the shape providers expect.
    #[test]
    fn tool_definition_shape_unchanged() {
        let tool = ChatCompletionTool {
            r#type: ChatCompletionToolType::Function,
            function: ChatCompletionFunction {
                name: "bash".into(),
                description: Some("run a command".into()),
                parameters: serde_json::json!({"type": "object"}),
            },
        };
        let json = serde_json::to_value(&tool).unwrap();
        assert_eq!(json["type"], "function");
        assert_eq!(json["function"]["name"], "bash");
    }

    /// Providers are split on the reasoning field name; both must land on
    /// reasoning_content, which is what the agent and TUI read.
    #[test]
    fn reasoning_field_is_normalized_onto_reasoning_content() {
        let body = r#"{
            "id":"1","object":"chat.completion","created":0,"model":"m",
            "choices":[{"index":0,"message":{
                "role":"assistant","content":"answer","reasoning":"because"
            },"finish_reason":"stop"}]
        }"#;

        let response: ChatCompletionResponse = serde_json::from_str(body).unwrap();
        let response = response.extract_think_content();

        let ChatMessage::Assistant { reasoning_content, content, .. } = &response.choices[0].message else {
            panic!("expected an assistant message");
        };
        assert_eq!(reasoning_content.as_deref(), Some("because"));
        assert!(matches!(content, Some(ChatMessageContent::Text(t)) if t == "answer"));
    }

    /// The pre-existing <think> extraction must keep working, and must win over
    /// a provider-supplied reasoning field.
    #[test]
    fn think_tags_still_extracted() {
        let body = r#"{
            "id":"1","object":"chat.completion","created":0,"model":"m",
            "choices":[{"index":0,"message":{
                "role":"assistant","content":"<think>pondering</think>final"
            },"finish_reason":"stop"}]
        }"#;

        let response: ChatCompletionResponse = serde_json::from_str(body).unwrap();
        let response = response.extract_think_content();

        let ChatMessage::Assistant { reasoning_content, content, .. } = &response.choices[0].message else {
            panic!("expected an assistant message");
        };
        assert_eq!(reasoning_content.as_deref(), Some("pondering"));
        assert!(matches!(content, Some(ChatMessageContent::Text(t)) if t == "final"));
    }
}
