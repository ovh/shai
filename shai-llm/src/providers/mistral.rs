// Mistral provider using flexible chat client with JSON hooks
use crate::chat::{ChatClient, JsonHooks};
use crate::provider::{EnvVar, LlmError, LlmProvider, LlmStream, ProviderInfo};
use async_trait::async_trait;
use futures::StreamExt;
use openai_dive::v1::error::APIError;
use openai_dive::v1::resources::{
    chat::{ChatCompletionChunkResponse, ChatCompletionParameters, ChatCompletionResponse},
    model::{ListModelResponse, Model},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize, Serialize)]
struct MistralModelCapabilities {
    completion_chat: bool,
    completion_fim: bool,
    function_calling: bool,
    fine_tuning: bool,
    vision: bool,
    classification: bool,
}

#[derive(Debug, Deserialize, Serialize)]
struct MistralModel {
    id: String,
    object: String,
    created: i64,
    owned_by: String,
    capabilities: MistralModelCapabilities,
    name: String,
    description: String,
    max_context_length: i32,
    aliases: Vec<String>,
    deprecation: Option<String>,
    deprecation_replacement_model: Option<String>,
    default_model_temperature: Option<f32>,
    #[serde(rename = "type")]
    model_type: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct MistralListModelResponse {
    object: String,
    data: Vec<MistralModel>,
}

impl From<MistralModel> for Model {
    fn from(mistral_model: MistralModel) -> Self {
        Model {
            id: mistral_model.id,
            object: mistral_model.object,
            created: Some(mistral_model.created.try_into().unwrap_or(0)),
            owned_by: mistral_model.owned_by,
        }
    }
}

pub struct MistralProvider {
    client: ChatClient,
    hooks: MistralHooks,
}

impl MistralProvider {
    pub fn new(api_key: String) -> Self {
        let client = ChatClient::new(api_key, "https://api.mistral.ai/v1".to_string());
        Self {
            client,
            hooks: MistralHooks,
        }
    }

    /// Create Mistral provider from environment variables
    /// Returns None if required environment variables are not set
    pub fn from_env() -> Option<Self> {
        std::env::var("MISTRAL_API_KEY").ok().map(Self::new)
    }
}

/// Mistral-specific hooks to fix JSON compatibility
#[derive(Clone, Copy)]
pub struct MistralHooks;

#[async_trait]
impl JsonHooks for MistralHooks {
    async fn before_send(&self, mut json: Value) -> Result<Value, APIError> {
        // Fix tool_choice for Mistral compatibility
        // Mistral uses "any" instead of "required" to force tool usage
        if let Some(tool_choice) = json.get_mut("tool_choice") {
            if tool_choice == "required" {
                *tool_choice = Value::String("any".to_string());
            }
        }

        Ok(json)
    }

    async fn after_receive(&self, mut json: Value) -> Result<Value, APIError> {
        // Fix missing "type" field in tool_calls for Mistral responses (non-streaming)
        if let Some(choices) = json.get_mut("choices").and_then(|c| c.as_array_mut()) {
            for choice in choices {
                if let Some(message) = choice.get_mut("message") {
                    if let Some(tool_calls) = message
                        .get_mut("tool_calls")
                        .and_then(|tc| tc.as_array_mut())
                    {
                        for tool_call in tool_calls {
                            if tool_call.get("type").is_none() {
                                tool_call.as_object_mut().unwrap().insert(
                                    "type".to_string(),
                                    Value::String("function".to_string()),
                                );
                            }
                        }
                    }
                }
            }
        }
        Ok(json)
    }

    async fn after_receive_stream(&self, mut json: Value) -> Result<Value, APIError> {
        // Fix missing "type" field in tool_calls for Mistral responses (streaming)
        // Streaming responses have a slightly different structure with delta
        if let Some(choices) = json.get_mut("choices").and_then(|c| c.as_array_mut()) {
            for choice in choices {
                if let Some(delta) = choice.get_mut("delta") {
                    if let Some(tool_calls) =
                        delta.get_mut("tool_calls").and_then(|tc| tc.as_array_mut())
                    {
                        for tool_call in tool_calls {
                            if tool_call.get("type").is_none() {
                                tool_call.as_object_mut().unwrap().insert(
                                    "type".to_string(),
                                    Value::String("function".to_string()),
                                );
                            }
                        }
                    }
                }
            }
        }
        Ok(json)
    }
}

#[async_trait]
impl LlmProvider for MistralProvider {
    async fn models(&self) -> Result<ListModelResponse, LlmError> {
        // Fetch models from Mistral API using the existing HTTP client
        let url = format!("{}/models", self.client.base_url);

        let response = self
            .client
            .http_client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.client.api_key))
            .send()
            .await
            .map_err(|e| Box::new(e) as LlmError)?;

        let mistral_response: MistralListModelResponse =
            response.json().await.map_err(|e| Box::new(e) as LlmError)?;

        // Filter models that support function calling and convert to OpenAI format
        let filtered_models: Vec<Model> = mistral_response
            .data
            .into_iter()
            .filter(|model| model.capabilities.function_calling)
            .map(|model| model.into())
            .collect();

        Ok(ListModelResponse {
            object: "list".to_string(),
            data: filtered_models,
        })
    }

    async fn default_model(&self) -> Result<String, LlmError> {
        Ok("mistral-small-latest".to_string())
    }

    async fn chat(
        &self,
        mut request: ChatCompletionParameters,
    ) -> Result<ChatCompletionResponse, LlmError> {
        // Mistral uses max_tokens instead of max_completion_tokens
        if request.max_completion_tokens.is_some() {
            request.max_tokens = request.max_completion_tokens;
            request.max_completion_tokens = None;
        }

        let response = self
            .client
            .chat_completion(&request, &self.hooks)
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

        // Mistral uses max_tokens instead of max_completion_tokens
        if request.max_completion_tokens.is_some() {
            request.max_tokens = request.max_completion_tokens;
            request.max_completion_tokens = None;
        }

        let stream = self
            .client
            .chat_completion_stream(&request, self.hooks)
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
        "mistral"
    }

    fn info() -> ProviderInfo {
        ProviderInfo {
            name: "mistral",
            display_name: "Mistral AI (Mixtral, Pixtral)",
            env_vars: vec![EnvVar::required("MISTRAL_API_KEY", "Mistral AI API key")],
        }
    }
}
