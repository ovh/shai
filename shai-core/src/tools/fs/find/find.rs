use super::structs::{FindToolParams, FindType, SearchResult};
use crate::tools::{tool, ToolResult};
use regex::Regex;
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use walkdir::WalkDir;

pub struct FindTool {
    default_exclude_patterns: Vec<String>,
}

impl Default for FindTool {
    fn default() -> Self {
        Self::new()
    }
}

impl FindTool {
    pub fn new() -> Self {
        Self {
            default_exclude_patterns: Vec::new(),
        }
    }

    pub fn with_exclude_patterns(mut self, patterns: Vec<String>) -> Self {
        self.default_exclude_patterns = patterns;
        self
    }

    fn should_include_file(
        &self,
        path: &Path,
        include_extensions: &Option<String>,
        exclude_patterns: &Option<String>,
    ) -> bool {
        let path_str = path.to_string_lossy();

        // Check default exclude patterns from config
        for pattern in &self.default_exclude_patterns {
            if path_str.contains(pattern.as_str()) {
                return false;
            }
        }

        // Check per-call exclude patterns
        if let Some(exclude) = exclude_patterns {
            for pattern in exclude.split(',') {
                let pattern = pattern.trim();
                if !pattern.is_empty() && path_str.contains(pattern) {
                    return false;
                }
            }
        }

        // Check include extensions
        if let Some(include) = include_extensions {
            if let Some(ext) = path.extension() {
                let ext_str = ext.to_string_lossy();
                for allowed_ext in include.split(',') {
                    let allowed_ext = allowed_ext.trim();
                    if !allowed_ext.is_empty() && ext_str == allowed_ext {
                        return true;
                    }
                }
                return false; // Has extension but not in allowed list
            } else {
                return false; // No extension but extensions are specified
            }
        }

        true
    }

    fn search_file_content(
        &self,
        file_path: &Path,
        pattern: &Regex,
        params: &FindToolParams,
    ) -> Vec<SearchResult> {
        let mut results = Vec::new();

        let file = match fs::File::open(file_path) {
            Ok(file) => file,
            Err(_) => return results,
        };

        let reader = BufReader::new(file);
        let lines: Vec<String> = reader
            .lines()
            .collect::<Result<Vec<_>, _>>()
            .unwrap_or_default();

        for (line_num, line) in lines.iter().enumerate() {
            if pattern.is_match(line) {
                let line_number = (line_num + 1) as u32;
                let context_lines = params.context_lines.unwrap_or(0);

                let mut context_before = Vec::new();
                let mut context_after = Vec::new();

                if context_lines > 0 {
                    let start = line_num.saturating_sub(context_lines as usize);
                    let end = std::cmp::min(line_num + context_lines as usize + 1, lines.len());

                    context_before = lines[start..line_num].to_vec();
                    context_after = lines[line_num + 1..end].to_vec();
                }

                results.push(SearchResult {
                    file_path: file_path.to_string_lossy().to_string(),
                    line_number: if params.show_line_numbers {
                        Some(line_number)
                    } else {
                        None
                    },
                    line_content: Some(line.clone()),
                    context_before,
                    context_after,
                    match_type: "content".to_string(),
                });

                if results.len() >= params.max_results as usize {
                    break;
                }
            }
        }

        results
    }

    fn search_filename(&self, file_path: &Path, pattern: &Regex) -> Option<SearchResult> {
        let filename = file_path.file_name()?.to_string_lossy();

        if pattern.is_match(&filename) {
            Some(SearchResult {
                file_path: file_path.to_string_lossy().to_string(),
                line_number: None, // Always None for filename searches
                line_content: None,
                context_before: vec![],
                context_after: vec![],
                match_type: "filename".to_string(),
            })
        } else {
            None
        }
    }
}

#[tool(name = "find", description = r#"A high-performance search utility for locating files or specific text within files across the project.

**Core Functionality:**
- Employs regular expressions for powerful content searches, allowing for complex pattern matching.
- Can also locate files based on a pattern in their name.
- Use the `find_type` parameter (`'content'`, `'filename'`, or `'both'`) to control the search mode.

