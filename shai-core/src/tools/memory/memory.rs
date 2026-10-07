use std::fs;
use std::path::PathBuf;

use chrono::Local;

use super::structs::{MemoryRemoveParams, MemoryScope, MemoryWriteParams};
use crate::tools::tool;
use crate::tools::types::ToolResult;

/// Maximum number of lines of an index injected into the system prompt.
pub const MAX_INDEX_LINES: usize = 200;
/// Maximum number of bytes of an index injected into the system prompt.
pub const MAX_INDEX_BYTES: usize = 25 * 1024;

/// Resolve the project memory directory.
///
/// Priority: `SHAI_MEMORY_PROJECT_DIR` env override (tests / power users),
/// then `.shai/memory/` at the git root, falling back to the current directory.
pub fn project_memory_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("SHAI_MEMORY_PROJECT_DIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    let base = crate::runners::coder::env::find_git_root()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    base.join(".shai").join("memory")
}

/// Resolve the global memory directory.
///
/// Priority: `SHAI_MEMORY_GLOBAL_DIR` env override, then `memory/` under the
/// shai config dir (`$XDG_CONFIG_HOME/shai` or `~/.config/shai`).
pub fn global_memory_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("SHAI_MEMORY_GLOBAL_DIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    let config_dir = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| PathBuf::from(h).join(".config"))
        });

    config_dir
        .map(|d| d.join("shai").join("memory"))
        .unwrap_or_else(|| PathBuf::from(".shai").join("memory"))
}

/// Path of the `MEMORY.md` index file for a scope.
pub fn index_path(scope: MemoryScope) -> PathBuf {
    let dir = match scope {
        MemoryScope::Project => project_memory_dir(),
        MemoryScope::Global => global_memory_dir(),
    };
    dir.join("MEMORY.md")
}

/// A loaded index, possibly truncated to the injection limits.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedIndex {
    /// Content within the line/byte limits.
    pub content: String,
    /// True if the file exceeded `MAX_INDEX_LINES` or `MAX_INDEX_BYTES`.
    pub truncated: bool,
}

/// Load an index file, capped at [`MAX_INDEX_LINES`] / [`MAX_INDEX_BYTES`].
pub fn load_index(scope: MemoryScope) -> LoadedIndex {
    let path = index_path(scope);
    let Ok(raw) = fs::read_to_string(&path) else {
        return LoadedIndex {
            content: String::new(),
            truncated: false,
        };
    };

    let mut content = String::new();
    let mut truncated = false;
    for line in raw.lines() {
        if content.lines().count() >= MAX_INDEX_LINES
            || content.len() + line.len() + 1 > MAX_INDEX_BYTES
        {
            truncated = true;
            break;
        }
        content.push_str(line);
        content.push('\n');
    }

    LoadedIndex { content, truncated }
}

/// Render the memory block injected at the `{{MEMORY}}` placeholder.
///
/// Returns an empty string when no index has any content, so nothing is
/// injected for users without memories yet.
pub fn render_memory_block() -> String {
    let global = load_index(MemoryScope::Global);
    let project = load_index(MemoryScope::Project);

    if global.content.trim().is_empty() && project.content.trim().is_empty() {
        return String::new();
    }

    let mut block = String::new();
    block.push_str("## Memory\n\n");
    block.push_str(
        "Persistent facts from previous sessions. Use `memory_write` to add a one-line \
         fact and `memory_remove` to prune stale ones. To save detailed notes in topic \
         files, load the `memory` skill first.\n",
    );

    if !global.content.trim().is_empty() {
        block.push_str("\n--- Begin global memory index ---\n");
        block.push_str(&global.content);
        block.push_str("--- End global memory index ---\n");
        if global.truncated {
            block.push_str(
                "(global index truncated: keep it under 200 lines / 25KB — curate it with \
                 memory_remove or rewrite it)\n",
            );
        }
    }

    if !project.content.trim().is_empty() {
        block.push_str("\n--- Begin project memory index ---\n");
        block.push_str(&project.content);
        block.push_str("--- End project memory index ---\n");
        if project.truncated {
            block.push_str(
                "(project index truncated: keep it under 200 lines / 25KB — curate it with \
                 memory_remove or rewrite it)\n",
            );
        }
    }

    block
}

