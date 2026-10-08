// Function calling with streaming - expected to fail but let's see what happens!
use futures::StreamExt;
use openai_dive::v1::resources::chat::{
    ChatCompletionFunction, ChatCompletionParameters, ChatCompletionTool, ChatCompletionToolType,
    ChatMessage, ChatMessageContent,
};
use openai_dive::v1::resources::chat::{
    ChatCompletionParametersBuilder, ChatCompletionToolChoice, DeltaChatMessage,
};
use serde_json::json;
use shai_llm::{client::LlmClient, provider::LlmError};

fn print_curl_command(request: &ChatCompletionParameters, api_key_var: &str, base_url: &str) {
    let json_body = serde_json::to_string_pretty(request).unwrap_or_else(|_| "{}".to_string());
    // Escape single quotes by replacing ' with '\''
    let escaped_json = json_body.replace("'", "'\\''");

    println!("Equivalent curl command:");
    println!("=======================");
    println!("curl -X POST {}/chat/completions \\", base_url);
    println!("  -H \"Content-Type: application/json\" \\");
    println!("  -H \"Authorization: Bearer ${}\" \\", api_key_var);
    println!("  -d '{}'", escaped_json);
    println!("=======================");
    println!();
}

#[tokio::main]
async fn main() -> Result<(), LlmError> {
    // Initialize client from environment variable
    let client = LlmClient::from_env_ovhcloud()
        .or_else(LlmClient::from_env_ovhcloud)
        .expect("environment variable not set");

    // Get available models and select one that supports function calling
    let models = client.provider().models().await?;
    let model = models
        .data
        .iter()
        .find(|m| m.id.contains("Llama-3_3") || m.id.contains("llama-3-3"))
        .or_else(|| models.data.iter().find(|m| m.id.contains("tiny")))
        .or_else(|| models.data.iter().find(|m| m.id.contains("Llama")))
        .map(|m| m.id.clone())
        .unwrap_or_else(|| models.data[0].id.clone());
    println!("Using model: {}", model);

    // Create messages
    let messages = vec![ChatMessage::User {
        content: ChatMessageContent::Text("What's the weather like in Tokyo, Japan? Please use the weather function to get real-time data.".to_string()),
        name: None,
    }];

    // Create a request that should trigger function calling
    let request = ChatCompletionParametersBuilder::default()
        .model(model)
        .messages(messages)
        .tools(vec![ChatCompletionTool {
            r#type: ChatCompletionToolType::Function,
            function: ChatCompletionFunction {
                name: "get_weather".to_string(),
                description: Some(
                    "Get current weather information for a given location".to_string(),
                ),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "location": {
                            "type": "string",
                            "description": "The city and country, e.g. Tokyo, Japan"
                        },
                        "unit": {
                            "type": "string",
                            "enum": ["celsius", "fahrenheit"],
                            "description": "The unit of temperature"
                        }
                    },
                    "required": ["location"]
                }),
            },
        }])
        .temperature(0.1f32)
        .max_completion_tokens(200u32)
        .stream(true) // Enable streaming!
        .build()
        .map_err(|e| format!("Failed to build parameters: {:?}", e))?;

    // Print equivalent curl command
    let provider_name = client.provider().name();
    let (api_key_var, base_url) = match provider_name {
        "mistral" => ("MISTRAL_API_KEY", "https://api.mistral.ai/v1"),
        "ovhcloud" => (
            "OVH_API_KEY",
            "https://oai.endpoints.kepler.ai.cloud.ovh.net/v1",
        ),
        _ => ("API_KEY", "https://api.unknown.com/v1"),
    };
    print_curl_command(&request, api_key_var, base_url);

    // Send the streaming request
    println!("Sending streaming request with tools...");
    let mut stream = client.chat_stream(request).await?;

    println!("Streaming response:");
    println!("==================");

    let mut tool_calls_chunks = Vec::new();
    let mut content_chunks = Vec::new();
    let mut chunk_count = 0;

    // Process each chunk as it arrives
    while let Some(chunk_result) = stream.next().await {
        match chunk_result {
            Ok(chunk) => {
                chunk_count += 1;
                println!("Chunk {}: {:?}", chunk_count, chunk);

                // Extract content from the first choice if available
                if let Some(choice) = chunk.choices.first() {
                    match &choice.delta {
                        DeltaChatMessage::Assistant {
                            content: Some(ChatMessageContent::Text(text)),
                            ..
                        } => {
                            if !text.is_empty() {
                                print!("{}", text);
                                content_chunks.push(text.clone());
                                use std::io::{self, Write};
                                io::stdout().flush().unwrap();
                            }
                        }
                        DeltaChatMessage::Assistant {
                            tool_calls: Some(tool_calls),
                            ..
                        } => {
                            println!(
                                "\n🔧 Tool calls chunk received (Assistant): {:?}",
                                tool_calls
                            );
                            tool_calls_chunks.extend(tool_calls.clone());
                        }
                        DeltaChatMessage::Untagged {
                            tool_calls: Some(tool_calls),
                            ..
                        } => {
                            println!(
                                "\n🔧 Tool calls chunk received (Untagged): {:?}",
                                tool_calls
                            );
                            tool_calls_chunks.extend(tool_calls.clone());
                        }
                        _ => {
                            // Other delta types
                            if chunk_count <= 5 {
                                // Only print first few chunks to avoid spam
                                println!("\nOther delta: {:?}", choice.delta);
                            }
                        }
                    }
                }

                // Check finish reason
                if let Some(choice) = chunk.choices.first() {
                    if let Some(finish_reason) = &choice.finish_reason {
                        println!("\nFinish reason: {:?}", finish_reason);
                        break;
                    }
                }

                // Limit chunks to prevent infinite loops
                if chunk_count > 100 {
                    println!("\nStopping after 100 chunks to prevent infinite loop");
                    break;
                }
            }
            Err(e) => {
                eprintln!("\nError processing stream chunk: {:?}", e);
                break;
            }
        }
    }

    println!("\n\n==================");
    println!("Summary:");
    println!("- Total chunks: {}", chunk_count);
    println!("- Content chunks: {}", content_chunks.len());
    println!("- Tool call chunks: {}", tool_calls_chunks.len());

    if !content_chunks.is_empty() {
        let full_content = content_chunks.join("");
        println!("- Full content: {}", full_content);
    }

    if !tool_calls_chunks.is_empty() {
        println!("- Tool calls received:");
        for (i, tool_call) in tool_calls_chunks.iter().enumerate() {
            println!("  {}. {:?}", i + 1, tool_call);
        }
    }

    if tool_calls_chunks.is_empty() && content_chunks.is_empty() {
        println!("- No tool calls or content received (this might be the expected failure!)");
    }

    Ok(())
}
