use axum::{
    extract::{rejection::JsonRejection, FromRequest},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use serde::{Deserialize, Serialize};
use tracing::error;

/// Error response structure for API errors
#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: ErrorDetail,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorDetail {
    pub message: String,
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

impl ErrorResponse {
    pub fn new(message: String, error_type: String, code: Option<String>) -> Self {
        Self {
            error: ErrorDetail {
                message,
                r#type: error_type,
                code,
            },
        }
    }

    pub fn not_found(message: String) -> Self {
        Self::new(
            message,
            "not_found".to_string(),
            Some("model_not_found".to_string()),
        )
    }

    pub fn invalid_request(message: String) -> Self {
        Self::new(message, "invalid_request".to_string(), None)
    }

    pub fn internal_error(message: String) -> Self {
        Self::new(message, "internal_error".to_string(), None)
    }
}

impl IntoResponse for ErrorResponse {
    fn into_response(self) -> Response {
        let status = match self.error.r#type.as_str() {
            "not_found" => StatusCode::NOT_FOUND,
            "invalid_request" => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, Json(self)).into_response()
    }
}

/// Custom JSON extractor that returns our ErrorResponse on deserialization failures
#[derive(FromRequest)]
#[from_request(via(axum::Json), rejection(ErrorResponse))]
pub struct ApiJson<T>(pub T);

impl From<JsonRejection> for ErrorResponse {
    fn from(rejection: JsonRejection) -> Self {
        let message = rejection.body_text();
        error!("JSON deserialization error: {}", message);
        ErrorResponse::invalid_request(message)
    }
}
