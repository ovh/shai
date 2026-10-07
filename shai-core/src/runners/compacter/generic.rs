use super::ansi::strip_ansi;
use regex::Regex;
use std::sync::OnceLock;

/// Lines matching this pattern are always preserved during head/tail truncation.
const ERROR_PATTERN: &str = "error|Error|ERROR|failed|FAILED|panic|FATAL|Exception";

/// Maximum number of error/diagnostic lines preserved from the omitted middle.
const MAX_PRESERVED_ERROR_LINES: usize = 20;

static ERROR_RE: OnceLock<Regex> = OnceLock::new();

fn is_error_line(line: &str) -> bool {
    ERROR_RE
        .get_or_init(|| Regex::new(ERROR_PATTERN).expect("ERROR_PATTERN is a valid regex"))
        .is_match(line)
}

/// Generic compaction applied to every tool result regardless of tool name.
///
/// 1. Strip ANSI escape sequences.
/// 2. Collapse consecutive duplicate lines into `…×N` notation.
/// 3. If the output still exceeds `max_chars`, keep head + tail with a
///    `[… N lines omitted …]` marker. Lines matching `ERROR_PATTERN` are
///    always preserved.
pub fn compact_generic(input: &str, max_chars: usize) -> String {
    let stripped = strip_ansi(input);
    let collapsed = collapse_duplicate_lines(&stripped);

    if stripped.len() <= max_chars {
        return collapsed;
    }

    truncate_head_tail(&collapsed, max_chars)
}

/// Collapse consecutive duplicate lines into `line …×N` notation.
fn collapse_duplicate_lines(input: &str) -> String {
    let lines: Vec<&str> = input.lines().collect();
    if lines.is_empty() {
        return input.to_string();
    }

    let mut result = String::with_capacity(input.len());
    let mut iter = lines.iter().peekable();

    while let Some(line) = iter.next() {
        let mut count = 1;
        while iter.peek() == Some(&line) {
            count += 1;
            iter.next();
        }

        if count > 1 {
            result.push_str(line);
            result.push_str(&format!(" …×{}", count));
        } else {
            result.push_str(line);
        }

        if iter.peek().is_some() {
            result.push('\n');
        }
    }

    result
}

/// Truncate `input` to head + tail with a marker, preserving error lines.
fn truncate_head_tail(input: &str, max_chars: usize) -> String {
    let lines: Vec<&str> = input.lines().collect();
    if lines.is_empty() {
        return input.to_string();
    }

    // Roughly split budget between head and tail (each gets ~40%)
    let char_budget = max_chars / 2;
    let mut head_count = 0;
    let mut tail_count = 0;
    let mut head_size = 0;
    let mut tail_size = 0;

    // Count how many lines fit in the head budget
    for (i, line) in lines.iter().enumerate() {
        let candidate = head_size + line.len() + 1;
        if candidate > char_budget {
            break;
        }
        head_size = candidate;
        head_count = i + 1;
    }

    // Count how many lines fit in the tail budget (from the end)
    for i in (0..lines.len()).rev() {
        let candidate = tail_size + lines[i].len() + 1;
        if candidate > char_budget {
            break;
        }
        tail_size = candidate;
        tail_count += 1;
    }

    // Ensure we don't overlap
    if head_count + tail_count >= lines.len() {
        return input.to_string();
    }

    let head_lines = &lines[..head_count];
    let tail_lines = &lines[lines.len() - tail_count..];
    let middle = &lines[head_count..lines.len() - tail_count];

    // Preserve error/diagnostic lines from the omitted middle so the agent
    // still sees compiler/test failures even when the bulk is truncated.
    let preserved: Vec<&&str> = middle
        .iter()
        .filter(|line| is_error_line(line))
        .take(MAX_PRESERVED_ERROR_LINES)
        .collect();
    let omitted = middle.len() - preserved.len();

    let mut result = String::with_capacity(max_chars + 64);
    for line in head_lines {
        result.push_str(line);
        result.push('\n');
    }
    result.push_str(&format!("[… {} lines omitted …]\n", omitted));
    if !preserved.is_empty() {
        result.push_str(&format!(
            "[{} preserved error line(s) from omitted middle:]\n",
            preserved.len()
        ));
        for line in &preserved {
            result.push_str(line);
            result.push('\n');
        }
    }
    for line in tail_lines {
        result.push_str(line);
        result.push('\n');
    }

    // Remove trailing newline
    if result.ends_with('\n') {
        result.pop();
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collapse_duplicate_lines() {
        let input = "foo\nfoo\nfoo\nbar\nbaz\nbaz";
        let result = collapse_duplicate_lines(input);
        assert_eq!(result, "foo …×3\nbar\nbaz …×2");
    }

    #[test]
    fn test_collapse_no_duplicates() {
        let input = "foo\nbar\nbaz";
        assert_eq!(collapse_duplicate_lines(input), "foo\nbar\nbaz");
    }

    #[test]
    fn test_truncate_preserves_short_input() {
        let input = "short\noutput";
        assert_eq!(compact_generic(input, 8000), "short\noutput");
    }

    #[test]
    fn test_truncate_large_output() {
        let mut input = String::new();
        for i in 0..1000 {
            input.push_str(&format!("line {}\n", i));
        }
        let result = compact_generic(&input, 100);
        assert!(result.contains("lines omitted"));
        assert!(result.lines().count() < 100);
    }

    #[test]
    fn test_strip_ansi_applied() {
        let input = "\x1b[31mhello\x1b[0m";
        assert_eq!(compact_generic(input, 8000), "hello");
    }

    #[test]
    fn test_truncate_preserves_error_lines_from_middle() {
        let mut lines: Vec<String> = (0..500)
            .map(|i| format!("info line number {}", i))
            .collect();
        lines.insert(
            250,
            "error[E0308]: mismatched types at the middle".to_string(),
        );
        let input = lines.join("\n");

        let result = compact_generic(&input, 2000);
        assert!(result.contains("error[E0308]: mismatched types at the middle"));
        assert!(result.contains("lines omitted"));
        // Non-error middle lines stay omitted
        assert!(!result.contains("info line number 250"));
        assert!(!result.contains("info line number 249"));
    }

    #[test]
    fn test_truncate_caps_preserved_error_lines() {
        let mut lines: Vec<String> = (0..600).map(|i| format!("padding line {}", i)).collect();
        for i in 0..30 {
            lines.insert(300 + i * 2, format!("ERROR unique diagnostic {}", i));
        }
        let input = lines.join("\n");

        let result = compact_generic(&input, 2000);
        assert_eq!(
            result.matches("ERROR unique diagnostic").count(),
            MAX_PRESERVED_ERROR_LINES
        );
    }
}
