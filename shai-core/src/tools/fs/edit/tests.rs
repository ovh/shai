use super::edit::EditTool;
use super::structs::{EditOperation, EditToolParams, FileEdit};
use crate::tools::{FsOperationLog, Tool, ToolCapability};
use shai_llm::ToolDescription;
use std::fs;
use std::sync::Arc;
use tempfile::tempdir;

#[test]
fn test_edit_tool_permissions() {
    let log = Arc::new(FsOperationLog::new());
    let tool = EditTool::new(log);
    let perms = tool.capabilities();
    assert!(perms.contains(&ToolCapability::Read));
    assert!(perms.contains(&ToolCapability::Write));
    assert_eq!(perms.len(), 2);
}

#[tokio::test]
async fn test_edit_tool_creation() {
    let log = Arc::new(FsOperationLog::new());
    let tool = EditTool::new(log);
    assert_eq!(&tool.name(), "edit");
    assert!(!tool.description().is_empty());
}

#[tokio::test]
async fn test_edit_file_replacement() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("test.txt");
    fs::write(&file_path, "Hello World").unwrap();

    let log = Arc::new(FsOperationLog::new());
    log.log_operation(
        crate::tools::FsOperationType::Read,
        file_path.to_string_lossy().to_string(),
    )
    .await;

    let tool = EditTool::new(log);
    let params = EditToolParams {
        files: vec![FileEdit {
            path: file_path.to_string_lossy().to_string(),
            edits: vec![EditOperation {
                old_string: "Hello".to_string(),
                new_string: "Hi".to_string(),
                replace_all: false,
                line_hash: None,
                insert_after_hash: None,
            }],
        }],
    };

    let result = tool.execute(params, None).await;
    assert!(result.is_success());

    let content = fs::read_to_string(&file_path).unwrap();
    assert_eq!(content, "Hi World");
}

#[tokio::test]
async fn test_edit_preview_functionality() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("test.txt");
    fs::write(&file_path, "Hello World\nSecond line\nThird line").unwrap();

    let log = Arc::new(FsOperationLog::new());
    log.log_operation(
        crate::tools::FsOperationType::Read,
        file_path.to_string_lossy().to_string(),
    )
    .await;

    let tool = EditTool::new(log);
    let params = EditToolParams {
        files: vec![FileEdit {
            path: file_path.to_string_lossy().to_string(),
            edits: vec![EditOperation {
                old_string: "Hello".to_string(),
                new_string: "Hi".to_string(),
                replace_all: false,
                line_hash: None,
                insert_after_hash: None,
            }],
        }],
    };

    let preview_result = tool.execute_preview(params.clone()).await;
    assert!(preview_result.is_some());

    let preview = preview_result.unwrap();
    assert!(preview.is_success());

    let output = match preview {
        crate::tools::ToolResult::Success { output, .. } => output,
        _ => panic!("Expected success result"),
    };

    assert!(output.contains("-"));
    assert!(output.contains("+"));
    assert!(output.contains("Hello"));
    assert!(output.contains("Hi"));

    let original_content = fs::read_to_string(&file_path).unwrap();
    assert_eq!(original_content, "Hello World\nSecond line\nThird line");
}

#[tokio::test]
async fn test_edit_multiple_replacements() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("test.txt");
    fs::write(&file_path, "Hello World, Hello Universe").unwrap();

    let log = Arc::new(FsOperationLog::new());
    log.log_operation(
        crate::tools::FsOperationType::Read,
        file_path.to_string_lossy().to_string(),
    )
    .await;

    let tool = EditTool::new(log);
    let params = EditToolParams {
        files: vec![FileEdit {
            path: file_path.to_string_lossy().to_string(),
            edits: vec![
                EditOperation {
                    old_string: "Hello".to_string(),
                    new_string: "Hi".to_string(),
                    replace_all: false,
                    line_hash: None,
                    insert_after_hash: None,
                },
                EditOperation {
                    old_string: "World".to_string(),
                    new_string: "Earth".to_string(),
                    replace_all: true,
                    line_hash: None,
                    insert_after_hash: None,
                },
            ],
        }],
    };

    let result = tool.execute(params, None).await;
    assert!(result.is_success());

    let content = fs::read_to_string(&file_path).unwrap();
    assert_eq!(content, "Hi Earth, Hello Universe");
}