/// Parse a fact line of the form `- [YYYY-MM-DD HH:MM:SS] fact text`.
/// Returns the fact text (without timestamp), or `None` if the line is not a fact.
pub(crate) fn parse_fact_text(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("- [")?;
    let end = rest.find("] ")?;
    let stamp = &rest[..end];
    // Light validation: expect "YYYY-MM-DD HH:MM:SS"
    if stamp.len() < 10 || !stamp.contains('-') || !stamp.contains(':') {
        return None;
    }
    Some(&rest[end + 2..])
}

/// Normalize a fact for comparison: trim, collapse whitespace, lowercase.
pub(crate) fn normalize(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Outcome of a successful write.
#[derive(Debug, Clone, PartialEq)]
pub enum WriteOutcome {
    /// Fact appended to the index at this path.
    Added(PathBuf),
    /// An identical fact was already present; nothing was written.
    AlreadyStored(PathBuf),
}

/// Append a one-line fact to the index of the given scope.
///
/// Enforces: non-empty single-line content, no exact duplicates, and the
/// index size cap (a full index must be curated before adding more).
pub fn write_fact(content: &str, scope: MemoryScope) -> Result<WriteOutcome, String> {
    let content = content.trim();
    if content.is_empty() {
        return Err("Memory content cannot be empty".to_string());
    }
    if content.contains('\n') {
        return Err(
            "Memory must be a single line. Save one concise fact per call, or put \
             detailed notes in a topic file (load the `memory` skill for the protocol)."
                .to_string(),
        );
    }

    let path = index_path(scope);
    let existing = fs::read_to_string(&path).unwrap_or_default();

    // Skip exact duplicates — the fact is already in context every session
    let new_norm = normalize(content);
    let duplicate = existing
        .lines()
        .filter_map(parse_fact_text)
        .any(|fact| normalize(fact) == new_norm);
    if duplicate {
        return Ok(WriteOutcome::AlreadyStored(path));
    }

    // Enforce the cap before appending
    let line_count = existing.lines().count();
    if line_count >= MAX_INDEX_LINES || existing.len() >= MAX_INDEX_BYTES {
        return Err(format!(
            "The memory index '{}' is at its cap ({} lines / {} bytes). Curate it first: \
             remove stale facts with memory_remove or rewrite it with the edit tool.",
            path.display(),
            MAX_INDEX_LINES,
            MAX_INDEX_BYTES
        ));
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create memory directory: {}", e))?;
    }

    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
    let entry = format!("- [{}] {}\n", timestamp, content);

    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("Failed to open memory index '{}': {}", path.display(), e))?;

    use std::io::Write;
    file.write_all(entry.as_bytes())
        .map_err(|e| format!("Failed to write to memory index: {}", e))?;

    Ok(WriteOutcome::Added(path))
}

/// Outcome of a removal: the fact texts that were removed.
pub fn remove_fact(query: &str, scope: MemoryScope) -> Result<Vec<String>, String> {
    let query = query.trim();
    if query.is_empty() {
        return Err("Removal query cannot be empty".to_string());
    }

    let path = index_path(scope);
    let raw = fs::read_to_string(&path)
        .map_err(|_| format!("No memory index found at '{}'", path.display()))?;

    let lines: Vec<&str> = raw.lines().collect();
    let query_norm = normalize(query);

    // Pass 1: exact match (normalized) — may hit several identical lines, remove them all
    let mut exact: Vec<usize> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if let Some(fact) = parse_fact_text(line) {
            if normalize(fact) == query_norm {
                exact.push(i);
            }
        }
    }

    let matched = if !exact.is_empty() {
        exact
    } else {
        // Pass 2: unique substring match
        let mut partial: Vec<usize> = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            if let Some(fact) = parse_fact_text(line) {
                if normalize(fact).contains(&query_norm) {
                    partial.push(i);
                }
            }
        }
        match partial.len() {
            0 => {
                return Err(format!(
                    "No memory matching '{}' in '{}'. Read the MEMORY.md file to find \
                     the exact wording.",
                    query,
                    path.display()
                ))
            }
            1 => partial,
            _ => {
                let candidates: Vec<String> = partial
                    .iter()
                    .take(5)
                    .map(|&i| {
                        let fact = parse_fact_text(lines[i]).unwrap_or(lines[i]);
                        let short: String = fact.chars().take(80).collect();
                        format!("  - {}", short)
                    })
                    .collect();
                return Err(format!(
                    "{} memories match '{}' — be more specific. Candidates:\n{}",
                    partial.len(),
                    query,
                    candidates.join("\n")
                ));
            }
        }
    };

    let removed: Vec<String> = matched
        .iter()
        .map(|&i| parse_fact_text(lines[i]).unwrap_or(lines[i]).to_string())
        .collect();

    let keep: Vec<&str> = lines
        .iter()
        .enumerate()
        .filter(|(i, _)| !matched.contains(i))
        .map(|(_, l)| *l)
        .collect();

    let mut new_content = keep.join("\n");
    if !new_content.is_empty() && raw.ends_with('\n') {
        new_content.push('\n');
    }

    atomic_write(&path, &new_content)?;
    Ok(removed)
}

