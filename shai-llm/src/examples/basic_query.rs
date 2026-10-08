// Basic query example showing simple chat completion with Mistral
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

    // Create a simple chat request
    let request = ChatCompletionParameters {
        model,
        messages: vec![ChatMessage::User {
            content: ChatMessageContent::Text("What is the capital of France?".to_string()),
            name: None,
        }],
        temperature: Some(0.7),
        max_tokens: Some(100),
        ..Default::default()
    };

    // Send the request and get response
    let response = client.chat(request).await?;

    // Print the response
    if let Some(choice) = response.choices.first() {
        match &choice.message {
            ChatMessage::Assistant {
                content: Some(ChatMessageContent::Text(text)),
                ..
            } => {
                println!("Response: {}", text);
            }
            _ => println!("No text response received"),
        }
    }

    Ok(())
}
