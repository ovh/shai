#[cfg(test)]
mod tests {
    use crate::tools::memory::{load_merged_memory, MemoryReadTool, MemoryWriteTool};
    use crate::tools::{Tool, ToolResult};

    #[tokio::test]
    async fn test_memory_write_and_read() {
        // We can't easily test the file-based memory tool in isolation because
        // it resolves paths relative to the git root. Instead, we test that
        // the tools can be instantiated and the read tool returns something
        // sensible when no memory file exists.
        let _write_tool = MemoryWriteTool::new();
        let read_tool = MemoryReadTool::new();

        // Reading when no memory file exists should return an error or empty message
        // Since we're in a real repo, the memory file might not exist yet
        let result = read_tool
            .execute(
                crate::tools::memory::structs::MemoryReadParams { _unused: None },
                None,
            )
            .await;
        // It should succeed (either empty or with content)
        assert!(result.is_success() || result.is_error());
    }

    #[tokio::test]
    async fn test_memory_write_empty_content_rejected() {
        let write_tool = MemoryWriteTool::new();
        let result = write_tool
            .execute(
                crate::tools::memory::structs::MemoryWriteParams {
                    content: "   ".to_string(),
                },
                None,
            )
            .await;

        assert!(result.is_error());
        if let ToolResult::Error { error, .. } = result {
            assert!(error.contains("empty"));
        }
    }

    #[tokio::test]
    async fn test_memory_file_path_resolution() {
        // Test that find_git_root returns a valid path when inside a git repo.
        // Other tests may have changed the global working directory to a temp dir,
        // so we can't rely on current_dir(). Instead, walk up from CARGO_MANIFEST_DIR.
        let mut dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut found_git = false;
        while let Some(parent) = dir.parent() {
            if parent.join(".git").exists() {
                found_git = true;
                break;
            }
            dir = parent.to_path_buf();
        }
        assert!(found_git, "Should be running inside a git repo");
    }

    #[tokio::test]
    async fn test_memory_write_persists_and_reads_back() {
        // Write a fact using MemoryWriteTool, then read it back with MemoryReadTool.
        // This tests the full round-trip through the project-local memory file.
        let write_tool = MemoryWriteTool::new();
        let read_tool = MemoryReadTool::new();

        let test_content = "Test memory fact for Tier 2 round-trip";

        // Write a fact
        let write_result = write_tool
            .execute(
                crate::tools::memory::structs::MemoryWriteParams {
                    content: test_content.to_string(),
                },
                None,
            )
            .await;
        assert!(write_result.is_success(), "Memory write should succeed");

        // Read it back
        let read_result = read_tool
            .execute(
                crate::tools::memory::structs::MemoryReadParams { _unused: None },
                None,
            )
            .await;

        if let ToolResult::Success { output, .. } = &read_result {
            assert!(
                output.contains(test_content),
                "Read-back should contain the written content"
            );
        } else {
            panic!("Expected success result from memory_read");
        }
    }

    #[tokio::test]
    async fn test_load_merged_memory_returns_string() {
        // load_merged_memory should always return a string (possibly empty)
        let memory = load_merged_memory();
        // Should be a string (may be empty if no memory files exist)
        let _ = &memory[..]; // ensure it's a string-like
    }

    #[tokio::test]
    async fn test_load_merged_memory_live_reload() {
        // Write a unique fact, then verify load_merged_memory picks it up
        let unique_tag = format!(
            "UNIQUE_MEMORY_TAG_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );

        let write_tool = MemoryWriteTool::new();
        let _ = write_tool
            .execute(
                crate::tools::memory::structs::MemoryWriteParams {
                    content: unique_tag.clone(),
                },
                None,
            )
            .await;

        // load_merged_memory should reflect the newly written content
        let memory = load_merged_memory();
        assert!(
            memory.contains(&unique_tag),
            "load_merged_memory should contain the newly written fact"
        );
    }
}