#[tokio::test]
async fn test_edit_multifile_basic() {
    let dir = tempdir().unwrap();
    let file_a = dir.path().join("a.txt");
    let file_b = dir.path().join("b.txt");
    fs::write(&file_a, "Hello World").unwrap();
    fs::write(&file_b, "Goodbye Moon").unwrap();

    let log = Arc::new(FsOperationLog::new());
    log.log_operation(
        crate::tools::FsOperationType::Read,
        file_a.to_string_lossy().to_string(),
    )
    .await;
    log.log_operation(
        crate::tools::FsOperationType::Read,
        file_b.to_string_lossy().to_string(),
    )
    .await;

    let tool = EditTool::new(log);
    let params = EditToolParams {
        files: vec![
            FileEdit {
                path: file_a.to_string_lossy().to_string(),
                edits: vec![EditOperation {
                    old_string: "Hello".to_string(),
                    new_string: "Greetings".to_string(),
                    replace_all: false,
                    line_hash: None,
                    insert_after_hash: None,
                }],
            },
            FileEdit {
                path: file_b.to_string_lossy().to_string(),
                edits: vec![EditOperation {
                    old_string: "Goodbye".to_string(),
                    new_string: "Farewell".to_string(),
                    replace_all: false,
                    line_hash: None,
                    insert_after_hash: None,
                }],
            },
        ],
    };

    let result = tool.execute(params, None).await;
    assert!(result.is_success());
    assert_eq!(fs::read_to_string(&file_a).unwrap(), "Greetings World");
    assert_eq!(fs::read_to_string(&file_b).unwrap(), "Farewell Moon");
}

#[tokio::test]
async fn test_edit_multifile_atomicity() {
    let dir = tempdir().unwrap();
    let file_a = dir.path().join("a.txt");
    let file_b = dir.path().join("b.txt");
    fs::write(&file_a, "Hello World").unwrap();
    fs::write(&file_b, "Goodbye Moon").unwrap();

    let log = Arc::new(FsOperationLog::new());
    log.log_operation(
        crate::tools::FsOperationType::Read,
        file_a.to_string_lossy().to_string(),
    )
    .await;
    log.log_operation(
        crate::tools::FsOperationType::Read,
        file_b.to_string_lossy().to_string(),
    )
    .await;

    let tool = EditTool::new(log);
    let params = EditToolParams {
        files: vec![
            FileEdit {
                path: file_a.to_string_lossy().to_string(),
                edits: vec![EditOperation {
                    old_string: "Hello".to_string(),
                    new_string: "Hi".to_string(),
                    replace_all: false,
                    line_hash: None,
                    insert_after_hash: None,
                }],
            },
            FileEdit {
                path: file_b.to_string_lossy().to_string(),
                edits: vec![EditOperation {
                    old_string: "NONEXISTENT".to_string(),
                    new_string: "error".to_string(),
                    replace_all: false,
                    line_hash: None,
                    insert_after_hash: None,
                }],
            },
        ],
    };

    let result = tool.execute(params, None).await;
    assert!(result.is_error());
    assert_eq!(fs::read_to_string(&file_a).unwrap(), "Hello World");
    assert_eq!(fs::read_to_string(&file_b).unwrap(), "Goodbye Moon");
}

#[tokio::test]
async fn test_edit_not_read_yet() {
    let dir = tempdir().unwrap();
    let file_a = dir.path().join("a.txt");
    fs::write(&file_a, "Hello World").unwrap();

    let log = Arc::new(FsOperationLog::new());
    let tool = EditTool::new(log);
    let params = EditToolParams {
        files: vec![FileEdit {
            path: file_a.to_string_lossy().to_string(),
            edits: vec![EditOperation {
                old_string: "Hello".to_string(),
                new_string: "Hi".to_string(),
                replace_all: false,
                line_hash: None,
                insert_after_hash: None,
            }],
        }],
    };

    let result = tool.execute(params, None).await;
    assert!(result.is_error());
}

#[tokio::test]
async fn test_edit_empty_params() {
    let log = Arc::new(FsOperationLog::new());
    let tool = EditTool::new(log);
    let params = EditToolParams { files: vec![] };
    let result = tool.execute(params, None).await;
    assert!(result.is_error());
}

#[tokio::test]
async fn test_hash_anchored_edit_basic() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("test.rs");
    fs::write(&file_path, "fn hello() {\n    println!(\"hello\");\n}").unwrap();

    let log = Arc::new(FsOperationLog::new());
    log.log_operation(
        crate::tools::FsOperationType::Read,
        file_path.to_string_lossy().to_string(),
    )
    .await;

    let tool = EditTool::new(log);
    let params = EditToolParams {
        files: vec![FileEdit {
            path: file_path.to_string_lossy().to_string(),
            edits: vec![EditOperation {
                old_string: String::new(),
                new_string: "    println!(\"goodbye\");".to_string(),
                replace_all: false,
                line_hash: Some(crate::tools::fs::hash::compute_line_hash(
                    "    println!(\"hello\");",
                )),
                insert_after_hash: None,
            }],
        }],
    };

    let result = tool.execute(params, None).await;
    assert!(result.is_success());
    let content = fs::read_to_string(&file_path).unwrap();
    assert!(content.contains("goodbye"));
    assert!(!content.contains("hello\");"));
}

