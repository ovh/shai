use super::read::ReadTool;
use super::structs::{ReadFileSpec, ReadToolParams};
use crate::tools::{FsOperationLog, Tool, ToolCapability};
use shai_llm::ToolDescription;
use std::fs;
use std::sync::Arc;
use tempfile::TempDir;

#[test]
fn test_read_tool_creation() {
    let log = Arc::new(FsOperationLog::new());
    let tool = ReadTool::new(log);

    assert_eq!(&tool.name(), "read");
    assert!(!tool.description().is_empty());

    let capabilities = tool.capabilities();
    assert!(capabilities.contains(&ToolCapability::Read));
    assert_eq!(capabilities.len(), 1);
}

#[tokio::test]
async fn test_read_tool_basic_file_reading() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let temp_path = temp_dir.path();

    let test_content = r#"Hello World
This is a test file
With multiple lines
For reading tests
End of file"#;

    let test_file_path = temp_path.join("test.txt");
    fs::write(&test_file_path, test_content).expect("Failed to write test file");

    let log = Arc::new(FsOperationLog::new());
    let read_tool = ReadTool::new(log);

    let params = ReadToolParams {
        files: vec![ReadFileSpec {
            path: test_file_path.to_string_lossy().to_string(),
            offset: None,
            limit: None,
            outline: false,
        }],
    };

    let result = read_tool.execute(params, None).await;
    match result {
        crate::tools::ToolResult::Success { output, .. } => {
            assert!(output.contains("Hello World"), "Should contain Hello World");
            assert!(output.contains("End of file"), "Should contain End of file");
            assert!(
                output.contains("With multiple lines"),
                "Should contain all lines"
            );
        }
        crate::tools::ToolResult::Error { error, .. } => {
            panic!("Read tool should succeed, got error: {}", error);
        }
        crate::tools::ToolResult::Denied => {
            panic!("Read tool was denied");
        }
    }
}

#[tokio::test]
async fn test_read_tool_line_range_reading() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let temp_path = temp_dir.path();

    let test_content = (1..=20)
        .map(|i| format!("Line {}: Content for line {}", i, i))
        .collect::<Vec<_>>()
        .join("\n");

    let test_file_path = temp_path.join("numbered_test.txt");
    fs::write(&test_file_path, test_content).expect("Failed to write test file");

    let log = Arc::new(FsOperationLog::new());
    let read_tool = ReadTool::new(log);

    let params_range = ReadToolParams {
        files: vec![ReadFileSpec {
            path: test_file_path.to_string_lossy().to_string(),
            offset: Some(5),
            limit: Some(6),
            outline: false,
        }],
    };

    let result_range = read_tool.execute(params_range, None).await;
    match result_range {
        crate::tools::ToolResult::Success { output, .. } => {
            assert!(
                output.contains("Line 5: Content for line 5"),
                "Should contain line 5"
            );
            assert!(
                output.contains("Line 10: Content for line 10"),
                "Should contain line 10"
            );
            assert!(
                !output.contains("Line 4: Content for line 4"),
                "Should not contain line 4"
            );
            assert!(
                !output.contains("Line 11: Content for line 11"),
                "Should not contain line 11"
            );

            let line_count = output.lines().count();
            // 1 header + 6 content lines + 1 blank + 1 footer = 9
            assert_eq!(
                line_count, 9,
                "Should have exactly 9 lines (header + 6 content lines + footer)"
            );
        }
        crate::tools::ToolResult::Error { error, .. } => {
            panic!("Read tool range should succeed, got error: {}", error);
        }
        crate::tools::ToolResult::Denied => {
            panic!("Read tool was denied");
        }
    }
}

#[tokio::test]
async fn test_read_tool_nonexistent_file() {
    let log = Arc::new(FsOperationLog::new());
    let read_tool = ReadTool::new(log);

    let params = ReadToolParams {
        files: vec![ReadFileSpec {
            path: "/nonexistent/path/file.txt".to_string(),
            offset: None,
            limit: None,
            outline: false,
        }],
    };

    let result = read_tool.execute(params, None).await;
    match result {
        crate::tools::ToolResult::Success { output, .. } => {
            assert!(
                output.contains("does not exist"),
                "Should report file does not exist"
            );
        }
        crate::tools::ToolResult::Error { .. } => {
            // Also acceptable
        }
        crate::tools::ToolResult::Denied => {
            panic!("Read tool was denied");
        }
    }
}

#[tokio::test]
async fn test_read_tool_empty_params() {
    let log = Arc::new(FsOperationLog::new());
    let read_tool = ReadTool::new(log);

    let params = ReadToolParams { files: vec![] };

    let result = read_tool.execute(params, None).await;
    assert!(result.is_error());
}

#[tokio::test]
async fn test_read_tool_multi_file() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let file_a = temp_dir.path().join("a.txt");
    let file_b = temp_dir.path().join("b.rs");
    fs::write(&file_a, "Hello World\nLine 2").unwrap();
    fs::write(&file_b, "fn main() {}\n").unwrap();

    let log = Arc::new(FsOperationLog::new());
    let read_tool = ReadTool::new(log);

    let params = ReadToolParams {
        files: vec![
            ReadFileSpec {
                path: file_a.to_string_lossy().to_string(),
                offset: None,
                limit: None,
                outline: false,
            },
            ReadFileSpec {
                path: file_b.to_string_lossy().to_string(),
                offset: None,
                limit: None,
                outline: false,
            },
        ],
    };

    let result = read_tool.execute(params, None).await;
    match result {
        crate::tools::ToolResult::Success { output, .. } => {
            assert!(output.contains("Hello World"));
            assert!(output.contains("fn main()"));
            assert!(output.contains("a.txt"));
            assert!(output.contains("b.rs"));
        }
        crate::tools::ToolResult::Error { error, .. } => {
            panic!("Multi-read should succeed, got error: {}", error);
        }
        crate::tools::ToolResult::Denied => {
            panic!("Multi-read was denied");
        }
    }
}

#[tokio::test]
async fn test_read_tool_mixed_existence() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let file_a = temp_dir.path().join("exists.txt");
    fs::write(&file_a, "content").unwrap();

    let log = Arc::new(FsOperationLog::new());
    let read_tool = ReadTool::new(log);

    let params = ReadToolParams {
        files: vec![
            ReadFileSpec {
                path: file_a.to_string_lossy().to_string(),
                offset: None,
                limit: None,
                outline: false,
            },
            ReadFileSpec {
                path: "/nonexistent/file.txt".to_string(),
                offset: None,
                limit: None,
                outline: false,
            },
        ],
    };

    let result = read_tool.execute(params, None).await;
    match result {
        crate::tools::ToolResult::Success { output, .. } => {
            assert!(output.contains("content"));
            assert!(output.contains("does not exist"));
        }
        _ => panic!("Read should succeed even with missing files"),
    }
}

#[test]
fn test_find_exclude_patterns_config() {
    let config = crate::config::agent::CompactionConfig::default();
    assert!(!config.find_exclude_patterns.is_empty());
    assert!(config.find_exclude_patterns.contains(&".git".to_string()));
    assert!(config.find_exclude_patterns.contains(&"target".to_string()));
    assert!(config
        .find_exclude_patterns
        .contains(&"node_modules".to_string()));
}

#[test]
fn test_max_cached_reads_config() {
    let config = crate::config::agent::CompactionConfig::default();
    assert_eq!(config.max_cached_reads, 100);
}
