/// Blatant COPY / PASTE from openai_dive to add hooks for json manipulation
///
// Flexible chat client with JSON manipulation hooks
use async_trait::async_trait;
use futures::{Stream, StreamExt};
use openai_dive::v1::{
    error::APIError,
    resources::chat::{
        ChatCompletionChunkResponse, ChatCompletionParameters, ChatCompletionResponse,
    },
};
use reqwest::{Method, RequestBuilder};
use reqwest_eventsource::{Event, EventSource, RequestBuilderExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fmt;
use std::pin::Pin;

use crate::error::{retry_after_from_headers, RateLimitedError};

/// Error type for [`ChatClient`].
///
/// Wraps the upstream [`APIError`] while adding a [`RateLimitedError`] variant
/// that preserves the `Retry-After` hint, which `APIError` discards.
#[derive(Debug)]
pub enum ChatError {
    Api(APIError),
    RateLimited(RateLimitedError),
}

impl ChatError {
    /// Return the `Retry-After` delay if this error carries one.
    pub fn retry_after(&self) -> Option<std::time::Duration> {
        match self {
            ChatError::RateLimited(e) => e.retry_after,
            ChatError::Api(_) => None,
        }
    }
}

impl From<APIError> for ChatError {
    fn from(e: APIError) -> Self {
        ChatError::Api(e)
    }
}

impl fmt::Display for ChatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChatError::Api(e) => write!(f, "{}", e),
            ChatError::RateLimited(e) => write!(f, "{}", e),
        }
    }
}

impl std::error::Error for ChatError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ChatError::Api(e) => Some(e),
            ChatError::RateLimited(e) => Some(e),
        }
    }
}

/// Trait for JSON manipulation hooks
#[async_trait]
pub trait JsonHooks: Send + Sync {
    /// Called before sending JSON to the API
    async fn before_send(&self, json: Value) -> Result<Value, APIError> {
        Ok(json) // Default: no modification
    }

    /// Called after receiving JSON from the API (non-streaming)
    async fn after_receive(&self, json: Value) -> Result<Value, APIError> {
        Ok(json) // Default: no modification
    }

    /// Called after receiving JSON from the API (streaming chunks)
    async fn after_receive_stream(&self, json: Value) -> Result<Value, APIError> {
        // Default: use the same logic as after_receive
        self.after_receive(json).await
    }
}

/// Default implementation with no hooks
pub struct NoHooks;

#[async_trait]
impl JsonHooks for NoHooks {}

/// Flexible chat client
#[derive(Clone, Debug)]
pub struct ChatClient {
    pub http_client: reqwest::Client,
    pub base_url: String,
    pub api_key: String,
    pub headers: Option<HashMap<String, String>>,
    pub organization: Option<String>,
    pub project: Option<String>,
}

impl ChatClient {
    /// Create a new chat client
    pub fn new(api_key: String, base_url: String) -> Self {
        Self {
            http_client: reqwest::Client::new(),
            base_url,
            api_key,
            headers: None,
            organization: None,
            project: None,
        }
    }

    /// Build a request with authentication headers
    fn build_request(&self, method: Method, path: &str, content_type: &str) -> RequestBuilder {
        let url = format!("{}{}", self.base_url, path);
        let mut request = self
            .http_client
            .request(method, &url)
            .header(reqwest::header::CONTENT_TYPE, content_type)
            .bearer_auth(&self.api_key);

        if let Some(headers) = &self.headers {
            for (key, value) in headers {
                request = request.header(key, value);
            }
        }

        if let Some(organization) = &self.organization {
            request = request.header("OpenAI-Organization", organization);
        }

        if let Some(project) = &self.project {
            request = request.header("OpenAI-Project", project);
        }

        request
    }

    /// Check status code and handle errors.
    ///
    /// Rate-limiting responses (429/503) are surfaced as [`ChatError::RateLimited`]
    /// so the caller can honor the `Retry-After` header. The header must be read
    /// before the body is consumed.
    async fn check_status_code(
        result: Result<reqwest::Response, reqwest::Error>,
    ) -> Result<reqwest::Response, ChatError> {
        match result {
            Ok(response) => {
                if response.status().is_success() {
                    Ok(response)
                } else {
                    let status = response.status();
                    let retry_after = retry_after_from_headers(response.headers());
                    let error_text = response.text().await.unwrap_or_default();

                    match status.as_u16() {
                        400 => Err(ChatError::Api(APIError::InvalidRequestError(error_text))),
                        401 => Err(ChatError::Api(APIError::AuthenticationError(error_text))),
                        403 => Err(ChatError::Api(APIError::PermissionError(error_text))),
                        404 => Err(ChatError::Api(APIError::NotFoundError(error_text))),
                        429 | 503 => Err(ChatError::RateLimited(RateLimitedError::new(
                            status.as_u16(),
                            retry_after,
                            error_text,
                        ))),
                        _ => Err(ChatError::Api(APIError::UnknownError(
                            status.as_u16(),
                            error_text,
                        ))),
                    }
                }
            }
            Err(error) => Err(ChatError::Api(APIError::ParseError(error.to_string()))),
        }
    }

