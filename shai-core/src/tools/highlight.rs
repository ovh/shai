use std::sync::OnceLock;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Parser, Query, QueryCursor};

use super::fs::symbol::HighlightRegistry;

/// ANSI color codes for syntax highlighting themes.
struct SyntaxTheme {
    keyword: &'static str,
    string: &'static str,
    number: &'static str,
    comment: &'static str,
    function: &'static str,
    type_name: &'static str,
    property: &'static str,
    operator: &'static str,
    punctuation: &'static str,
    variable: &'static str,
    constant: &'static str,
    reset: &'static str,
}

impl SyntaxTheme {
    const fn dark() -> Self {
        Self {
            keyword: "\x1b[34m",     // Blue
            string: "\x1b[32m",      // Green
            number: "\x1b[33m",      // Yellow
            comment: "\x1b[90m",     // Dim gray
            function: "\x1b[36m",    // Cyan
            type_name: "\x1b[35m",   // Magenta
            property: "\x1b[36m",    // Cyan
            operator: "\x1b[37m",    // White
            punctuation: "\x1b[37m", // White
            variable: "\x1b[37m",    // White
            constant: "\x1b[33m",    // Yellow
            reset: "\x1b[0m",
        }
    }
}

static REGISTRY: OnceLock<HighlightRegistry> = OnceLock::new();

fn registry() -> &'static HighlightRegistry {
    REGISTRY
        .get_or_init(|| HighlightRegistry::new().expect("Failed to initialize HighlightRegistry"))
}

/// Map a tree-sitter capture name to an ANSI color prefix.
///
/// Capture names are dot-separated (e.g. "keyword.function").
/// Returns `None` if the capture should not be highlighted.
fn capture_to_ansi(capture: &str, theme: &SyntaxTheme) -> Option<&'static str> {
    let capture = capture.split('.').next().unwrap_or(capture);
    match capture {
        "keyword" => Some(theme.keyword),
        "string" => Some(theme.string),
        "comment" => Some(theme.comment),
        "number" => Some(theme.number),
        "function" => Some(theme.function),
        "type" => Some(theme.type_name),
        "property" => Some(theme.property),
        "operator" => Some(theme.operator),
        "punctuation" => Some(theme.punctuation),
        "variable" => Some(theme.variable),
        "constant" => Some(theme.constant),
        "constructor" => Some(theme.function),
        "boolean" => Some(theme.keyword),
        "module" => Some(theme.type_name),
        "tag" => Some(theme.keyword),
        "attribute" => Some(theme.property),
        // These captures don't add color themselves
        "embedded" | "escape" | "error" | "markup" => None,
        _ => None,
    }
}

/// Highlight source code content with ANSI escape codes.
///
/// Returns the content unchanged if the file type is not supported or highlighting fails.
pub fn highlight_content(content: &str, file_path: &str) -> String {
    let entry = match registry().entry_for_path(file_path) {
        Some(entry) => entry,
        None => return content.to_string(),
    };

    let mut parser = Parser::new();
    if parser.set_language(entry.language()).is_err() {
        return content.to_string();
    }

    let tree = match parser.parse(content.as_bytes(), None) {
        Some(t) => t,
        None => return content.to_string(),
    };

    let query = match Query::new(entry.language(), entry.highlights_query()) {
        Ok(q) => q,
        Err(_) => return content.to_string(),
    };

    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), content.as_bytes());

    // Collect all captures: (start_byte, end_byte, capture_name)
    let mut highlights: Vec<(usize, usize, &str)> = Vec::new();
    let capture_names = query.capture_names();
    while let Some(mat) = matches.next() {
        for capture in mat.captures.iter() {
            let capture_name = capture_names
                .get(capture.index as usize)
                .map(|s| s.as_ref())
                .unwrap_or("");
            let node = capture.node;
            highlights.push((node.start_byte(), node.end_byte(), capture_name));
        }
    }

    // Sort by start position, then by end position (descending = longer ranges first)
    highlights.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| b.1.cmp(&a.1)));

    // Remove overlapping captures — keep the first (longest) at each position
    let mut filtered: Vec<(usize, usize, &str)> = Vec::new();
    for cap in highlights {
        if filtered.is_empty() || cap.0 >= filtered.last().unwrap().1 {
            filtered.push(cap);
        }
    }

    // Build highlighted string
    let theme = SyntaxTheme::dark();
    let mut result = String::with_capacity(content.len() + content.len() / 4);
    let mut last_end = 0;

    for (start, end, capture_name) in &filtered {
        if start < &last_end {
            continue;
        }
        result.push_str(&content[last_end..*start]);
        let color = capture_to_ansi(capture_name, &theme);
        if let Some(color) = color {
            result.push_str(color);
            result.push_str(&content[*start..*end]);
            result.push_str(theme.reset);
        } else {
            result.push_str(&content[*start..*end]);
        }
        last_end = *end;
    }
    result.push_str(&content[last_end..]);

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_highlight_rust() {
        let content = "fn main() { let x = 42; }";
        let result = highlight_content(content, "test.rs");
        // Should contain some ANSI escape codes
        assert!(result.contains("\x1b["));
        // Should still contain the original text
        assert!(result.contains("fn"));
        assert!(result.contains("main"));
    }

    #[test]
    fn test_highlight_python() {
        let content = "def hello():\n    return 42";
        let result = highlight_content(content, "test.py");
        assert!(result.contains("\x1b["));
        assert!(result.contains("def"));
    }

    #[test]
    fn test_highlight_unsupported_extension() {
        let content = "some random text";
        let result = highlight_content(content, "test.xyz");
        assert_eq!(result, content);
    }

    #[test]
    fn test_highlight_no_extension() {
        let content = "some random text";
        let result = highlight_content(content, "testfile");
        assert_eq!(result, content);
    }
}
