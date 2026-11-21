// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: OVH SAS

//! A scripted, in-process LLM for tests that drive the real agent loop.
//!
//! A [`Script`] holds the assistant replies of a whole conversation, in order.
//! Any number of clients can be created from it; each one answers a chat
//! request with the next reply and records that it was the one asked. Tests can
//! then assert not only what the agent did, but which client and model served
//! each step. Nothing here opens a socket.

use async_trait::async_trait;
use openai_dive::v1::resources::chat::{
    ChatCompletionChoice, ChatCompletionParameters, ChatCompletionResponse, ChatMessage,
    ChatMessageContent, Function, ToolCall,
};
use openai_dive::v1::resources::model::ListModelResponse;
use shai_llm::provider::{LlmError, LlmProvider, LlmStream, ProviderInfo};
use shai_llm::LlmClient;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Source of tool call ids that are unique across a test binary.
static NEXT_CALL_ID: AtomicUsize = AtomicUsize::new(0);

/// One chat request, as seen by the client that served it.
#[derive(Debug, Clone)]
pub(crate) struct Served {
    /// Name of the scripted client that received the request.
    pub client: &'static str,
    /// Model named in the request.
    pub model: String,
    /// Messages sent with the request, system prompt included.
    pub messages: Vec<ChatMessage>,
    /// Names of the tools offered with the request.
    pub tools: Vec<String>,
}

#[derive(Default)]
struct ScriptState {
    replies: VecDeque<ChatMessage>,
    served: Vec<Served>,
}

/// The replies of a conversation, shared by every client created from it.
#[derive(Clone, Default)]
pub(crate) struct Script {
    state: Arc<Mutex<ScriptState>>,
}

impl Script {
    pub(crate) fn new(replies: Vec<ChatMessage>) -> Self {
        Self {
            state: Arc::new(Mutex::new(ScriptState {
                replies: replies.into(),
                served: Vec::new(),
            })),
        }
    }

    /// A client that serves this script under the provider name `name`.
    pub(crate) fn client(&self, name: &'static str) -> Arc<LlmClient> {
        Arc::new(LlmClient::from_provider(Box::new(ScriptedProvider {
            name,
            script: self.clone(),
        })))
    }

    /// Every request received so far, in order.
    pub(crate) fn served(&self) -> Vec<Served> {
        self.state.lock().unwrap().served.clone()
    }

    /// The client that served each request, in order.
    pub(crate) fn served_by(&self) -> Vec<&'static str> {
        self.served().iter().map(|s| s.client).collect()
    }

    /// Number of scripted replies not consumed yet.
    pub(crate) fn remaining(&self) -> usize {
        self.state.lock().unwrap().replies.len()
    }
}

struct ScriptedProvider {
    name: &'static str,
    script: Script,
}

#[async_trait]
impl LlmProvider for ScriptedProvider {
    async fn models(&self) -> Result<ListModelResponse, LlmError> {
        Err("a scripted provider has no model listing".into())
    }

    async fn default_model(&self) -> Result<String, LlmError> {
        Err("a scripted provider has no default model".into())
    }

    async fn chat(
        &self,
        request: ChatCompletionParameters,
    ) -> Result<ChatCompletionResponse, LlmError> {
        let mut state = self.script.state.lock().unwrap();
        state.served.push(Served {
            client: self.name,
            model: request.model.clone(),
            messages: request.messages.clone(),
            tools: request
                .tools
                .unwrap_or_default()
                .into_iter()
                .map(|t| t.function.name)
                .collect(),
        });
        // A request the script did not plan for ends the run with a plain
        // reply; tests catch it through `served_by`.
        let message = state
            .replies
            .pop_front()
            .unwrap_or_else(|| text("(script exhausted)"));
        Ok(ChatCompletionResponse {
            id: None,
            choices: vec![ChatCompletionChoice {
                index: 0,
                message,
                finish_reason: None,
                logprobs: None,
            }],
            created: 0,
            model: request.model,
            service_tier: None,
            system_fingerprint: None,
            object: "chat.completion".to_string(),
            usage: None,
        })
    }

    async fn chat_stream(&self, _request: ChatCompletionParameters) -> Result<LlmStream, LlmError> {
        Err("a scripted provider does not stream".into())
    }

    fn supports_functions(&self, _model: String) -> bool {
        true
    }

    fn supports_structured_output(&self, _model: String) -> bool {
        false
    }

    fn name(&self) -> &'static str {
        self.name
    }

    fn info() -> ProviderInfo {
        ProviderInfo {
            name: "scripted",
            display_name: "Scripted (tests)",
            env_vars: Vec::new(),
        }
    }
}

/// An assistant reply with text and no tool call: ends the run.
pub(crate) fn text(content: &str) -> ChatMessage {
    ChatMessage::Assistant {
        content: Some(ChatMessageContent::Text(content.to_string())),
        reasoning: None,
        reasoning_content: None,
        tool_calls: None,
        name: None,
        audio: None,
        refusal: None,
    }
}

/// An assistant reply that calls the given `(tool, arguments)` pairs.
pub(crate) fn calls(tool_calls: &[(&str, serde_json::Value)]) -> ChatMessage {
    ChatMessage::Assistant {
        content: None,
        reasoning: None,
        reasoning_content: None,
        tool_calls: Some(
            tool_calls
                .iter()
                .map(|(name, arguments)| ToolCall {
                    id: format!(
                        "call_{}_{}",
                        name,
                        NEXT_CALL_ID.fetch_add(1, Ordering::Relaxed)
                    ),
                    r#type: "function".to_string(),
                    function: Function {
                        name: name.to_string(),
                        arguments: arguments.to_string(),
                    },
                })
                .collect(),
        ),
        name: None,
        audio: None,
        refusal: None,
    }
}

/// An assistant reply that calls one tool.
pub(crate) fn call(tool: &str, arguments: serde_json::Value) -> ChatMessage {
    calls(&[(tool, arguments)])
}
