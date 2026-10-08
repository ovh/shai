use std::fs;
use std::path::PathBuf;

use chrono::Local;

use super::structs::{MemoryReadParams, MemoryWriteParams};
use crate::tools::tool;
use crate::tools::types::ToolResult;

/// Resolve the global memory file path (`~/.config/shai/memory.md`).
fn global_memory_path() -> Option<PathBuf> {
    let config_dir = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| PathBuf::from(h).join(".config"))
        });

    config_dir.map(|d| d.join("shai").join("memory.md"))
}

/// Resolve the project-local memory file path.
///
/// Priority:
/// 1. `.shai/memory.md` at the git root (if it exists)
/// 2. `.shai/memory.md` at CWD
fn project_memory_path() -> PathBuf {
    let git_root = crate::runners::coder::env::find_git_root();
    let base = git_root.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    base.join(".shai").join("memory.md")
}

/// Load and merge memory facts from both global and project-local memory files.
///
/// Global memory (`~/.config/shai/memory.md`) is loaded first, then project-local
/// memory (`.shai/memory.md` at the git root) is appended. This gives project-local
/// memory precedence in ordering while making both available.
pub fn load_merged_memory() -> String {
    let mut combined = String::new();

    // Global memory first
    if let Some(global_path) = global_memory_path() {
        if let Ok(content) = fs::read_to_string(&global_path) {
            if !content.trim().is_empty() {
                combined.push_str("--- Begin global memory ---\n");
                combined.push_str(&content);
                if !content.ends_with('\n') {
                    combined.push('\n');
                }
                combined.push_str("--- End global memory ---\n");
            }
        }
    }

    // Project-local memory second
    let project_path = project_memory_path();
    if let Ok(content) = fs::read_to_string(&project_path) {
        if !content.trim().is_empty() {
            if !combined.is_empty() {
                combined.push('\n');
            }
            combined.push_str("--- Begin project memory ---\n");
            combined.push_str(&content);
            if !content.ends_with('\n') {
                combined.push('\n');
            }
            combined.push_str("--- End project memory ---\n");
        }
    }

    combined
}

/// Append a memory fact to the project-local memory file.
fn append_memory_fact(content: &str) -> Result<(), String> {
    let memory_path = project_memory_path();
    let parent = memory_path
        .parent()
        .ok_or_else(|| "Cannot determine memory file directory".to_string())?;

    // Ensure the parent directory exists
    fs::create_dir_all(parent).map_err(|e| format!("Failed to create memory directory: {}", e))?;

    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let entry = format!("- [{}] {}\n", timestamp, content);

    // Append to existing file or create new
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&memory_path)
        .map_err(|e| {
            format!(
                "Failed to open memory file '{}': {}",
                memory_path.display(),
                e
            )
        })?;

    use std::io::Write;
    file.write_all(entry.as_bytes())
        .map_err(|e| format!("Failed to write to memory file: {}", e))?;

    Ok(())
}

/// Read all memory facts from both global and project-local memory files.
fn read_memory_facts() -> Result<String, String> {
    Ok(load_merged_memory())
}

/// MemoryWriteTool — append a fact to the project's memory file.
#[derive(Clone)]
pub struct MemoryWriteTool;

#[tool(
    name = "memory_write",
    description = r#"Save a fact or instruction to the project's memory file for future reference. Use this to persistently store important context about the project (e.g., conventions, decisions, architecture notes) that should be available in future sessions.

**Examples:**
```json
{"content": "The project uses Rust edition 2021 and requires nightly toolchain."}
```
"#
)]
impl MemoryWriteTool {
    pub fn new() -> Self {
        Self
    }

    async fn execute(&self, params: MemoryWriteParams) -> ToolResult {
        if params.content.trim().is_empty() {
            return ToolResult::error("Memory content cannot be empty".to_string());
        }

        match append_memory_fact(&params.content) {
            Ok(()) => {
                let memory_path = project_memory_path();
                ToolResult::success(format!("Memory saved to '{}'.", memory_path.display()))
            }
            Err(e) => ToolResult::error(e),
        }
    }
}

impl Default for MemoryWriteTool {
    fn default() -> Self {
        Self::new()
    }
}

/// MemoryReadTool — read all facts from the project's memory file.
#[derive(Clone)]
pub struct MemoryReadTool;

#[tool(
    name = "memory_read",
    description = "Read all saved memory facts from the project's memory file. Returns the full contents of the memory file."
)]
impl MemoryReadTool {
    pub fn new() -> Self {
        Self
    }

    async fn execute(&self, _params: MemoryReadParams) -> ToolResult {
        match read_memory_facts() {
            Ok(content) => {
                if content.is_empty() {
                    ToolResult::success("No memory facts stored yet.".to_string())
                } else {
                    ToolResult::success(content)
                }
            }
            Err(e) => ToolResult::error(e),
        }
    }
}

impl Default for MemoryReadTool {
    fn default() -> Self {
        Self::new()
    }
}
