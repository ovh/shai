use async_trait::async_trait;
use futures::Stream;
use openai_dive::v1::endpoints::chat::Chat;
use openai_dive::v1::resources::{
    chat::{ChatCompletionChunkResponse, ChatCompletionParameters, ChatCompletionResponse},
    model::ListModelResponse,
};
use std::error::Error;
use std::fmt::Debug;

pub type LlmError = Box<dyn Error + Send + Sync>;
pub type LlmStream =
    Box<dyn Stream<Item = Result<ChatCompletionChunkResponse, LlmError>> + Send + Unpin>;

#[derive(Debug, Clone)]
pub struct EnvVar {
    pub name: String,
    pub description: String,
    pub required: bool,
}

#[derive(Debug, Clone)]
pub struct ProviderInfo {
    pub name: &'static str,
    pub display_name: &'static str,
    pub env_vars: Vec<EnvVar>,
}

impl EnvVar {
    pub fn required(name: &str, description: &str) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            required: true,
        }
    }

    pub fn optional(name: &str, description: &str) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            required: false,
        }
    }
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn models(&self) -> Result<ListModelResponse, LlmError>;

    async fn default_model(&self) -> Result<String, LlmError> {
        let models = self.models().await?;
        models
            .data
            .first()
            .map(|m| m.id.clone())
            .ok_or_else(|| "no model available".into())
    }

    async fn chat(
        &self,
        request: ChatCompletionParameters,
    ) -> Result<ChatCompletionResponse, LlmError>;

    async fn chat_stream(&self, request: ChatCompletionParameters) -> Result<LlmStream, LlmError>;

    fn supports_functions(&self, model: String) -> bool;

    fn supports_structured_output(&self, model: String) -> bool;

    fn name(&self) -> &'static str;

    /// Returns provider information including environment variables
    fn info() -> ProviderInfo
    where
        Self: Sized;
}

impl Debug for dyn LlmProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let debug = format!("LlmProvider({})", self.name());
        write!(f, "{}", debug)
    }
}
