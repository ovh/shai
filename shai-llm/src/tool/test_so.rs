#[cfg(test)]
mod structured_output_integration_tests {
    use crate::tool::{AssistantResponse, StructuredOutputBuilder};
    use crate::{client::LlmClient, ToolDescription};
    use openai_dive::v1::resources::chat::ChatCompletionParametersBuilder;
    use openai_dive::v1::resources::chat::{ChatMessage, ChatMessageContent};
    use paste;
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};
    use std::sync::Arc;
    use tokio;

    // Simple read tool parameters
    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    pub struct ReadParams {
        pub path: String,
    }

    // Simple write tool parameters
    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    pub struct WriteParams {
        pub path: String,
        pub content: String,
    }

    // Simple read tool
    pub struct ReadTool;

    impl ToolDescription for ReadTool {
        fn name(&self) -> String {
            "read_file".to_string()
        }

        fn description(&self) -> String {
            "Read a file from the filesystem".to_string()
        }

        fn parameters_schema(&self) -> serde_json::Value {
            use schemars::schema_for;
            let schema = schema_for!(ReadParams);
            serde_json::to_value(schema).unwrap_or_default()
        }
    }

    // Simple write tool
    pub struct WriteTool;

    impl ToolDescription for WriteTool {
        fn name(&self) -> String {
            "write_file".to_string()
        }

        fn description(&self) -> String {
            "Write content to a file".to_string()
        }

        fn parameters_schema(&self) -> serde_json::Value {
            use schemars::schema_for;
            let schema = schema_for!(WriteParams);
            serde_json::to_value(schema).unwrap_or_default()
        }
    }

    fn create_test_tools() -> Vec<Arc<dyn ToolDescription>> {
        vec![Arc::new(ReadTool), Arc::new(WriteTool)]
    }

    fn create_llm_client(provider: &str, env_var: &str) -> Option<LlmClient> {
        if std::env::var(env_var).is_err() {
            return None;
        }

        match provider {
            "openai" => LlmClient::from_env_openai(),
            "anthropic" => LlmClient::from_env_anthropic(),
            "mistral" => LlmClient::from_env_mistral(),
            "ollama" => LlmClient::from_env_ollama(),
            _ => None,
        }
    }

    async fn test_no_tool_call(provider: &str, model: &str, env_var: &str) {
        let client = match create_llm_client(provider, env_var) {
            Some(client) => client,
            None => {
                println!("⏭️  Skipping {}: {} not set", provider, env_var);
                return;
            }
        };

        let tools = create_test_tools();

        let request = ChatCompletionParametersBuilder::default()
            .model(model)
            .messages(vec![ChatMessage::User {
                content: ChatMessageContent::Text(
                    "Just say hello, don't use any tools.".to_string(),
                ),
                name: None,
            }])
            .with_structured_output(&tools)
            .temperature(0.1f32)
            .max_completion_tokens(200u32)
            .build()
            .unwrap();

        let response = client.chat(request).await.unwrap();

        if let Some(choice) = response.choices.first() {
            if let ChatMessage::Assistant {
                content: Some(ChatMessageContent::Text(text)),
                ..
            } = &choice.message
            {
                let structured_response: AssistantResponse = serde_json::from_str(text).unwrap();

                assert!(!structured_response.content.is_empty());
                assert!(
                    structured_response.tools.is_none()
                        || structured_response.tools.as_ref().unwrap().is_empty()
                );

                println!("✅ {} no-tool test passed", provider);
            }
        }
    }

    async fn test_must_use_read_tool(provider: &str, model: &str, env_var: &str) {
        let client = match create_llm_client(provider, env_var) {
            Some(client) => client,
            None => {
                println!("⏭️  Skipping {}: {} not set", provider, env_var);
                return;
            }
        };

        let tools = create_test_tools();

        let request = ChatCompletionParametersBuilder::default()
            .model(model)
            .messages(vec![
                ChatMessage::User {
                    content: ChatMessageContent::Text(
                        "You MUST read the file 'example.txt' using the read_file tool. This is required.".to_string()
                    ),
                    name: None,
                }
            ])
            .with_structured_output(&tools)
            .temperature(0.1f32)
            .max_completion_tokens(500u32)
            .build()
            .unwrap();

        let response = client.chat(request).await.unwrap();

        if let Some(choice) = response.choices.first() {
            if let ChatMessage::Assistant {
                content: Some(ChatMessageContent::Text(text)),
                ..
            } = &choice.message
            {
                let structured_response: AssistantResponse = serde_json::from_str(text).unwrap();

                assert!(!structured_response.content.is_empty());
                assert!(
                    structured_response.tools.is_some(),
                    "Tools should be present"
                );

                let tools = structured_response.tools.unwrap();
                assert!(!tools.is_empty(), "At least one tool should be called");

                // Find the read_file tool call
                let read_tool_called = tools.iter().any(|tool| tool.tool_name == "read_file");
                assert!(read_tool_called, "read_file tool must be called");

                println!("✅ {} read-tool test passed", provider);
            }
        }
    }

    macro_rules! generate_structured_output_tests {
        ($($provider:ident: $model:expr, $env_var:expr);*) => {
            paste::paste! {
                $(
                    #[tokio::test]
                    async fn [<test_ $provider _no_tool_call>]() {
                        test_no_tool_call(stringify!($provider), $model, $env_var).await;
                    }

                    #[tokio::test]
                    async fn [<test_ $provider _must_use_read_tool>]() {
                        test_must_use_read_tool(stringify!($provider), $model, $env_var).await;
                    }
                )*
            }
        };
    }

    generate_structured_output_tests! {
        openai: "gpt-4o-mini", "OPENAI_API_KEY";
        anthropic: "claude-3-5-sonnet-20241022", "ANTHROPIC_API_KEY";
        mistral: "mistral-large-latest", "MISTRAL_API_KEY";
        ovhcloud: "Mistral-Nemo-Instruct-2407", "MISTRAL_API_KEY";
        ollama: "smollm2:latest", "OLLAMA_BASE_URL"
    }
}
