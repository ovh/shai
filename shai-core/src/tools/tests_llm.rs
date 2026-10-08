#[cfg(test)]
mod llm_integration_tests {
    use crate::config::config::ShaiConfig;
    use crate::runners::test_helpers::init_test_logging;
    use crate::tools::{
        AnyTool, BashTool, EditTool, FetchTool, FindTool, FsOperationLog, LsTool, ReadTool,
        TodoReadTool, TodoStorage, TodoWriteTool, WriteTool,
    };
    use openai_dive::v1::resources::chat::{
        ChatCompletionParametersBuilder, ChatCompletionToolChoice,
    };
    use openai_dive::v1::resources::chat::{ChatMessage, ChatMessageContent};
    use std::sync::Arc;
    use tracing::debug;

    /// Test a tool with the first available LLM provider
    async fn test_tool_with_llm(
        tool: Arc<dyn AnyTool>,
        prompt: &str,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        init_test_logging();

        let (llm_client, model) = ShaiConfig::get_llm()
            .await
            .map_err(|e| format!("No LLM provider available: {}", e))?;

        println!(
            "Testing tool '{}' with model '{}' from provider '{}'",
            &tool.name(),
            model,
            llm_client.provider_name()
        );

        // Create messages that should trigger tool usage
        let messages = vec![
            ChatMessage::System {
                content: ChatMessageContent::Text(
                    format!("You are a helpful assistant. You must absolutely use the {} tool to respond to the user's request. Do not explain or apologize, just use the tool.", &tool.name())
                ),
                name: None,
            },
            ChatMessage::User {
                content: ChatMessageContent::Text(prompt.to_string()),
                name: None,
            }
        ];

        // Create chat completion request that forces tool usage
        let request = ChatCompletionParametersBuilder::default()
            .model(model)
            .messages(messages)
            .tools(vec![tool.to_openai()])
            .tool_choice(ChatCompletionToolChoice::Auto) // Force tool usage
            .temperature(0.1f32)
            .build()
            .map_err(|e| format!("Failed to build request: {}", e))?;

        // Debug the request being sent
        debug!(target: "misc", "Tool schema: {}", serde_json::to_string_pretty(&tool.to_openai()).unwrap());
        debug!(target: "misc", "Chat request: {}", serde_json::to_string_pretty(&request).unwrap());

        // Make the request
        let response = llm_client.chat(request).await.map_err(|e| {
            let error_str = e.to_string().to_lowercase();
            if error_str.contains("connection")
                || error_str.contains("refused")
                || error_str.contains("error sending request")
                || error_str.contains("tcp connect error")
            {
                format!("LLM service not available: {}", e)
            } else {
                format!("LLM request failed: {}", e)
            }
        })?;

        // Debug the response received
        debug!(target: "misc", "Chat response: {}", serde_json::to_string_pretty(&response).unwrap());

        // Check if tool was called
        if let Some(choice) = response.choices.first() {
            debug!(target: "misc", "Response choice message: {:?}", choice.message);
            if let ChatMessage::Assistant { tool_calls, .. } = &choice.message {
                if let Some(calls) = tool_calls {
                    debug!(target: "misc", "Tool calls found: {}", calls.len());
                    for call in calls {
                        debug!(target: "misc", "Tool call: {} -> {}", call.function.name, &tool.name());
                    }
                    let tool_was_called =
                        calls.iter().any(|call| call.function.name == tool.name());
                    if tool_was_called {
                        println!(
                            "✅ Tool '{}' PASSED with provider '{}'",
                            &tool.name(),
                            llm_client.provider_name()
                        );
                    } else {
                        println!("❌ Tool '{}' FAILED - tool not called", &tool.name());
                    }
                    return Ok(tool_was_called);
                } else {
                    debug!(target: "misc", "Tool calls is None");
                }
            } else {
                debug!(target: "misc", "Message is not Assistant type");
            }
        } else {
            debug!(target: "misc", "No choices in response");
        }

        println!(
            "❌ Tool '{}' FAILED - no tool calls in response",
            &tool.name()
        );
        Ok(false)
    }

