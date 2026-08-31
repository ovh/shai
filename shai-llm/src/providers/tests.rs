use crate::provider::{LlmProvider, LlmError};
use openai_dive::v1::resources::{
    chat::{ChatCompletionFunction, ChatCompletionParametersBuilder, ChatCompletionTool, ChatCompletionToolChoice, ChatCompletionToolType, ChatMessage, ChatMessageContent, DeltaChatMessage, ChatCompletionResponseFormat, JsonSchemaBuilder},
    model::ListModelResponse,
};
use futures::StreamExt;
use serde_json::json;

/// Test function calling with boolean parameters to detect model-specific JSON issues
pub async fn test_provider_function_calling_boolean_params(provider: Box<dyn LlmProvider>) {
    let model_id = match provider.default_model().await {
        Ok(m) => m,
        Err(e) => {
            println!("Skipping {} function calling test: cannot get default model: {:?}", provider.name(), e);
            return;
        }
    };
    
    println!("trying with model: {}", model_id.clone());
    let request = ChatCompletionParametersBuilder::default()
        .model(model_id.to_string())
        .messages(vec![
            ChatMessage::User {
                content: ChatMessageContent::Text("I need to read the file 'main.py' with line numbers shown. You MUST use the read_file function.".to_string()),
                name: None,
            }
        ])
        .tools(vec![ChatCompletionTool {
            r#type: ChatCompletionToolType::Function,
            function: ChatCompletionFunction {
                name: "read_file".to_string(),
                description: Some("Read a file from the filesystem".to_string()),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Path to the file to read"
                        },
                        "line_start": {
                            "type": "integer",
                            "description": "Starting line number (optional)"
                        },
                        "line_end": {
                            "type": "integer", 
                            "description": "Ending line number (optional)"
                        },
                        "show_line_numbers": {
                            "type": "boolean",
                            "description": "Whether to include line numbers in the output"
                        }
                    },
                    "required": ["path"]
                }),
            },
        }])
        .tool_choice(ChatCompletionToolChoice::Auto)
        .temperature(0.1)
        .max_completion_tokens(200u32)
        .build()
        .expect("Failed to build ChatCompletionParameters");
    
    let result = provider.chat(request).await;
    
    match result {
        Ok(response) => {
            if let Some(choice) = response.choices.first() {
                if let ChatMessage::Assistant { content, tool_calls: Some(tool_calls), .. } = &choice.message {
                    for tool_call in tool_calls {
                        if tool_call.function.name == "read_file" {
                            println!("🔍 {} function call arguments: {}", provider.name(), tool_call.function.arguments);
                            
                            // Try to parse the arguments to detect boolean type issues
                            match serde_json::from_str::<serde_json::Value>(&tool_call.function.arguments) {
                                Ok(args) => {
                                    if let Some(show_line_numbers) = args.get("show_line_numbers") {
                                        match show_line_numbers {
                                            serde_json::Value::Bool(b) => {
                                                println!("✓ {} correctly generated boolean: {}", provider.name(), b);
                                            }
                                            serde_json::Value::String(s) => {
                                                println!("⚠️  {} generated string boolean: '{}' (this will cause deserialization errors!)", provider.name(), s);
                                                
                                                // Test if this would fail in ReadToolParams
                                                let test_params = json!({
                                                    "path": "main.py",
                                                    "show_line_numbers": s
                                                });
                                                
                                                // This simulates what happens in your Read tool
                                                #[derive(serde::Deserialize)]
                                                struct TestParams {
                                                    path: String,
                                                    show_line_numbers: bool,
                                                }
                                                
                                                match serde_json::from_value::<TestParams>(test_params) {
                                                    Ok(_) => println!("  → Surprisingly, deserialization worked anyway!"),
                                                    Err(e) => println!("  → Deserialization error: {}", e),
                                                }
                                            }
                                            _ => {
                                                println!("❓ {} generated unexpected type for boolean: {:?}", provider.name(), show_line_numbers);
                                            }
                                        }
                                    } else {
                                        println!("ℹ️  {} didn't include show_line_numbers parameter", provider.name());
                                    }
                                }
                                Err(e) => {
                                    println!("❌ {} generated invalid JSON: {}", provider.name(), e);
                                }
                            }
                        }
                    }
                    println!("✓ {} function calling test completed", provider.name());
                } else {
                    println!("ℹ️ didn't make function calls (might not support it or gave direct response): {:?}", choice);
                }
            }
        }
        Err(e) => {
            println!("❌ {} function calling failed: {:?}", provider.name(), e);
        }
    }
}

