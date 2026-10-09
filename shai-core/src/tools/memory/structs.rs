use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Which memory index a tool operates on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum MemoryScope {
    /// Project-local memory (`.shai/memory/` at the git root).
    #[default]
    Project,
    /// Global, user-wide memory (`~/.config/shai/memory/`).
    Global,
}

impl fmt::Display for MemoryScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MemoryScope::Project => write!(f, "project"),
            MemoryScope::Global => write!(f, "global"),
        }
    }
}

/// Parameters for writing a memory fact to the index.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MemoryWriteParams {
    /// The fact to remember, on a single line.
    pub content: String,
    /// Which index to write to. Defaults to "project".
    #[serde(default)]
    pub scope: MemoryScope,
}

/// Parameters for removing a memory fact from the index.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MemoryRemoveParams {
    /// The fact (or a unique fragment of it) to remove.
    pub content: String,
    /// Which index to remove from. Defaults to "project".
    #[serde(default)]
    pub scope: MemoryScope,
}
