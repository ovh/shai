use super::super::{FsOperationLog, FsOperationType};
use super::structs::{WriteFileSpec, WriteToolParams};
use crate::tools::{tool, ToolResult};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

#[derive(Clone)]
pub struct WriteTool {
    operation_log: Arc<FsOperationLog>,
}

impl WriteTool {
    pub fn new(operation_log: Arc<FsOperationLog>) -> Self {
        Self { operation_log }
    }

    fn perform_write(&self, params: &WriteFileSpec) -> Result<String, String> {
        let path = Path::new(&params.path);

        let file_existed = path.exists();

        if let Some(parent) = path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
        }

        fs::write(path, &params.content).map_err(|e| e.to_string())?;

        let action = if file_existed { "updated" } else { "created" };

        Ok(format!(
            "Successfully {} file '{}' with {} bytes",
            action,
            params.path,
            params.content.len()
        ))
    }
}

#[tool(name = "write", description = r#"Creates new files or completely overwrites existing files with specified content.

**Guidelines:**
- To overwrite an existing file, you must first have read it with the `read` tool. This is a safety measure to ensure you are aware of the content being replaced.
- This tool is primarily for creating new files when explicitly instructed. For modifying existing files, use the `edit` tool.
- Do not create files proactively, especially documentation. Only create files when the user's request cannot be fulfilled by modifying existing ones.

**Examples:**
Create a new file:
```json
{"files": [{"path": "src/new_module.rs", "content": "pub fn hello() {\n    println!(\"hello\");\n"}]}
```
Write multiple files:
```json
{"files": [{"path": "src/mod.rs", "content": "pub mod foo;"}, {"path": "src/foo.rs", "content": "// TODO"}]}
```
"#, capabilities = [Write])]
impl WriteTool {
    async fn execute_preview(&self, params: WriteToolParams) -> Option<ToolResult> {
        let mut outputs = Vec::new();
        let mut meta = HashMap::new();
        meta.insert("file_count".to_string(), json!(params.files.len()));

        for file_spec in &params.files {
            outputs.push(format!("=== {} ===\n{}", file_spec.path, file_spec.content));
        }

        Some(ToolResult::Success {
            output: outputs.join("\n\n"),
            metadata: Some(meta),
        })
    }

    async fn execute(&self, params: WriteToolParams) -> ToolResult {
        if params.files.is_empty() {
            return ToolResult::error("At least one file is required".to_string());
        }

        // Validate that existing files have been read first
        for file_spec in &params.files {
            if Path::new(&file_spec.path).exists() {
                if let Err(err) = self
                    .operation_log
                    .validate_edit_permission(&file_spec.path)
                    .await
                {
                    return ToolResult::error(err);
                }
            }
        }

        let mut outputs = Vec::new();
        let mut meta = HashMap::new();
        let mut file_details: Vec<serde_json::Value> = Vec::new();

        for file_spec in &params.files {
            match self.perform_write(file_spec) {
                Ok(message) => {
                    self.operation_log
                        .log_operation(FsOperationType::Write, file_spec.path.clone())
                        .await;

                    let detail = json!({
                        "path": file_spec.path,
                        "content_length": file_spec.content.len(),
                        "line_count": file_spec.content.lines().count(),
                    });
                    file_details.push(detail);
                    outputs.push(format!("{}: {}", file_spec.path, message));
                }
                Err(e) => {
                    return ToolResult::error(format!(
                        "Write failed for '{}': {}",
                        file_spec.path, e
                    ));
                }
            }
        }

        meta.insert("file_count".to_string(), json!(params.files.len()));
        meta.insert("files".to_string(), json!(file_details));

        ToolResult::Success {
            output: outputs.join("\n"),
            metadata: Some(meta),
        }
    }
}