    /// Chat completion with JSON hooks
    pub async fn chat_completion<H: JsonHooks>(
        &self,
        parameters: &ChatCompletionParameters,
        hooks: &H,
    ) -> Result<ChatCompletionResponse, ChatError> {
        // Serialize to JSON and apply before_send hook
        let mut json =
            serde_json::to_value(parameters).map_err(|e| APIError::ParseError(e.to_string()))?;
        json = hooks.before_send(json).await?;

        // Send request
        let result = self
            .build_request(Method::POST, "/chat/completions", "application/json")
            .json(&json)
            .send()
            .await;

        let response = Self::check_status_code(result).await?;

        // Get response text and apply after_receive hook
        let response_text = response
            .text()
            .await
            .map_err(|error| APIError::ParseError(error.to_string()))?;

        let mut response_json: Value = serde_json::from_str(&response_text)
            .map_err(|e| APIError::ParseError(e.to_string()))?;

        response_json = hooks.after_receive(response_json).await?;

        // Deserialize the modified JSON
        let completion_response: ChatCompletionResponse = serde_json::from_value(response_json)
            .map_err(|e| APIError::ParseError(e.to_string()))?;

        Ok(completion_response)
    }

    /// Chat completion streaming with JSON hooks
    pub async fn chat_completion_stream<H: JsonHooks + 'static>(
        &self,
        parameters: &ChatCompletionParameters,
        hooks: H,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<ChatCompletionChunkResponse, APIError>> + Send>>,
        ChatError,
    > {
        // Serialize to JSON and apply before_send hook
        let mut json =
            serde_json::to_value(parameters).map_err(|e| APIError::ParseError(e.to_string()))?;
        json = hooks.before_send(json).await?;

        // Create event source for streaming
        let event_source = self
            .build_request(Method::POST, "/chat/completions", "application/json")
            .json(&json)
            .eventsource()
            .map_err(|e| APIError::ParseError(e.to_string()))?;

        // Return stream that processes events
        let stream = async_stream::stream! {
            let mut event_source = event_source;
            while let Some(event) = event_source.next().await {
                match event {
                    Ok(Event::Open) => {}
                    Ok(Event::Message(message)) => {
                        if message.data == "[DONE]" {
                            break;
                        }

                        // Parse the event data
                        match serde_json::from_str::<Value>(&message.data) {
                            Ok(json) => {
                                // Apply after_receive_stream hook
                                match hooks.after_receive_stream(json).await {
                                    Ok(modified_json) => {
                                        // Deserialize the modified JSON
                                        match serde_json::from_value::<ChatCompletionChunkResponse>(modified_json) {
                                            Ok(chunk) => yield Ok(chunk),
                                            Err(e) => yield Err(APIError::ParseError(e.to_string())),
                                        }
                                    }
                                    Err(e) => yield Err(e),
                                }
                            }
                            Err(e) => yield Err(APIError::ParseError(e.to_string())),
                        }
                    }
                    Err(e) => yield Err(APIError::StreamError(e.to_string())),
                }
            }
        };

        Ok(Box::pin(stream))
    }
}

// Note: types are already imported above, no need to re-export

#[cfg(test)]
mod tests {
    use super::*;
    use openai_dive::v1::resources::chat::{
        ChatCompletionParametersBuilder, ChatMessage, ChatMessageContent,
    };

    fn sample_params() -> ChatCompletionParameters {
        ChatCompletionParametersBuilder::default()
            .model("test-model".to_string())
            .messages(vec![ChatMessage::User {
                content: ChatMessageContent::Text("hello".into()),
                name: None,
            }])
            .build()
            .expect("valid chat parameters")
    }

    #[tokio::test]
    async fn rate_limit_surfaces_retry_after() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("POST", "/chat/completions")
            .with_status(429)
            .with_header("retry-after", "2")
            .with_body(r#"{"error":"rate limited"}"#)
            .create_async()
            .await;

        let client = ChatClient::new("test-key".into(), server.url());
        let err = client
            .chat_completion(&sample_params(), &NoHooks)
            .await
            .expect_err("expected a rate-limit error");

        match err {
            ChatError::RateLimited(rate_limited) => {
                assert_eq!(rate_limited.status, 429);
                assert_eq!(
                    rate_limited.retry_after,
                    Some(std::time::Duration::from_secs(2))
                );
            }
            other => panic!("expected ChatError::RateLimited, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn service_unavailable_surfaces_retry_after() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("POST", "/chat/completions")
            .with_status(503)
            .with_header("retry-after", "5")
            .with_body("unavailable")
            .create_async()
            .await;

        let client = ChatClient::new("test-key".into(), server.url());
        let err = client
            .chat_completion(&sample_params(), &NoHooks)
            .await
            .expect_err("expected a 503 error");

        assert_eq!(err.retry_after(), Some(std::time::Duration::from_secs(5)));
    }

    #[tokio::test]
    async fn auth_error_stays_api_variant() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("POST", "/chat/completions")
            .with_status(401)
            .with_body("unauthorized")
            .create_async()
            .await;

        let client = ChatClient::new("bad-key".into(), server.url());
        let err = client
            .chat_completion(&sample_params(), &NoHooks)
            .await
            .expect_err("expected an auth error");

        assert!(matches!(
            err,
            ChatError::Api(APIError::AuthenticationError(_))
        ));
        assert_eq!(err.retry_after(), None);
    }
}
