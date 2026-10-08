use crate::tools::McpClient;
use serde::{Deserialize, Serialize};

use super::{HttpClient, SseClient, StdioClient};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthToken {
    pub access_token: String,
    /// Unix timestamp (seconds since epoch) when the token expires
    pub expires_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum McpConfig {
    #[serde(rename = "stdio")]
    Stdio { command: String, args: Vec<String> },
    #[serde(rename = "http")]
    Http {
        url: String,
        #[serde(flatten)]
        auth: Option<OAuthToken>,
    },
    #[serde(rename = "sse")]
    Sse { url: String },
}

impl OAuthToken {
    /// Check if the token is expired or will expire within the next 60 seconds
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64;

            // Consider expired if it expires within the next 60 seconds (safety margin)
            expires_at <= now + 60
        } else {
            // If no expiration time, assume it's still valid
            false
        }
    }
}

/// Factory function to create an MCP client from configuration
pub fn create_mcp_client(config: McpConfig) -> Box<dyn McpClient> {
    match config {
        McpConfig::Stdio { command, args } => Box::new(StdioClient::new(command, args)),
        McpConfig::Http { url, auth } => {
            let bearer_token = auth.map(|t| t.access_token);
            Box::new(HttpClient::new_with_auth(url, bearer_token))
        }
        McpConfig::Sse { url } => Box::new(SseClient::new(url)),
    }
}
