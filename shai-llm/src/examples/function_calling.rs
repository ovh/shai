// Function calling example using OVH provider instead of Mistral
use openai_dive::v1::resources::chat::{
    ChatCompletionFunction, ChatCompletionParameters, ChatCompletionTool, ChatCompletionToolType,
    ChatMessage, ChatMessageContent,
};
use openai_dive::v1::resources::chat::{ChatCompletionParametersBuilder, ChatCompletionToolChoice};
use serde_json::json;
use shai_llm::{client::LlmClient, provider::LlmError};

#[tokio::main]
async fn main() -> Result<(), LlmError> {
    // Initialize OVH client from environment variables (OVH_API_KEY, OVH_BASE_URL)
    let client = LlmClient::from_env_mistral().expect("environment variables not set");

    let models = client.provider().models().await?;
    //println!("Available models:");
    //for model in &models.data {
    //    println!("  - {}", model.id);
    //}

    // Use a smaller model to avoid rate limits
    let model = models
        .data
        .iter()
        .find(|m| m.id.contains("small") || m.id.contains("tiny"))
        .or_else(|| models.data.iter().find(|m| m.id.contains("nemo")))
        .or_else(|| models.data.iter().find(|m| m.id.contains("medium")))
        .map(|m| m.id.clone())
        .unwrap_or_else(|| {
            println!("Warning: Meta-Llama-3_3-70B-Instruct not found, using first chat model");
            models
                .data
                .iter()
                .find(|m| m.id.contains("Instruct") || m.id.contains("Chat"))
                .map(|m| m.id.clone())
                .unwrap_or_else(|| models.data[0].id.clone())
        });
    println!("Using model: {}", model);

    // Create messages - be explicit about needing current weather data
    let messages = vec![ChatMessage::User {
        content: ChatMessageContent::Text("I need the current weather information for Paris, France. Please use the available weather function to get real-time data.".to_string()),
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
                            "description": "The city and state, e.g. San Francisco, CA"
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
        .build()
        .map_err(|e| format!("Failed to build parameters: {:?}", e))?;

    // Send the request
    println!("Sending request with tools... \n{:?}", request);
    let response = match client.chat(request).await {
        Ok(response) => response,
        Err(e) => {
            println!("Error making chat request: {:?}", e);
            return Err(e);
        }
    };

    // Check if the model wants to call a function
    let message = response.choices[0].message.clone();

    if let ChatMessage::Assistant {
        tool_calls: Some(ref tool_calls),
        ..
    } = message
    {
        println!("Model wants to call function(s):");

        for tool_call in tool_calls {
            let name = &tool_call.function.name;
            let arguments = &tool_call.function.arguments;

            println!("Function call ID: {}", tool_call.id);
            println!("Function name: {}", name);
            println!("Function arguments: {}", arguments);

            if name == "get_weather" {
                // Parse arguments
                let args: serde_json::Value = serde_json::from_str(arguments)?;
                let location = args["location"].as_str().unwrap_or("Unknown");

                // Mock weather data
                let function_result = json!({
                    "location": location,
                    "temperature": 18,
                    "unit": "celsius",
                    "condition": "Partly cloudy",
                    "humidity": 65,
                    "wind_speed": 12
                })
                .to_string();

                println!("Function result: {}", function_result);

                // Create follow-up request with function result
                let follow_up_request = ChatCompletionParametersBuilder::default()
                    .model(response.model.clone())
                    .messages(vec![
                        ChatMessage::User {
                            content: ChatMessageContent::Text("I need the current weather information for Paris, France. Please use the available weather function to get real-time data.".to_string()),
                            name: None,
                        },
                        message.clone(),
                        ChatMessage::Tool {
                            content: ChatMessageContent::Text(function_result),
                            tool_call_id: tool_call.id.clone(),
                        }
                    ])
                    .temperature(0.1f32)
                    .max_completion_tokens(200u32)
                    .build()
                    .map_err(|e| format!("Failed to build follow-up parameters: {:?}", e))?;

                // Get final response
                let final_response = client.chat(follow_up_request).await?;

                if let Some(final_choice) = final_response.choices.first() {
                    match &final_choice.message {
                        ChatMessage::Assistant {
                            content: Some(ChatMessageContent::Text(text)),
                            ..
                        } => {
                            println!("\nFinal response: {}", text);
                        }
                        _ => println!("No text in final response"),
                    }
                }
            }
        }
    } else if let ChatMessage::Assistant {
        content: Some(ChatMessageContent::Text(text)),
        ..
    } = message
    {
        println!("Direct response (no function call): {}", text);
    } else {
        println!("Unexpected response format");
    }

    Ok(())
}