/// Write content atomically: temp file in the same directory + rename.
fn atomic_write(path: &PathBuf, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create memory directory: {}", e))?;
    }
    let tmp = path.with_extension("md.tmp");
    fs::write(&tmp, content).map_err(|e| format!("Failed to write temp file: {}", e))?;
    fs::rename(&tmp, path).map_err(|e| format!("Failed to update memory index: {}", e))?;
    Ok(())
}

/// MemoryWriteTool — append a one-line fact to the memory index.
#[derive(Clone)]
pub struct MemoryWriteTool;

#[tool(
    name = "memory_write",
    description = r#"Add a single one-line fact to the memory index (MEMORY.md). Memory persists across sessions and is injected into every prompt, so keep facts short and durable (conventions, decisions, preferences).

For detailed notes, do NOT stack long facts here: load the `memory` skill and write a topic file instead, then add a one-line pointer to the index.

Params:
- content: the fact, on a single line
- scope: "project" (default, shared project knowledge in .shai/memory/) or "global" (personal cross-project preferences in ~/.config/shai/memory/)

**Examples:**
```json
{"content": "Always run cargo fmt before committing"}
{"content": "User prefers concise answers without emojis", "scope": "global"}
```
"#,
    capabilities = [ToolCapability::Write]
)]
impl MemoryWriteTool {
    pub fn new() -> Self {
        Self
    }

    async fn execute(&self, params: MemoryWriteParams) -> ToolResult {
        match write_fact(&params.content, params.scope) {
            Ok(WriteOutcome::Added(path)) => ToolResult::success(format!(
                "Memory saved to '{}' ({} index).",
                path.display(),
                params.scope
            )),
            Ok(WriteOutcome::AlreadyStored(path)) => ToolResult::success(format!(
                "This fact is already stored in '{}'. No changes made.",
                path.display()
            )),
            Err(e) => ToolResult::error(e),
        }
    }
}

impl Default for MemoryWriteTool {
    fn default() -> Self {
        Self::new()
    }
}

/// MemoryRemoveTool — remove a fact from the memory index.
#[derive(Clone)]
pub struct MemoryRemoveTool;

#[tool(
    name = "memory_remove",
    description = r#"Remove a fact from the memory index (MEMORY.md). Matches by exact fact text first, then by unique substring. If several facts match, the call fails and lists candidates — retry with more specific wording.

Use this to prune stale or superseded facts and keep the index lean.

Params:
- content: the fact (or a unique fragment of it) to remove
- scope: "project" (default) or "global"

**Examples:**
```json
{"content": "Always run cargo fmt before committing"}
{"content": "emojis", "scope": "global"}
```
"#,
    capabilities = [ToolCapability::Write]
)]
impl MemoryRemoveTool {
    pub fn new() -> Self {
        Self
    }

    async fn execute(&self, params: MemoryRemoveParams) -> ToolResult {
        match remove_fact(&params.content, params.scope) {
            Ok(removed) => ToolResult::success(format!(
                "Removed {} fact(s) from the {} memory index:\n{}",
                removed.len(),
                params.scope,
                removed
                    .iter()
                    .map(|f| format!("- {}", f))
                    .collect::<Vec<_>>()
                    .join("\n")
            )),
            Err(e) => ToolResult::error(e),
        }
    }
}

impl Default for MemoryRemoveTool {
    fn default() -> Self {
        Self::new()
    }
}
