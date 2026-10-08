use openai_dive::v1::resources::chat::{
    ChatCompletionParametersBuilder, ChatMessage, ChatMessageContent,
};
use shai_llm::{client::LlmClient, provider::LlmError};

use super::prompt::gerund_prompt;

pub async fn gerund(
    llm: LlmClient,
    model: String,
    message: String,
) -> Result<ChatMessage, LlmError> {
    let message = if message.is_empty() {
        "the user has sent an empty message".to_string()
    } else {
        message
    };
    let mut messages = vec![ChatMessage::User {
        content: ChatMessageContent::Text(message.clone()),
        name: None,
    }];
    messages.push(ChatMessage::System {
        content: ChatMessageContent::Text(gerund_prompt()),
        name: None,
    });

    let request = ChatCompletionParametersBuilder::default()
        .model(model.clone())
        .messages(messages)
        .temperature(0.1f32)
        .build()?;

    // submit it to our big brain coder
    let response = llm.chat(request).await?;

    Ok(response.choices[0].message.clone())
}
