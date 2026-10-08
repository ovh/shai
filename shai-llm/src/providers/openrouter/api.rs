use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OpenRouterModelsResponse {
    pub data: Vec<OpenRouterModel>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OpenRouterModel {
    pub id: String,
    pub name: String,
    pub created: i64,
    pub description: String,
    pub architecture: OpenRouterArchitecture,
    pub top_provider: OpenRouterTopProvider,
    pub pricing: OpenRouterPricing,
    pub context_length: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hugging_face_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub per_request_limits: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supported_parameters: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OpenRouterArchitecture {
    pub input_modalities: Vec<String>,
    pub output_modalities: Vec<String>,
    pub tokenizer: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OpenRouterTopProvider {
    pub is_moderated: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OpenRouterPricing {
    pub prompt: String,
    pub completion: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_cache_read: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_cache_write: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web_search: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub internal_reasoning: Option<String>,
}

impl OpenRouterModel {
    /// Convert OpenRouter model to openai_dive Model format
    pub fn to_openai_model(&self) -> openai_dive::v1::resources::model::Model {
        openai_dive::v1::resources::model::Model {
            id: self.id.clone(),
            object: "model".to_string(),
            created: Some(self.created as u32),
            owned_by: "openrouter".to_string(),
        }
    }
}

impl OpenRouterModelsResponse {
    /// Convert OpenRouter models response to openai_dive ListModelResponse format
    pub fn to_openai_models_response(
        &self,
    ) -> openai_dive::v1::resources::model::ListModelResponse {
        openai_dive::v1::resources::model::ListModelResponse {
            object: "list".to_string(),
            data: self.data.iter().map(|m| m.to_openai_model()).collect(),
        }
    }
}
