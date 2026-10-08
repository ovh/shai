pub mod mcp;
pub mod mcp_config;
pub mod mcp_http;
pub mod mcp_oauth;
pub mod mcp_sse;
pub mod mcp_stdio;

#[cfg(test)]
mod tests;

pub use mcp::{get_mcp_tools, McpClient, McpToolDescription};
pub use mcp_config::{create_mcp_client, McpConfig, OAuthToken};
pub use mcp_http::HttpClient;
pub use mcp_sse::SseClient;
pub use mcp_stdio::StdioClient;