    /// Helper macro to run LLM test with graceful fallback for connection issues
    macro_rules! llm_test_with_fallback {
        ($test_fn:expr, $error_msg:expr) => {
            match $test_fn.await {
                Ok(success) => assert!(success, $error_msg),
                Err(e) => {
                    let error_str = e.to_string().to_lowercase();
                    if error_str.contains("not available")
                        || error_str.contains("no llm provider")
                        || error_str.contains("connection")
                        || error_str.contains("refused")
                    {
                        println!("⚠️  Skipping LLM test - {}", e);
                        return; // Skip test if LLM service not available
                    } else {
                        panic!("LLM test failed with unexpected error: {}", e);
                    }
                }
            }
        };
    }

    #[tokio::test]
    async fn test_bash_tool_with_llm() {
        let tool: Arc<dyn AnyTool> = Arc::new(BashTool::new());
        llm_test_with_fallback!(
            test_tool_with_llm(tool, "Run the command 'echo hello world' using bash"),
            "BashTool should be called by LLM"
        );
    }

    #[tokio::test]
    async fn test_ls_tool_with_llm() {
        let tool: Arc<dyn AnyTool> = Arc::new(LsTool::new());
        llm_test_with_fallback!(
            test_tool_with_llm(tool, "List the files in the current directory"),
            "LsTool should be called by LLM"
        );
    }

    #[tokio::test]
    async fn test_fetch_tool_with_llm() {
        let tool: Arc<dyn AnyTool> = Arc::new(FetchTool::new());
        llm_test_with_fallback!(
            test_tool_with_llm(
                tool,
                "Fetch the content from https://httpbin.org/json and show me the response"
            ),
            "FetchTool should be called by LLM"
        );
    }

    #[tokio::test]
    async fn test_find_tool_with_llm() {
        let tool: Arc<dyn AnyTool> = Arc::new(FindTool::new());
        llm_test_with_fallback!(
            test_tool_with_llm(
                tool,
                "Search for files containing 'test' in the current directory"
            ),
            "FindTool should be called by LLM"
        );
    }

    #[tokio::test]
    async fn test_todo_read_tool_with_llm() {
        let tool: Arc<dyn AnyTool> = Arc::new(TodoReadTool::new(Arc::new(TodoStorage::new())));
        llm_test_with_fallback!(
            test_tool_with_llm(tool, "Show me the current todo list"),
            "TodoReadTool should be called by LLM"
        );
    }

    #[tokio::test]
    async fn test_todo_write_tool_with_llm() {
        let tool: Arc<dyn AnyTool> = Arc::new(TodoWriteTool::new(Arc::new(TodoStorage::new())));
        llm_test_with_fallback!(
            test_tool_with_llm(tool, "Create a new todo item with the content 'Test LLM integration' and status 'pending'"),
            "TodoWriteTool should be called by LLM"
        );
    }

    #[tokio::test]
    async fn test_write_tool_with_llm() {
        let tool: Arc<dyn AnyTool> = Arc::new(WriteTool::new(Arc::new(FsOperationLog::new())));
        llm_test_with_fallback!(
            test_tool_with_llm(
                tool,
                "Write 'Hello LLM Test' to the file '/tmp/test_write.txt'"
            ),
            "WriteTool should be called by LLM"
        );
    }

    #[tokio::test]
    async fn test_read_tool_with_llm() {
        let tool: Arc<dyn AnyTool> = Arc::new(ReadTool::new(Arc::new(FsOperationLog::new())));
        llm_test_with_fallback!(
            test_tool_with_llm(tool, "Read the contents of the file 'Cargo.toml'"),
            "ReadTool should be called by LLM"
        );
    }

    #[tokio::test]
    async fn test_edit_tool_with_llm() {
        let tool: Arc<dyn AnyTool> = Arc::new(EditTool::new(Arc::new(FsOperationLog::new())));
        llm_test_with_fallback!(
            test_tool_with_llm(
                tool,
                "In the file 'Cargo.toml', replace 'name' with 'project_name'"
            ),
            "EditTool should be called by LLM"
        );
    }

    #[tokio::test]
    async fn test_multiedit_tool_with_llm() {
        let tool: Arc<dyn AnyTool> = Arc::new(EditTool::new(Arc::new(FsOperationLog::new())));
        llm_test_with_fallback!(
            test_tool_with_llm(tool, "In the file 'Cargo.toml', replace 'name' with 'project_name' and 'v0.0.1' with 'v0.0.2'"),
            "EditTool should be called by LLM"
        );
    }
}
