use crate::tool::ToolBox;
use crate::ToolCallMethod;

// llm/client.rs
use super::provider::{LlmError, LlmProvider, LlmStream, ProviderInfo};
use super::providers::{
    anthropic::AnthropicProvider, mistral::MistralProvider, ollama::OllamaProvider,
    openai::OpenAIProvider, openai_compatible::OpenAICompatibleProvider,
    openrouter::OpenRouterProvider, ovhcloud::OvhCloudProvider,
};
use openai_dive::v1::resources::chat::ChatCompletionParametersBuilder;
use openai_dive::v1::resources::{
    chat::{ChatCompletionParameters, ChatCompletionResponse, ChatMessage, ChatMessageContent},
    model::ListModelResponse,
};
use regex::Regex;
use std::sync::LazyLock;
use tracing::warn;

static THINK_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<think>(.*?)</think>").unwrap());

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

    /// Ollama with an optional API key, sent as a Bearer token.
    pub fn ollama_with_key(base_url: String, api_key: Option<String>) -> Self {
        Self {
            provider: Box::new(OllamaProvider::with_api_key(Some(base_url), api_key)),
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

    /// Read a process environment variable.
    ///
    /// Tests can shadow the process environment for the current thread, so the
    /// fallback can be exercised without touching the real, process-wide
    /// variables that other tests read.
    fn process_env(key: &str) -> Option<String> {
        #[cfg(test)]
        if let Some(fake) = test_env::get() {
            return fake.get(key).cloned();
        }
        std::env::var(key).ok()
    }

    /// Helper function to get a value from config or fall back to environment variable
    fn get_or_env(
        env_values: &std::collections::HashMap<String, String>,
        key: &str,
    ) -> Option<String> {
        env_values
            .get(key)
            .cloned()
            .or_else(|| Self::process_env(key))
    }

    /// Create a provider dynamically based on name and environment values
    /// Falls back to actual environment variables if not found in config
    pub fn create_provider(
        provider_name: &str,
        env_values: &std::collections::HashMap<String, String>,
    ) -> Result<Self, LlmError> {
        ProviderSettings::resolve(provider_name, SettingsSource::ConfigOrEnv, &|key| {
            Self::get_or_env(env_values, key)
        })
        .map(ProviderSettings::into_client)
    }

    /// Create a provider from `env_values` alone.
    ///
    /// Unlike [`LlmClient::create_provider`], a setting missing from
    /// `env_values` is never looked up in the process environment. Use this for
    /// any client other than the session's primary one: the primary provider's
    /// key and base URL are exported to the process environment, and the
    /// fallback would hand them to a client that talks to another endpoint.
    ///
    /// Blank values count as missing. Two more rules apply, because such a
    /// client must only talk to an endpoint its own entry stands for:
    /// anonymous OVHcloud access is not accepted (a keyless `ovhcloud` entry
    /// is an error), and an `ollama` entry must carry its base URL instead of
    /// getting the built-in localhost default.
    ///
    /// The returned client is not tied to a model. Do not ask it for one with
    /// [`LlmClient::default_model`], which answers with the primary model:
    /// always send the model name the caller was configured with.
    ///
    /// No network I/O is performed.
    pub fn create_provider_isolated(
        provider_name: &str,
        env_values: &std::collections::HashMap<String, String>,
    ) -> Result<Self, LlmError> {
        Self::isolated_settings(provider_name, env_values).map(|settings| settings.0.into_client())
    }

    /// The settings [`LlmClient::create_provider_isolated`] would build a
    /// client from, or the reason it cannot build one.
    ///
    /// Two sets of `env_values` that yield equal settings yield equivalent
    /// clients, whatever else they contain. Nothing is built and no network
    /// I/O is performed.
    pub fn isolated_settings(
        provider_name: &str,
        env_values: &std::collections::HashMap<String, String>,
    ) -> Result<IsolatedSettings, LlmError> {
        ProviderSettings::resolve(provider_name, SettingsSource::ConfigOnly, &|key| {
            env_values
                .get(key)
                .filter(|value| !value.trim().is_empty())
                .cloned()
        })
        .map(IsolatedSettings)
    }
}

/// Everything a client for one provider is built from, once each setting has
/// been looked up. Keeping this apart from the client itself makes the
/// outcome of the lookup something that can be compared and tested.
///
/// It holds credentials, so it only has a `Debug` form in test builds.
#[derive(Clone, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
enum ProviderSettings {
    OpenAi {
        api_key: String,
    },
    Anthropic {
        api_key: String,
    },
    Ollama {
        base_url: String,
        api_key: Option<String>,
    },
    Mistral {
        api_key: String,
    },
    OvhCloud {
        api_key: String,
        base_url: Option<String>,
    },
    OpenRouter {
        api_key: String,
    },
    OpenAiCompatible {
        api_key: String,
        base_url: String,
    },
}

impl ProviderSettings {
    fn resolve(
        provider_name: &str,
        source: SettingsSource,
        lookup: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Self, LlmError> {
        let required = |key: &str| -> Result<String, LlmError> {
            lookup(key).ok_or_else(|| format!("{} not found in {}", key, source.describe()).into())
        };

        match provider_name {
            "openai" => Ok(Self::OpenAi {
                api_key: required("OPENAI_API_KEY")?,
            }),
            "anthropic" => Ok(Self::Anthropic {
                api_key: required("ANTHROPIC_API_KEY")?,
            }),
            "ollama" => {
                let base_url = match source {
                    SettingsSource::ConfigOrEnv => lookup("OLLAMA_BASE_URL")
                        .unwrap_or_else(|| "http://localhost:11434/v1".to_string()),
                    // The entry has to say where it points: the built-in
                    // default is not an endpoint its owner chose for it.
                    SettingsSource::ConfigOnly => required("OLLAMA_BASE_URL")?,
                };
                Ok(Self::Ollama {
                    base_url,
                    api_key: lookup("OLLAMA_API_KEY"),
                })
            }
            "mistral" => Ok(Self::Mistral {
                api_key: required("MISTRAL_API_KEY")?,
            }),
            "ovhcloud" => {
                let api_key = match source {
                    // The primary model may run on OVHcloud's anonymous tier.
                    SettingsSource::ConfigOrEnv => lookup("OVH_API_KEY").unwrap_or_default(),
                    SettingsSource::ConfigOnly => required("OVH_API_KEY")?,
                };
                Ok(Self::OvhCloud {
                    api_key,
                    base_url: lookup("OVH_BASE_URL"),
                })
            }
            "openrouter" => Ok(Self::OpenRouter {
                api_key: required("OPENROUTER_API_KEY")?,
            }),
            "openai_compatible" => Ok(Self::OpenAiCompatible {
                api_key: required("OPENAI_COMPATIBLE_API_KEY")?,
                base_url: required("OPENAI_COMPATIBLE_BASE_URL")?,
            }),
            _ => Err(format!("Unknown provider: {}", provider_name).into()),
        }
    }

    fn into_client(self) -> LlmClient {
        match self {
            Self::OpenAi { api_key } => LlmClient::openai(api_key),
            Self::Anthropic { api_key } => LlmClient::anthropic(api_key),
            Self::Ollama { base_url, api_key } => LlmClient::ollama_with_key(base_url, api_key),
            Self::Mistral { api_key } => LlmClient::mistral(api_key),
            Self::OvhCloud { api_key, base_url } => LlmClient::ovhcloud(api_key, base_url),
            Self::OpenRouter { api_key } => LlmClient::openrouter(api_key),
            Self::OpenAiCompatible { api_key, base_url } => {
                LlmClient::compatible(api_key, base_url)
            }
        }
    }

    fn provider_name(&self) -> &'static str {
        match self {
            Self::OpenAi { .. } => "openai",
            Self::Anthropic { .. } => "anthropic",
            Self::Ollama { .. } => "ollama",
            Self::Mistral { .. } => "mistral",
            Self::OvhCloud { .. } => "ovhcloud",
            Self::OpenRouter { .. } => "openrouter",
            Self::OpenAiCompatible { .. } => "openai_compatible",
        }
    }
}

/// The settings an isolated client is built from.
///
/// Opaque on purpose. It can be compared, and it prints which provider it is
/// for but none of its values, which include credentials.
#[derive(Clone, PartialEq, Eq)]
pub struct IsolatedSettings(ProviderSettings);

impl std::fmt::Debug for IsolatedSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "IsolatedSettings({})", self.0.provider_name())
    }
}

