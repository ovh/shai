use std::sync::Arc;

use openai_dive::v1::resources::chat::{
    ChatCompletionParametersBuilder, ChatCompletionResponseFormat, ChatMessage, ChatMessageContent,
    JsonSchemaBuilder,
};
use serde::{Deserialize, Serialize};
use shai_llm::{client::LlmClient, provider::LlmError};

use super::prompt::clifix_prompt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliFixResponse {
    pub short_rational: Option<String>,
    pub fixed_cli: String,
}

pub async fn clifix(
    llm: Arc<LlmClient>,
    model: String,
    messages: Vec<ChatMessage>,
) -> Result<CliFixResponse, LlmError> {
    let mut messages = messages.clone();
    messages.push(ChatMessage::System {
        content: ChatMessageContent::Text(clifix_prompt()),
        name: None,
    });

    let request = ChatCompletionParametersBuilder::default()
        .model(model.clone())
        .messages(messages)
        .temperature(0.1f32)
        .response_format(ChatCompletionResponseFormat::JsonSchema {
            json_schema: JsonSchemaBuilder::default()
                .name("cli_fix_response")
                .description("Response format for CLI fix with rationale and fixed command")
                .schema(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "short_rational": { "type": "string" },
                        "fixed_cli": { "type": "string" }
                    },
                    "required": ["fixed_cli"],
                    "additionalProperties": false
                }))
                .strict(true)
                .build()
                .map_err(|e| -> LlmError { e.into() })?,
        })
        .build()
        .map_err(|e| -> LlmError { e.into() })?;

    /*
    if let Ok(json) = serde_json::to_string_pretty(&request) {
        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        let filename = format!("request_{}.json", timestamp);
        let _ = std::fs::write(&filename, json);
    }
    */

    let response = llm.chat(request).await?;

    if let ChatMessage::Assistant {
        content: Some(ChatMessageContent::Text(content)),
        ..
    } = response.choices[0].message.clone()
    {
        let parsed: CliFixResponse = serde_json::from_str(&content).map_err(|e| -> LlmError {
            format!("Failed to parse CLI fix response: {}", e).into()
        })?;
        Ok(parsed)
    } else {
        Err("No content in response".into())
    }
}
