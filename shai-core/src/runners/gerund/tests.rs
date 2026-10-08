use super::gerund::gerund;
use super::prompt::gerund_prompt;
use openai_dive::v1::resources::chat::{ChatMessage, ChatMessageContent};

use crate::runners::test_helpers::{get_test_llm_client, get_test_model_for_provider};

#[tokio::test]
async fn test_gerund_with_simple_message() {
    let llm_client = get_test_llm_client();
    let model = get_test_model_for_provider(&llm_client).await;

    let message = "I am working on a new feature".to_string();
    let result = gerund(llm_client, model, message.clone()).await;

    assert!(
        result.is_ok(),
        "Gerund should successfully process simple message"
    );

    let response = result.unwrap();
    match response {
        ChatMessage::Assistant { content, .. } => {
            assert!(content.is_some(), "Assistant should provide content");
            if let Some(ChatMessageContent::Text(text)) = content {
                println!("🔤 Gerund for '{}': '{}'", message, text);
                assert!(!text.is_empty(), "Gerund response should not be empty");
                // Gerund should be a single word with first letter capitalized
                assert!(!text.contains(' '), "Gerund should be a single word");
                assert!(
                    text.chars().next().unwrap().is_uppercase(),
                    "Gerund should start with capital letter"
                );
            }
        }
        _ => panic!("Expected Assistant message"),
    }
}

#[tokio::test]
async fn test_gerund_with_coding_message() {
    let llm_client = get_test_llm_client();
    let model = get_test_model_for_provider(&llm_client).await;
    println!("{:?} {:?}", llm_client, model);

    let message = "Debugging the authentication system".to_string();
    let result = gerund(llm_client, model, message.clone()).await;
    println!("{:?}", result);

    assert!(
        result.is_ok(),
        "Gerund should successfully process coding message"
    );

    let response = result.unwrap();

    match response {
        ChatMessage::Assistant { content, .. } => {
            assert!(content.is_some(), "Assistant should provide content");
            if let Some(ChatMessageContent::Text(text)) = content {
                println!("🔤 Gerund for '{}': '{}'", message, text);
                assert!(!text.is_empty(), "Gerund response should not be empty");
                assert!(!text.contains(' '), "Gerund should be a single word");
                assert!(
                    text.chars().next().unwrap().is_uppercase(),
                    "Gerund should start with capital letter"
                );
            }
        }
        _ => panic!("Expected Assistant message"),
    }
}

#[tokio::test]
async fn test_gerund_with_different_activities() {
    let test_messages = vec![
        "Writing unit tests",
        "Refactoring legacy code",
        "Implementing new API endpoints",
        "Reviewing pull requests",
        "Optimizing database queries",
    ];

    for message in test_messages {
        let llm_client = get_test_llm_client();
        let model = get_test_model_for_provider(&llm_client).await;
        let result = gerund(llm_client, model, message.to_string()).await;

        assert!(result.is_ok(), "Gerund should process message: {}", message);

        let response = result.unwrap();
        match response {
            ChatMessage::Assistant { content, .. } => {
                assert!(
                    content.is_some(),
                    "Assistant should provide content for: {}",
                    message
                );
                if let Some(ChatMessageContent::Text(text)) = content {
                    println!("🔤 Gerund for '{}': '{}'", message, text);
                    assert!(
                        !text.is_empty(),
                        "Gerund response should not be empty for: {}",
                        message
                    );
                    assert!(
                        !text.contains(' '),
                        "Gerund should be a single word for: {}",
                        message
                    );
                    assert!(
                        text.chars().next().unwrap().is_uppercase(),
                        "Gerund should start with capital letter for: {}",
                        message
                    );
                }
            }
            _ => panic!("Expected Assistant message for: {}", message),
        }
    }
}

#[tokio::test]
async fn test_gerund_prompt_generation() {
    // Test that the prompt generation works
    let prompt = gerund_prompt();

    assert!(!prompt.is_empty(), "Gerund prompt should not be empty");
    assert!(
        prompt.contains("uplifting"),
        "Prompt should emphasize uplifting words"
    );
    assert!(
        prompt.contains("joy"),
        "Prompt should emphasize joyful words"
    );
}

#[tokio::test]
async fn test_gerund_with_empty_message() {
    let llm_client = get_test_llm_client();
    let model = get_test_model_for_provider(&llm_client).await;

    let message = "".to_string();
    let result = gerund(llm_client, model, message.clone()).await;

    // Even with empty message, should still return a valid response
    assert!(
        result.is_ok(),
        "Gerund should handle empty message gracefully"
    );

    let response = result.unwrap();
    match response {
        ChatMessage::Assistant { content, .. } => {
            assert!(
                content.is_some(),
                "Assistant should provide content even for empty message"
            );
            if let Some(ChatMessageContent::Text(text)) = content {
                println!("🔤 Gerund for empty message: '{}'", text);
            }
        }
        _ => panic!("Expected Assistant message"),
    }
}

#[tokio::test]
async fn test_gerund_with_long_message() {
    let llm_client = get_test_llm_client();
    let model = get_test_model_for_provider(&llm_client).await;

    let message = "I am working on a very complex feature that involves multiple microservices, database migrations, API changes, frontend updates, and comprehensive testing across all components to ensure backwards compatibility and performance optimization".to_string();
    let result = gerund(llm_client, model, message.clone()).await;

    assert!(result.is_ok(), "Gerund should handle long message");

    let response = result.unwrap();
    match response {
        ChatMessage::Assistant { content, .. } => {
            assert!(
                content.is_some(),
                "Assistant should provide content for long message"
            );
            if let Some(ChatMessageContent::Text(text)) = content {
                println!("🔤 Gerund for long message: '{}'", text);
                assert!(
                    !text.is_empty(),
                    "Gerund response should not be empty for long message"
                );
                assert!(
                    !text.contains(' '),
                    "Gerund should be a single word even for long input"
                );
            }
        }
        _ => panic!("Expected Assistant message"),
    }
}

#[test]
fn test_llm_client_selection() {
    // Test that our client selection method works
    let client = get_test_llm_client();
    let provider_name = client.provider_name();

    // Should be one of our supported providers
    assert!(
        [
            "openai",
            "anthropic",
            "openrouter",
            "openai_compatible",
            "ovhcloud",
            "mistral",
            "ollama"
        ]
        .contains(&provider_name),
        "Should select a valid provider, got: {}",
        provider_name
    );
}

#[tokio::test]
async fn test_model_selection_for_providers() {
    let client = get_test_llm_client();
    let model = get_test_model_for_provider(&client).await;

    // Should return a non-empty model name
    assert!(!model.is_empty(), "Should return a non-empty model name");
}
