// llm/providers/ollama.rs
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

pub struct OllamaProvider {
    client: Client,
}

impl OllamaProvider {
    pub fn new(base_url: Option<String>, api_key: Option<String>) -> Self {
        let mut client = Client::new(api_key.unwrap_or("ollama".to_string()));
        let url = base_url.unwrap_or("http://localhost:11434/v1".to_string());
        client.set_base_url(&url);
        Self { client }
    }

    // Create Ollama provider from environment variables
    pub fn from_env() -> Option<Self> {
        // Ollama is always available as it defaults to localhost
        Some(Self::new(
            std::env::var("OLLAMA_BASE_URL").ok(),
            std::env::var("OLLAMA_API_KEY").ok(),
        ))
    }
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    async fn models(&self) -> Result<ListModelResponse, LlmError> {
        let response = self
            .client
            .models()
            .list()
            .await
            .map_err(|e| Box::new(e) as LlmError)?;
        Ok(response)
    }

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

    fn supports_functions(&self, _model: String) -> bool {
        true
    }

    fn supports_structured_output(&self, _model: String) -> bool {
        true
    }

    fn name(&self) -> &'static str {
        "ollama"
    }

    fn info() -> ProviderInfo {
        ProviderInfo {
            name: "ollama",
            display_name: "Ollama",
            env_vars: vec![
                EnvVar::optional(
                    "OLLAMA_BASE_URL",
                    "Ollama API Base URL (default: http://localhost:11434/v1)",
                ),
                EnvVar::optional("OLLAMA_API_KEY", "Ollama API Key (optional)"),
            ],
        }
    }
}
