use std::collections::HashMap;
use std::path::Path;
use tree_sitter_tags::TagsConfiguration;

/// Custom tags query for Bash — captures functions and top-level variables.
const BASH_TAGS_QUERY: &str = r#"
(function_definition
  name: (_) @name
  (#set! tag.kind "function")) @definition.function

(variable_assignment
  name: (_) @name
  (#set! tag.kind "constant")) @definition.constant
"#;

/// Registry mapping file extensions to tree-sitter tags configurations.
///
/// Each configuration knows how to parse a specific language and extract
/// symbol tags (functions, classes, methods, etc.) from source code.
pub struct LanguageRegistry {
    configs: HashMap<&'static str, TagsConfigEntry>,
}

struct TagsConfigEntry {
    config: TagsConfiguration,
    language_name: &'static str,
}

impl LanguageRegistry {
    /// Create a new registry with all supported languages pre-loaded.
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let mut configs = HashMap::new();

        macro_rules! register {
            ($ext:expr, $lang_name:expr, $language:expr, $query:expr) => {
                configs.insert(
                    $ext,
                    TagsConfigEntry {
                        config: TagsConfiguration::new($language, $query, "")?,
                        language_name: $lang_name,
                    },
                );
            };
        }

        register!(
            "rs",
            "rust",
            tree_sitter_rust::LANGUAGE.into(),
            tree_sitter_rust::TAGS_QUERY
        );
        register!(
            "py",
            "python",
            tree_sitter_python::LANGUAGE.into(),
            tree_sitter_python::TAGS_QUERY
        );
        register!(
            "js",
            "javascript",
            tree_sitter_javascript::LANGUAGE.into(),
            tree_sitter_javascript::TAGS_QUERY
        );
        register!(
            "jsx",
            "javascript",
            tree_sitter_javascript::LANGUAGE.into(),
            tree_sitter_javascript::TAGS_QUERY
        );
        register!(
            "ts",
            "typescript",
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            tree_sitter_typescript::TAGS_QUERY
        );
        register!(
            "tsx",
            "typescript",
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            tree_sitter_typescript::TAGS_QUERY
        );
        register!(
            "go",
            "go",
            tree_sitter_go::LANGUAGE.into(),
            tree_sitter_go::TAGS_QUERY
        );
        register!(
            "rb",
            "ruby",
            tree_sitter_ruby::LANGUAGE.into(),
            tree_sitter_ruby::TAGS_QUERY
        );
        register!(
            "c",
            "c",
            tree_sitter_c::LANGUAGE.into(),
            tree_sitter_c::TAGS_QUERY
        );
        register!(
            "h",
            "c",
            tree_sitter_c::LANGUAGE.into(),
            tree_sitter_c::TAGS_QUERY
        );
        register!(
            "cpp",
            "cpp",
            tree_sitter_cpp::LANGUAGE.into(),
            tree_sitter_cpp::TAGS_QUERY
        );
        register!(
            "cc",
            "cpp",
            tree_sitter_cpp::LANGUAGE.into(),
            tree_sitter_cpp::TAGS_QUERY
        );
        register!(
            "cxx",
            "cpp",
            tree_sitter_cpp::LANGUAGE.into(),
            tree_sitter_cpp::TAGS_QUERY
        );
        register!(
            "hpp",
            "cpp",
            tree_sitter_cpp::LANGUAGE.into(),
            tree_sitter_cpp::TAGS_QUERY
        );
        register!(
            "java",
            "java",
            tree_sitter_java::LANGUAGE.into(),
            tree_sitter_java::TAGS_QUERY
        );
        register!(
            "php",
            "php",
            tree_sitter_php::LANGUAGE_PHP.into(),
            tree_sitter_php::TAGS_QUERY
        );
        register!(
            "lua",
            "lua",
            tree_sitter_lua::LANGUAGE.into(),
            tree_sitter_lua::TAGS_QUERY
        );
        register!(
            "sh",
            "bash",
            tree_sitter_bash::LANGUAGE.into(),
            BASH_TAGS_QUERY
        );
        register!(
            "bash",
            "bash",
            tree_sitter_bash::LANGUAGE.into(),
            BASH_TAGS_QUERY
        );

        Ok(Self { configs })
    }

    /// Get the tags configuration for a given file path extension.
    /// Returns None if the file type is not supported.
    pub fn config_for_path(&self, path: &str) -> Option<&TagsConfiguration> {
        let ext = Path::new(path).extension().and_then(|e| e.to_str())?;

        self.configs.get(ext).map(|entry| &entry.config)
    }

    /// Get the language name for a given file path extension.
    /// Returns None if the file type is not supported.
    pub fn language_name_for_path(&self, path: &str) -> Option<&str> {
        let ext = Path::new(path).extension().and_then(|e| e.to_str())?;

        self.configs.get(ext).map(|entry| entry.language_name)
    }

    /// Check if a file path is supported by the registry.
    pub fn is_supported(&self, path: &str) -> bool {
        Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|ext| self.configs.contains_key(ext))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creates_successfully() {
        let registry = LanguageRegistry::new();
        assert!(registry.is_ok());
    }

    #[test]
    fn test_supported_extensions() {
        let registry = LanguageRegistry::new().unwrap();

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
        let registry = LanguageRegistry::new().unwrap();

        assert!(!registry.is_supported("foo.txt"));
        assert!(!registry.is_supported("foo.md"));
        assert!(!registry.is_supported("foo.json"));
        assert!(!registry.is_supported("foo"));
    }

    #[test]
    fn test_language_name_for_path() {
        let registry = LanguageRegistry::new().unwrap();

        assert_eq!(registry.language_name_for_path("foo.rs"), Some("rust"));
        assert_eq!(registry.language_name_for_path("foo.py"), Some("python"));
        assert_eq!(registry.language_name_for_path("foo.txt"), None);
    }
}
