use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ReadFileSpec {
    /// Path to the file to read
    pub path: String,
    /// Line number to start reading from (1-indexed). Defaults to 1.
    #[serde(default)]
    pub offset: Option<u32>,
    /// Maximum number of lines to read. Defaults to 2000.
    #[serde(default)]
    pub limit: Option<u32>,
    /// When true, return a compact symbol outline instead of full file content.
    /// Falls back to full read if the language is unsupported.
    #[serde(default)]
    pub outline: bool,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ReadToolParams {
    /// One or more files to read
    pub files: Vec<ReadFileSpec>,
}
