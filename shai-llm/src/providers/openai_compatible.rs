// llm/providers/openai_compatible.rs
use crate::provider::{LlmProvider, LlmError, LlmStream, ProviderInfo, EnvVar};
use async_trait::async_trait;
use futures::StreamExt;
use openai_dive::v1::{
    api::Client,
    resources::{
        chat::{ChatCompletionParameters, ChatCompletionResponse, ChatCompletionChunkResponse},
        model::ListModelResponse,
        shared::Usage,
    },
};
use serde_json::Value;

pub struct OpenAICompatibleProvider {
    client: Client,
}

impl OpenAICompatibleProvider {
    pub fn new(api_key: String, base_url: String) -> Self {
        let mut client = Client::new(api_key);
        client.set_base_url(&base_url);
        Self { client }
    }

    /// Create OpenAI Compatible provider from environment variables
    /// Returns None if required environment variables are not set
    pub fn from_env() -> Option<Self> {
        match (std::env::var("OPENAI_COMPATIBLE_API_KEY"), std::env::var("OPENAI_COMPATIBLE_BASE_URL")) {
            (Ok(api_key), Ok(base_url)) => {
                Some(Self::new(api_key, base_url))
            }
            _ => None
        }
    }

    fn adjust_response_tokens(&self, mut response: ChatCompletionResponse) -> ChatCompletionResponse {
        if let Some(ref mut usage) = response.usage {
            if let Some(details) = &usage.completion_tokens_details {
                let reasoning_tokens = details.reasoning_tokens;
                if let Some(completion) = usage.completion_tokens {
                    usage.completion_tokens = Some(completion.saturating_sub(reasoning_tokens));
                }
            }
        }
        response
    }
}

#[async_trait]
impl LlmProvider for OpenAICompatibleProvider {
    async fn models(&self) -> Result<ListModelResponse, LlmError> {
        let response = self.client.models().list().await
            .map_err(|e| Box::new(e) as LlmError)?;
        Ok(response)
    }

    async fn chat(&self, request: ChatCompletionParameters) -> Result<ChatCompletionResponse, LlmError> {
        let response = self.client.chat().create(request).await
            .map_err(|e| Box::new(e) as LlmError)?;

        Ok(self.adjust_response_tokens(response))
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
        "openai_compatible"
    }
    
    fn info() -> ProviderInfo {
        ProviderInfo {
            name: "openai_compatible",
            display_name: "OpenAI Compatible API",
            env_vars: vec![
                EnvVar::required("OPENAI_COMPATIBLE_API_KEY", "API key for OpenAI-compatible service"),
                EnvVar::required("OPENAI_COMPATIBLE_BASE_URL", "Base URL for OpenAI-compatible service"),
            ],
        }
    }
    
}

