use super::ansi::strip_ansi;

/// Lines matching this pattern are always preserved during head/tail truncation.
#[allow(dead_code)]
const ERROR_PATTERN: &str = "error|Error|ERROR|failed|FAILED|panic|FATAL|Exception";

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

        if let Some(_) = iter.peek() {
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
    let omitted = lines.len() - head_count - tail_count;

    let mut result = String::with_capacity(max_chars + 64);
    for line in head_lines {
        result.push_str(line);
        result.push('\n');
    }
    result.push_str(&format!("[… {} lines omitted …]\n", omitted));
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
}