#[tokio::test]
async fn test_hash_anchored_edit_not_found() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("test.rs");
    fs::write(&file_path, "fn hello() {\n}\n").unwrap();

    let log = Arc::new(FsOperationLog::new());
    log.log_operation(
        crate::tools::FsOperationType::Read,
        file_path.to_string_lossy().to_string(),
    )
    .await;

    let tool = EditTool::new(log);
    let params = EditToolParams {
        files: vec![FileEdit {
            path: file_path.to_string_lossy().to_string(),
            edits: vec![EditOperation {
                old_string: String::new(),
                new_string: "replaced".to_string(),
                replace_all: false,
                line_hash: Some("deadbeef".to_string()),
                insert_after_hash: None,
            }],
        }],
    };

    let result = tool.execute(params, None).await;
    assert!(result.is_error());
}

#[tokio::test]
async fn test_hash_anchored_edit_replace_all() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("test.py");
    fs::write(&file_path, "x = 1\nx = 1\nx = 1").unwrap();

    let log = Arc::new(FsOperationLog::new());
    log.log_operation(
        crate::tools::FsOperationType::Read,
        file_path.to_string_lossy().to_string(),
    )
    .await;

    let tool = EditTool::new(log);
    let params = EditToolParams {
        files: vec![FileEdit {
            path: file_path.to_string_lossy().to_string(),
            edits: vec![EditOperation {
                old_string: String::new(),
                new_string: "x = 0".to_string(),
                replace_all: true,
                line_hash: Some(crate::tools::fs::hash::compute_line_hash("x = 1")),
                insert_after_hash: None,
            }],
        }],
    };

    let result = tool.execute(params, None).await;
    assert!(result.is_success());
    let content = fs::read_to_string(&file_path).unwrap();
    assert_eq!(content, "x = 0\nx = 0\nx = 0");
}

#[tokio::test]
async fn test_hash_anchored_edit_first_match_only() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("test.py");
    fs::write(&file_path, "x = 1\nx = 1\nx = 1").unwrap();

    let log = Arc::new(FsOperationLog::new());
    log.log_operation(
        crate::tools::FsOperationType::Read,
        file_path.to_string_lossy().to_string(),
    )
    .await;

    let tool = EditTool::new(log);
    let params = EditToolParams {
        files: vec![FileEdit {
            path: file_path.to_string_lossy().to_string(),
            edits: vec![EditOperation {
                old_string: String::new(),
                new_string: "x = 0".to_string(),
                replace_all: false,
                line_hash: Some(crate::tools::fs::hash::compute_line_hash("x = 1")),
                insert_after_hash: None,
            }],
        }],
    };

    let result = tool.execute(params, None).await;
    assert!(result.is_success());
    let content = fs::read_to_string(&file_path).unwrap();
    assert_eq!(content, "x = 0\nx = 1\nx = 1");
}

#[tokio::test]
async fn test_hash_anchored_multifile_edit() {
    let dir = tempdir().unwrap();
    let file_a = dir.path().join("a.rs");
    let file_b = dir.path().join("b.rs");
    fs::write(&file_a, "fn foo() {}\n").unwrap();
    fs::write(&file_b, "fn bar() {}\n").unwrap();

    let log = Arc::new(FsOperationLog::new());
    log.log_operation(
        crate::tools::FsOperationType::Read,
        file_a.to_string_lossy().to_string(),
    )
    .await;
    log.log_operation(
        crate::tools::FsOperationType::Read,
        file_b.to_string_lossy().to_string(),
    )
    .await;

    let tool = EditTool::new(log);
    let params = EditToolParams {
        files: vec![
            FileEdit {
                path: file_a.to_string_lossy().to_string(),
                edits: vec![EditOperation {
                    old_string: String::new(),
                    new_string: "fn foo() -> u32 { 42 }".to_string(),
                    replace_all: false,
                    line_hash: Some(crate::tools::fs::hash::compute_line_hash("fn foo() {}")),
                    insert_after_hash: None,
                }],
            },
            FileEdit {
                path: file_b.to_string_lossy().to_string(),
                edits: vec![EditOperation {
                    old_string: String::new(),
                    new_string: "fn bar() -> u32 { 0 }".to_string(),
                    replace_all: false,
                    line_hash: Some(crate::tools::fs::hash::compute_line_hash("fn bar() {}")),
                    insert_after_hash: None,
                }],
            },
        ],
    };

    let result = tool.execute(params, None).await;
    assert!(result.is_success());
    assert_eq!(
        fs::read_to_string(&file_a).unwrap(),
        "fn foo() -> u32 { 42 }"
    );
    assert_eq!(
        fs::read_to_string(&file_b).unwrap(),
        "fn bar() -> u32 { 0 }"
    );
}

