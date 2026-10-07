#[cfg(test)]
mod tests {
    use crate::tools::memory::{
        memory::{
            self, index_path, load_index, remove_fact, render_memory_block, write_fact,
            WriteOutcome, MAX_INDEX_LINES,
        },
        structs::{MemoryRemoveParams, MemoryScope, MemoryWriteParams},
        MemoryRemoveTool, MemoryWriteTool,
    };
    use crate::tools::{Tool, ToolResult};
    use std::sync::Mutex;
    use tempfile::TempDir;

    /// Env vars are process-global; serialize tests that override memory paths.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct MemoryDirs {
        project: TempDir,
        global: TempDir,
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    impl MemoryDirs {
        fn new() -> Self {
            let guard = ENV_LOCK.lock().unwrap();
            let project = TempDir::new().unwrap();
            let global = TempDir::new().unwrap();
            std::env::set_var("SHAI_MEMORY_PROJECT_DIR", project.path());
            std::env::set_var("SHAI_MEMORY_GLOBAL_DIR", global.path());
            Self {
                project,
                global,
                _guard: guard,
            }
        }

        fn project_index(&self) -> std::path::PathBuf {
            self.project.path().join("MEMORY.md")
        }

        fn global_index(&self) -> std::path::PathBuf {
            self.global.path().join("MEMORY.md")
        }
    }

    impl Drop for MemoryDirs {
        fn drop(&mut self) {
            std::env::remove_var("SHAI_MEMORY_PROJECT_DIR");
            std::env::remove_var("SHAI_MEMORY_GLOBAL_DIR");
        }
    }

    fn seed_index(path: &std::path::Path, facts: &[&str]) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let content: String = facts
            .iter()
            .map(|f| format!("- [2026-10-08 10:00:00] {}\n", f))
            .collect();
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn test_parse_fact_text() {
        assert_eq!(
            memory::parse_fact_text("- [2026-10-08 10:00:00] use pnpm"),
            Some("use pnpm")
        );
        assert_eq!(memory::parse_fact_text("# A heading"), None);
        assert_eq!(memory::parse_fact_text("- [not-a-stamp] x"), None);
        assert_eq!(memory::parse_fact_text("- plain bullet"), None);
    }

    #[test]
    fn test_normalize() {
        assert_eq!(memory::normalize("  Foo   BAR "), "foo bar");
    }

    #[test]
    fn test_write_fact_appends_timestamped_line() {
        let dirs = MemoryDirs::new();
        let outcome = write_fact("always run cargo fmt", MemoryScope::Project).unwrap();
        let memory::WriteOutcome::Added(path) = outcome else {
            panic!("expected Added, got {:?}", outcome)
        };
        assert_eq!(path, dirs.project_index());

        let content = std::fs::read_to_string(dirs.project_index()).unwrap();
        let line = content.trim_end();
        assert!(line.starts_with("- ["), "line was: {}", line);
        assert!(
            line.ends_with("] always run cargo fmt"),
            "line was: {}",
            line
        );
    }

    #[test]
    fn test_write_fact_rejects_empty_and_multiline() {
        let _dirs = MemoryDirs::new();
        assert!(memory::write_fact("   ", MemoryScope::Project).is_err());
        let err = memory::write_fact("line one\nline two", MemoryScope::Project).unwrap_err();
        assert!(err.contains("single line"));
    }

    #[test]
    fn test_write_fact_deduplicates() {
        let _dirs = MemoryDirs::new();
        let first = memory::write_fact("use pnpm", MemoryScope::Project).unwrap();
        assert!(matches!(first, memory::WriteOutcome::Added(_)));

        let second = memory::write_fact("  Use   PNPM  ", MemoryScope::Project).unwrap();
        assert!(matches!(second, memory::WriteOutcome::AlreadyStored(_)));
    }

    #[test]
    fn test_write_fact_refuses_when_at_line_cap() {
        let dirs = MemoryDirs::new();
        let facts: Vec<String> = (0..MAX_INDEX_LINES)
            .map(|i| format!("fact {}", i))
            .collect();
        let fact_refs: Vec<&str> = facts.iter().map(|s| s.as_str()).collect();
        seed_index(&dirs.project_index(), &fact_refs);

        let err = memory::write_fact("one more", MemoryScope::Project).unwrap_err();
        assert!(err.contains("cap"));
    }

    #[test]
    fn test_write_fact_scope_global() {
        let dirs = MemoryDirs::new();
        let outcome = memory::write_fact("prefer concise answers", MemoryScope::Global).unwrap();
        let memory::WriteOutcome::Added(path) = outcome else {
            panic!("expected Added")
        };
        assert_eq!(path, dirs.global_index());
        assert!(!dirs.project_index().exists());
    }

    #[test]
    fn test_remove_fact_exact_match() {
        let dirs = MemoryDirs::new();
        seed_index(
            &dirs.project_index(),
            &["use pnpm", "run cargo fmt before commit"],
        );

        let removed = memory::remove_fact("use pnpm", MemoryScope::Project).unwrap();
        assert_eq!(removed, vec!["use pnpm".to_string()]);

        let remaining = std::fs::read_to_string(dirs.project_index()).unwrap();
        assert!(!remaining.contains("pnpm"));
        assert!(remaining.contains("cargo fmt"));
    }

    #[test]
    fn test_remove_fact_substring_match() {
        let dirs = MemoryDirs::new();
        seed_index(&dirs.project_index(), &["always use cargo nextest"]);

        let removed = memory::remove_fact("nextest", MemoryScope::Project).unwrap();
        assert_eq!(removed.len(), 1);
        assert!(std::fs::read_to_string(dirs.project_index())
            .unwrap()
            .trim()
            .is_empty());
    }

