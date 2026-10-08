use schemars::JsonSchema;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct FetchToolParams {
    /// URL to fetch data from
    pub url: String,
    /// HTTP method to use
    #[serde(default = "default_method")]
    pub method: HttpMethod,
    /// HTTP headers to send (optional)
    #[serde(default)]
    pub headers: Option<HashMap<String, String>>,
    /// Request body for POST/PUT (optional)
    #[serde(default)]
    pub body: Option<String>,
    /// Request timeout in seconds (optional, defaults to 30)
    #[serde(default = "default_timeout")]
    pub timeout: u64,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "UPPERCASE")]
#[schemars(inline)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
}

fn default_method() -> HttpMethod {
    HttpMethod::Get
}

fn default_timeout() -> u64 {
    30
}
