#[cfg(test)]
mod integration_tests {
    use crate::tools::fs::{
        edit::structs::{EditOperation, EditToolParams, FileEdit},
        find::structs::FindToolParams,
        ls::structs::LsToolParams,
        read::structs::{ReadFileSpec, ReadToolParams},
        write::structs::{WriteFileSpec, WriteToolParams},
    };
    use crate::tools::{EditTool, FindTool, FsOperationLog, LsTool, ReadTool, Tool, WriteTool};
    use std::sync::Arc;
    use tempfile::tempdir;

    /// Test 1: Basic file operations workflow
    /// Tests: write -> ls -> read -> edit in sequence
    #[tokio::test]
    async fn test_basic_file_operations_workflow() {
        let temp_dir = tempdir().unwrap();
        let temp_path = temp_dir.path();

        let fs_log = Arc::new(FsOperationLog::new());

        let ls_tool = LsTool::new();
        let write_tool = WriteTool::new(fs_log.clone());
        let read_tool = ReadTool::new(fs_log.clone());
        let edit_tool = EditTool::new(fs_log.clone());

        // 1. List empty directory
        let ls_result = ls_tool
            .execute(
                LsToolParams {
                    directory: temp_path.to_string_lossy().to_string(),
                    recursive: false,
                    show_hidden: false,
                    long_format: false,
                    max_depth: None,
                    max_files: None,
                },
                None,
            )
            .await;
        assert!(ls_result.is_success());

        // 2. Write a file
        let file_path = temp_path.join("test.txt");
        let write_result = write_tool
            .execute(
                WriteToolParams {
                    files: vec![WriteFileSpec {
                        path: file_path.to_string_lossy().to_string(),
                        content: "Hello, World!\nThis is a test file.".to_string(),
                    }],
                },
                None,
            )
            .await;
        assert!(write_result.is_success());

        // 3. List directory again - should show the new file
        let ls_result = ls_tool
            .execute(
                LsToolParams {
                    directory: temp_path.to_string_lossy().to_string(),
                    recursive: false,
                    show_hidden: false,
                    long_format: false,
                    max_depth: None,
                    max_files: None,
                },
                None,
            )
            .await;
        assert!(ls_result.is_success());
        if let crate::tools::types::ToolResult::Success { output, .. } = ls_result {
            assert!(output.contains("test.txt"));
        }

        // 4. Read the file
        let read_result = read_tool
            .execute(
                ReadToolParams {
                    files: vec![ReadFileSpec {
                        path: file_path.to_string_lossy().to_string(),
                        offset: None,
                        limit: None,
                        outline: false,
                    }],
                },
                None,
            )
            .await;
        assert!(read_result.is_success());
        if let crate::tools::types::ToolResult::Success { output, .. } = read_result {
            assert!(output.contains("Hello, World!"));
            assert!(output.contains("This is a test file."));
        }

        // 5. Edit the file (should work since we read it)
        let edit_result = edit_tool
            .execute(
                EditToolParams {
                    files: vec![FileEdit {
                        path: file_path.to_string_lossy().to_string(),
                        edits: vec![
                            EditOperation {
                                old_string: "Hello, World!".to_string(),
                                new_string: "Hello, Universe!".to_string(),
                                replace_all: false,
                                line_hash: None,
                                insert_after_hash: None,
                            },
                            EditOperation {
                                old_string: "test file".to_string(),
                                new_string: "example document".to_string(),
                                replace_all: false,
                                line_hash: None,
                                insert_after_hash: None,
                            },
                        ],
                    }],
                },
                None,
            )
            .await;
        assert!(edit_result.is_success());

        // 6. Read final result to verify all edits
        let final_read = read_tool
            .execute(
                ReadToolParams {
                    files: vec![ReadFileSpec {
                        path: file_path.to_string_lossy().to_string(),
                        offset: None,
                        limit: None,
                        outline: false,
                    }],
                },
                None,
            )
            .await;
        assert!(final_read.is_success());
        if let crate::tools::types::ToolResult::Success { output, .. } = final_read {
            assert!(output.contains("Hello, Universe!"));
            assert!(output.contains("example document"));
        }
    }

    /// Test 2: Edit validation - files must be read before editing
    #[tokio::test]
    async fn test_edit_requires_read_first() {
        let temp_dir = tempdir().unwrap();
        let temp_path = temp_dir.path();

        let fs_log = Arc::new(FsOperationLog::new());
        let write_tool = WriteTool::new(fs_log.clone());
        let read_tool = ReadTool::new(fs_log.clone());
        let edit_tool = EditTool::new(fs_log.clone());

        let file1_path = temp_path.join("file1.txt");
        let file2_path = temp_path.join("file2.txt");

        // Write both files
        let _ = write_tool
            .execute(
                WriteToolParams {
                    files: vec![WriteFileSpec {
                        path: file1_path.to_string_lossy().to_string(),
                        content: "Content of file 1".to_string(),
                    }],
                },
                None,
            )
            .await;

        let _ = write_tool
            .execute(
                WriteToolParams {
                    files: vec![WriteFileSpec {
                        path: file2_path.to_string_lossy().to_string(),
                        content: "Content of file 2".to_string(),
                    }],
                },
                None,
            )
            .await;

        // Try to edit file1 without reading it first - should fail
        let edit_result = edit_tool
            .execute(
                EditToolParams {
                    files: vec![FileEdit {
                        path: file1_path.to_string_lossy().to_string(),
                        edits: vec![EditOperation {
                            old_string: "Content".to_string(),
                            new_string: "Modified content".to_string(),
                            replace_all: false,
                            line_hash: None,
                            insert_after_hash: None,
                        }],
                    }],
                },
                None,
            )
            .await;
        assert!(edit_result.is_error());

        // Now read file1 and try editing again - should succeed
        let _ = read_tool
            .execute(
                ReadToolParams {
                    files: vec![ReadFileSpec {
                        path: file1_path.to_string_lossy().to_string(),
                        offset: None,
                        limit: None,
                        outline: false,
                    }],
                },
                None,
            )
            .await;

        let edit_result = edit_tool
            .execute(
                EditToolParams {
                    files: vec![FileEdit {
                        path: file1_path.to_string_lossy().to_string(),
                        edits: vec![EditOperation {
                            old_string: "Content".to_string(),
                            new_string: "Modified content".to_string(),
                            replace_all: false,
                            line_hash: None,
                            insert_after_hash: None,
                        }],
                    }],
                },
                None,
            )
            .await;
        assert!(edit_result.is_success());
    }

