#![allow(clippy::module_inception)]
// async_trait-generated futures trip double_must_use on methods returning Result
#![allow(clippy::double_must_use)]
pub mod chat;
pub mod client;
pub mod error;
pub mod logging;
pub mod provider;
pub mod providers;
pub mod tool;

// Re-export our client
pub use client::LlmClient;

pub use error::{parse_retry_after, retry_after_from_headers, RateLimitedError};

pub use tool::{
    AssistantResponse, ContainsTool, FunctionCallingAutoBuilder, FunctionCallingRequiredBuilder,
    IntoChatMessage, StructuredOutputBuilder, ToolBox, ToolCallMethod, ToolDescription,
};
