use std::fs;
use std::path::{Path, PathBuf};

/// Supported source formats for import.
#[derive(Debug)]
pub enum ImportSource {
    /// `CLAUDE.md` at repo root or `.claude/CLAUDE.md`
    Claude,
    /// `.cursorrules` or `.cursor/rules` file
    Cursor,
}

impl ImportSource {
    pub fn label(&self) -> &'static str {
        match self {
            ImportSource::Claude => "Claude",
            ImportSource::Cursor => "Cursor",
        }
    }
}

/// Detect and read a `CLAUDE.md` file from the given base directory.
/// Checks `CLAUDE.md` and `.claude/CLAUDE.md`.
fn detect_claude(base: &Path) -> Option<(PathBuf, String)> {
    // Direct CLAUDE.md at root
    let claude_md = base.join("CLAUDE.md");
    if claude_md.is_file() {
        if let Ok(content) = fs::read_to_string(&claude_md) {
            return Some((claude_md, content));
        }
    }

    // .claude/CLAUDE.md
    let claude_dir_md = base.join(".claude").join("CLAUDE.md");
    if claude_dir_md.is_file() {
        if let Ok(content) = fs::read_to_string(&claude_dir_md) {
            return Some((claude_dir_md, content));
        }
    }

    None
}

/// Detect and read a `.cursorrules` or `.cursor/rules` file from the given base directory.
fn detect_cursor(base: &Path) -> Option<(PathBuf, String)> {
    // .cursorrules at root
    let cursorrules = base.join(".cursorrules");
    if cursorrules.is_file() {
        if let Ok(content) = fs::read_to_string(&cursorrules) {
            return Some((cursorrules, content));
        }
    }

    // .cursor/rules at root
    let cursor_rules = base.join(".cursor").join("rules");
    if cursor_rules.is_file() {
        if let Ok(content) = fs::read_to_string(&cursor_rules) {
            return Some((cursor_rules, content));
        }
    }

    None
}

/// Detect all importable configs in the given directory.
/// Returns a list of (source, path, content) tuples.
pub fn detect_importable(base: &Path) -> Vec<(ImportSource, PathBuf, String)> {
    let mut results = Vec::new();

    if let Some((path, content)) = detect_claude(base) {
        results.push((ImportSource::Claude, path, content));
    }

    if let Some((path, content)) = detect_cursor(base) {
        results.push((ImportSource::Cursor, path, content));
    }

    results
}

/// Import configs from the given directory into an `AGENTS.md` file.
///
/// If `AGENTS.md` already exists and `overwrite` is false, the imported
/// content is appended. Returns the path to the AGENTS.md file and the
/// number of sources imported.
pub fn import_to_agents_md(base_dir: &Path, overwrite: bool) -> Result<(PathBuf, usize), String> {
    let sources = detect_importable(base_dir);

    if sources.is_empty() {
        return Err("No .claude or .cursor configuration files found.".into());
    }

    let agents_path = base_dir.join("AGENTS.md");
    let mut combined = String::new();

    // If not overwriting and AGENTS.md exists, preserve existing content
    if !overwrite && agents_path.exists() {
        if let Ok(existing) = fs::read_to_string(&agents_path) {
            combined = existing;
            if !combined.ends_with('\n') {
                combined.push('\n');
            }
            combined.push('\n');
        }
    }

    // Append a header if we're starting fresh
    if combined.is_empty() {
        combined.push_str("# AGENTS.md\n\n");
        combined.push_str("_Imported from external tooling configuration._\n\n");
    }

    for (source, path, content) in &sources {
        let header = format!(
            "\n---\n## Imported from {} ({})\n\n",
            source.label(),
            path.display()
        );
        combined.push_str(&header);
        combined.push_str(content);
        combined.push('\n');
    }

    fs::write(&agents_path, &combined).map_err(|e| format!("Failed to write AGENTS.md: {}", e))?;

    Ok((agents_path, sources.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_detect_claude_md() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        fs::write(base.join("CLAUDE.md"), "# Claude Rules\n\nBe helpful.").unwrap();

        let sources = detect_importable(base);
        assert_eq!(sources.len(), 1);
        assert!(matches!(sources[0].0, ImportSource::Claude));
        assert_eq!(sources[0].2, "# Claude Rules\n\nBe helpful.");
    }

    #[test]
    fn test_detect_claude_dir() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        fs::create_dir_all(base.join(".claude")).unwrap();
        fs::write(
            base.join(".claude").join("CLAUDE.md"),
            "# Claude Rules\n\nBe helpful.",
        )
        .unwrap();

        let sources = detect_importable(base);
        assert_eq!(sources.len(), 1);
        assert!(matches!(sources[0].0, ImportSource::Claude));
    }

    #[test]
    fn test_detect_cursorrules() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        fs::write(base.join(".cursorrules"), "# Cursor Rules\n\nBe precise.").unwrap();

        let sources = detect_importable(base);
        assert_eq!(sources.len(), 1);
        assert!(matches!(sources[0].0, ImportSource::Cursor));
        assert_eq!(sources[0].2, "# Cursor Rules\n\nBe precise.");
    }

    #[test]
    fn test_detect_cursor_rules_file() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        fs::create_dir_all(base.join(".cursor")).unwrap();
        fs::write(
            base.join(".cursor").join("rules"),
            "# Cursor Rules\n\nBe precise.",
        )
        .unwrap();

        let sources = detect_importable(base);
        assert_eq!(sources.len(), 1);
        assert!(matches!(sources[0].0, ImportSource::Cursor));
    }

    #[test]
    fn test_detect_both() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        fs::write(base.join("CLAUDE.md"), "# Claude").unwrap();
        fs::write(base.join(".cursorrules"), "# Cursor").unwrap();

        let sources = detect_importable(base);
        assert_eq!(sources.len(), 2);
    }

    #[test]
    fn test_detect_none() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        let sources = detect_importable(base);
        assert!(sources.is_empty());
    }

    #[test]
    fn test_import_creates_agents_md() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        fs::write(base.join("CLAUDE.md"), "# Claude Rules\n\nBe helpful.").unwrap();

        let (path, count) = import_to_agents_md(base, false).unwrap();
        assert_eq!(count, 1);
        assert!(path.ends_with("AGENTS.md"));

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("Claude"));
        assert!(content.contains("Be helpful."));
    }

    #[test]
    fn test_import_appends_to_existing() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        fs::write(base.join("AGENTS.md"), "# Existing\n\nExisting content.\n").unwrap();
        fs::write(base.join("CLAUDE.md"), "# Claude Rules\n\nBe helpful.").unwrap();

        let (path, count) = import_to_agents_md(base, false).unwrap();
        assert_eq!(count, 1);

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("Existing content."));
        assert!(content.contains("Be helpful."));
    }

    #[test]
    fn test_import_overwrite() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        fs::write(base.join("AGENTS.md"), "# Existing\n\nExisting content.\n").unwrap();
        fs::write(base.join("CLAUDE.md"), "# Claude Rules\n\nBe helpful.").unwrap();

        let (path, count) = import_to_agents_md(base, true).unwrap();
        assert_eq!(count, 1);

        let content = fs::read_to_string(&path).unwrap();
        assert!(!content.contains("Existing content."));
        assert!(content.contains("Be helpful."));
    }

    #[test]
    fn test_import_no_files() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        let result = import_to_agents_md(base, false);
        assert!(result.is_err());
    }
}