/// Generic test functions that work with any LlmProvider
pub async fn test_provider_basic_functionality(provider: Box<dyn LlmProvider>) {
    // Test basic trait methods
    let name = provider.name();
    assert!(!name.is_empty(), "Provider name should not be empty");
    println!("✓ {} basic functionality test passed", name);
}

pub async fn test_provider_models_endpoint(provider: Box<dyn LlmProvider>) {
    let result = provider.models().await;
    
    match result {
        Ok(models_response) => {
            assert_eq!(models_response.object, "list");
            println!("✓ {} models endpoint test passed with {} models", provider.name(), models_response.data.len());
        }
        Err(e) => {
            panic!("{} models endpoint failed: {:?}", provider.name(), e);
        }
    }
}

pub async fn test_provider_chat_completion(provider: Box<dyn LlmProvider>) {
    let model_id = provider.default_model().await;
    let model_id = match model_id {
        Ok(m) => m,
        Err(e) => {
            panic!("Cannot test chat stream: models endpoint failed: {:?}", e);
        }
    };
    
    let request = ChatCompletionParametersBuilder::default()
        .model(model_id.to_string())
        .messages(vec![
            ChatMessage::User {
                content: ChatMessageContent::Text("Say 'test successful' exactly".to_string()),
                name: None,
            }
        ])
        .temperature(0.1)
        .max_completion_tokens(10u32)
        .build()
        .expect("Failed to build ChatCompletionParameters");
    
    let result = provider.chat(request).await;
    
    match result {
        Ok(response) => {
            assert!(!response.choices.is_empty(), "Should have at least one choice");
            match &response.choices[0].message {
                ChatMessage::Assistant { content: Some(ChatMessageContent::Text(text)), .. } => {
                    assert!(!text.is_empty(), "Should have content");
                    println!("✓ {} chat completion test passed", provider.name());
                }
                _ => {
                    println!("✓ {} chat completion test passed (non-text response)", provider.name());
                }
            }
        }
        Err(e) => {
            panic!("{} chat completion failed: {:?}", provider.name(), e);
        }
    }
}

pub async fn test_provider_chat_stream(provider: Box<dyn LlmProvider>) {
    let model_id = provider.default_model().await;
    let model_id = match model_id {
        Ok(m) => m,
        Err(e) => {
            panic!("Cannot test chat stream: models endpoint failed: {:?}", e);
        }
    };

    let request = ChatCompletionParametersBuilder::default()
        .model(model_id.to_string())
        .messages(vec![
            ChatMessage::User {
                content: ChatMessageContent::Text("Count from 1 to 3".to_string()),
                name: None,
            }
        ])
        .temperature(0.1)
        .max_completion_tokens(20u32)
        .stream(true)
        .build()
        .expect("Failed to build ChatCompletionParameters");
    
    println!("request: {:?}", request);
    let stream_result = provider.chat_stream(request).await;
    
    match stream_result {
        Ok(mut stream) => {
            let mut chunk_count = 0;
            let mut total_content = String::new();
            
            while let Some(chunk_result) = stream.next().await {
                match chunk_result {
                    Ok(chunk) => {
                        if !chunk.choices.is_empty() {
                            match &chunk.choices[0].delta {
                                DeltaChatMessage::Assistant { content: Some(ChatMessageContent::Text(text)), .. } |
                                DeltaChatMessage::Untagged { content: Some(ChatMessageContent::Text(text)), .. } => {
                                    if !text.is_empty() {
                                        total_content.push_str(text);
                                        chunk_count += 1;
                                    }
                                }
                                _ => {
                                    // Other delta types or empty content
                                }
                            }
                        }
                        
                        // Limit test to prevent infinite loops
                        if chunk_count > 50 {
                            break;
                        }
                    }
                    Err(e) => {
                        panic!("{} chat stream chunk failed: {:?}", provider.name(), e);
                    }
                }
            }
            
            // Some providers might not send content chunks, so we'll be lenient
            println!("✓ {} chat stream test passed ({} chunks, content: '{}')", 
                    provider.name(), chunk_count, total_content.trim());
        }
        Err(e) => {
            panic!("{} chat stream failed: {:?}", provider.name(), e);
        }
    }
}

