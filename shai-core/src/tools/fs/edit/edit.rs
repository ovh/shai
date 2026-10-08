use super::super::{FsOperationLog, FsOperationType};
use super::structs::EditToolParams;
use crate::tools::fs::hash::compute_line_hash;
use crate::tools::{tool, ToolResult};
use serde_json::json;
use similar::{ChangeTag, TextDiff};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

#[derive(Clone)]
pub struct EditTool {
    operation_log: Arc<FsOperationLog>,
    context_lines: usize,
}

impl EditTool {
    pub fn new(operation_log: Arc<FsOperationLog>) -> Self {
        Self::with_context_lines(operation_log, 3)
    }

    pub fn with_context_lines(operation_log: Arc<FsOperationLog>, context_lines: usize) -> Self {
        Self {
            operation_log,
            context_lines,
        }
    }

    pub fn myers_diff(&self, before_content: &str, after_content: &str) -> String {
        let diff = TextDiff::from_lines(before_content, after_content);

        let has_changes = diff
            .iter_all_changes()
            .any(|change| change.tag() != ChangeTag::Equal);
        if !has_changes {
            return "No changes".to_string();
        }

        #[derive(Debug)]
        struct ChangeInfo {
            tag: ChangeTag,
            content: String,
            old_line: usize,
            new_line: usize,
        }

        let mut all_changes = Vec::new();
        let mut line_num_old = 1;
        let mut line_num_new = 1;

        for change in diff.iter_all_changes() {
            let change_info = ChangeInfo {
                tag: change.tag(),
                content: change.value().trim_end().to_string(),
                old_line: line_num_old,
                new_line: line_num_new,
            };

            all_changes.push(change_info);

            match change.tag() {
                ChangeTag::Delete => line_num_old += 1,
                ChangeTag::Insert => line_num_new += 1,
                ChangeTag::Equal => {
                    line_num_old += 1;
                    line_num_new += 1;
                }
            }
        }

        let mut change_positions = Vec::new();
        for (idx, change) in all_changes.iter().enumerate() {
            if change.tag != ChangeTag::Equal {
                change_positions.push(idx);
            }
        }

        if change_positions.is_empty() {
            return "No changes".to_string();
        }

        let mut ranges_to_show = Vec::new();
        let context = self.context_lines;

        let mut current_start = change_positions[0].saturating_sub(context);
        let mut current_end = (change_positions[0] + context + 1).min(all_changes.len());

        for &pos in &change_positions[1..] {
            let range_start = pos.saturating_sub(context);
            let range_end = (pos + context + 1).min(all_changes.len());

            if range_start <= current_end {
                current_end = range_end;
            } else {
                ranges_to_show.push((current_start, current_end));
                current_start = range_start;
                current_end = range_end;
            }
        }
        ranges_to_show.push((current_start, current_end));

        let mut diff_output = Vec::new();

        for (range_idx, &(start, end)) in ranges_to_show.iter().enumerate() {
            if range_idx > 0 {
                diff_output.push("\x1b[2;37m...\x1b[0m".to_string());
            }

            for change in &all_changes[start..end] {
                let (sign, style) = match change.tag {
                    ChangeTag::Delete => ("-", "\x1b[48;5;88;37m"),
                    ChangeTag::Insert => ("+", "\x1b[48;5;28;37m"),
                    ChangeTag::Equal => (" ", ""),
                };

                let line_no = match change.tag {
                    ChangeTag::Delete => change.old_line,
                    ChangeTag::Insert => change.new_line,
                    ChangeTag::Equal => change.old_line,
                };

                if change.tag == ChangeTag::Equal {
                    diff_output.push(format!(
                        "\x1b[2;37m{:4}\x1b[0m   {}",
                        line_no, change.content
                    ));
                } else {
                    diff_output.push(format!(
                        "\x1b[2;37m{:4}\x1b[0m {}{} {}\x1b[0m",
                        line_no, style, sign, change.content
                    ));
                }
            }
        }

        diff_output.join("\n")
    }

    pub fn perform_edit_on_content(
        &self,
        content: &str,
        old_string: &str,
        new_string: &str,
        replace_all: bool,
        line_hash: Option<&str>,
        insert_after_hash: Option<&str>,
    ) -> Result<(String, usize), String> {
        // Handle insert after hash
        if let Some(hash) = insert_after_hash {
            let lines: Vec<&str> = content.lines().collect();
            let mut new_lines: Vec<String> = Vec::new();
            let mut insertions = 0;

            for line in &lines {
                new_lines.push(line.to_string());
                if compute_line_hash(line) == hash && (replace_all || insertions == 0) {
                    new_lines.push(new_string.to_string());
                    insertions += 1;
                }
            }

            if insertions == 0 {
                return Err(format!(
                    "Line hash '{}' not found in file (file may have changed)",
                    hash
                ));
            }

            return Ok((new_lines.join("\n"), insertions));
        }

        // Handle line hash replacement
        if let Some(hash) = line_hash {
            let lines: Vec<&str> = content.lines().collect();
            let mut replacements = 0;
            let mut new_lines: Vec<String> = Vec::new();

            for line in &lines {
                if compute_line_hash(line) == hash && (replace_all || replacements == 0) {
                    new_lines.push(new_string.to_string());
                    replacements += 1;
                } else {
                    new_lines.push(line.to_string());
                }
            }

            if replacements == 0 {
                return Err(format!(
                    "Line hash '{}' not found in file (file may have changed)",
                    hash
                ));
            }

            return Ok((new_lines.join("\n"), replacements));
        }

        // Check if the old_string exists in the content
        if !content.contains(old_string) {
            return Err(format!(
                "Pattern '{}' not found in file. Re-read the file to get current content.",
                old_string.chars().take(80).collect::<String>()
            ));
        }

        // Perform the replacement
        let (new_content, replacements) = if replace_all {
            let new_content = content.replace(old_string, new_string);
            let replacements = content.matches(old_string).count();
            (new_content, replacements)
        } else {
            let new_content = content.replacen(old_string, new_string, 1);
            (new_content, 1)
        };

        Ok((new_content, replacements))
    }

