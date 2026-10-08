use std::collections::HashMap;
use std::path::Path;
use tree_sitter::Language;

/// Registry mapping file extensions to tree-sitter highlight configurations.
///
/// Stores the `Language` and highlights query string needed to perform
/// syntax highlighting via tree-sitter's `Query` API.
pub struct HighlightRegistry {
    configs: HashMap<&'static str, HighlightEntry>,
}

pub struct HighlightEntry {
    language: Language,
    highlights_query: &'static str,
    language_name: &'static str,
}

impl HighlightRegistry {
    /// Create a new registry with all supported languages pre-loaded.
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let mut configs = HashMap::new();

        macro_rules! register {
            ($ext:expr, $lang_name:expr, $language:expr, $query:expr) => {
                configs.insert(
                    $ext,
                    HighlightEntry {
                        language: $language,
                        highlights_query: $query,
                        language_name: $lang_name,
                    },
                );
            };
        }

        register!(
            "rs",
            "rust",
            tree_sitter_rust::LANGUAGE.into(),
            tree_sitter_rust::HIGHLIGHTS_QUERY
        );
        register!(
            "py",
            "python",
            tree_sitter_python::LANGUAGE.into(),
            tree_sitter_python::HIGHLIGHTS_QUERY
        );
        register!(
            "js",
            "javascript",
            tree_sitter_javascript::LANGUAGE.into(),
            tree_sitter_javascript::HIGHLIGHT_QUERY
        );
        register!(
            "jsx",
            "javascript",
            tree_sitter_javascript::LANGUAGE.into(),
            tree_sitter_javascript::HIGHLIGHT_QUERY
        );
        register!(
            "ts",
            "typescript",
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            tree_sitter_typescript::HIGHLIGHTS_QUERY
        );
        register!(
            "tsx",
            "typescript",
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            tree_sitter_typescript::HIGHLIGHTS_QUERY
        );
        register!(
            "go",
            "go",
            tree_sitter_go::LANGUAGE.into(),
            tree_sitter_go::HIGHLIGHTS_QUERY
        );
        register!(
            "rb",
            "ruby",
            tree_sitter_ruby::LANGUAGE.into(),
            tree_sitter_ruby::HIGHLIGHTS_QUERY
        );
        register!(
            "c",
            "c",
            tree_sitter_c::LANGUAGE.into(),
            tree_sitter_c::HIGHLIGHT_QUERY
        );
        register!(
            "h",
            "c",
            tree_sitter_c::LANGUAGE.into(),
            tree_sitter_c::HIGHLIGHT_QUERY
        );
        register!(
            "cpp",
            "cpp",
            tree_sitter_cpp::LANGUAGE.into(),
            tree_sitter_cpp::HIGHLIGHT_QUERY
        );
        register!(
            "cc",
            "cpp",
            tree_sitter_cpp::LANGUAGE.into(),
            tree_sitter_cpp::HIGHLIGHT_QUERY
        );
        register!(
            "cxx",
            "cpp",
            tree_sitter_cpp::LANGUAGE.into(),
            tree_sitter_cpp::HIGHLIGHT_QUERY
        );
        register!(
            "hpp",
            "cpp",
            tree_sitter_cpp::LANGUAGE.into(),
            tree_sitter_cpp::HIGHLIGHT_QUERY
        );
        register!(
            "java",
            "java",
            tree_sitter_java::LANGUAGE.into(),
            tree_sitter_java::HIGHLIGHTS_QUERY
        );
        register!(
            "php",
            "php",
            tree_sitter_php::LANGUAGE_PHP.into(),
            tree_sitter_php::HIGHLIGHTS_QUERY
        );
        register!(
            "lua",
            "lua",
            tree_sitter_lua::LANGUAGE.into(),
            tree_sitter_lua::HIGHLIGHTS_QUERY
        );
        register!(
            "sh",
            "bash",
            tree_sitter_bash::LANGUAGE.into(),
            tree_sitter_bash::HIGHLIGHT_QUERY
        );
        register!(
            "bash",
            "bash",
            tree_sitter_bash::LANGUAGE.into(),
            tree_sitter_bash::HIGHLIGHT_QUERY
        );

        Ok(Self { configs })
    }

    /// Get the highlight entry for a given file path extension.
    pub fn entry_for_path(&self, path: &str) -> Option<&HighlightEntry> {
        let ext = Path::new(path).extension().and_then(|e| e.to_str())?;
        self.configs.get(ext)
    }

    /// Check if a file path is supported by the registry.
    pub fn is_supported(&self, path: &str) -> bool {
        Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|ext| self.configs.contains_key(ext))
    }
}

impl HighlightEntry {
    /// Get the tree-sitter language.
    pub fn language(&self) -> &Language {
        &self.language
    }

    /// Get the highlights query string.
    pub fn highlights_query(&self) -> &'static str {
        self.highlights_query
    }

    /// Get the language name (e.g. "rust", "python").
    pub fn language_name(&self) -> &'static str {
        self.language_name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creates_successfully() {
        let registry = HighlightRegistry::new();
        assert!(registry.is_ok());
    }

    #[test]
    fn test_supported_extensions() {
        let registry = HighlightRegistry::new().unwrap();

        assert!(registry.is_supported("foo.rs"));
        assert!(registry.is_supported("foo.py"));
        assert!(registry.is_supported("foo.js"));
        assert!(registry.is_supported("foo.ts"));
        assert!(registry.is_supported("foo.go"));
        assert!(registry.is_supported("foo.rb"));
        assert!(registry.is_supported("foo.c"));
        assert!(registry.is_supported("foo.cpp"));
        assert!(registry.is_supported("foo.java"));
        assert!(registry.is_supported("foo.php"));
        assert!(registry.is_supported("foo.lua"));
        assert!(registry.is_supported("foo.sh"));
        assert!(registry.is_supported("foo.bash"));
    }

    #[test]
    fn test_unsupported_extensions() {
        let registry = HighlightRegistry::new().unwrap();

        assert!(!registry.is_supported("foo.txt"));
        assert!(!registry.is_supported("foo.md"));
        assert!(!registry.is_supported("foo.json"));
        assert!(!registry.is_supported("foo"));
    }

    #[test]
    fn test_entry_for_path() {
        let registry = HighlightRegistry::new().unwrap();

        let entry = registry.entry_for_path("foo.rs").unwrap();
        assert_eq!(entry.language_name(), "rust");

        let entry = registry.entry_for_path("foo.py").unwrap();
        assert_eq!(entry.language_name(), "python");

        assert!(registry.entry_for_path("foo.txt").is_none());
    }
}