**Filtering and Scope:**
- Narrow your search to specific file types by providing a comma-separated list of extensions to `include_extensions` (e.g., 'rs,js,py').
- Exclude irrelevant directories and files (like `target` or `.git`) using the `exclude_patterns` parameter to speed up the search.

**Output:**
- Returns a list of matching file paths, sorted with the most recently modified files appearing first. This helps prioritize recently changed files.

**IMPORTANT:** This is the preferred tool for searching files by name or content. Do not use `bash` with `grep` or `find` commands — use this tool instead.

**Examples:**
Search file contents for a pattern:
```json
{"pattern": "fn main", "find_type": "content"}
```
Search filenames matching a pattern:
```json
{"pattern": "\\.rs$", "find_type": "filename"}
```
Search only in specific file types:
```json
{"pattern": "TODO", "find_type": "content", "include_extensions": "rs,py"}
```
"#, capabilities = [ToolCapability::Read])]

impl FindTool {
    async fn execute(&self, params: FindToolParams) -> ToolResult {
        let mut meta = HashMap::new();
        meta.insert("pattern".to_string(), json!(params.pattern));
        let default_path = ".".to_string();
        let search_path = params.path.as_ref().unwrap_or(&default_path);
        meta.insert("path".to_string(), json!(search_path));
        meta.insert("case_sensitive".to_string(), json!(params.case_sensitive));
        meta.insert("max_results".to_string(), json!(params.max_results));
        meta.insert(
            "find_type".to_string(),
            json!(format!("{:?}", params.find_type)),
        );

        // Create regex pattern
        let regex_flags = if params.case_sensitive { "" } else { "(?i)" };

        let pattern_str = if params.whole_word {
            format!("{}\\b{}\\b", regex_flags, regex::escape(&params.pattern))
        } else {
            format!("{}{}", regex_flags, params.pattern)
        };

        let pattern = match Regex::new(&pattern_str) {
            Ok(regex) => regex,
            Err(e) => {
                return ToolResult::Error {
                    error: format!("Invalid regex pattern: {}", e),
                    metadata: Some(meta),
                };
            }
        };

        let mut all_results = Vec::new();

        // Walk through directory
        for entry in WalkDir::new(search_path)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();

            // Skip directories for content search
            if path.is_dir() {
                continue;
            }

            // Apply file filters
            if !self.should_include_file(path, &params.include_extensions, &params.exclude_patterns)
            {
                continue;
            }

            // Search based on find_type
            match params.find_type {
                FindType::Content => {
                    let mut content_results = self.search_file_content(path, &pattern, &params);
                    all_results.append(&mut content_results);
                }
                FindType::Filename => {
                    if let Some(filename_result) = self.search_filename(path, &pattern) {
                        all_results.push(filename_result);
                    }
                }
                FindType::Both => {
                    // Search filename first
                    if let Some(filename_result) = self.search_filename(path, &pattern) {
                        all_results.push(filename_result);
                    }
                    // Then search content
                    let mut content_results = self.search_file_content(path, &pattern, &params);
                    all_results.append(&mut content_results);
                }
            }

            if all_results.len() >= params.max_results as usize {
                break;
            }
        }

        // Truncate results to max_results
        all_results.truncate(params.max_results as usize);

        meta.insert("results_count".to_string(), json!(all_results.len()));

        ToolResult::Success {
            output: serde_json::to_string_pretty(&all_results).unwrap_or_default(),
            metadata: Some(meta),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shai_llm::ToolDescription;

    #[test]
    fn test_find_schema_required_fields() {
        let tool = FindTool::new();
        let schema = tool.parameters_schema();

        // Verify all expected properties exist
        let props = schema["properties"]
            .as_object()
            .expect("properties should be an object");
        assert!(props.contains_key("pattern"));
        assert!(props.contains_key("path"));
        assert!(props.contains_key("include_extensions"));
        assert!(props.contains_key("exclude_patterns"));
        assert!(props.contains_key("max_results"));
        assert!(props.contains_key("case_sensitive"));
        assert!(props.contains_key("find_type"));
        assert!(props.contains_key("show_line_numbers"));
        assert!(props.contains_key("context_lines"));
        assert!(props.contains_key("whole_word"));
    }
}
