use std::sync::Arc;

use async_trait::async_trait;
use openai_dive::v1::resources::chat::{
    ChatCompletionParametersBuilder, ChatCompletionToolChoice, ChatMessage, ChatMessageContent,
};
use shai_llm::client::LlmClient;

use crate::agent::brain::ThinkerDecision;
use crate::agent::{Agent, AgentBuilder, AgentError, Brain, ThinkerContext};
use crate::tools::{
    AnyTool, FetchTool, FindTool, LsTool, ReadTool, TodoReadTool, TodoStorage, TodoWriteTool,
};

use super::prompt::searcher_next_step;

#[derive(Clone)]
pub struct SearcherBrain {
    pub llm: Arc<LlmClient>,
    pub model: String,
}

impl SearcherBrain {
    pub fn new(llm: Arc<LlmClient>, model: String) -> Self {
        Self { llm, model }
    }

    /// Generic method to make LLM requests with custom system prompts and tools
    async fn chat_with_tools(
        &self,
        messages: Vec<ChatMessage>,
        tools: &Vec<Arc<dyn AnyTool>>,
        tool_choice: ChatCompletionToolChoice,
    ) -> Result<ChatMessage, AgentError> {
        let request = ChatCompletionParametersBuilder::default()
            .model(&self.model)
            .messages(messages)
            .tools(tools.iter().map(|t| t.to_openai()).collect::<Vec<_>>())
            .tool_choice(tool_choice)
            .temperature(0.1f32)
            .build()
            .map_err(|e| AgentError::LlmError(e.to_string()))?;

        let response = self
            .llm
            .chat(request)
            .await
            .map_err(|e| AgentError::LlmError(e.to_string()))?;

        Ok(response.choices[0].message.clone())
    }
}

#[async_trait]
impl Brain for SearcherBrain {
    async fn next_step(
        &mut self,
        context: ThinkerContext,
        _budget: crate::agent::brain::ToolBudgetRef,
    ) -> Result<ThinkerDecision, AgentError> {
        let mut trace = context.trace.clone();

        trace.insert(
            0,
            ChatMessage::System {
                content: ChatMessageContent::Text(searcher_next_step()),
                name: None,
            },
        );
        let brain_decision = self
            .chat_with_tools(
                trace,
                &context.available_tools,
                ChatCompletionToolChoice::Auto,
            )
            .await?;

        // stop here if there's no other tool calls
        if let ChatMessage::Assistant { tool_calls, .. } = &brain_decision {
            if tool_calls.as_ref().is_none_or(|calls| calls.is_empty()) {
                return Ok(ThinkerDecision::agent_pause(brain_decision));
            }
        }

        Ok(ThinkerDecision::agent_continue(brain_decision))
    }
}

pub fn searcher(llm: Arc<LlmClient>, model: String) -> impl Agent {
    // Create shared storage for todo tools
    let todo_storage = Arc::new(TodoStorage::new());

    // Only read-only tools for the searcher
    let fetch = Box::new(FetchTool::new());
    let find = Box::new(FindTool::new());
    let ls = Box::new(LsTool::new());
    let read = Box::new(ReadTool::new(Arc::new(crate::tools::FsOperationLog::new())));
    let todoread = Box::new(TodoReadTool::new(todo_storage.clone()));
    let todowrite = Box::new(TodoWriteTool::new(todo_storage.clone()));
    let toolbox: Vec<Box<dyn AnyTool>> = vec![fetch, find, ls, read, todoread, todowrite];

    AgentBuilder::with_brain(Box::new(SearcherBrain {
        llm: llm.clone(),
        model,
    }))
    .tools(toolbox)
    .build()
}