/// Provider creation function that uses provider from_env() methods directly
pub fn create_provider_from_env(provider_name: &str) -> Option<Box<dyn LlmProvider>> {
    match provider_name {
        "openai" => crate::providers::openai::OpenAIProvider::from_env().map(|p| Box::new(p) as Box<dyn LlmProvider>),
        "anthropic" => crate::providers::anthropic::AnthropicProvider::from_env().map(|p| Box::new(p) as Box<dyn LlmProvider>),
        "ollama" => crate::providers::ollama::OllamaProvider::from_env().map(|p| Box::new(p) as Box<dyn LlmProvider>),
        "openrouter" => crate::providers::openrouter::OpenRouterProvider::from_env().map(|p| Box::new(p) as Box<dyn LlmProvider>),
        "openai_compatible" => crate::providers::openai_compatible::OpenAICompatibleProvider::from_env().map(|p| Box::new(p) as Box<dyn LlmProvider>),
        "ovhcloud" => crate::providers::ovhcloud::OvhCloudProvider::from_env().map(|p| Box::new(p) as Box<dyn LlmProvider>),
        "mistral" => crate::providers::mistral::MistralProvider::from_env().map(|p| Box::new(p) as Box<dyn LlmProvider>),
        _ => None,
    }
}

/// Macro to generate tests for all providers
macro_rules! register_providers_for_testing {
    ($($provider_name:ident),*) => {
        paste::paste! {
            $(
                #[tokio::test]
                async fn [<test_ $provider_name _basic_functionality>]() {
                    if let Some(provider) = create_provider_from_env(stringify!($provider_name)) {
                        test_provider_basic_functionality(provider).await;
                    } else {
                        println!("Skipping {} basic functionality test: required environment variables not set", stringify!($provider_name));
                    }
                }

                #[tokio::test]
                async fn [<test_ $provider_name _models_endpoint>]() {
                    if let Some(provider) = create_provider_from_env(stringify!($provider_name)) {
                        test_provider_models_endpoint(provider).await;
                    } else {
                        println!("Skipping {} models endpoint test: required environment variables not set", stringify!($provider_name));
                    }
                }

                #[tokio::test]
                async fn [<test_ $provider_name _chat_completion>]() {
                    if let Some(provider) = create_provider_from_env(stringify!($provider_name)) {
                        test_provider_chat_completion(provider).await;
                    } else {
                        println!("Skipping {} chat completion test: required environment variables not set", stringify!($provider_name));
                    }
                }

                #[tokio::test]
                async fn [<test_ $provider_name _chat_stream>]() {
                    if let Some(provider) = create_provider_from_env(stringify!($provider_name)) {
                        test_provider_chat_stream(provider).await;
                    } else {
                        println!("Skipping {} chat stream test: required environment variables not set", stringify!($provider_name));
                    }
                }

                #[tokio::test]
                async fn [<test_ $provider_name _function_calling_boolean_params>]() {
                    if let Some(provider) = create_provider_from_env(stringify!($provider_name)) {
                        test_provider_function_calling_boolean_params(provider).await;
                    } else {
                        println!("Skipping {} function calling boolean test: required environment variables not set", stringify!($provider_name));
                    }
                }
            )*
        }
    };
}

// Register all providers for testing
register_providers_for_testing!(
    openai,
    anthropic,
    ollama,
    openrouter,
    openai_compatible,
    ovhcloud,
    mistral
);

