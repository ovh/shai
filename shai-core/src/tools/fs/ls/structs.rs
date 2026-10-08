use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct LsToolParams {
    /// Directory to list (defaults to current directory)
    #[serde(default = "default_directory")]
    pub directory: String,
    /// Whether to list files recursively (defaults to false)
    #[serde(default)]
    pub recursive: bool,
    /// Show hidden files (files starting with .)
    #[serde(default)]
    pub show_hidden: bool,
    /// Show detailed information (size, permissions, etc.)
    #[serde(default)]
    pub long_format: bool,
    /// Maximum depth for recursive listing (None = unlimited)
    #[serde(default)]
    pub max_depth: Option<u32>,
    /// Maximum number of files to return (defaults to 200, set to None for unlimited)
    #[serde(default = "default_max_files")]
    pub max_files: Option<u32>,
}

fn default_directory() -> String {
    ".".to_string()
}

fn default_max_files() -> Option<u32> {
    Some(200) // Reasonable limit for LLM context
}

#[derive(Debug, Clone)]
pub struct FileInfo {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<std::time::SystemTime>,
    pub permissions: String,
}
