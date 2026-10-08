use super::fetch::FetchTool;
use crate::tools::{Tool, ToolCapability};
use shai_llm::ToolDescription;

#[test]
fn test_fetch_tool_permissions() {
    let tool = FetchTool::new();
    let perms = tool.capabilities();
    assert!(perms.contains(&ToolCapability::Network));
    assert_eq!(perms.len(), 1);
}

#[tokio::test]
async fn test_fetch_tool_creation() {
    let tool = FetchTool::new();
    assert_eq!(&tool.name(), "fetch");
    assert!(!tool.description().is_empty());
}

// Note: Actual network tests would require internet connectivity
// In a real environment, you'd test with mock servers or local endpoints
