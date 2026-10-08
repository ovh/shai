// Streaming query example showing real-time response chunks
use futures::StreamExt;
use openai_dive::v1::resources::chat::DeltaChatMessage;
use openai_dive::v1::resources::chat::{ChatCompletionParameters, ChatMessage, ChatMessageContent};
use shai_llm::{client::LlmClient, provider::LlmError};

#[tokio::main]
async fn main() -> Result<(), LlmError> {
    // Initialize Mistral client from environment variable (MISTRAL_API_KEY)
    let client =
        LlmClient::from_env_mistral().expect("MISTRAL_API_KEY environment variable not set");

    // Get the default model
    let model = client.provider().default_model().await?;
    println!("Using model: {}", model);

    // Create a streaming chat request
    let request = ChatCompletionParameters {
        model,
        messages: vec![ChatMessage::User {
            content: ChatMessageContent::Text(
                "Write a short story about a robot learning to paint.".to_string(),
            ),
            name: None,
        }],
        temperature: Some(0.8),
        max_tokens: Some(300),
        stream: Some(true),
        ..Default::default()
    };

    // Send the streaming request
    let mut stream = client.chat_stream(request).await?;

    println!("Streaming response:");
    println!("==================");

    let mut full_response = String::new();

    // Process each chunk as it arrives
    while let Some(chunk_result) = stream.next().await {
        match chunk_result {
            Ok(chunk) => {
                // Extract content from the first choice if available
                if let Some(choice) = chunk.choices.first() {
                    match &choice.delta {
                        DeltaChatMessage::Assistant {
                            content: Some(ChatMessageContent::Text(text)),
                            ..
                        }
                        | DeltaChatMessage::Untagged {
                            content: Some(ChatMessageContent::Text(text)),
                            ..
                        } if !text.is_empty() => {
                            print!("{}", text);
                            full_response.push_str(text);
                            // Flush stdout to show text immediately
                            use std::io::{self, Write};
                            io::stdout().flush().unwrap();
                        }
                        _ => {
                            // Handle other delta types or empty content
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("\nError processing stream chunk: {:?}", e);
                break;
            }
        }
    }

    println!("\n\n==================");
    println!("Full response length: {} characters", full_response.len());

    Ok(())
}
