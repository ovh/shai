use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct WriteFileSpec {
    /// Path to the file to write
    pub path: String,
    /// Content to write to the file
    pub content: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct WriteToolParams {
    /// One or more files to write
    pub files: Vec<WriteFileSpec>,
}
