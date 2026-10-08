// Query with conversation history example
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

    // Start conversation with system message and initial user message
    let mut conversation = vec![
        ChatMessage::System {
            content: ChatMessageContent::Text(
                "You are a helpful assistant that provides concise answers.".to_string(),
            ),
            name: None,
        },
        ChatMessage::User {
            content: ChatMessageContent::Text("What is the capital of France?".to_string()),
            name: None,
        },
    ];

    // First request
    let request = ChatCompletionParameters {
        model: model.clone(),
        messages: conversation.clone(),
        temperature: Some(0.7),
        max_tokens: Some(100),
        ..Default::default()
    };

    let response = client.chat(request).await?;

    // Add assistant's response to conversation history
    if let Some(choice) = response.choices.first() {
        println!("Q: What is the capital of France?");
        match &choice.message {
            ChatMessage::Assistant {
                content: Some(ChatMessageContent::Text(text)),
                ..
            } => {
                println!("A: {}", text);
                conversation.push(choice.message.clone());
            }
            _ => println!("No text response received"),
        }
    }

    // Follow-up question using conversation history
    conversation.push(ChatMessage::User {
        content: ChatMessageContent::Text("What is the population of that city?".to_string()),
        name: None,
    });

    let follow_up_request = ChatCompletionParameters {
        model,
        messages: conversation,
        temperature: Some(0.7),
        max_tokens: Some(100),
        ..Default::default()
    };

    let follow_up_response = client.chat(follow_up_request).await?;

    // Print follow-up response
    if let Some(choice) = follow_up_response.choices.first() {
        println!("\nQ: What is the population of that city?");
        match &choice.message {
            ChatMessage::Assistant {
                content: Some(ChatMessageContent::Text(text)),
                ..
            } => {
                println!("A: {}", text);
            }
            _ => println!("No text response received"),
        }
    }

    Ok(())
}
