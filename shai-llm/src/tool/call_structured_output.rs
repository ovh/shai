use std::sync::Arc;
use serde::{Serialize, Deserialize};
use schemars::JsonSchema;
use serde_json::Value;
use async_trait::async_trait;
use openai_dive::v1::resources::chat::{
    ChatCompletionParameters, ChatCompletionParametersBuilder, ChatCompletionResponse, ChatCompletionResponseFormat, JsonSchemaBuilder,
    ChatMessage, ChatMessageContent, Function, ToolCall as LlmToolCall
};
use crate::provider::LlmError;
use crate::tool::ToolBox;
use crate::LlmClient;

/// Tool call structure for structured output JSON schema
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ToolCall {
    pub tool_name: String,
    pub tool_parameter: Value,
}

/// Response structure for structured output JSON schema
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct AssistantResponse {
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolCall>>,
}


/// Utility functions for structured output with tools
pub trait StructuredOutputBuilder {
    fn with_structured_output(&mut self, tools: &ToolBox) -> &mut Self;
}

impl StructuredOutputBuilder for ChatCompletionParametersBuilder {
    /// Add structured output response format to a ChatCompletionParametersBuilder
    /// This enforces the model to respond with the AssistantResponse schema including
    /// tool-specific parameter validation
    fn with_structured_output(
        &mut self, 
        tools: &ToolBox
    ) -> &mut ChatCompletionParametersBuilder {
        // Generate base schema from the struct
        let base_schema = schemars::schema_for!(AssistantResponse);
        let mut schema_value = serde_json::to_value(base_schema).unwrap();

        // Dynamically build the tools schema with specific parameter schemas for each tool
        if !tools.is_empty() {
            let tool_schemas: Vec<Value> = tools.iter().map(|tool| {
                let mut param_schema = tool.parameters_schema();
                
                // Ensure the parameter schema has additionalProperties: false
                if let Some(param_obj) = param_schema.as_object_mut() {
                    param_obj.insert("additionalProperties".to_string(), serde_json::Value::Bool(false));
                }
                
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "tool_name": { 
                            "type": "string",
                            "const": tool.name() 
                        },
                        "tool_parameter": param_schema
                    },
                    "required": ["tool_name", "tool_parameter"],
                    "additionalProperties": false
                })
            }).collect();

            // Update the schema with the specific tools definition
            if let Some(properties) = schema_value["properties"].as_object_mut() {
                if let Some(tools_prop) = properties.get_mut("tools") {
                    // Replace the entire tools property definition
                    *tools_prop = serde_json::json!({
                        "type": ["array", "null"],
                        "items": {
                            "oneOf": tool_schemas
                        }
                    });
                }
            }
        }

        // Ensure additionalProperties is false for the root schema
        if let Some(schema_obj) = schema_value.as_object_mut() {
            schema_obj.insert("additionalProperties".to_string(), serde_json::Value::Bool(false));
        }

        let json_schema = JsonSchemaBuilder::default()
            .name("assistant_response")
            .schema(schema_value)
            .strict(true)
            .build()
            .unwrap();

        self.response_format(ChatCompletionResponseFormat::JsonSchema {
            json_schema,
        })
    }
}


#[async_trait]
pub trait ToolCallStructuredOutput {
    async fn chat_with_tools_so(
        &self,
        request: ChatCompletionParameters,
        tools: &ToolBox
    ) -> Result<ChatCompletionResponse, LlmError>;
}

#[async_trait]
impl ToolCallStructuredOutput for LlmClient {
    async fn chat_with_tools_so(
        &self,
        request: ChatCompletionParameters,
        tools: &ToolBox
    ) -> Result<ChatCompletionResponse, LlmError> {
        // Generate tool documentation to prepend to system message
        let tools_doc = if !tools.is_empty() {
            let mut doc = String::from("\n\n# Available Tools\n\nYou have access to the following tools:\n\n");
            
            for tool in tools {
                doc.push_str(&format!("## {}\n", tool.name()));
                doc.push_str(&format!("**Description**: {}\n\n", tool.description()));
                doc.push_str("**Parameters Schema**:\n```json\n");
                doc.push_str(&serde_json::to_string_pretty(&tool.parameters_schema()).unwrap_or_default());
                doc.push_str("\n```\n\n");
            }
            doc
        } else {
            String::new()
        };

        // Prepend tools documentation to the first system message
        let mut messages = request.messages.clone();
        if let Some(ChatMessage::System { content: ChatMessageContent::Text(ref mut system_text), .. }) = messages.get_mut(0) {
            *system_text = format!("{}{}", system_text, tools_doc);
        }

        let request = ChatCompletionParametersBuilder::default()
            .model(&request.model)
            .messages(request.messages)
            .temperature(0.3)
            .with_structured_output(&tools)
            .build()
            .map_err(|e| LlmError::from(e.to_string()))?;

        let mut response = self
            .chat(request.clone())
            .await
            .map_err(|e| LlmError::from(e.to_string()))?;
        
        // Parse the structured output
        let structured_response: AssistantResponse = match &response.choices[0].message {
            ChatMessage::Assistant { content: Some(ChatMessageContent::Text(text)), .. } => {
                serde_json::from_str(text)
                    .map_err(|e| LlmError::from(format!("Failed to parse structured response: {}", e)))?
            }
            _ => return Err("Expected Assistant message with text content".into()),
        };

        response.choices[0].message = structured_response.into_chatmessage();
        Ok(response)
    }
}



pub trait IntoChatMessage {
    /// Convert a structured AssistantResponse back to a ChatMessage with tool calls
    fn into_chatmessage(self) -> ChatMessage;
}

impl IntoChatMessage for AssistantResponse {
    fn into_chatmessage(self) -> ChatMessage {

        // Convert tools to OpenAI tool calls format
        let tool_calls = self.tools.map(|tools| {
            tools.into_iter().map(|tool| {
                // Generate random 9-letter ID
                let random_id: String = (0..9)
                    .map(|_| {
                        let chars = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
                        chars[fastrand::usize(..chars.len())] as char
                    })
                    .collect();
                
                LlmToolCall {
                    id: format!("call_{}", random_id),
                    r#type: "function".to_string(),
                    function: Function {
                        name: tool.tool_name,
                        arguments: tool.tool_parameter.to_string(),
                    },
                }
            }).collect()
        });

        ChatMessage::Assistant {
            content: Some(ChatMessageContent::Text(self.content)),
            reasoning: None,
            reasoning_content: self.reasoning_content,
            tool_calls,
            refusal: None,
            name: None,
            audio: None,
        }
    }
}