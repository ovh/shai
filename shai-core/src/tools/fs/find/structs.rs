use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FindToolParams {
    /// The pattern to search for (supports regex)
    pub pattern: String,
    /// Directory to search in (defaults to current directory)
    #[serde(default)]
    pub path: Option<String>,
    /// File extensions to include (e.g., "rs,js,py")
    #[serde(default)]
    pub include_extensions: Option<String>,
    /// File patterns to exclude (e.g., "target,node_modules,.git")
    #[serde(default)]
    pub exclude_patterns: Option<String>,
    /// Maximum number of results to return
    #[serde(default = "default_max_results")]
    pub max_results: u32,
    /// Whether to use case-sensitive search
    #[serde(default)]
    pub case_sensitive: bool,
    /// Find type: content (search file contents) or filename (search file names)
    #[serde(default = "default_find_type")]
    pub find_type: FindType,
    /// Show line numbers in results
    #[serde(default = "default_show_line_numbers")]
    pub show_line_numbers: bool,
    /// Maximum lines of context around matches
    #[serde(default)]
    pub context_lines: Option<u32>,
    /// Use whole word matching
    #[serde(default)]
    pub whole_word: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub enum FindType {
    Content,
    Filename,
    Both,
}

fn default_max_results() -> u32 {
    100
}
fn default_find_type() -> FindType {
    FindType::Content
}
fn default_show_line_numbers() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub file_path: String,
    pub line_number: Option<u32>,
    pub line_content: Option<String>,
    pub context_before: Vec<String>,
    pub context_after: Vec<String>,
    pub match_type: String, // "content" or "filename"
}
