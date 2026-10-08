use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parameters for writing a memory fact.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MemoryWriteParams {
    /// The fact or instruction to remember.
    pub content: String,
}

/// Parameters for reading memory facts.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MemoryReadParams {
    /// Unused field for empty params compatibility.
    pub _unused: Option<String>,
}
