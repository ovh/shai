use super::structs::{FileInfo, LsToolParams};
use crate::tools::{tool, ToolResult};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

pub struct LsTool;

impl LsTool {
    pub fn new() -> Self {
        Self
    }

    fn get_file_info(&self, path: &Path) -> Result<FileInfo, Box<dyn std::error::Error>> {
        let metadata = fs::metadata(path)?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        let permissions = self.format_permissions(&metadata);

        Ok(FileInfo {
            name,
            path: path.to_string_lossy().to_string(),
            is_dir: metadata.is_dir(),
            size: metadata.len(),
            modified: metadata.modified().ok(),
            permissions,
        })
    }

    fn format_permissions(&self, metadata: &fs::Metadata) -> String {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = metadata.permissions().mode();
            let user = if mode & 0o400 != 0 { 'r' } else { '-' };
            let user_w = if mode & 0o200 != 0 { 'w' } else { '-' };
            let user_x = if mode & 0o100 != 0 { 'x' } else { '-' };
            let group = if mode & 0o040 != 0 { 'r' } else { '-' };
            let group_w = if mode & 0o020 != 0 { 'w' } else { '-' };
            let group_x = if mode & 0o010 != 0 { 'x' } else { '-' };
            let other = if mode & 0o004 != 0 { 'r' } else { '-' };
            let other_w = if mode & 0o002 != 0 { 'w' } else { '-' };
            let other_x = if mode & 0o001 != 0 { 'x' } else { '-' };

            let file_type = if metadata.is_dir() { 'd' } else { '-' };

            format!(
                "{}{}{}{}{}{}{}{}{}{}",
                file_type, user, user_w, user_x, group, group_w, group_x, other, other_w, other_x
            )
        }
        #[cfg(not(unix))]
        {
            if metadata.permissions().readonly() {
                "r--r--r--".to_string()
            } else {
                "rw-rw-rw-".to_string()
            }
        }
    }

    fn format_size(&self, size: u64) -> String {
        if size < 1024 {
            format!("{}B", size)
        } else if size < 1024 * 1024 {
            format!("{:.1}K", size as f64 / 1024.0)
        } else if size < 1024 * 1024 * 1024 {
            format!("{:.1}M", size as f64 / (1024.0 * 1024.0))
        } else {
            format!("{:.1}G", size as f64 / (1024.0 * 1024.0 * 1024.0))
        }
    }

    fn list_directory(
        &self,
        params: &LsToolParams,
        current_depth: u32,
        files_collected: &mut u32,
    ) -> Result<Vec<FileInfo>, Box<dyn std::error::Error>> {
        let path = Path::new(&params.directory);

        // Early return if we've hit the limit
        let max_files = params.max_files.unwrap_or(200);
        if *files_collected >= max_files {
            return Ok(Vec::new());
        }

        if !path.exists() {
            return Err(format!("Directory '{}' does not exist", params.directory).into());
        }

        if !path.is_dir() {
            return Err(format!("'{}' is not a directory", params.directory).into());
        }

        let mut files = Vec::new();

        // Check max depth
        if let Some(max_depth) = params.max_depth {
            if current_depth > max_depth {
                return Ok(files);
            }
        }

        let entries = fs::read_dir(path)?;
        let mut dir_entries: Vec<_> = entries.collect::<Result<Vec<_>, _>>()?;

        // Sort entries by name
        dir_entries.sort_by(|a, b| a.file_name().cmp(&b.file_name()));

        for entry in dir_entries {
            // Check if we've reached the max files limit
            if *files_collected >= max_files {
                break;
            }

            let entry_path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();

            // Skip hidden files unless requested
            if !params.show_hidden && name.starts_with('.') {
                continue;
            }

            match self.get_file_info(&entry_path) {
                Ok(file_info) => {
                    files.push(file_info.clone());
                    *files_collected += 1;

                    // Recurse into subdirectories if requested
                    if params.recursive && file_info.is_dir {
                        // The global counter will be checked at the start of the recursive call
                        let subdir_params = LsToolParams {
                            directory: entry_path.to_string_lossy().to_string(),
                            recursive: true,
                            show_hidden: params.show_hidden,
                            long_format: params.long_format,
                            max_depth: params.max_depth,
                            max_files: params.max_files, // Keep original params
                        };

                        match self.list_directory(
                            &subdir_params,
                            current_depth + 1,
                            files_collected,
                        ) {
                            Ok(mut subdirs) => files.append(&mut subdirs),
                            Err(_) => continue, // Skip inaccessible directories
                        }
                    }
                }
                Err(_) => continue, // Skip inaccessible files
            }
        }

        Ok(files)
    }

    fn format_output(&self, files: &[FileInfo], params: &LsToolParams) -> String {
        if files.is_empty() {
            return "No files found".to_string();
        }

        let truncated = if let Some(max_files) = params.max_files {
            files.len() >= max_files as usize
        } else {
            false
        };

        let base_output = if params.long_format {
            let mut output = Vec::new();
            for file in files {
                let size_str = self.format_size(file.size);
                let modified_str = file
                    .modified
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| {
                        let secs = d.as_secs();
                        let dt = chrono::DateTime::from_timestamp(secs as i64, 0)
                            .unwrap_or_else(|| chrono::Utc::now());
                        dt.format("%Y-%m-%d %H:%M").to_string()
                    })
                    .unwrap_or_else(|| "unknown".to_string());

                let file_type = if file.is_dir { "/" } else { "" };
                output.push(format!(
                    "{} {:>8} {} {}{}",
                    file.permissions, size_str, modified_str, file.name, file_type
                ));
            }
            output.join("\n")
        } else {
            // Show one file per line for better readability
            files
                .iter()
                .map(|f| {
                    if f.is_dir {
                        format!("{}/", f.name)
                    } else {
                        f.name.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        };

        if truncated {
            format!(
                "{}\n\n... (output truncated, showing first {} files)",
                base_output,
                files.len()
            )
        } else {
            base_output
        }
    }
}

#[tool(name = "ls", description = r#"Provides a directory listing, showing the files and subdirectories contained within a specified location. It is your tool for exploring the file system structure.

**Usage:**
- The `directory` parameter must be an absolute path to the location you wish to inspect.
- By default, lists files non-recursively to avoid overwhelming output.
- Set `recursive: true` to include subdirectories (use with caution in large directories).
- Default limit of 200 files prevents excessive output. Increase `max_files` if you need more, or set to `null` for unlimited.

**Recommendations:**
- For large directories, consider using the `find` tool instead, which offers powerful filtering and search capabilities.
- Use `recursive: true` carefully, especially in directories like `node_modules/` which contain thousands of files."#, capabilities = [ToolCapability::Read])]
impl LsTool {
    async fn execute(&self, params: LsToolParams) -> ToolResult {
        let mut files_collected = 0;
        match self.list_directory(&params, 0, &mut files_collected) {
            Ok(files) => {
                let output = self.format_output(&files, &params);

                let mut meta = HashMap::new();
                meta.insert("directory".to_string(), json!(params.directory));
                meta.insert("file_count".to_string(), json!(files.len()));
                meta.insert("recursive".to_string(), json!(params.recursive));
                meta.insert("show_hidden".to_string(), json!(params.show_hidden));
                meta.insert("long_format".to_string(), json!(params.long_format));

                if let Some(max_depth) = params.max_depth {
                    meta.insert("max_depth".to_string(), json!(max_depth));
                }
                if let Some(max_files) = params.max_files {
                    meta.insert("max_files".to_string(), json!(max_files));
                    meta.insert(
                        "truncated".to_string(),
                        json!(files.len() >= max_files as usize),
                    );
                }

                ToolResult::Success {
                    output,
                    metadata: Some(meta),
                }
            }
            Err(e) => ToolResult::error(format!("Failed to list directory: {}", e)),
        }
    }
}
