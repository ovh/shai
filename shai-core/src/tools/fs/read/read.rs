use super::super::{FsOperationLog, FsOperationType};
use super::structs::{ReadFileSpec, ReadToolParams};
use crate::tools::fs::hash::compute_line_hash;
use crate::tools::fs::symbol::{extract_symbols, format_outline, LanguageRegistry};
use crate::tools::{tool, ToolResult};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::io::{self, BufRead, BufReader, Read};
use std::path::Path;
use std::sync::Arc;

const DEFAULT_READ_LIMIT: u32 = 2000;
const MAX_LINE_LENGTH: usize = 2000;
const MAX_BYTES: usize = 50 * 1024;
const MAX_LINE_SUFFIX: &str = "... (line truncated to 2000 chars)";
const BINARY_SAMPLE_SIZE: usize = 4096;

const BINARY_EXTENSIONS: &[&str] = &[
    ".zip", ".tar", ".gz", ".exe", ".dll", ".so", ".class", ".jar", ".war", ".7z", ".doc", ".docx",
    ".xls", ".xlsx", ".ppt", ".pptx", ".odt", ".ods", ".odp", ".bin", ".dat", ".obj", ".o", ".lib",
    ".wasm", ".pyc", ".pyo", ".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".ico", ".tiff",
    ".pdf", ".mp3", ".mp4", ".avi", ".mov",
];

#[derive(Clone)]
pub struct ReadTool {
    operation_log: Arc<FsOperationLog>,
    language_registry: Arc<LanguageRegistry>,
}

impl ReadTool {
    pub fn new(operation_log: Arc<FsOperationLog>) -> Self {
        let language_registry = Arc::new(LanguageRegistry::new().unwrap_or_else(|e| {
            tracing::error!("Failed to initialize language registry: {}", e);
            panic!("Failed to initialize language registry: {}", e)
        }));
        Self {
            operation_log,
            language_registry,
        }
    }

    pub fn with_language_registry(
        operation_log: Arc<FsOperationLog>,
        language_registry: Arc<LanguageRegistry>,
    ) -> Self {
        Self {
            operation_log,
            language_registry,
        }
    }

    fn is_binary_file(path: &str) -> bool {
        if let Some(ext) = Path::new(path).extension().and_then(|e| e.to_str()) {
            let ext_lower = format!(".{}", ext.to_lowercase());
            if BINARY_EXTENSIONS.contains(&ext_lower.as_str()) {
                return true;
            }
        }

        let file = match fs::File::open(path) {
            Ok(f) => f,
            Err(_) => return false,
        };
        let mut reader = io::BufReader::new(file);
        let mut buf = vec![0u8; BINARY_SAMPLE_SIZE];
        let n = reader.read(&mut buf).unwrap_or(0);
        if n == 0 {
            return false;
        }

        let sample = &buf[..n];
        let mut non_printable = 0;
        for &b in sample {
            if b == 0 {
                return true;
            }
            if b < 9 || (b > 13 && b < 32) {
                non_printable += 1;
            }
        }
        (non_printable as f64 / n as f64) > 0.3
    }

    fn read_file_content(&self, params: &ReadFileSpec) -> io::Result<String> {
        // If outline mode is requested, try to produce a symbol outline
        if params.outline {
            if let Some(config) = self.language_registry.config_for_path(&params.path) {
                let content = fs::read_to_string(&params.path)?;
                let symbols = extract_symbols(&content, config);
                return Ok(format_outline(&symbols, &params.path));
            }
            // Fall through to normal read if language is unsupported
        }

        let offset = params.offset.unwrap_or(1).max(1) as usize;
        let limit = params.limit.unwrap_or(DEFAULT_READ_LIMIT) as usize;

        let file = fs::File::open(&params.path)?;
        let reader = BufReader::new(file);

        let mut lines: Vec<(u32, String)> = Vec::new();
        let mut total_lines = 0usize;
        let mut bytes_written = 0usize;

        for (i, line_result) in reader.lines().enumerate() {
            let line_num = i as u32 + 1;
            total_lines = line_num as usize;

            if line_num < offset as u32 {
                continue;
            }
            if lines.len() >= limit {
                break;
            }

            let line = line_result?;
            let truncated = if line.len() > MAX_LINE_LENGTH {
                format!(
                    "{}{}",
                    line.chars().take(MAX_LINE_LENGTH).collect::<String>(),
                    MAX_LINE_SUFFIX
                )
            } else {
                line
            };

            let line_size = truncated.len();
            if bytes_written + line_size > MAX_BYTES && !lines.is_empty() {
                break;
            }

            lines.push((line_num, truncated));
            bytes_written += line_size;
        }

        // Build output with line numbers and hashes
        let mut output = lines
            .iter()
            .map(|(line_num, content)| {
                let hash = compute_line_hash(content);
                format!("{:4}: {} {}", line_num, hash, content)
            })
            .collect::<Vec<_>>()
            .join("\n");

        // Add truncation footer
        let last_line = offset + lines.len() - 1;
        let truncated = lines.len() == limit as usize || (lines.len() as u32) < total_lines as u32;
        if truncated {
            if lines.len() == limit as usize {
                output.push_str(&format!(
                    "\n\n(Showing lines {}-{} of {}. Use offset={} to continue.)",
                    offset,
                    last_line,
                    total_lines,
                    last_line + 1
                ));
            } else {
                output.push_str(&format!(
                    "\n\n(Output capped at {}KB. Showing lines {}-{} of {}. Use offset={} to continue.)",
                    MAX_BYTES / 1024,
                    offset,
                    last_line,
                    total_lines,
                    last_line + 1
                ));
            }
        } else {
            output.push_str(&format!("\n\n(End of file - {} lines)", total_lines));
        }

        Ok(output)
    }
}

