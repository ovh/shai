use super::ls::LsTool;
use super::structs::LsToolParams;
use crate::tools::{Tool, ToolCapability};
use shai_llm::ToolDescription;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_ls_tool_permissions() {
    let tool = LsTool::new();
    let caps = tool.capabilities();
    assert!(caps.contains(&ToolCapability::Read));
    assert_eq!(caps.len(), 1);
}

#[tokio::test]
async fn test_ls_tool_creation() {
    let tool = LsTool::new();
    assert_eq!(&tool.name(), "ls");
    assert!(!tool.description().is_empty());
}

#[tokio::test]
async fn test_ls_tool_basic_listing() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let temp_path = temp_dir.path();

    fs::write(temp_path.join("file1.txt"), "content1").unwrap();
    fs::write(temp_path.join("file2.txt"), "content2").unwrap();
    fs::write(temp_path.join("README.md"), "# README").unwrap();

    let tool = LsTool::new();
    let params = LsToolParams {
        directory: temp_path.to_string_lossy().to_string(),
        recursive: false,
        show_hidden: false,
        long_format: false,
        max_depth: None,
        max_files: None,
    };

    let result = tool.execute(params, None).await;
    match result {
        crate::tools::ToolResult::Success { output, .. } => {
            assert!(output.contains("file1.txt"));
            assert!(output.contains("file2.txt"));
            assert!(output.contains("README.md"));
        }
        _ => panic!("LsTool should succeed"),
    }
}

#[tokio::test]
async fn test_ls_tool_recursive() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let temp_path = temp_dir.path();

    fs::create_dir_all(temp_path.join("subdir")).unwrap();
    fs::write(temp_path.join("file1.txt"), "content1").unwrap();
    fs::write(temp_path.join("subdir/file2.txt"), "content2").unwrap();

    let tool = LsTool::new();
    let params = LsToolParams {
        directory: temp_path.to_string_lossy().to_string(),
        recursive: true,
        show_hidden: false,
        long_format: false,
        max_depth: None,
        max_files: None,
    };

    let result = tool.execute(params, None).await;
    match result {
        crate::tools::ToolResult::Success { output, .. } => {
            assert!(output.contains("file1.txt"));
            assert!(output.contains("file2.txt"));
            assert!(output.contains("subdir"));
        }
        _ => panic!("LsTool should succeed"),
    }
}

#[tokio::test]
async fn test_ls_tool_hidden_files() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let temp_path = temp_dir.path();

    fs::write(temp_path.join("visible.txt"), "content").unwrap();
    fs::write(temp_path.join(".hidden"), "hidden").unwrap();

    // Without show_hidden
    let tool = LsTool::new();
    let params = LsToolParams {
        directory: temp_path.to_string_lossy().to_string(),
        recursive: false,
        show_hidden: false,
        long_format: false,
        max_depth: None,
        max_files: None,
    };

    let result = tool.execute(params, None).await;
    match result {
        crate::tools::ToolResult::Success { output, .. } => {
            assert!(output.contains("visible.txt"));
            assert!(!output.contains(".hidden"));
        }
        _ => panic!("LsTool should succeed"),
    }

    // With show_hidden
    let params = LsToolParams {
        directory: temp_path.to_string_lossy().to_string(),
        recursive: false,
        show_hidden: true,
        long_format: false,
        max_depth: None,
        max_files: None,
    };

    let result = tool.execute(params, None).await;
    match result {
        crate::tools::ToolResult::Success { output, .. } => {
            assert!(output.contains("visible.txt"));
            assert!(output.contains(".hidden"));
        }
        _ => panic!("LsTool should succeed"),
    }
}

#[tokio::test]
async fn test_ls_tool_long_format() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let temp_path = temp_dir.path();

    fs::write(temp_path.join("file1.txt"), "content").unwrap();

    let tool = LsTool::new();
    let params = LsToolParams {
        directory: temp_path.to_string_lossy().to_string(),
        recursive: false,
        show_hidden: false,
        long_format: true,
        max_depth: None,
        max_files: None,
    };

    let result = tool.execute(params, None).await;
    match result {
        crate::tools::ToolResult::Success { output, .. } => {
            assert!(output.contains("file1.txt"));
        }
        _ => panic!("LsTool should succeed"),
    }
}

#[tokio::test]
async fn test_ls_tool_nonexistent_directory() {
    let tool = LsTool::new();
    let params = LsToolParams {
        directory: "/nonexistent/path/that/does/not/exist".to_string(),
        recursive: false,
        show_hidden: false,
        long_format: false,
        max_depth: None,
        max_files: None,
    };

    let result = tool.execute(params, None).await;
    match result {
        crate::tools::ToolResult::Error { error, .. } => {
            assert!(
                error.contains("does not exist") || error.contains("Failed"),
                "Error should mention the directory issue, got: {error}"
            );
        }
        _ => panic!("LsTool should return error for nonexistent directory"),
    }
}

#[tokio::test]
async fn test_ls_tool_max_files_limit() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let temp_path = temp_dir.path();

    for i in 0..10 {
        fs::write(temp_path.join(format!("file{i}.txt")), "content").unwrap();
    }

    let tool = LsTool::new();
    let params = LsToolParams {
        directory: temp_path.to_string_lossy().to_string(),
        recursive: false,
        show_hidden: false,
        long_format: false,
        max_depth: None,
        max_files: Some(5),
    };

    let result = tool.execute(params, None).await;
    match result {
        crate::tools::ToolResult::Success { output, .. } => {
            assert!(
                output.contains("truncated"),
                "Output should contain truncation message"
            );
        }
        _ => panic!("LsTool should succeed"),
    }
}

#[tokio::test]
async fn test_ls_tool_max_depth() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let temp_path = temp_dir.path();

    fs::create_dir_all(temp_path.join("a/b")).unwrap();
    fs::write(temp_path.join("root.txt"), "root").unwrap();
    fs::write(temp_path.join("a/file.txt"), "file").unwrap();
    fs::write(temp_path.join("a/b/deep.txt"), "deep").unwrap();

    let tool = LsTool::new();
    let params = LsToolParams {
        directory: temp_path.to_string_lossy().to_string(),
        recursive: true,
        show_hidden: false,
        long_format: false,
        max_depth: Some(1),
        max_files: None,
    };

    let result = tool.execute(params, None).await;
    match result {
        crate::tools::ToolResult::Success { output, .. } => {
            assert!(output.contains("root.txt"));
            assert!(output.contains("file.txt"));
            assert!(
                !output.contains("deep.txt"),
                "Files beyond max_depth should not be listed"
            );
        }
        _ => panic!("LsTool should succeed"),
    }
}