    /// Test 3: Complex multi-file operations with find tool
    #[tokio::test]
    async fn test_complex_multi_file_operations() {
        let temp_dir = tempdir().unwrap();
        let temp_path = temp_dir.path();

        let fs_log = Arc::new(FsOperationLog::new());
        let find_tool = FindTool::new();
        let write_tool = WriteTool::new(fs_log.clone());
        let read_tool = ReadTool::new(fs_log.clone());
        let edit_tool = EditTool::new(fs_log.clone());

        let files = vec![
            ("config.json", r#"{"name": "test", "version": "1.0"}"#),
            ("readme.txt", "This is a readme file\nWith multiple lines"),
            ("script.py", "print('Hello, Python!')\nprint('Second line')"),
            ("data.json", r#"{"items": [1, 2, 3], "active": true}"#),
        ];

        // Write all files
        for (filename, content) in &files {
            let file_path = temp_path.join(filename);
            let write_result = write_tool
                .execute(
                    WriteToolParams {
                        files: vec![WriteFileSpec {
                            path: file_path.to_string_lossy().to_string(),
                            content: content.to_string(),
                        }],
                    },
                    None,
                )
                .await;
            assert!(write_result.is_success());
        }

        // Use find to search for content in files
        let find_result = find_tool
            .execute(
                FindToolParams {
                    pattern: "name".to_string(),
                    path: Some(temp_path.to_string_lossy().to_string()),
                    include_extensions: Some("json".to_string()),
                    exclude_patterns: None,
                    max_results: 100,
                    case_sensitive: false,
                    find_type: crate::tools::fs::find::structs::FindType::Content,
                    show_line_numbers: false,
                    context_lines: None,
                    whole_word: false,
                },
                None,
            )
            .await;
        assert!(find_result.is_success());

        // Read and edit the config.json file
        let config_path = temp_path.join("config.json");
        let read_result = read_tool
            .execute(
                ReadToolParams {
                    files: vec![ReadFileSpec {
                        path: config_path.to_string_lossy().to_string(),
                        offset: None,
                        limit: None,
                        outline: false,
                    }],
                },
                None,
            )
            .await;
        assert!(read_result.is_success());

        // Edit the version in config.json
        let edit_result = edit_tool
            .execute(
                EditToolParams {
                    files: vec![FileEdit {
                        path: config_path.to_string_lossy().to_string(),
                        edits: vec![EditOperation {
                            old_string: r#""version": "1.0""#.to_string(),
                            new_string: r#""version": "2.0""#.to_string(),
                            replace_all: false,
                            line_hash: None,
                            insert_after_hash: None,
                        }],
                    }],
                },
                None,
            )
            .await;
        assert!(edit_result.is_success());

        // Read and modify the Python script
        let script_path = temp_path.join("script.py");
        let read_result = read_tool
            .execute(
                ReadToolParams {
                    files: vec![ReadFileSpec {
                        path: script_path.to_string_lossy().to_string(),
                        offset: None,
                        limit: None,
                        outline: false,
                    }],
                },
                None,
            )
            .await;
        assert!(read_result.is_success());

        let edit_result = edit_tool
            .execute(
                EditToolParams {
                    files: vec![FileEdit {
                        path: script_path.to_string_lossy().to_string(),
                        edits: vec![EditOperation {
                            old_string: "Hello, Python!".to_string(),
                            new_string: "Hello, World from Python!".to_string(),
                            replace_all: false,
                            line_hash: None,
                            insert_after_hash: None,
                        }],
                    }],
                },
                None,
            )
            .await;
        assert!(edit_result.is_success());

        // Verify final state by reading modified files
        let final_config_read = read_tool
            .execute(
                ReadToolParams {
                    files: vec![ReadFileSpec {
                        path: config_path.to_string_lossy().to_string(),
                        offset: None,
                        limit: None,
                        outline: false,
                    }],
                },
                None,
            )
            .await;
        assert!(final_config_read.is_success());
        if let crate::tools::types::ToolResult::Success { output, .. } = final_config_read {
            assert!(output.contains(r#""version": "2.0""#));
        }

        let final_script_read = read_tool
            .execute(
                ReadToolParams {
                    files: vec![ReadFileSpec {
                        path: script_path.to_string_lossy().to_string(),
                        offset: None,
                        limit: None,
                        outline: false,
                    }],
                },
                None,
            )
            .await;
        assert!(final_script_read.is_success());
        if let crate::tools::types::ToolResult::Success { output, .. } = final_script_read {
            assert!(output.contains("Hello, World from Python!"));
        }

        // Verify operation log tracked everything
        let operations = fs_log.get_all_operations().await;
        assert!(operations.len() >= 8);

        let read_files = fs_log.get_read_files().await;
        assert!(read_files.contains(&config_path.to_string_lossy().to_string()));
        assert!(read_files.contains(&script_path.to_string_lossy().to_string()));
    }
}