#[tokio::test]
async fn test_insert_after_hash_basic() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("test.rs");
    fs::write(&file_path, "fn main() {\n}\n").unwrap();

    let log = Arc::new(FsOperationLog::new());
    log.log_operation(
        crate::tools::FsOperationType::Read,
        file_path.to_string_lossy().to_string(),
    )
    .await;

    let tool = EditTool::new(log);
    let params = EditToolParams {
        files: vec![FileEdit {
            path: file_path.to_string_lossy().to_string(),
            edits: vec![EditOperation {
                old_string: String::new(),
                new_string: "    println!(\"hello\");".to_string(),
                replace_all: false,
                line_hash: None,
                insert_after_hash: Some(crate::tools::fs::hash::compute_line_hash("fn main() {")),
            }],
        }],
    };

    let result = tool.execute(params, None).await;
    assert!(result.is_success());
    let content = fs::read_to_string(&file_path).unwrap();
    assert!(content.contains("fn main() {"));
    assert!(content.contains("    println!(\"hello\");"));
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines[0], "fn main() {");
    assert_eq!(lines[1], "    println!(\"hello\");");
}

#[tokio::test]
async fn test_insert_after_hash_not_found() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("test.rs");
    fs::write(&file_path, "fn main() {\n}\n").unwrap();

    let log = Arc::new(FsOperationLog::new());
    log.log_operation(
        crate::tools::FsOperationType::Read,
        file_path.to_string_lossy().to_string(),
    )
    .await;

    let tool = EditTool::new(log);
    let params = EditToolParams {
        files: vec![FileEdit {
            path: file_path.to_string_lossy().to_string(),
            edits: vec![EditOperation {
                old_string: String::new(),
                new_string: "    println!(\"hello\");".to_string(),
                replace_all: false,
                line_hash: None,
                insert_after_hash: Some("deadbeef".to_string()),
            }],
        }],
    };

    let result = tool.execute(params, None).await;
    assert!(result.is_error());
}

#[test]
fn test_compute_line_hash_consistency() {
    use crate::tools::fs::hash::compute_line_hash;
    assert_eq!(compute_line_hash("hello"), compute_line_hash("hello"));
    assert_ne!(compute_line_hash("hello"), compute_line_hash("world"));
    assert_eq!(compute_line_hash("").len(), 8);
}

#[test]
fn test_myers_diff_algorithm() {
    let log = Arc::new(FsOperationLog::new());
    let tool = EditTool::new(log);

    let before = "line1\nline2\nline3";
    let after = "line1\nmodified line2\nline3";

    let diff = tool.myers_diff(before, after);

    assert!(diff.contains("\x1b[48;5;88;37m")); // Red background for deletions
    assert!(diff.contains("\x1b[48;5;28;37m")); // Green background for additions
    assert!(diff.contains("line2"));
    assert!(diff.contains("modified line2"));
    assert!(diff.contains("\x1b[2;37m")); // Dim gray for line numbers
}

#[test]
fn test_myers_diff_no_changes() {
    let log = Arc::new(FsOperationLog::new());
    let tool = EditTool::new(log);

    let content = "line1\nline2\nline3";
    let diff = tool.myers_diff(content, content);
    assert_eq!(diff, "No changes");
}

#[tokio::test]
async fn test_drain_edited_files() {
    let log = FsOperationLog::new();
    log.log_operation(crate::tools::FsOperationType::Edit, "foo.rs".to_string())
        .await;
    log.log_operation(
        crate::tools::FsOperationType::MultiEdit,
        "bar.py".to_string(),
    )
    .await;
    log.log_operation(crate::tools::FsOperationType::Read, "baz.txt".to_string())
        .await;

    let drained = log.drain_edited_files().await;
    assert_eq!(drained.len(), 2);
    assert!(drained.contains(&"foo.rs".to_string()));
    assert!(drained.contains(&"bar.py".to_string()));

    let drained_again = log.drain_edited_files().await;
    assert!(drained_again.is_empty());
}

#[test]
fn test_verification_config_default() {
    let config = crate::config::agent::VerificationConfig::default();
    assert!(!config.enabled);
    assert_eq!(config.timeout_secs, 30);
    assert!(config.commands.contains_key("rust"));
    assert!(config.commands.contains_key("python"));
    assert!(config.commands.contains_key("go"));
}
