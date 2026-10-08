use async_trait::async_trait;
use std::sync::Arc;

use openai_dive::v1::resources::chat::{
    ChatCompletionFunction, ChatCompletionParameters, ChatCompletionParametersBuilder,
    ChatCompletionResponse, ChatCompletionTool, ChatCompletionToolChoice, ChatCompletionToolType,
    ChatMessage,
};

use crate::{
    provider::LlmError,
    tool::{
        call_fc_auto::ToolCallFunctionCallingAuto,
        call_fc_required::ToolCallFunctionCallingRequired,
        call_structured_output::ToolCallStructuredOutput, ToolBox,
    },
    LlmClient, ToolCallMethod, ToolDescription,
};

#[async_trait]
pub trait LlmToolCall {
    async fn chat_with_tools(
        &self,
        request: ChatCompletionParameters,
        tools: &ToolBox,
        method: ToolCallMethod,
    ) -> Result<ChatCompletionResponse, LlmError>;
}

#[async_trait]
impl LlmToolCall for LlmClient {
    async fn chat_with_tools(
        &self,
        request: ChatCompletionParameters,
        tools: &ToolBox,
        method: ToolCallMethod,
    ) -> Result<ChatCompletionResponse, LlmError> {
        match method {
            ToolCallMethod::Auto => self.chat_with_tools_try_all(request, tools).await,
            ToolCallMethod::FunctionCall => self.chat_with_tools_fc_auto(request, tools).await,
            ToolCallMethod::FunctionCallRequired => {
                self.chat_with_tools_fc_required(request, tools).await
            }
            ToolCallMethod::StructuredOutput => self.chat_with_tools_so(request, tools).await,
            ToolCallMethod::Parsing => Err(LlmError::from("method not supported")),
        }
    }
}

#[async_trait]
pub trait ToolCallAuto {
    async fn chat_with_tools_try_all(
        &self,
        request: ChatCompletionParameters,
        tools: &ToolBox,
    ) -> Result<ChatCompletionResponse, LlmError>;
}

#[async_trait]
impl ToolCallAuto for LlmClient {
    async fn chat_with_tools_try_all(
        &self,
        request: ChatCompletionParameters,
        tools: &ToolBox,
    ) -> Result<ChatCompletionResponse, LlmError> {
        if let Ok(result) = self.chat_with_tools_fc_auto(request.clone(), tools).await {
            return Ok(result);
        }

        if let Ok(result) = self
            .chat_with_tools_fc_required(request.clone(), tools)
            .await
        {
            return Ok(result);
        }

        self.chat_with_tools_so(request, tools).await
    }
}
