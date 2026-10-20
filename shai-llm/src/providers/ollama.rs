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
    },
};

const OLLAMA_BASE_URL: &str = "http://127.0.0.1:11434/v1";

pub struct OllamaProvider {
    client: Client,
}

impl OllamaProvider {
    pub fn new(base_url: Option<String>) -> Self {
        Self::with_api_key(base_url, None)
    }

    /// Create an Ollama provider that authenticates with `api_key`, sent as
    /// `Authorization: Bearer <api_key>` (e.g. ollama.com or a server behind an
    /// authenticating proxy). `None` behaves exactly like [`OllamaProvider::new`].
    pub fn with_api_key(base_url: Option<String>, api_key: Option<String>) -> Self {
        let mut client = Client::new(api_key.unwrap_or_default());
        let url = base_url.unwrap_or_else(|| OLLAMA_BASE_URL.to_string());
        client.set_base_url(&url);
        Self { client }
    }

    /// Create Ollama provider from environment variables
    /// Returns None unless OLLAMA_BASE_URL or OLLAMA_API_KEY is set
    pub fn from_env() -> Option<Self> {
        Self::from_vars(
            std::env::var("OLLAMA_BASE_URL").ok(),
            std::env::var("OLLAMA_API_KEY").ok(),
        )
    }

    /// Either variable selects Ollama; a missing base URL falls back to the default.
    fn from_vars(base_url: Option<String>, api_key: Option<String>) -> Option<Self> {
        if base_url.is_none() && api_key.is_none() {
            return None;
        }
        Some(Self::with_api_key(base_url, api_key))
    }
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    async fn models(&self) -> Result<ListModelResponse, LlmError> {
        super::models::list_models_compat(&self.client).await
    }

    async fn default_model(&self) -> Result<String, LlmError> {
        let models = self.models().await?; // Get the models

        models
            .data
            .iter()
            .find(|m| m.id.to_lowercase().contains("smol"))
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
        "ollama"
    }

    fn info() -> ProviderInfo {
        ProviderInfo {
            name: "ollama",
            display_name: "Ollama",
            env_vars: vec![
                EnvVar::optional("OLLAMA_BASE_URL", "ollama base open ai compat url"),
                EnvVar::optional(
                    "OLLAMA_API_KEY",
                    "API key sent as a Bearer token (only needed if the server requires auth)",
                ),
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_key_is_optional_and_defaults_to_empty() {
        let provider = OllamaProvider::new(None);
        assert_eq!(provider.client.api_key, "");
        assert_eq!(provider.client.base_url, OLLAMA_BASE_URL);

        let provider = OllamaProvider::with_api_key(None, None);
        assert_eq!(provider.client.api_key, "");
    }

    #[test]
    fn info_lists_optional_api_key() {
        let info = OllamaProvider::info();
        let key = info
            .env_vars
            .iter()
            .find(|v| v.name == "OLLAMA_API_KEY")
            .expect("OLLAMA_API_KEY should be listed");
        assert!(!key.required);
    }

    #[test]
    fn from_vars_selects_ollama_when_either_variable_is_set() {
        assert!(OllamaProvider::from_vars(None, None).is_none());

        let key_only = OllamaProvider::from_vars(None, Some("k".to_string())).unwrap();
        assert_eq!(key_only.client.base_url, OLLAMA_BASE_URL);
        assert_eq!(key_only.client.api_key, "k");

        let url_only =
            OllamaProvider::from_vars(Some("http://gpu-box:11434/v1".to_string()), None).unwrap();
        assert_eq!(url_only.client.base_url, "http://gpu-box:11434/v1");
        assert_eq!(url_only.client.api_key, "");
    }

    #[tokio::test]
    async fn api_key_is_sent_as_bearer_token() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/v1/models")
            .match_header("authorization", "Bearer test-key")
            .with_status(200)
            .with_body(r#"{"object":"list","data":[{"id":"llama3"}]}"#)
            .create_async()
            .await;

        let provider = OllamaProvider::with_api_key(
            Some(format!("{}/v1", server.url())),
            Some("test-key".to_string()),
        );
        let models = provider.models().await.expect("models");
        assert_eq!(models.data[0].id, "llama3");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn no_authorization_header_without_api_key() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/v1/models")
            .match_header("authorization", mockito::Matcher::Missing)
            .with_status(200)
            .with_body(r#"{"object":"list","data":[{"id":"llama3"}]}"#)
            .create_async()
            .await;

        let provider = OllamaProvider::new(Some(format!("{}/v1", server.url())));
        provider.models().await.expect("models");
        mock.assert_async().await;
    }
}
