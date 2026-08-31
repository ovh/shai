// llm/providers/models.rs
//! Lenient `GET /models` listing for OpenAI-compatible endpoints.
//!
//! `openai_dive`'s `models().list()` deserializes into `ListModelResponse`, whose
//! `object` field (and `Model::object` / `Model::owned_by`) are mandatory. Plenty of
//! OpenAI-compatible gateways omit them, which makes the whole listing fail with an
//! opaque `missing field \`object\`` even though the payload is perfectly usable.
//!
//! This module fetches the listing with the client's own http client and keeps only
//! what we actually need: the model ids.

use openai_dive::v1::{
    api::Client,
    resources::model::{ListModelResponse, Model},
};
use serde::Deserialize;
use serde_json::Value;

use crate::provider::LlmError;

/// The payload shape of `/models`: either the usual `{"data": [...]}` wrapper or,
/// for some gateways, a bare array of models.
#[derive(Deserialize)]
#[serde(untagged)]
enum ModelListPayload {
    Wrapped { data: Vec<Value> },
    Bare(Vec<Value>),
}

impl ModelListPayload {
    fn into_entries(self) -> Vec<Value> {
        match self {
            Self::Wrapped { data } => data,
            Self::Bare(data) => data,
        }
    }
}

/// List the models exposed by an OpenAI-compatible endpoint, tolerating missing
/// `object` / `owned_by` / `created` fields. Entries without an `id` are skipped.
pub async fn list_models_compat(client: &Client) -> Result<ListModelResponse, LlmError> {
    let url = format!("{}/models", client.base_url.trim_end_matches('/'));

    let mut request = client.http_client.get(&url);
    if !client.api_key.is_empty() {
        request = request.bearer_auth(&client.api_key);
    }
    if let Some(headers) = &client.headers {
        for (key, value) in headers {
            request = request.header(key, value);
        }
    }

    let response = request.send().await.map_err(|e| Box::new(e) as LlmError)?;

    let status = response.status();
    let body = response.text().await.map_err(|e| Box::new(e) as LlmError)?;

    if !status.is_success() {
        return Err(format!("models endpoint returned {}: {}", status, truncate(&body)).into());
    }

    parse_models(&body)
}

/// Parse a `/models` payload into the OpenAI representation, filling in defaults for
/// whatever the endpoint left out.
fn parse_models(body: &str) -> Result<ListModelResponse, LlmError> {
    let payload: ModelListPayload = serde_json::from_str(body)
        .map_err(|e| -> LlmError { format!("could not parse model list ({}): {}", e, truncate(body)).into() })?;

    let data = payload
        .into_entries()
        .into_iter()
        .filter_map(|entry| {
            let id = entry.get("id")?.as_str()?.to_string();
            Some(Model {
                id,
                object: entry
                    .get("object")
                    .and_then(Value::as_str)
                    .unwrap_or("model")
                    .to_string(),
                created: entry
                    .get("created")
                    .and_then(Value::as_i64)
                    .and_then(|c| u32::try_from(c).ok()),
                owned_by: entry
                    .get("owned_by")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            })
        })
        .collect();

    Ok(ListModelResponse {
        object: "list".to_string(),
        data,
    })
}

fn truncate(body: &str) -> String {
    const MAX: usize = 200;
    match body.char_indices().nth(MAX) {
        Some((idx, _)) => format!("{}...", &body[..idx]),
        None => body.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_listing_without_object_fields() {
        let body = r#"{"data":[
            {"id":"vendor/Model-A","created":1788183530,"owned_by":"someone"},
            {"id":"vendor/Model-B","name":"Model B"}
        ]}"#;

        let response = parse_models(body).expect("listing should parse");

        assert_eq!(response.object, "list");
        assert_eq!(response.data.len(), 2);
        assert_eq!(response.data[0].id, "vendor/Model-A");
        assert_eq!(response.data[0].object, "model");
        assert_eq!(response.data[0].created, Some(1788183530));
        assert_eq!(response.data[0].owned_by, "someone");
        // second entry has neither object, created nor owned_by
        assert_eq!(response.data[1].id, "vendor/Model-B");
        assert_eq!(response.data[1].object, "model");
        assert_eq!(response.data[1].created, None);
        assert_eq!(response.data[1].owned_by, "");
    }

    #[test]
    fn accepts_standard_openai_listing() {
        let body = r#"{"object":"list","data":[
            {"id":"gpt-4o","object":"model","created":1715367049,"owned_by":"system"}
        ]}"#;

        let response = parse_models(body).expect("listing should parse");

        assert_eq!(response.data.len(), 1);
        assert_eq!(response.data[0].id, "gpt-4o");
        assert_eq!(response.data[0].owned_by, "system");
    }

    #[test]
    fn accepts_bare_array_listing() {
        let body = r#"[{"id":"model-a"},{"id":"model-b"}]"#;

        let response = parse_models(body).expect("listing should parse");

        let ids: Vec<_> = response.data.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["model-a", "model-b"]);
    }

    #[test]
    fn skips_entries_without_id() {
        let body = r#"{"data":[{"id":"model-a"},{"name":"no id here"},{"id":42}]}"#;

        let response = parse_models(body).expect("listing should parse");

        assert_eq!(response.data.len(), 1);
        assert_eq!(response.data[0].id, "model-a");
    }

    #[test]
    fn reports_unparsable_payload() {
        let error = parse_models("not json at all").expect_err("should fail");
        assert!(error.to_string().contains("could not parse model list"));
    }
}
