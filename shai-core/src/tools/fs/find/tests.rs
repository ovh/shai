use super::find::FindTool;
use super::structs::{FindToolParams, FindType};
use crate::tools::Tool;
use shai_llm::ToolDescription;
use tempfile::TempDir;
use std::fs;


#[tokio::test]
async fn test_find_tool_creation() {
    let tool = FindTool::new();
    assert_eq!(&tool.name(), "search");
    assert!(!tool.description().is_empty());
}

#[tokio::test]
async fn test_find_tool_content_search() {
    // Create a temporary directory with test files
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let temp_path = temp_dir.path();

    // Create test files
    let test_content = r#"struct User {
    pub id: u64,
    pub name: String,
    pub email: String,
}

impl User {
    pub fn new(id: u64, name: String, email: String) -> Self {
        Self { id, name, email }
    }
    
    pub fn validate_email(&self) -> bool {
        self.email.contains('@')
    }
}"#;

    let other_content = r#"use std::collections::HashMap;

fn main() {
    let data = HashMap::new();
    println!("Hello world");
}"#;

    fs::write(temp_path.join("user.rs"), test_content).expect("Failed to write user.rs");
    fs::write(temp_path.join("main.rs"), other_content).expect("Failed to write main.rs");
    fs::write(temp_path.join("README.md"), "# Test Project\nThis is a test").expect("Failed to write README.md");

    let find_tool = FindTool::new();

    // Test 1: Search for "struct" in content
    let params = FindToolParams {
        pattern: "struct".to_string(),
        path: Some(temp_path.to_string_lossy().to_string()),
        include_extensions: Some("rs".to_string()),
        exclude_patterns: None,
        max_results: 10,
        case_sensitive: false,
        find_type: FindType::Content,
        show_line_numbers: true,
        context_lines: None,
        whole_word: false,
    };

    let result = find_tool.execute(params, None).await;
    match result {
        crate::tools::ToolResult::Success { output, .. } => {
            assert!(output.contains("struct User"), "Should find struct User in results");
            assert!(output.contains("user.rs"), "Should identify user.rs as the file");
            assert!(output.contains("line_number"), "Should include line numbers");
        },
        crate::tools::ToolResult::Error { error, .. } => {
            panic!("Find tool should succeed, got error: {}", error);
        },
        crate::tools::ToolResult::Denied => {
            panic!("Find tool was denied");
        }
    }

    // Test 2: Search for "email" in content with case sensitivity
    let params = FindToolParams {
        pattern: "email".to_string(),
        path: Some(temp_path.to_string_lossy().to_string()),
        include_extensions: Some("rs".to_string()),
        exclude_patterns: None,
        max_results: 10,
        case_sensitive: true,
        find_type: FindType::Content,
        show_line_numbers: true,
        context_lines: Some(1),
        whole_word: false,
    };

    let result = find_tool.execute(params, None).await;
    match result {
        crate::tools::ToolResult::Success { output, .. } => {
            assert!(output.contains("email"), "Should find email in results");
            assert!(output.contains("context_before") || output.contains("context_after"), "Should include context");
        },
        crate::tools::ToolResult::Error { error, .. } => {
            panic!("Find tool should succeed, got error: {}", error);
        },
        crate::tools::ToolResult::Denied => {
            panic!("Find tool was denied");
        }
    }
}

#[tokio::test]
async fn test_find_tool_filename_search() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let temp_path = temp_dir.path();

    // Create test files with different names
    fs::write(temp_path.join("user_model.rs"), "// User model").expect("Failed to write user_model.rs");
    fs::write(temp_path.join("auth_service.rs"), "// Auth service").expect("Failed to write auth_service.rs");
    fs::write(temp_path.join("database.rs"), "// Database").expect("Failed to write database.rs");
    fs::write(temp_path.join("config.toml"), "# Config").expect("Failed to write config.toml");

    let find_tool = FindTool::new();

    // Test: Search for files containing "user" in filename
    let params = FindToolParams {
        pattern: "user".to_string(),
        path: Some(temp_path.to_string_lossy().to_string()),
        include_extensions: None,
        exclude_patterns: None,
        max_results: 10,
        case_sensitive: false,
        find_type: FindType::Filename,
        show_line_numbers: false,
        context_lines: None,
        whole_word: false,
    };

    let result = find_tool.execute(params, None).await;
    match result {
        crate::tools::ToolResult::Success { output, .. } => {
            assert!(output.contains("user_model.rs"), "Should find user_model.rs");
            assert!(output.contains("filename"), "Should indicate filename match type");
            
            // Parse JSON to check the results structure
            let results: Vec<crate::tools::fs::find::SearchResult> = serde_json::from_str(&output).expect("Should parse JSON results");
            assert!(!results.is_empty(), "Should have results");
            
            // Check that filename matches have None for line_number
            for result in &results {
                if result.match_type == "filename" {
                    assert!(result.line_number.is_none(), "Filename matches should not have line numbers");
                }
            }
        },
        crate::tools::ToolResult::Error { error, .. } => {
            panic!("Find tool should succeed, got error: {}", error);
        },
        crate::tools::ToolResult::Denied => {
            panic!("Find tool was denied");
        }
    }
}