/// Where [`ProviderSettings::resolve`] may read provider settings from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsSource {
    /// The provider's config entry, then the process environment.
    ConfigOrEnv,
    /// The provider's config entry only.
    ConfigOnly,
}

impl SettingsSource {
    fn describe(self) -> &'static str {
        match self {
            SettingsSource::ConfigOrEnv => "config or environment",
            SettingsSource::ConfigOnly => "the provider's config entry",
        }
    }
}

/// Per-thread stand-in for the process environment, for tests.
#[cfg(test)]
mod test_env {
    use std::cell::RefCell;
    use std::collections::HashMap;

    thread_local! {
        static FAKE_ENV: RefCell<Option<HashMap<String, String>>> = const { RefCell::new(None) };
    }

    pub(super) fn get() -> Option<HashMap<String, String>> {
        FAKE_ENV.with(|fake| fake.borrow().clone())
    }

    /// Run `f` with `vars` as the whole environment seen by this thread.
    pub(super) fn with<T>(vars: &[(&str, &str)], f: impl FnOnce() -> T) -> T {
        let vars = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        FAKE_ENV.with(|fake| *fake.borrow_mut() = Some(vars));
        let result = f();
        FAKE_ENV.with(|fake| *fake.borrow_mut() = None);
        result
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
    /// Default maximum number of retry attempts for transient errors.
    const DEFAULT_MAX_RETRIES: usize = 3;
    /// Base delay in milliseconds for exponential backoff.
    const RETRY_BASE_DELAY_MS: u64 = 1000;
    /// Cap for a server-announced `Retry-After` delay (5 minutes).
    const MAX_RETRY_AFTER_MS: u64 = 300_000;

    /// Returns true if the error is likely transient and worth retrying.
    ///
    /// Client errors that won't fix themselves by retrying (bad request,
    /// authentication, permission, not found) are not retried. Everything else
    /// (rate limits, server errors, network issues) is.
    fn is_retryable_error(error: &LlmError) -> bool {
        use openai_dive::v1::error::APIError;

        // 429/503/529 are transient by nature
        if error
            .downcast_ref::<crate::error::RateLimitedError>()
            .is_some()
        {
            return true;
        }

        if let Some(chat_error) = error.downcast_ref::<crate::chat::ChatError>() {
            match chat_error {
                crate::chat::ChatError::RateLimited(_) => return true,
                crate::chat::ChatError::Api(api_error) => {
                    return Self::is_retryable_api_error(api_error)
                }
            }
        }

        match error.downcast_ref::<APIError>() {
            Some(api_error) => Self::is_retryable_api_error(api_error),
            // Network errors, unexpected failures, etc: retry
            None => true,
        }
    }

    fn is_retryable_api_error(error: &openai_dive::v1::error::APIError) -> bool {
        use openai_dive::v1::error::APIError;
        match error {
            APIError::InvalidRequestError(_)
            | APIError::AuthenticationError(_)
            | APIError::PermissionError(_)
            | APIError::NotFoundError(_) => false,
            APIError::UnknownError(status, _) => !matches!(status, 400 | 401 | 403 | 404),
            _ => true,
        }
    }

    /// Extract a `Retry-After` delay announced by the server, if any.
    fn retry_after_from_error(error: &LlmError) -> Option<std::time::Duration> {
        if let Some(rate_limited) = error.downcast_ref::<crate::error::RateLimitedError>() {
            return rate_limited.retry_after;
        }
        if let Some(chat_error) = error.downcast_ref::<crate::chat::ChatError>() {
            return chat_error.retry_after();
        }
        None
    }

    /// Compute the delay before the next retry attempt.
    ///
    /// Honors the server-announced `Retry-After` delay (capped at
    /// [`MAX_RETRY_AFTER_MS`]) when present, otherwise falls back to
    /// exponential backoff.
    fn retry_delay_ms(error: &LlmError, attempt: usize) -> u64 {
        if let Some(retry_after) = Self::retry_after_from_error(error) {
            return retry_after
                .as_millis()
                .min(Self::MAX_RETRY_AFTER_MS as u128) as u64;
        }
        Self::RETRY_BASE_DELAY_MS * (1 << attempt)
    }

    pub async fn chat(
        &self,
        request: ChatCompletionParameters,
    ) -> Result<ChatCompletionResponse, LlmError> {
        let request = request.fix_mistral_alternating();

        let max_retries = Self::max_retries_from_env();
        let mut last_error: Option<LlmError> = None;

        for attempt in 0..=max_retries {
            match self.provider.chat(request.clone()).await {
                Ok(response) => return Ok(response.extract_think_content()),
                Err(error) => {
                    crate::logging::log_llm_error(&request, &error, self.provider_name());

                    if attempt < max_retries && Self::is_retryable_error(&error) {
                        let delay_ms = Self::retry_delay_ms(&error, attempt);
                        warn!(
                            target: "shai_llm::client",
                            "LLM request failed (attempt {}/{}), retrying in {}ms: {}",
                            attempt + 1,
                            max_retries,
                            delay_ms,
                            error
                        );
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                        last_error = Some(error);
                        continue;
                    }

                    return Err(error);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| "unknown error".into()))
    }

    pub async fn chat_stream(
        &self,
        request: ChatCompletionParameters,
    ) -> Result<LlmStream, LlmError> {
        let request = request.fix_mistral_alternating();

        let max_retries = Self::max_retries_from_env();
        let mut last_error: Option<LlmError> = None;

        for attempt in 0..=max_retries {
            match self.provider.chat_stream(request.clone()).await {
                Ok(stream) => return Ok(stream),
                Err(error) => {
                    if attempt < max_retries && Self::is_retryable_error(&error) {
                        let delay_ms = Self::retry_delay_ms(&error, attempt);
                        warn!(
                            target: "shai_llm::client",
                            "LLM stream request failed (attempt {}/{}), retrying in {}ms: {}",
                            attempt + 1,
                            max_retries,
                            delay_ms,
                            error
                        );
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                        last_error = Some(error);
                        continue;
                    }

                    return Err(error);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| "unknown error".into()))
    }

    /// Read the max retries override from the `SHAI_LLM_MAX_RETRIES` env var.
    /// Falls back to [`DEFAULT_MAX_RETRIES`].
    fn max_retries_from_env() -> usize {
        std::env::var("SHAI_LLM_MAX_RETRIES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(Self::DEFAULT_MAX_RETRIES)
    }
}

pub trait ExtractThinkContent {
    /// Extract <think> content from assistant messages and move it to reasoning_content
    fn extract_think_content(self) -> ChatCompletionResponse;
}

impl ExtractThinkContent for ChatCompletionResponse {
    fn extract_think_content(mut self) -> ChatCompletionResponse {
        for choice in &mut self.choices {
            if let ChatMessage::Assistant {
                reasoning,
                reasoning_content,
                content,
                ..
            } = &mut choice.message
            {
                // Providers are split on the field name: some send `reasoning`, some
                // `reasoning_content`. Normalize onto reasoning_content, which is what
                // the rest of shai reads.
                if reasoning_content.is_none() {
                    if let Some(text) = reasoning.take().filter(|t| !t.trim().is_empty()) {
                        *reasoning_content = Some(text);
                    }
                }

                if let Some(ChatMessageContent::Text(content_text)) = content {
                    if let Some(reasoning) = THINK_REGEX
                        .captures(content_text)
                        .map(|c| c.get(1).unwrap().as_str().trim())
                    {
                        *reasoning_content = Some(reasoning.to_string());
                        let cleaned = THINK_REGEX.replace_all(content_text, "").trim().to_string();
                        *content = if cleaned.is_empty() {
                            None
                        } else {
                            Some(ChatMessageContent::Text(cleaned))
                        };
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
        if !self.model.to_lowercase().contains("mistral") {
            return self;
        }

        let mut res = self.clone();
        let (mut i, mut pos) = (0, 0);
        while i < res.messages.len() {
            match &res.messages[i] {
                ChatMessage::User { .. } => {
                    if pos % 2 != 0 {
                        res.messages.insert(
                            i,
                            ChatMessage::Assistant {
                                content: Some(ChatMessageContent::Text(
                                    "I understand.".to_string(),
                                )),
                                reasoning: None,
                                reasoning_content: None,
                                tool_calls: None,
                                refusal: None,
                                name: None,
                                audio: None,
                            },
                        );
                    }
                    pos += 1;
                }
                ChatMessage::Assistant { tool_calls, .. }
                    if tool_calls.as_ref().is_none_or(|calls| calls.is_empty()) =>
                {
                    if pos % 2 == 0 {
                        res.messages.insert(
                            i,
                            ChatMessage::User {
                                content: ChatMessageContent::Text("Go ahead.".to_string()),
                                name: None,
                            },
                        );
                    }
                    pos += 1;
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
    use crate::client::ExtractThinkContent;
    use openai_dive::v1::resources::chat::{
        ChatCompletionFunction, ChatCompletionResponse, ChatCompletionTool,
        ChatCompletionToolChoice, ChatCompletionToolType, ChatMessage, ChatMessageContent,
        ImageUrlDetail,
    };

    /// 1.4.x switched `rename_all` from "lowercase" to "snake_case" on these enums.
    /// Every variant we rely on is a single word, so the wire format must be unchanged.
    #[test]
    fn role_tags_and_enums_serialize_unchanged() {
        let cases = vec![
            (
                ChatMessage::System {
                    content: ChatMessageContent::Text("s".into()),
                    name: None,
                },
                "system",
            ),
            (
                ChatMessage::User {
                    content: ChatMessageContent::Text("u".into()),
                    name: None,
                },
                "user",
            ),
            (
                ChatMessage::Developer {
                    content: ChatMessageContent::Text("d".into()),
                    name: None,
                },
                "developer",
            ),
            (
                ChatMessage::Tool {
                    content: ChatMessageContent::Text("t".into()),
                    tool_call_id: "1".into(),
                },
                "tool",
            ),
            (
                ChatMessage::Assistant {
                    content: Some(ChatMessageContent::Text("a".into())),
                    reasoning: None,
                    reasoning_content: None,
                    refusal: None,
                    name: None,
                    audio: None,
                    tool_calls: None,
                },
                "assistant",
            ),
        ];

        for (msg, expected_role) in cases {
            let json = serde_json::to_value(&msg).unwrap();
            assert_eq!(
                json["role"], expected_role,
                "role tag changed for {expected_role}"
            );
        }

        assert_eq!(
            serde_json::to_value(ChatCompletionToolType::Function).unwrap(),
            "function"
        );
        assert_eq!(
            serde_json::to_value(ChatCompletionToolChoice::Auto).unwrap(),
            "auto"
        );
        assert_eq!(
            serde_json::to_value(ChatCompletionToolChoice::None).unwrap(),
            "none"
        );
        assert_eq!(
            serde_json::to_value(ChatCompletionToolChoice::Required).unwrap(),
            "required"
        );
        assert_eq!(serde_json::to_value(ImageUrlDetail::Auto).unwrap(), "auto");
    }

    /// An assistant message must not serialize `reasoning` / `reasoning_content` when unset.
    #[test]
    fn unset_reasoning_is_not_serialized() {
        let msg = ChatMessage::Assistant {
            content: Some(ChatMessageContent::Text("hello".into())),
            reasoning: None,
            reasoning_content: None,
            refusal: None,
            name: None,
            audio: None,
            tool_calls: None,
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

        let ChatMessage::Assistant {
            reasoning_content,
            content,
            ..
        } = &response.choices[0].message
        else {
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

        let ChatMessage::Assistant {
            reasoning_content,
            content,
            ..
        } = &response.choices[0].message
        else {
            panic!("expected an assistant message");
        };
        assert_eq!(reasoning_content.as_deref(), Some("pondering"));
        assert!(matches!(content, Some(ChatMessageContent::Text(t)) if t == "final"));
    }
}

#[cfg(test)]
mod retry_logic {
    use super::LlmClient;
    use crate::chat::ChatError;
    use crate::error::RateLimitedError;
    use crate::provider::LlmError;
    use openai_dive::v1::error::APIError;
    use std::time::Duration;

    fn boxed<E: std::error::Error + Send + Sync + 'static>(e: E) -> LlmError {
        Box::new(e)
    }

    #[test]
    fn retry_delay_honors_retry_after() {
        let err = boxed(RateLimitedError::new(
            429,
            Some(Duration::from_secs(7)),
            "slow down".into(),
        ));
        assert_eq!(LlmClient::retry_delay_ms(&err, 0), 7_000);
        // Retry-After wins over the exponential backoff for any attempt
        assert_eq!(LlmClient::retry_delay_ms(&err, 3), 7_000);
    }

    #[test]
    fn retry_delay_caps_large_retry_after() {
        let err = boxed(RateLimitedError::new(
            429,
            Some(Duration::from_secs(3600)),
            "slow down".into(),
        ));
        assert_eq!(
            LlmClient::retry_delay_ms(&err, 0),
            LlmClient::MAX_RETRY_AFTER_MS
        );
    }

    #[test]
    fn retry_delay_falls_back_to_exponential_backoff() {
        let err = boxed(APIError::ServerError("boom".into()));
        assert_eq!(LlmClient::retry_delay_ms(&err, 0), 1_000);
        assert_eq!(LlmClient::retry_delay_ms(&err, 1), 2_000);
        assert_eq!(LlmClient::retry_delay_ms(&err, 2), 4_000);
    }

    #[test]
    fn retry_after_from_error_reads_both_wrappers() {
        let direct = boxed(RateLimitedError::new(
            503,
            Some(Duration::from_secs(3)),
            "unavailable".into(),
        ));
        assert_eq!(
            LlmClient::retry_after_from_error(&direct),
            Some(Duration::from_secs(3))
        );

        let wrapped = boxed(ChatError::RateLimited(RateLimitedError::new(
            429,
            Some(Duration::from_secs(5)),
            "limited".into(),
        )));
        assert_eq!(
            LlmClient::retry_after_from_error(&wrapped),
            Some(Duration::from_secs(5))
        );

        let none = boxed(APIError::RateLimitError("no header".into()));
        assert_eq!(LlmClient::retry_after_from_error(&none), None);
    }

    #[test]
    fn client_errors_are_not_retried() {
        for err in [
            boxed(APIError::InvalidRequestError("bad".into())),
            boxed(APIError::AuthenticationError("auth".into())),
            boxed(APIError::PermissionError("denied".into())),
            boxed(APIError::NotFoundError("missing".into())),
            boxed(APIError::UnknownError(404, "missing".into())),
        ] {
            assert!(
                !LlmClient::is_retryable_error(&err),
                "{:?} should not be retryable",
                err
            );
        }
    }

    #[test]
    fn transient_errors_are_retried() {
        for err in [
            boxed(APIError::RateLimitError("limit".into())),
            boxed(APIError::ServerError("boom".into())),
            boxed(APIError::UnknownError(500, "boom".into())),
            boxed(RateLimitedError::new(429, None, "limit".into())),
            boxed(ChatError::RateLimited(RateLimitedError::new(
                503,
                None,
                "unavailable".into(),
            ))),
        ] {
            assert!(
                LlmClient::is_retryable_error(&err),
                "{:?} should be retryable",
                err
            );
        }
    }

    #[test]
    fn chat_error_api_variant_delegates_to_api_rules() {
        let auth = boxed(ChatError::Api(APIError::AuthenticationError("auth".into())));
        assert!(!LlmClient::is_retryable_error(&auth));
        let server = boxed(ChatError::Api(APIError::ServerError("boom".into())));
        assert!(LlmClient::is_retryable_error(&server));
    }
}

#[cfg(test)]
mod provider_settings_tests {
    use super::{test_env, LlmClient, ProviderSettings};
    use std::collections::HashMap;

    /// Every key a provider reads, each with a value, as the process
    /// environment would hold them once a primary provider has exported them.
    const FULL_ENV: &[(&str, &str)] = &[
        ("OPENAI_API_KEY", "env-openai"),
        ("ANTHROPIC_API_KEY", "env-anthropic"),
        ("MISTRAL_API_KEY", "env-mistral"),
        ("OPENROUTER_API_KEY", "env-openrouter"),
        ("OPENAI_COMPATIBLE_API_KEY", "env-compatible"),
        ("OPENAI_COMPATIBLE_BASE_URL", "http://env.invalid/v1"),
        ("OVH_API_KEY", "env-ovh"),
        ("OVH_BASE_URL", "http://env.invalid/v1"),
        ("OLLAMA_API_KEY", "env-ollama"),
        ("OLLAMA_BASE_URL", "http://env.invalid/v1"),
    ];

    const ALL_PROVIDERS: &[&str] = &[
        "openai",
        "anthropic",
        "mistral",
        "openrouter",
        "openai_compatible",
        "ovhcloud",
        "ollama",
    ];

    fn vars(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn isolated(provider: &str, entry: &[(&str, &str)]) -> Result<ProviderSettings, String> {
        LlmClient::isolated_settings(provider, &vars(entry))
            .map(|settings| settings.0)
            .map_err(|e| e.to_string())
    }

    #[test]
    fn create_provider_falls_back_to_the_environment() {
        // Baseline for the tests below: the per-thread environment is what the
        // fallback sees, so an isolated client ignoring it is a real result.
        test_env::with(FULL_ENV, || {
            for provider in ALL_PROVIDERS {
                assert!(
                    LlmClient::create_provider(provider, &HashMap::new()).is_ok(),
                    "{provider} should be built from the environment"
                );
            }
        });
    }

    #[test]
    fn isolated_provider_never_completes_an_entry_from_the_environment() {
        test_env::with(FULL_ENV, || {
            for provider in ALL_PROVIDERS {
                let error = isolated(provider, &[])
                    .expect_err("an empty entry must not be completed from the environment");
                assert!(
                    error.contains("not found in the provider's config entry"),
                    "{provider}: {error}"
                );
                assert!(!error.contains("env-"), "{provider}: {error}");
                assert!(LlmClient::create_provider_isolated(provider, &HashMap::new()).is_err());
            }
        });
    }

    #[test]
    fn isolated_settings_are_exactly_what_the_entry_holds() {
        // With every variable present in the environment, the settings must
        // still be the entry's own: required keys, optional keys and base URLs.
        test_env::with(FULL_ENV, || {
            use ProviderSettings::*;
            let key = "entry-key".to_string();
            let url = "http://127.0.0.1:9/v1".to_string();

            assert_eq!(
                isolated("openai", &[("OPENAI_API_KEY", "entry-key")]),
                Ok(OpenAi {
                    api_key: key.clone()
                })
            );
            assert_eq!(
                isolated("anthropic", &[("ANTHROPIC_API_KEY", "entry-key")]),
                Ok(Anthropic {
                    api_key: key.clone()
                })
            );
            assert_eq!(
                isolated("mistral", &[("MISTRAL_API_KEY", "entry-key")]),
                Ok(Mistral {
                    api_key: key.clone()
                })
            );
            assert_eq!(
                isolated("openrouter", &[("OPENROUTER_API_KEY", "entry-key")]),
                Ok(OpenRouter {
                    api_key: key.clone()
                })
            );
            assert_eq!(
                isolated(
                    "openai_compatible",
                    &[
                        ("OPENAI_COMPATIBLE_API_KEY", "entry-key"),
                        ("OPENAI_COMPATIBLE_BASE_URL", "http://127.0.0.1:9/v1"),
                    ]
                ),
                Ok(OpenAiCompatible {
                    api_key: key.clone(),
                    base_url: url.clone()
                })
            );
            // No base URL in the entry: the provider's own endpoint, not the
            // one the environment names.
            assert_eq!(
                isolated("ovhcloud", &[("OVH_API_KEY", "entry-key")]),
                Ok(OvhCloud {
                    api_key: key.clone(),
                    base_url: None
                })
            );
            // No key in the entry: none is sent, not the environment's.
            assert_eq!(
                isolated("ollama", &[("OLLAMA_BASE_URL", "http://127.0.0.1:9/v1")]),
                Ok(Ollama {
                    base_url: url.clone(),
                    api_key: None
                })
            );
            assert_eq!(
                isolated(
                    "ollama",
                    &[
                        ("OLLAMA_BASE_URL", "http://127.0.0.1:9/v1"),
                        ("OLLAMA_API_KEY", "entry-key"),
                    ]
                ),
                Ok(Ollama {
                    base_url: url,
                    api_key: Some(key)
                })
            );
        });
    }

    #[test]
    fn isolated_clients_are_built_for_complete_entries() {
        test_env::with(&[], || {
            let entries: &[(&str, &[(&str, &str)])] = &[
                ("openai", &[("OPENAI_API_KEY", "k")]),
                ("anthropic", &[("ANTHROPIC_API_KEY", "k")]),
                ("mistral", &[("MISTRAL_API_KEY", "k")]),
                ("openrouter", &[("OPENROUTER_API_KEY", "k")]),
                (
                    "openai_compatible",
                    &[
                        ("OPENAI_COMPATIBLE_API_KEY", "k"),
                        ("OPENAI_COMPATIBLE_BASE_URL", "http://127.0.0.1:9/v1"),
                    ],
                ),
                ("ovhcloud", &[("OVH_API_KEY", "k")]),
                ("ollama", &[("OLLAMA_BASE_URL", "http://127.0.0.1:9/v1")]),
            ];
            for (provider, entry) in entries {
                let client = LlmClient::create_provider_isolated(provider, &vars(entry))
                    .unwrap_or_else(|e| panic!("{provider}: {e}"));
                assert_eq!(client.provider_name(), *provider);
            }
        });
    }

    #[test]
    fn isolated_provider_treats_blank_values_as_missing() {
        test_env::with(FULL_ENV, || {
            for blank in ["", "   "] {
                assert!(isolated("openai", &[("OPENAI_API_KEY", blank)]).is_err());
                assert!(isolated("ollama", &[("OLLAMA_BASE_URL", blank)]).is_err());
            }
            // One of two required settings is not enough.
            let error =
                isolated("openai_compatible", &[("OPENAI_COMPATIBLE_API_KEY", "k")]).unwrap_err();
            assert!(error.contains("OPENAI_COMPATIBLE_BASE_URL"), "{error}");
            // A blank optional setting is the same as none.
            assert_eq!(
                isolated(
                    "ollama",
                    &[
                        ("OLLAMA_BASE_URL", "http://127.0.0.1:9/v1"),
                        ("OLLAMA_API_KEY", " ")
                    ]
                ),
                isolated("ollama", &[("OLLAMA_BASE_URL", "http://127.0.0.1:9/v1")])
            );
        });
    }

    #[test]
    fn isolated_provider_refuses_anonymous_ovhcloud() {
        test_env::with(&[], || {
            let entry = vars(&[("OVH_BASE_URL", "http://127.0.0.1:9/v1")]);
            // The primary model may be anonymous...
            assert!(LlmClient::create_provider("ovhcloud", &entry).is_ok());
            // ...a secondary one may not.
            let error = LlmClient::create_provider_isolated("ovhcloud", &entry)
                .unwrap_err()
                .to_string();
            assert!(error.contains("OVH_API_KEY"), "{error}");
        });
    }

    #[test]
    fn isolated_ollama_needs_an_explicit_base_url() {
        // The shell names a remote endpoint; the entries do not name any.
        let env = &[("OLLAMA_BASE_URL", "https://remote.invalid/v1")];
        test_env::with(env, || {
            for entry in [vars(&[]), vars(&[("OLLAMA_API_KEY", "k")])] {
                // The primary path completes the entry from the environment,
                // or from the localhost default...
                assert!(LlmClient::create_provider("ollama", &entry).is_ok());
                // ...the isolated path does neither: nothing in the entry
                // says where the conversation, or the key, should go.
                let error = LlmClient::create_provider_isolated("ollama", &entry)
                    .unwrap_err()
                    .to_string();
                assert!(error.contains("OLLAMA_BASE_URL"), "{error}");
            }
        });
    }

    #[test]
    fn isolated_settings_compare_by_what_the_client_is_built_from() {
        test_env::with(FULL_ENV, || {
            let base = vars(&[
                ("OPENAI_COMPATIBLE_API_KEY", "k"),
                ("OPENAI_COMPATIBLE_BASE_URL", "http://127.0.0.1:9/v1"),
            ]);
            let settings = LlmClient::isolated_settings("openai_compatible", &base).unwrap();

            // Settings the provider does not read, and blank ones, do not count.
            let mut noisy = base.clone();
            noisy.insert("OPENAI_API_KEY".to_string(), "other".to_string());
            noisy.insert("UNRELATED".to_string(), "x".to_string());
            noisy.insert("OLLAMA_API_KEY".to_string(), String::new());
            assert_eq!(
                LlmClient::isolated_settings("openai_compatible", &noisy).unwrap(),
                settings
            );

            // A different key or host does.
            for (name, value) in [
                ("OPENAI_COMPATIBLE_API_KEY", "k2"),
                ("OPENAI_COMPATIBLE_BASE_URL", "http://127.0.0.1:10/v1"),
            ] {
                let mut other = base.clone();
                other.insert(name.to_string(), value.to_string());
                assert_ne!(
                    LlmClient::isolated_settings("openai_compatible", &other).unwrap(),
                    settings
                );
            }

            // The provider is printable, the values are not.
            assert_eq!(
                format!("{settings:?}"),
                "IsolatedSettings(openai_compatible)"
            );
        });
    }

    #[test]
    fn unknown_provider_is_an_error_in_both_modes() {
        test_env::with(FULL_ENV, || {
            assert!(LlmClient::create_provider("nope", &HashMap::new()).is_err());
            assert!(LlmClient::create_provider_isolated("nope", &HashMap::new()).is_err());
            assert!(LlmClient::isolated_settings("nope", &HashMap::new()).is_err());
        });
    }

    /// The leak this constructor exists to prevent: the primary provider's key
    /// sits in the environment, and a second entry for the same provider type
    /// points at another host without a key of its own.
    #[tokio::test]
    async fn isolated_ollama_does_not_send_the_environment_key_to_its_endpoint() {
        let mut server = mockito::Server::new_async().await;
        let entry = vars(&[("OLLAMA_BASE_URL", &format!("{}/v1", server.url()))]);
        let env = &[("OLLAMA_API_KEY", "env-ollama")];

        let leaked = server
            .mock("GET", "/v1/models")
            .match_header("authorization", "Bearer env-ollama")
            .with_status(200)
            .with_body(r#"{"object":"list","data":[{"id":"m"}]}"#)
            .expect(1)
            .create_async()
            .await;

        // The fallback constructor does send it, so the mock can catch a leak.
        let fallback = test_env::with(env, || LlmClient::create_provider("ollama", &entry))
            .expect("fallback client");
        fallback.models().await.expect("models");
        leaked.assert_async().await;
        leaked.remove_async().await;

        let anonymous = server
            .mock("GET", "/v1/models")
            .match_header("authorization", mockito::Matcher::Missing)
            .with_status(200)
            .with_body(r#"{"object":"list","data":[{"id":"m"}]}"#)
            .expect(1)
            .create_async()
            .await;

        let isolated = test_env::with(env, || {
            LlmClient::create_provider_isolated("ollama", &entry)
        })
        .expect("isolated client");
        isolated.models().await.expect("models");
        anonymous.assert_async().await;
    }
}
