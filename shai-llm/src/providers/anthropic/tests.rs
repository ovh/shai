#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::anthropic::AnthropicProvider;
    use crate::provider::LlmProvider;
    use openai_dive::v1::resources::chat::{ChatMessage, ChatMessageContent, ChatCompletionParametersBuilder};
    use serde_json::json;

    fn setup_provider() -> AnthropicProvider {
        // Assume ANTHROPIC_API_KEY exists in environment
        AnthropicProvider::from_env().expect("ANTHROPIC_API_KEY must be set for tests")
    }

    #[tokio::test]
    async fn test_system_prompt_handling() {
        let provider = setup_provider();
        let default_model = provider.default_model().await.unwrap();
        
        // Create a request with system and user messages
        let request = ChatCompletionParametersBuilder::default()
            .model(default_model)
            .messages(vec![
                ChatMessage::System {
                    content: ChatMessageContent::Text("You are a helpful assistant.".to_string()),
                    name: None,
                },
                ChatMessage::User {
                    content: ChatMessageContent::Text("Hello!".to_string()),
                    name: None,
                },
            ])
            .build()
            .unwrap();

        let anthropic_format = provider.convert_to_anthropic_format(&request);
        
        // Check that system message is extracted to top-level system parameter
        assert!(anthropic_format.get("system").is_some());
        assert_eq!(anthropic_format["system"].as_str().unwrap(), "You are a helpful assistant.");
        
        // Check that messages array only contains non-system messages
        let messages = anthropic_format["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"].as_str().unwrap(), "user");
        assert_eq!(messages[0]["content"].as_str().unwrap(), "Hello!");
    }

    #[tokio::test]
    async fn test_multiple_system_messages() {
        let provider = setup_provider();
        let default_model = provider.default_model().await.unwrap();
        
        // Create a request with multiple system messages
        let request = ChatCompletionParametersBuilder::default()
            .model(default_model)
            .messages(vec![
                ChatMessage::System {
                    content: ChatMessageContent::Text("You are a helpful assistant.".to_string()),
                    name: None,
                },
                ChatMessage::System {
                    content: ChatMessageContent::Text("Always be concise.".to_string()),
                    name: None,
                },
                ChatMessage::User {
                    content: ChatMessageContent::Text("Hello!".to_string()),
                    name: None,
                },
            ])
            .build()
            .unwrap();

        let anthropic_format = provider.convert_to_anthropic_format(&request);
        
        // Check that system messages are combined
        assert!(anthropic_format.get("system").is_some());
        let system_content = anthropic_format["system"].as_str().unwrap();
        assert!(system_content.contains("You are a helpful assistant."));
        assert!(system_content.contains("Always be concise."));
        
        // Check that messages array only contains non-system messages
        let messages = anthropic_format["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"].as_str().unwrap(), "user");
    }

    #[tokio::test]
    async fn test_no_system_messages() {
        let provider = setup_provider();
        let default_model = provider.default_model().await.unwrap();
        
        // Create a request with no system messages
        let request = ChatCompletionParametersBuilder::default()
            .model(default_model)
            .messages(vec![
                ChatMessage::User {
                    content: ChatMessageContent::Text("Hello!".to_string()),
                    name: None,
                },
            ])
            .build()
            .unwrap();

        let anthropic_format = provider.convert_to_anthropic_format(&request);
        
        // Check that no system parameter is added when there are no system messages
        assert!(anthropic_format.get("system").is_none());
        
        // Check that messages array contains the user message
        let messages = anthropic_format["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"].as_str().unwrap(), "user");
    }

    #[tokio::test]
    async fn test_tool_result_conversion() {
        let provider = setup_provider();
        let default_model = provider.default_model().await.unwrap();
        
        // Create a request with a tool call exchange
        let request = ChatCompletionParametersBuilder::default()
            .model(default_model)
            .messages(vec![
                ChatMessage::User {
                    content: ChatMessageContent::Text("make me a hello world in main.py".to_string()),
                    name: None,
                },
                ChatMessage::Assistant {
                    content: None,
                    reasoning: None,
                    reasoning_content: None,
                    refusal: None,
                    name: None,
                    audio: None,
                    tool_calls: Some(vec![openai_dive::v1::resources::chat::ToolCall {
                        id: "toolu_018qHepKa8d4rbZ9qskd2vqw".to_string(),
                        r#type: "function".to_string(),
                        function: openai_dive::v1::resources::chat::Function {
                            name: "write".to_string(),
                            arguments: "{\"content\":\"print(\\\"Hello, World!\\\")\",\"path\":\"/Users/lloiseau/Work/test/main.py\"}".to_string(),
                        }
                    }])
                },
                ChatMessage::Tool {
                    content: ChatMessageContent::Text("Successfully updated file '/Users/lloiseau/Work/test/main.py' with 22 bytes".to_string()),
                    tool_call_id: "toolu_018qHepKa8d4rbZ9qskd2vqw".to_string(),
                },
            ])
            .build()
            .unwrap();

        let anthropic_format = provider.convert_to_anthropic_format(&request);
        
        // Check that messages array has the correct structure
        let messages = anthropic_format["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 3);
        
        // Check user message
        assert_eq!(messages[0]["role"].as_str().unwrap(), "user");
        assert_eq!(messages[0]["content"].as_str().unwrap(), "make me a hello world in main.py");
        
        // Check assistant message (should have tool_use content blocks)
        assert_eq!(messages[1]["role"].as_str().unwrap(), "assistant");
        let assistant_content = messages[1]["content"].as_array().unwrap();
        assert_eq!(assistant_content.len(), 1);
        assert_eq!(assistant_content[0]["type"].as_str().unwrap(), "tool_use");
        assert_eq!(assistant_content[0]["id"].as_str().unwrap(), "toolu_018qHepKa8d4rbZ9qskd2vqw");
        assert_eq!(assistant_content[0]["name"].as_str().unwrap(), "write");
        
        // Check tool result message
        assert_eq!(messages[2]["role"].as_str().unwrap(), "user");
        let tool_result_content = messages[2]["content"].as_array().unwrap();
        assert_eq!(tool_result_content.len(), 1);
        assert_eq!(tool_result_content[0]["type"].as_str().unwrap(), "tool_result");
        assert_eq!(tool_result_content[0]["tool_use_id"].as_str().unwrap(), "toolu_018qHepKa8d4rbZ9qskd2vqw");
        assert_eq!(tool_result_content[0]["content"].as_str().unwrap(), "Successfully updated file '/Users/lloiseau/Work/test/main.py' with 22 bytes");
    }
}