#[tokio::test]
async fn test_find_tool_with_filters() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let temp_path = temp_dir.path();

    // Create test directory structure
    fs::create_dir_all(temp_path.join("src")).expect("Failed to create src directory");
    fs::create_dir_all(temp_path.join("target")).expect("Failed to create target directory");

    fs::write(temp_path.join("src/lib.rs"), "pub struct Library;").expect("Failed to write lib.rs");
    fs::write(temp_path.join("src/main.rs"), "fn main() {}").expect("Failed to write main.rs");
    fs::write(temp_path.join("target/debug"), "binary file").expect("Failed to write debug file");
    fs::write(temp_path.join("README.md"), "# Project").expect("Failed to write README.md");

    let find_tool = FindTool::new();

    // Test: Search for "struct" but exclude target directory and only include .rs files
    let params = FindToolParams {
        pattern: "struct".to_string(),
        path: Some(temp_path.to_string_lossy().to_string()),
        include_extensions: Some("rs".to_string()),
        exclude_patterns: Some("target".to_string()),
        max_results: 10,
        case_sensitive: false,
        find_type: FindType::Content,
        show_line_numbers: true,
        context_lines: None,
        whole_word: false,
    };

    let result = find_tool.execute(params, None).await;
    match result {
        crate::tools::ToolResult::Success { output, .. } => {
            assert!(output.contains("Library"), "Should find struct Library");
            assert!(output.contains("lib.rs"), "Should find in lib.rs");
            assert!(!output.contains("target"), "Should exclude target directory");
            assert!(!output.contains("README.md"), "Should exclude non-.rs files");
        },
        crate::tools::ToolResult::Error { error, .. } => {
            panic!("Find tool should succeed, got error: {}", error);
        },
        crate::tools::ToolResult::Denied => {
            panic!("Find tool was denied");
        }
    }
}

#[tokio::test]
async fn test_find_tool_regex_pattern() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let temp_path = temp_dir.path();

    let code_content = r#"fn calculate_user_score(user: &User) -> u32 {
    user.score + 10
}

fn calculate_admin_score(admin: &Admin) -> u32 {
    admin.score + 100
}

fn calculate_guest_score() -> u32 {
    0
}"#;

    fs::write(temp_path.join("calculator.rs"), code_content).expect("Failed to write calculator.rs");

    let find_tool = FindTool::new();

    // Test: Search for functions that start with "calculate_" using regex
    let params = FindToolParams {
        pattern: r"fn calculate_\w+".to_string(),
        path: Some(temp_path.to_string_lossy().to_string()),
        include_extensions: Some("rs".to_string()),
        exclude_patterns: None,
        max_results: 10,
        case_sensitive: false,
        find_type: FindType::Content,
        show_line_numbers: true,
        context_lines: None,
        whole_word: false,
    };

    let result = find_tool.execute(params, None).await;
    match result {
        crate::tools::ToolResult::Success { output, .. } => {
            assert!(output.contains("calculate_user_score"), "Should find calculate_user_score");
            assert!(output.contains("calculate_admin_score"), "Should find calculate_admin_score");
            assert!(output.contains("calculate_guest_score"), "Should find calculate_guest_score");
            
            // Should find all 3 functions
            let lines_with_calculate = output.matches("calculate_").count();
            assert!(lines_with_calculate >= 3, "Should find at least 3 calculate functions");
        },
        crate::tools::ToolResult::Error { error, .. } => {
            panic!("Find tool should succeed, got error: {}", error);
        },
        crate::tools::ToolResult::Denied => {
            panic!("Find tool was denied");
        }
    }
}

#[tokio::test]
async fn test_find_tool_invalid_regex() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let temp_path = temp_dir.path();

    fs::write(temp_path.join("test.rs"), "fn test() {}").expect("Failed to write test.rs");

    let find_tool = FindTool::new();

    // Test: Invalid regex pattern should return error
    let params = FindToolParams {
        pattern: "[invalid regex(".to_string(),
        path: Some(temp_path.to_string_lossy().to_string()),
        include_extensions: None,
        exclude_patterns: None,
        max_results: 10,
        case_sensitive: false,
        find_type: FindType::Content,
        show_line_numbers: true,
        context_lines: None,
        whole_word: false,
    };

    let result = find_tool.execute(params, None).await;
    match result {
        crate::tools::ToolResult::Success { .. } => {
            panic!("Find tool should return error for invalid regex");
        },
        crate::tools::ToolResult::Error { error, .. } => {
            assert!(error.contains("Invalid regex pattern"), "Should indicate regex error");
        },
        crate::tools::ToolResult::Denied => {
            panic!("Find tool was denied");
        }
    }
}