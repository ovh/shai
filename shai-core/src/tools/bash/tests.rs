use super::bash::BashTool;
use super::structs::BashToolParams;
use crate::tools::{Tool, ToolCapability};
use serde_json::json;
use shai_llm::ToolDescription;
use std::collections::HashMap;

#[test]
fn test_bash_tool_permissions() {
    let tool = BashTool::new();
    let perms = tool.capabilities();
    assert!(perms.contains(&ToolCapability::Read));
    assert!(perms.contains(&ToolCapability::Write));
    assert!(perms.contains(&ToolCapability::Network));
    assert_eq!(perms.len(), 3);
}

#[tokio::test]
async fn test_bash_tool_creation() {
    let tool = BashTool::new();
    assert_eq!(&tool.name(), "bash");
    assert!(!tool.description().is_empty());
}

#[tokio::test]
async fn test_bash_tool_execution() {
    let tool = BashTool::new();
    let params = BashToolParams {
        command: "echo hello".to_string(),
        timeout: None,
        working_dir: None,
        env: HashMap::new(),
    };

    let result = Tool::execute(&tool, params, None).await;
    assert!(result.is_success());
    if let crate::tools::types::ToolResult::Success { output, metadata } = result {
        assert!(output.contains("hello"));
        let metadata = metadata.unwrap();
        assert_eq!(metadata["exit_code"], json!(0));
        assert_eq!(metadata["success"], json!(true));
    } else {
        panic!("Expected success result");
    }
}