#[tool(name = "read", description = r#"Reads one or more files in a single call. Output always includes line numbers and hashes for precise editing.

**Usage:**
- Pass an array of file specs in the `files` parameter.
- Each spec requires a `path` and optionally `offset` (1-indexed start line) and `limit` (max lines to read, default 2000).
- Set `outline: true` to return a compact symbol outline instead of full file content. Useful for understanding file structure before reading specific sections.
- Lines longer than 2000 characters are truncated.
- Output is capped at 50KB per file; use `offset` to paginate through larger files.

**Best Practices:**
- When exploring an unfamiliar codebase, use `outline: true` on the first read to understand file structure.
- Use full reads (without `outline`) when you need exact content for editing.
- When investigating a task, read multiple potentially relevant files in a single call to build context.

**IMPORTANT:** This is the preferred tool for inspecting file contents. Do not use `bash` with `cat`, `less`, `head`, `tail`, or `bat` --- use this tool instead.

**Examples:**
Read a single file:
```json
{"files": [{"path": "src/main.rs"}]}
```
Read with offset and limit:
```json
{"files": [{"path": "src/main.rs", "offset": 50, "limit": 100}]}
```
Read multiple files:
```json
{"files": [{"path": "src/main.rs"}, {"path": "src/lib.rs"}]}
```
Get symbol outline:
```json
{"files": [{"path": "src/main.rs", "outline": true}]}
```
"#, capabilities = [Read])]
impl ReadTool {
    async fn execute(&self, params: ReadToolParams) -> ToolResult {
        if params.files.is_empty() {
            return ToolResult::error("At least one file path is required".to_string());
        }

        let mut outputs = Vec::new();
        let mut total_lines = 0usize;
        let mut file_count = 0usize;

        for file_spec in &params.files {
            let path = Path::new(&file_spec.path);

            if !path.exists() {
                outputs.push(format!(
                    "=== {} ===\n[Error: File does not exist]",
                    file_spec.path
                ));
                continue;
            }
            if !path.is_file() {
                outputs.push(format!(
                    "=== {} ===\n[Error: Path is not a file]",
                    file_spec.path
                ));
                continue;
            }

            // Check for binary files
            if Self::is_binary_file(&file_spec.path) {
                outputs.push(format!(
                    "=== {} ===\n[Error: Cannot read binary file]",
                    file_spec.path
                ));
                continue;
            }

            match self.read_file_content(file_spec) {
                Ok(content) => {
                    total_lines += content.lines().count();
                    file_count += 1;
                    self.operation_log
                        .log_operation(FsOperationType::Read, file_spec.path.clone())
                        .await;
                    outputs.push(format!("=== {} ===\n{}", file_spec.path, content));
                }
                Err(e) => {
                    outputs.push(format!("=== {} ===\n[Error: {}]", file_spec.path, e));
                }
            }
        }

        let mut meta = HashMap::new();
        meta.insert("file_count".to_string(), json!(file_count));
        meta.insert("total_lines".to_string(), json!(total_lines));

        ToolResult::Success {
            output: outputs.join("\n\n"),
            metadata: Some(meta),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shai_llm::ToolDescription;

    #[test]
    fn test_read_schema_required_fields() {
        let operation_log = Arc::new(FsOperationLog::new());
        let tool = ReadTool::new(operation_log);
        let schema = tool.parameters_schema();

        // The "files" field should be present in properties
        let files_prop = &schema["properties"]["files"];
        assert_eq!(files_prop["type"].as_str().unwrap_or(""), "array");
    }
}