    pub fn commit_edit(&self, path: &str, new_content: &str) -> Result<(), String> {
        fs::write(path, new_content).map_err(|e| e.to_string())
    }
}

#[tool(name = "edit", description = r#"Executes find-and-replace operations across one or more files atomically. All edits are applied in memory first; if any edit fails, no files are modified.

**Parameters:**
- `files`: Array of `{ path, edits: [{ old_string, new_string, replace_all?, line_hash?, insert_after_hash? }] }`

**Edit Modes:**
- **String replacement**: Set `old_string` and `new_string` to replace text within the file.
- **Hash-anchored replacement**: Set `line_hash` to target specific line(s) by their hash from the `read` output. Replaces the entire line(s) with `new_string`.
- **Insertion**: Set `insert_after_hash` to insert `new_string` after the line(s) matching the hash.

**Critical:**
- You must first use the `read` tool to inspect any file before editing it.
- Edits within each file are applied sequentially.
- The entire operation is atomic — if any edit fails, no files are modified.

**Examples:**
Replace text in a file:
```json
{"files": [{"path": "src/main.rs", "edits": [{"old_string": "fn main() {", "new_string": "fn main() -> Result<(), Box<dyn std::error::Error> {"}]}]}
```
Replace all occurrences:
```json
{"files": [{"path": "src/lib.rs", "edits": [{"old_string": "todo!()", "new_string": "unimplemented!()", "replace_all": true}]}]}
```
Edit multiple files atomically:
```json
{"files": [{"path": "src/mod.rs", "edits": [{"old_string": "foo", "new_string": "bar"}]}, {"path": "src/lib.rs", "edits": [{"old_string": "baz", "new_string": "qux"}]}]}
```
"#, capabilities = [ToolCapability::Read, ToolCapability::Write])]
impl EditTool {
    async fn execute_preview(&self, params: EditToolParams) -> Option<ToolResult> {
        Some(self.execute_internal(params, true).await)
    }

    async fn execute(&self, params: EditToolParams) -> ToolResult {
        self.execute_internal(params, false).await
    }

    async fn execute_internal(&self, params: EditToolParams, preview: bool) -> ToolResult {
        if params.files.is_empty() {
            return ToolResult::error("At least one file edit is required".to_string());
        }

        // Phase 1: Validate all files have been read
        for file_edit in &params.files {
            if let Err(err) = self
                .operation_log
                .validate_edit_permission(&file_edit.path)
                .await
            {
                return ToolResult::error(err);
            }
        }

        // Phase 2: Apply all edits in memory
        let mut staged_writes: Vec<(String, String)> = Vec::new();
        let mut diffs = Vec::new();

        for file_edit in &params.files {
            let path = Path::new(&file_edit.path);

            if !path.exists() {
                return ToolResult::error(format!("File does not exist: {}", file_edit.path));
            }

            let mut current_content = match fs::read_to_string(path) {
                Ok(c) => c,
                Err(e) => return ToolResult::error(format!("Failed to read file: {}", e)),
            };
            let original_content = current_content.clone();

            for (index, edit) in file_edit.edits.iter().enumerate() {
                match self.perform_edit_on_content(
                    &current_content,
                    &edit.old_string,
                    &edit.new_string,
                    edit.replace_all,
                    edit.line_hash.as_deref(),
                    edit.insert_after_hash.as_deref(),
                ) {
                    Ok((new_content, _replacements)) => {
                        current_content = new_content;
                    }
                    Err(error) => {
                        return ToolResult::error(format!(
                            "File '{}', Edit #{}: {}",
                            file_edit.path,
                            index + 1,
                            error
                        ));
                    }
                }
            }

            let diff = self.myers_diff(&original_content, &current_content);
            diffs.push((file_edit.path.clone(), diff));
            staged_writes.push((file_edit.path.clone(), current_content));
        }

        // Phase 3: Write all files (only after all edits succeeded)
        if !preview {
            for (file_path, content) in &staged_writes {
                if let Err(e) = self.commit_edit(file_path, content) {
                    return ToolResult::error(format!("Failed to write '{}': {}", file_path, e));
                }
            }
        }

        // Phase 4: Log all operations
        if !preview {
            for file_edit in &params.files {
                self.operation_log
                    .log_operation(FsOperationType::Edit, file_edit.path.clone())
                    .await;
            }
        }

        let mut meta = HashMap::new();
        meta.insert("file_count".to_string(), json!(params.files.len()));
        meta.insert(
            "total_edits".to_string(),
            json!(params.files.iter().map(|f| f.edits.len()).sum::<usize>()),
        );
        meta.insert("preview_mode".to_string(), json!(preview));

        let file_details: Vec<serde_json::Value> = params
            .files
            .iter()
            .map(|f| {
                json!({
                    "path": f.path,
                    "edit_count": f.edits.len(),
                })
            })
            .collect();
        meta.insert("files".to_string(), json!(file_details));

        let combined_diff = diffs
            .iter()
            .map(|(path, diff)| format!("=== {} ===\n{}", path, diff))
            .collect::<Vec<_>>()
            .join("\n\n");

        ToolResult::Success {
            output: combined_diff,
            metadata: Some(meta),
        }
    }
}
