// llm/providers/ovhcloud.rs
use crate::provider::{EnvVar, LlmError, LlmProvider, LlmStream, ProviderInfo};
use async_trait::async_trait;
use futures::StreamExt;
use openai_dive::v1::{
    api::Client,
    error::APIError,
    resources::{
        chat::{ChatCompletionChunkResponse, ChatCompletionParameters, ChatCompletionResponse},
        model::ListModelResponse,
        shared::Usage,
    },
};
use serde_json::Value;

const OVH_API_BASE: &str = "https://oai.endpoints.kepler.ai.cloud.ovh.net/v1";

/// Appended to rate-limit errors when no API key is configured, so users who
/// hit the anonymous limit learn how to raise it.
pub const ANONYMOUS_RATE_LIMIT_HINT: &str = "You are using OVHcloud AI Endpoints anonymously, \
which is limited to 2 requests per minute. Run `shai auth` to configure an API key and raise \
the limit to 400 requests per minute.";

pub struct OvhCloudProvider {
    client: Client,
    /// True when no API key is configured (anonymous access).
    anonymous: bool,
}

impl OvhCloudProvider {
    pub fn new(api_key: String, base_url: Option<String>) -> Self {
        let anonymous = api_key.trim().is_empty();
        let mut client = Client::new(api_key);
        let url = base_url.unwrap_or_else(|| OVH_API_BASE.to_string());
        client.set_base_url(&url);
        Self { client, anonymous }
    }

    /// Convert an API error, pointing anonymous users at `shai auth` when they
    /// hit the rate limit. The error variant is kept so retries still apply.
    fn map_api_error(&self, error: APIError) -> LlmError {
        match error {
            APIError::RateLimitError(message) if self.anonymous => {
                Box::new(APIError::RateLimitError(format!(
                    "{}\n{}",
                    message.trim_end(),
                    ANONYMOUS_RATE_LIMIT_HINT
                )))
            }
            other => Box::new(other),
        }
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
}

#[async_trait]
impl LlmProvider for OvhCloudProvider {
    async fn models(&self) -> Result<ListModelResponse, LlmError> {
        super::models::list_models_compat(&self.client).await
    }

    async fn default_model(&self) -> Result<String, LlmError> {
        let models = self.models().await?; // Get the models

        models
            .data
            .iter()
            .find(|m| m.id.to_lowercase().contains("nemo"))
            .or_else(|| models.data.first())
            .map(|m| m.id.clone())
            .ok_or_else(|| "no model available".into())
    }

    async fn chat(
        &self,
        request: ChatCompletionParameters,
    ) -> Result<ChatCompletionResponse, LlmError> {
        let sanitized_request = self.sanitize_request(request);
        let mut response = self
            .client
            .chat()
            .create(sanitized_request)
            .await
            .map_err(|e| self.map_api_error(e))?;

        Ok(response)
    }

    async fn chat_stream(
        &self,
        mut request: ChatCompletionParameters,
    ) -> Result<LlmStream, LlmError> {
        request.stream = Some(true);
        let sanitized_request = self.sanitize_request(request);

        let stream = self
            .client
            .chat()
            .create_stream(sanitized_request)
            .await
            .map_err(|e| self.map_api_error(e))?;

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
        "ovhcloud"
    }

    fn info() -> ProviderInfo {
        ProviderInfo {
            name: "ovhcloud",
            display_name: "OVHcloud AI Endpoints",
            env_vars: vec![
                EnvVar::required("OVH_API_KEY", "OVHcloud API key"),
                EnvVar::optional(
                    "OVH_BASE_URL",
                    "OVHcloud base URL (defaults to standard endpoint)",
                ),
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openai_dive::v1::resources::chat::{
        ChatCompletionParametersBuilder, ChatMessage, ChatMessageContent,
    };

    // Body returned by OVHcloud AI Endpoints when the anonymous limit is hit (#2)
    const OVH_RATE_LIMIT_BODY: &str =
        r#"{"message":"API rate limit exceeded","request_id":"4e7fb36eb78dcb16c3862c6ec2d85f05"}"#;

    fn sample_params() -> ChatCompletionParameters {
        ChatCompletionParametersBuilder::default()
            .model("Qwen3-32B")
            .messages(vec![ChatMessage::User {
                content: ChatMessageContent::Text("hello".into()),
                name: None,
            }])
            .build()
            .expect("valid chat parameters")
    }

    async fn rate_limited_chat(api_key: &str) -> APIError {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("POST", "/chat/completions")
            .with_status(429)
            .with_body(OVH_RATE_LIMIT_BODY)
            .create_async()
            .await;

        let provider = OvhCloudProvider::new(api_key.to_string(), Some(server.url()));
        let err = provider
            .chat(sample_params())
            .await
            .expect_err("expected a rate-limit error");
        *err.downcast::<APIError>()
            .expect("error should stay an openai_dive APIError")
    }

    #[tokio::test]
    async fn anonymous_rate_limit_suggests_shai_auth() {
        match rate_limited_chat("").await {
            APIError::RateLimitError(message) => {
                // provider message is kept for debugging
                assert!(message.contains("API rate limit exceeded"));
                assert!(message.contains("shai auth"));
            }
            other => panic!("expected APIError::RateLimitError, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn authenticated_rate_limit_is_unchanged() {
        match rate_limited_chat("test-key").await {
            APIError::RateLimitError(message) => {
                assert_eq!(message, OVH_RATE_LIMIT_BODY);
            }
            other => panic!("expected APIError::RateLimitError, got {:?}", other),
        }
    }

    #[test]
    fn non_rate_limit_errors_have_no_hint() {
        let provider = OvhCloudProvider::new(String::new(), None);
        let err = provider.map_api_error(APIError::AuthenticationError("bad key".into()));
        assert_eq!(err.to_string(), "bad key");
    }
}
