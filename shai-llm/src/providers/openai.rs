// llm/providers/openai.rs
use crate::provider::{LlmProvider, LlmError, LlmStream, ProviderInfo, EnvVar};
use async_trait::async_trait;
use futures::StreamExt;
use openai_dive::v1::{
    api::Client,
    resources::{
        chat::{ChatCompletionParameters, ChatCompletionResponse, ChatCompletionChunkResponse},
        model::ListModelResponse,
    },
};

pub struct OpenAIProvider {
    client: Client,
}

impl OpenAIProvider {
    pub fn new(api_key: String) -> Self {
        let mut client = Client::new(api_key);
        client.set_base_url("https://api.openai.com/v1");
        Self { client }
    }

    /// Create OpenAI provider from environment variables
    /// Returns None if required environment variables are not set
    pub fn from_env() -> Option<Self> {
        std::env::var("OPENAI_API_KEY").ok().map(|api_key| {
            Self::new(api_key)
        })
    }
}

#[async_trait]
impl LlmProvider for OpenAIProvider {
    async fn models(&self) -> Result<ListModelResponse, LlmError> {
        super::models::list_models_compat(&self.client).await
    }

    async fn default_model(&self) -> Result<String, LlmError> {
        let models = self.models().await?; // Get the models
    
        models.data.iter()
            .find(|m| m.id.to_lowercase().contains("gpt4"))
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
        // Ensure streaming is enabled
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
        "openai"
    }
    
    fn info() -> ProviderInfo {
        ProviderInfo {
            name: "openai",
            display_name: "OpenAI (GPT-4, GPT-3.5)",
            env_vars: vec![
                EnvVar::required("OPENAI_API_KEY", "OpenAI API key"),
            ],
        }
    }
    
}

