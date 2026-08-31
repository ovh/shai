// llm/providers/ovhcloud.rs
use crate::provider::{LlmProvider, LlmError, LlmStream, ProviderInfo, EnvVar};
use async_trait::async_trait;
use futures::StreamExt;
use openai_dive::v1::{
    api::Client,
    resources::{
        chat::{ChatCompletionParameters, ChatCompletionResponse, ChatCompletionChunkResponse},
        model::ListModelResponse,
    },
    error::APIError
};

const OLLAMA_BASE_URL: &str = "http://127.0.0.1:11434/v1";

pub struct OllamaProvider {
    client: Client,
}

impl OllamaProvider {
    pub fn new(base_url: Option<String>) -> Self {
        let mut client = Client::new(String::new());
        let url = base_url.unwrap_or_else(|| OLLAMA_BASE_URL.to_string());
        client.set_base_url(&url);
        Self { client }
    }

    /// Create OVH Cloud provider from environment variables
    /// Returns None if required environment variables are not set
    pub fn from_env() -> Option<Self> {
        std::env::var("OLLAMA_BASE_URL").ok().map(|api_key| {
            let base_url = std::env::var("OLLAMA_BASE_URL").ok();
            Self::new(base_url)
        })
    }
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    async fn models(&self) -> Result<ListModelResponse, LlmError> {
        super::models::list_models_compat(&self.client).await
    }

    async fn default_model(&self) -> Result<String, LlmError> {
        let models = self.models().await?; // Get the models
    
        models.data.iter()
            .find(|m| m.id.to_lowercase().contains("smol"))
            .or_else(|| models.data.first())
            .map(|m| m.id.clone())
            .ok_or_else(|| "no model available".into())
    }

    async fn chat(&self, request: ChatCompletionParameters) -> Result<ChatCompletionResponse, LlmError> {
        let response = self.client.chat().create(request).await
            .map_err(|e| Box::new(e) as LlmError)?;
        Ok(response)
    }

    async fn chat_stream(&self, mut request: ChatCompletionParameters) -> Result<LlmStream, LlmError> {
        request.stream = Some(true);
        
        let stream = self.client.chat().create_stream(request).await
            .map_err(|e| Box::new(e) as LlmError)?;

        let converted_stream = stream.map(|result| {
            result.map_err(|e| Box::new(e) as LlmError)
        });

        Ok(Box::new(Box::pin(converted_stream)))
    }

    fn supports_functions(&self, model: String) -> bool {
        true
    }

    fn supports_structured_output(&self, model: String) -> bool {
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
                EnvVar::optional("OLLAMA_BASE_URL", "ollama base open ai compat url"),
            ],
        }
    }
    
}