/// Additional integration tests
#[cfg(test)]
mod integration_tests {
    use super::*;

    #[tokio::test]
    async fn test_anthropic_hardcoded_models() {
        // Anthropic should always work since it returns hardcoded models
        if let Some(provider) = create_provider_from_env("anthropic") {
            let result = provider.models().await;
            assert!(result.is_ok(), "Anthropic models should always work (hardcoded)");
            
            let models = result.unwrap();
            assert!(!models.data.is_empty(), "Anthropic should have hardcoded models");
            assert!(models.data.iter().any(|m| m.id.contains("claude")), "Should contain Claude models");
            
            println!("✓ Anthropic hardcoded models test passed with {} models", models.data.len());
        } else {
            println!("Skipping Anthropic hardcoded models test: ANTHROPIC_API_KEY not set");
        }
    }

    #[tokio::test]
    async fn test_anthropic_tool_call_conversation() {
        if let Some(provider) = create_provider_from_env("anthropic") {
            let model_id = match provider.default_model().await {
                Ok(m) => m,
                Err(e) => {
                    println!("Skipping Anthropic tool call conversation test: cannot get default model: {:?}", e);
                    return;
                }
            };

            // Create a conversation with tool call and tool result
            let request = ChatCompletionParametersBuilder::default()
                .model(model_id.to_string())
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
                                name: "write_file".to_string(),
                                arguments: "{\"content\":\"print(\\\"Hello, World!\\\")\",\"path\":\"main.py\"}".to_string(),
                            }
                        }])
                    },
                    ChatMessage::Tool {
                        content: ChatMessageContent::Text("Successfully updated file 'main.py' with 22 bytes".to_string()),
                        tool_call_id: "toolu_018qHepKa8d4rbZ9qskd2vqw".to_string(),
                    },
                ])
                .tools(vec![openai_dive::v1::resources::chat::ChatCompletionTool {
                    r#type: openai_dive::v1::resources::chat::ChatCompletionToolType::Function,
                    function: openai_dive::v1::resources::chat::ChatCompletionFunction {
                        name: "write_file".to_string(),
                        description: Some("Write content to a file".to_string()),
                        parameters: json!({
                            "type": "object",
                            "properties": {
                                "path": {
                                    "type": "string",
                                    "description": "Path to the file to write"
                                },
                                "content": {
                                    "type": "string",
                                    "description": "Content to write to the file"
                                }
                            },
                            "required": ["path", "content"]
                        }),
                    },
                }])
                .temperature(0.1)
                .max_completion_tokens(100u32)
                .build()
                .expect("Failed to build ChatCompletionParameters");

            let result = provider.chat(request).await;
            
            match result {
                Ok(response) => {
                    println!("✓ Anthropic tool call conversation test passed");
                    if let Some(choice) = response.choices.first() {
                        match &choice.message {
                            ChatMessage::Assistant { content, .. } => {
                                println!("Response: {:?}", content);
                            }
                            _ => {
                                println!("Unexpected response type: {:?}", choice.message);
                            }
                        }
                    }
                }
                Err(e) => {
                    panic!("Anthropic tool call conversation failed: {:?}", e);
                }
            }
        } else {
            println!("Skipping Anthropic tool call conversation test: ANTHROPIC_API_KEY not set");
        }
    }

    #[tokio::test]
    async fn test_openai_multi_turn_tool_conversation() {
        if let Some(provider) = create_provider_from_env("openai") {
            let model_id = match provider.default_model().await {
                Ok(m) => m,
                Err(e) => {
                    println!("Skipping OpenAI multi-turn tool conversation test: cannot get default model: {:?}", e);
                    return;
                }
            };

            // Create a multi-turn conversation with tool calls
            let request = ChatCompletionParametersBuilder::default()
                .model(model_id.to_string())
                .messages(vec![
                    ChatMessage::User {
                        content: ChatMessageContent::Text("Create a file called hello.py with a hello world function".to_string()),
                        name: None,
                    },
                    ChatMessage::Assistant {
                        content: Some(ChatMessageContent::Text("I'll create a hello.py file with a hello world function for you.".to_string())),
                        reasoning: None,
                        reasoning_content: None,
                        refusal: None,
                        name: None,
                        audio: None,
                        tool_calls: Some(vec![openai_dive::v1::resources::chat::ToolCall {
                            id: "call_1".to_string(),
                            r#type: "function".to_string(),
                            function: openai_dive::v1::resources::chat::Function {
                                name: "write_file".to_string(),
                                arguments: "{\"path\":\"hello.py\",\"content\":\"def hello_world():\\n    print(\\\"Hello, World!\\\")\\n\\nif __name__ == \\\"__main__\\\":\\n    hello_world()\"}".to_string(),
                            }
                        }])
                    },
                    ChatMessage::Tool {
                        content: ChatMessageContent::Text("File hello.py created successfully with 73 bytes".to_string()),
                        tool_call_id: "call_1".to_string(),
                    },
                    ChatMessage::User {
                        content: ChatMessageContent::Text("Now read the file to verify its contents".to_string()),
                        name: None,
                    },
                    ChatMessage::Assistant {
                        content: Some(ChatMessageContent::Text("I'll read the hello.py file to verify its contents.".to_string())),
                        reasoning: None,
                        reasoning_content: None,
                        refusal: None,
                        name: None,
                        audio: None,
                        tool_calls: Some(vec![openai_dive::v1::resources::chat::ToolCall {
                            id: "call_2".to_string(),
                            r#type: "function".to_string(),
                            function: openai_dive::v1::resources::chat::Function {
                                name: "read_file".to_string(),
                                arguments: "{\"path\":\"hello.py\"}".to_string(),
                            }
                        }])
                    },
                    ChatMessage::Tool {
                        content: ChatMessageContent::Text("def hello_world():\n    print(\"Hello, World!\")\n\nif __name__ == \"__main__\":\n    hello_world()".to_string()),
                        tool_call_id: "call_2".to_string(),
                    },
                ])
                .tools(vec![
                    openai_dive::v1::resources::chat::ChatCompletionTool {
                        r#type: openai_dive::v1::resources::chat::ChatCompletionToolType::Function,
                        function: openai_dive::v1::resources::chat::ChatCompletionFunction {
                            name: "write_file".to_string(),
                            description: Some("Write content to a file".to_string()),
                            parameters: json!({
                                "type": "object",
                                "properties": {
                                    "path": {
                                        "type": "string",
                                        "description": "Path to the file to write"
                                    },
                                    "content": {
                                        "type": "string",
                                        "description": "Content to write to the file"
                                    }
                                },
                                "required": ["path", "content"]
                            }),
                        },
                    },
                    openai_dive::v1::resources::chat::ChatCompletionTool {
                        r#type: openai_dive::v1::resources::chat::ChatCompletionToolType::Function,
                        function: openai_dive::v1::resources::chat::ChatCompletionFunction {
                            name: "read_file".to_string(),
                            description: Some("Read content from a file".to_string()),
                            parameters: json!({
                                "type": "object",
                                "properties": {
                                    "path": {
                                        "type": "string",
                                        "description": "Path to the file to read"
                                    }
                                },
                                "required": ["path"]
                            }),
                        },
                    }
                ])
                .temperature(0.1)
                .max_completion_tokens(200u32)
                .build()
                .expect("Failed to build ChatCompletionParameters");

            let result = provider.chat(request).await;
            
            match result {
                Ok(response) => {
                    println!("✓ OpenAI multi-turn tool conversation test passed");
                    if let Some(choice) = response.choices.first() {
                        match &choice.message {
                            ChatMessage::Assistant { content, .. } => {
                                println!("Final response: {:?}", content);
                            }
                            _ => {
                                println!("Unexpected response type: {:?}", choice.message);
                            }
                        }
                    }
                }
                Err(e) => {
                    panic!("OpenAI multi-turn tool conversation failed: {:?}", e);
                }
            }
        } else {
            println!("Skipping OpenAI multi-turn tool conversation test: OPENAI_API_KEY not set");
        }
    }

}