    #[test]
    fn test_remove_fact_ambiguous_lists_candidates() {
        let dirs = MemoryDirs::new();
        seed_index(
            &dirs.project_index(),
            &["prefer rust for tools", "prefer go for services"],
        );

        let err = memory::remove_fact("prefer", MemoryScope::Project).unwrap_err();
        assert!(err.contains("2 memories match"));
        assert!(err.contains("rust for tools"));
        assert!(err.contains("go for services"));
        // Nothing removed
        assert_eq!(
            std::fs::read_to_string(dirs.project_index())
                .unwrap()
                .lines()
                .count(),
            2
        );
    }

    #[test]
    fn test_remove_fact_not_found_and_missing_index() {
        let dirs = MemoryDirs::new();
        let err = memory::remove_fact("nope", MemoryScope::Project).unwrap_err();
        assert!(err.contains("No memory index found"));

        seed_index(&dirs.project_index(), &["something else"]);
        let err = memory::remove_fact("nope", MemoryScope::Project).unwrap_err();
        assert!(err.contains("No memory matching"));
    }

    #[test]
    fn test_remove_fact_preserves_non_fact_lines() {
        let dirs = MemoryDirs::new();
        std::fs::write(
            dirs.project_index(),
            "# Manually curated section\n- [2026-10-08 10:00:00] stale fact\nnote line\n",
        )
        .unwrap();

        memory::remove_fact("stale fact", MemoryScope::Project).unwrap();
        let remaining = std::fs::read_to_string(dirs.project_index()).unwrap();
        assert!(remaining.contains("# Manually curated section"));
        assert!(remaining.contains("note line"));
        assert!(!remaining.contains("stale fact"));
    }

    #[test]
    fn test_load_index_caps_lines() {
        let dirs = MemoryDirs::new();
        let facts: Vec<String> = (0..MAX_INDEX_LINES + 50)
            .map(|i| format!("fact {}", i))
            .collect();
        let fact_refs: Vec<&str> = facts.iter().map(|s| s.as_str()).collect();
        seed_index(&dirs.project_index(), &fact_refs);

        let loaded = load_index(MemoryScope::Project);
        assert!(loaded.truncated);
        assert_eq!(loaded.content.lines().count(), MAX_INDEX_LINES);
        assert!(loaded.content.contains("fact 0"));
        assert!(!loaded
            .content
            .contains(&format!("fact {}", MAX_INDEX_LINES)));
    }

    #[test]
    fn test_render_memory_block_empty_when_no_memory() {
        let _dirs = MemoryDirs::new();
        assert_eq!(render_memory_block(), "");
    }

    #[test]
    fn test_render_memory_block_merges_scopes_with_guidance() {
        let dirs = MemoryDirs::new();
        seed_index(&dirs.global_index(), &["prefer concise answers"]);
        seed_index(&dirs.project_index(), &["use cargo nextest"]);

        let block = render_memory_block();
        assert!(block.contains("## Memory"));
        assert!(block.contains("memory_write"));
        assert!(block.contains("`memory` skill"));
        assert!(block.contains("--- Begin global memory index ---"));
        assert!(block.contains("prefer concise answers"));
        assert!(block.contains("--- Begin project memory index ---"));
        assert!(block.contains("use cargo nextest"));
        // Global comes before project
        assert!(
            block.find("global memory index").unwrap()
                < block.find("project memory index").unwrap()
        );
        assert!(!block.contains("truncated"));
    }

    #[test]
    fn test_render_memory_block_truncation_notice() {
        let dirs = MemoryDirs::new();
        let facts: Vec<String> = (0..MAX_INDEX_LINES + 1)
            .map(|i| format!("fact {}", i))
            .collect();
        let fact_refs: Vec<&str> = facts.iter().map(|s| s.as_str()).collect();
        seed_index(&dirs.project_index(), &fact_refs);

        let block = render_memory_block();
        assert!(block.contains("project index truncated"));
    }

    #[tokio::test]
    async fn test_memory_write_tool_round_trip_with_remove_tool() {
        let _dirs = MemoryDirs::new();
        let write = MemoryWriteTool::new();
        let remove = MemoryRemoveTool::new();

        let result = write
            .execute(
                MemoryWriteParams {
                    content: "the deploy script lives in scripts/deploy.sh".to_string(),
                    scope: MemoryScope::Project,
                },
                None,
            )
            .await;
        assert!(result.is_success());

        let result = remove
            .execute(
                MemoryRemoveParams {
                    content: "deploy script".to_string(),
                    scope: MemoryScope::Project,
                },
                None,
            )
            .await;
        match result {
            ToolResult::Success { output, .. } => assert!(output.contains("deploy.sh")),
            other => panic!("expected success, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_memory_write_tool_scope_serializes_lowercase() {
        let params: MemoryWriteParams =
            serde_json::from_str(r#"{"content": "x", "scope": "global"}"#).unwrap();
        assert_eq!(params.scope, MemoryScope::Global);
        let params: MemoryWriteParams = serde_json::from_str(r#"{"content": "x"}"#).unwrap();
        assert_eq!(params.scope, MemoryScope::Project);
    }

    #[test]
    fn test_index_path_respects_env_overrides() {
        let dirs = MemoryDirs::new();
        assert_eq!(index_path(MemoryScope::Project), dirs.project_index());
        assert_eq!(index_path(MemoryScope::Global), dirs.global_index());
    }
}
