use super::structs::{WriteFileSpec, WriteToolParams};
use super::write::WriteTool;
use crate::tools::{FsOperationLog, Tool, ToolCapability};
use shai_llm::ToolDescription;
use std::fs;
use std::sync::Arc;
use tempfile::tempdir;

#[test]
fn test_write_tool_permissions() {
    let log = Arc::new(FsOperationLog::new());
    let tool = WriteTool::new(log);
    let perms = tool.capabilities();
    assert!(perms.contains(&ToolCapability::Write));
    assert_eq!(perms.len(), 1);
}

#[tokio::test]
async fn test_write_tool_creation() {
    let log = Arc::new(FsOperationLog::new());
    let tool = WriteTool::new(log);
    assert_eq!(&tool.name(), "write");
    assert!(!tool.description().is_empty());
}

#[tokio::test]
async fn test_write_new_file() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("new_file.txt");

    let log = Arc::new(FsOperationLog::new());
    let tool = WriteTool::new(log);
    let params = WriteToolParams {
        files: vec![WriteFileSpec {
            path: file_path.to_string_lossy().to_string(),
            content: "Hello, World!".to_string(),
        }],
    };

    let result = tool.execute(params, None).await;
    assert!(result.is_success());

    let content = fs::read_to_string(&file_path).unwrap();
    assert_eq!(content, "Hello, World!");
}

#[tokio::test]
async fn test_write_multi_file() {
    let dir = tempdir().unwrap();
    let file_a = dir.path().join("a.txt");
    let file_b = dir.path().join("b.txt");

    let log = Arc::new(FsOperationLog::new());
    let tool = WriteTool::new(log);
    let params = WriteToolParams {
        files: vec![
            WriteFileSpec {
                path: file_a.to_string_lossy().to_string(),
                content: "content A".to_string(),
            },
            WriteFileSpec {
                path: file_b.to_string_lossy().to_string(),
                content: "content B".to_string(),
            },
        ],
    };

    let result = tool.execute(params, None).await;
    assert!(result.is_success());
    assert_eq!(fs::read_to_string(&file_a).unwrap(), "content A");
    assert_eq!(fs::read_to_string(&file_b).unwrap(), "content B");
}

#[tokio::test]
async fn test_write_empty_params() {
    let log = Arc::new(FsOperationLog::new());
    let tool = WriteTool::new(log);
    let params = WriteToolParams { files: vec![] };
    let result = tool.execute(params, None).await;
    assert!(result.is_error());
}
