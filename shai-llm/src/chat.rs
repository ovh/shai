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
use std::pin::Pin;

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

    /// Check status code and handle errors
    async fn check_status_code(
        result: Result<reqwest::Response, reqwest::Error>,
    ) -> Result<reqwest::Response, APIError> {
        match result {
            Ok(response) => {
                if response.status().is_success() {
                    Ok(response)
                } else {
                    let status = response.status();
                    let error_text = response.text().await.unwrap_or_default();

                    match status.as_u16() {
                        400 => Err(APIError::InvalidRequestError(error_text)),
                        401 => Err(APIError::AuthenticationError(error_text)),
                        403 => Err(APIError::PermissionError(error_text)),
                        404 => Err(APIError::NotFoundError(error_text)),
                        429 => Err(APIError::RateLimitError(error_text)),
                        500 => Err(APIError::UnknownError(500, error_text)),
                        503 => Err(APIError::UnknownError(503, error_text)),
                        _ => Err(APIError::UnknownError(status.as_u16(), error_text)),
                    }
                }
            }
            Err(error) => Err(APIError::ParseError(error.to_string())),
        }
    }

    /// Chat completion with JSON hooks
    pub async fn chat_completion<H: JsonHooks>(
        &self,
        parameters: &ChatCompletionParameters,
        hooks: &H,
    ) -> Result<ChatCompletionResponse, APIError> {
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
        APIError,
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
