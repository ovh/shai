use super::api::OpenRouterModelsResponse;
use crate::provider::{EnvVar, LlmError, LlmProvider, LlmStream, ProviderInfo};
use async_trait::async_trait;
use futures::StreamExt;
use openai_dive::v1::{
    api::Client,
    resources::{
        chat::{ChatCompletionChunkResponse, ChatCompletionParameters, ChatCompletionResponse},
        model::ListModelResponse,
    },
};
use reqwest;

const OPENROUTER_API_BASE: &str = "https://openrouter.ai/api/v1";

pub struct OpenRouterProvider {
    client: Client,
    api_key: String,
    base_url: String,
    http_client: reqwest::Client,
}

impl OpenRouterProvider {
    pub fn new(api_key: String) -> Self {
        let mut client = Client::new(api_key.clone());
        client.set_base_url(OPENROUTER_API_BASE);
        Self {
            client,
            api_key,
            base_url: OPENROUTER_API_BASE.to_string(),
            http_client: reqwest::Client::new(),
        }
    }

    /// Create OpenRouter provider from environment variables
    /// Returns None if required environment variables are not set
    pub fn from_env() -> Option<Self> {
        std::env::var("OPENROUTER_API_KEY").ok().map(Self::new)
    }

    /// Get OpenRouter models using their native API format
    pub async fn openrouter_models(&self) -> Result<OpenRouterModelsResponse, LlmError> {
        let url = format!("{}/models", self.base_url);

        let response = self
            .http_client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .send()
            .await
            .map_err(|e| Box::new(e) as LlmError)?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(Box::new(std::io::Error::other(format!(
                "OpenRouter API error {}: {}",
                status, text
            ))) as LlmError);
        }

        let openrouter_response: OpenRouterModelsResponse =
            response.json().await.map_err(|e| Box::new(e) as LlmError)?;

        Ok(openrouter_response)
    }
}

#[async_trait]
impl LlmProvider for OpenRouterProvider {
    async fn models(&self) -> Result<ListModelResponse, LlmError> {
        let openrouter_response = self.openrouter_models().await?;
        Ok(openrouter_response.to_openai_models_response())
    }

    async fn default_model(&self) -> Result<String, LlmError> {
        let models = self.models().await?;

        let keywords = ["free"];
        models
            .data
            .iter()
            .find(|m| keywords.iter().any(|kw| m.id.to_lowercase().contains(kw)))
            .or_else(|| models.data.first())
            .map(|m| m.id.clone())
            .ok_or_else(|| "no model available".into())
    }

    async fn chat(
        &self,
        request: ChatCompletionParameters,
    ) -> Result<ChatCompletionResponse, LlmError> {
        let response = self
            .client
            .chat()
            .create(request)
            .await
            .map_err(|e| Box::new(e) as LlmError)?;
        Ok(response)
    }

    async fn chat_stream(
        &self,
        mut request: ChatCompletionParameters,
    ) -> Result<LlmStream, LlmError> {
        // Ensure streaming is enabled
        request.stream = Some(true);

        let stream = self
            .client
            .chat()
            .create_stream(request)
            .await
            .map_err(|e| Box::new(e) as LlmError)?;

        let converted_stream = stream.map(|result| result.map_err(|e| Box::new(e) as LlmError));

        Ok(Box::new(Box::pin(converted_stream)))
    }

    fn supports_functions(&self, model: String) -> bool {
        true
    }

    fn supports_structured_output(&self, model: String) -> bool {
        true
    }

    fn name(&self) -> &'static str {
        "openrouter"
    }

    fn info() -> ProviderInfo {
        ProviderInfo {
            name: "openrouter",
            display_name: "OpenRouter (Multiple AI Providers)",
            env_vars: vec![EnvVar::required("OPENROUTER_API_KEY", "OpenRouter API key")],
        }
    }
}
