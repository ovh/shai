use tree_sitter_tags::{TagsConfiguration, TagsContext};

/// A single extracted symbol from a source file.
#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub kind: String,
    pub line_start: u32,
    pub line_end: u32,
}

/// Extract symbols from source code using a pre-loaded tags configuration.
///
/// Returns an empty vector if parsing fails or no symbols are found.
pub fn extract_symbols(content: &str, config: &TagsConfiguration) -> Vec<Symbol> {
    let mut context = TagsContext::new();
    let source = content.as_bytes();

    let tags_iter = match context.generate_tags(config, source, None) {
        Ok((iter, _)) => iter,
        Err(_) => return Vec::new(),
    };

    let mut symbols = Vec::new();

    for tag in tags_iter.flatten() {
        if !tag.is_definition {
            continue;
        }

        let name = extract_name(source, tag.name_range.clone());
        if name.is_empty() {
            continue;
        }

        let line_start = byte_to_line(content, tag.range.start);
        let line_end = byte_to_line(content, tag.range.end);

        symbols.push(Symbol {
            name,
            kind: String::new(), // kind is determined by syntax_type_id
            line_start,
            line_end,
        });
    }

    symbols
}

/// Extract the name string from source bytes given a byte range.
fn extract_name(source: &[u8], range: std::ops::Range<usize>) -> String {
    if range.start >= source.len() || range.end > source.len() {
        return String::new();
    }
    String::from_utf8_lossy(&source[range]).to_string()
}

/// Convert a byte offset to a 1-based line number.
fn byte_to_line(content: &str, byte_offset: usize) -> u32 {
    content[..byte_offset.min(content.len())]
        .lines()
        .count()
        .max(1) as u32
}

/// Format symbols as a compact outline string.
///
/// Example output:
/// ```text
/// === src/tools/fs/read/read.rs ===
///   L1-L134    struct ReadTool
///   L18-L20    fn new()
///   L22-L114   fn read_file_content()
/// ```
pub fn format_outline(symbols: &[Symbol], path: &str) -> String {
    if symbols.is_empty() {
        return format!("=== {} ===\n(no symbols found)", path);
    }

    let mut output = format!("=== {} ===\n", path);

    for symbol in symbols {
        let line_range = if symbol.line_start == symbol.line_end {
            format!("L{}", symbol.line_start)
        } else {
            format!("L{}-L{}", symbol.line_start, symbol.line_end)
        };

        output.push_str(&format!("  {:10} {}\n", line_range, symbol.name));
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_symbols_rust() {
        let registry = super::super::registry::LanguageRegistry::new().unwrap();
        let config = registry.config_for_path("test.rs").unwrap();

        let source = r#"
pub struct Foo {
    x: i32,
}

impl Foo {
    pub fn bar(&self) -> i32 {
        self.x
    }
}

fn main() {
    println!("hello");
}
"#;

        let symbols = extract_symbols(source, config);
        assert!(!symbols.is_empty(), "should extract at least one symbol");
    }

    #[test]
    fn test_extract_symbols_python() {
        let registry = super::super::registry::LanguageRegistry::new().unwrap();
        let config = registry.config_for_path("test.py").unwrap();

        let source = r#"
class Foo:
    def bar(self):
        return 42

def main():
    print("hello")
"#;

        let symbols = extract_symbols(source, config);
        assert!(!symbols.is_empty(), "should extract at least one symbol");
    }

    #[test]
    fn test_extract_symbols_bash() {
        let registry = super::super::registry::LanguageRegistry::new().unwrap();
        let config = registry.config_for_path("test.sh").unwrap();

        let source = r#"
function greet() {
    echo "hello"
}

NAME="world"
"#;

        let symbols = extract_symbols(source, config);
        assert!(!symbols.is_empty(), "should extract at least one symbol");
    }

    #[test]
    fn test_format_outline_empty() {
        let outline = format_outline(&[], "empty.rs");
        assert!(outline.contains("no symbols found"));
    }

    #[test]
    fn test_format_outline_nonempty() {
        let symbols = vec![
            Symbol {
                name: "foo".to_string(),
                kind: "function".to_string(),
                line_start: 1,
                line_end: 10,
            },
            Symbol {
                name: "bar".to_string(),
                kind: "function".to_string(),
                line_start: 12,
                line_end: 12,
            },
        ];

        let outline = format_outline(&symbols, "test.rs");
        assert!(outline.contains("=== test.rs ==="));
        assert!(outline.contains("L1-L10"));
        assert!(outline.contains("L12"));
        assert!(outline.contains("foo"));
        assert!(outline.contains("bar"));
    }
}
