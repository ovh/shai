// llm/providers/ovhcloud.rs
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
    error::APIError
};
use serde_json::Value;

const OVH_API_BASE: &str = "https://oai.endpoints.kepler.ai.cloud.ovh.net/v1";

pub struct OvhCloudProvider {
    client: Client,
}

impl OvhCloudProvider {
    pub fn new(api_key: String, base_url: Option<String>) -> Self {
        let mut client = Client::new(api_key);
        let url = base_url.unwrap_or_else(|| OVH_API_BASE.to_string());
        client.set_base_url(&url);
        Self { client }
    }

    /// Create OVH Cloud provider from environment variables
    /// Returns None if required environment variables are not set
    pub fn from_env() -> Option<Self> {
        std::env::var("OVH_API_KEY").ok().map(|api_key| {
            let base_url = std::env::var("OVH_BASE_URL").ok();
            Self::new(api_key, base_url)
        })
    }

    fn sanitize_request(&self, mut request: ChatCompletionParameters) -> ChatCompletionParameters {
        // OVH uses max_tokens instead of max_completion_tokens
        if request.max_completion_tokens.is_some() {
            request.max_tokens = request.max_completion_tokens;
            request.max_completion_tokens = None;
        }

        request
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
impl LlmProvider for OvhCloudProvider {
    async fn models(&self) -> Result<ListModelResponse, LlmError> {
        let response = self.client.models().list().await
            .map_err(|e| Box::new(e) as LlmError)?;
        Ok(response)
    }

    async fn default_model(&self) -> Result<String, LlmError> {
        let models = self.models().await?; // Get the models
    
        models.data.iter()
            .find(|m| m.id.to_lowercase().contains("nemo"))
            .or_else(|| models.data.first())
            .map(|m| m.id.clone())
            .ok_or_else(|| "no model available".into())
    }

    async fn chat(&self, request: ChatCompletionParameters) -> Result<ChatCompletionResponse, LlmError> {
        let sanitized_request = self.sanitize_request(request);
        let response = self.client.chat().create(sanitized_request).await
            .map_err(|e| Box::new(e) as LlmError)?;

        Ok(self.adjust_response_tokens(response))
    }

    async fn chat_stream(&self, mut request: ChatCompletionParameters) -> Result<LlmStream, LlmError> {
        request.stream = Some(true);
        let sanitized_request = self.sanitize_request(request);
        
        let stream = self.client.chat().create_stream(sanitized_request).await
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
        "ovhcloud"
    }
    
    fn info() -> ProviderInfo {
        ProviderInfo {
            name: "ovhcloud",
            display_name: "OVHcloud AI Endpoints",
            env_vars: vec![
                EnvVar::required("OVH_API_KEY", "OVHcloud API key"),
                EnvVar::optional("OVH_BASE_URL", "OVHcloud base URL (defaults to standard endpoint)"),
            ],
        }
    }
    
}

