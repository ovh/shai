use super::ansi::strip_ansi;
use regex::Regex;
use std::sync::OnceLock;

/// Lines matching this pattern are candidates for preservation during
/// head/tail truncation (subject to the preserved-error budget).
const ERROR_PATTERN: &str = "error|Error|ERROR|failed|FAILED|panic|FATAL|Exception";

/// Maximum number of error/diagnostic lines preserved from the omitted middle.
const MAX_PRESERVED_ERROR_LINES: usize = 20;

/// Fraction of `max_chars` reserved for the preserved-error block.
const PRESERVED_BUDGET_DIVISOR: usize = 4;

/// Room kept aside for marker lines (`[… N lines omitted …]` + preserved header).
const MARKER_RESERVE: usize = 64;

/// Worst-case length of the preserved block header line (count of 2 digits).
const MAX_PRESERVED_HEADER_LEN: usize = 49;

/// Suffix appended to a preserved line truncated to fit the budget.
const TRUNCATION_SUFFIX: &str = " …";

/// Minimum useful fragment length before bothering to truncate a line.
const MIN_TRUNCATED_FRAGMENT: usize = 16;

/// Cap on budget-refinement passes (converges in 2-3 in practice).
const MAX_BUDGET_PASSES: usize = 8;

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
///    `[… N lines omitted …]` marker. Up to `max_chars / 4` is reserved to
///    also preserve error/diagnostic lines (matching `ERROR_PATTERN`) from
///    the omitted middle. The assembled result never exceeds `max_chars`.
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

/// Count how many lines fit from the start and from the end of `lines`
/// within `char_budget` (each), without overlapping.
fn fit_head_tail(lines: &[&str], char_budget: usize) -> (usize, usize) {
    let mut head_count = 0;
    let mut head_size = 0;
    for (i, line) in lines.iter().enumerate() {
        let candidate = head_size + line.len() + 1;
        if candidate > char_budget {
            break;
        }
        head_size = candidate;
        head_count = i + 1;
    }

    let mut tail_count = 0;
    let mut tail_size = 0;
    for i in (0..lines.len()).rev() {
        if i < head_count {
            break;
        }
        let candidate = tail_size + lines[i].len() + 1;
        if candidate > char_budget {
            break;
        }
        tail_size = candidate;
        tail_count += 1;
    }

    (head_count, tail_count)
}

fn preserved_header(count: usize) -> String {
    format!("[{} preserved error line(s) from omitted middle:]", count)
}

/// Cost in chars of the preserved block as assembled: header line + one
/// newline per preserved line + the lines themselves.
fn preserved_block_cost(block: &[String]) -> usize {
    if block.is_empty() {
        return 0;
    }
    preserved_header(block.len()).len() + 1 + block.iter().map(|line| line.len() + 1).sum::<usize>()
}

/// Collect up to [`MAX_PRESERVED_ERROR_LINES`] error lines from the omitted
/// middle. The returned block (header included) never costs more than
/// `budget` chars; a line that only partially fits is truncated with
/// [`TRUNCATION_SUFFIX`] when a useful fragment remains.
fn collect_preserved(middle: &[&str], budget: usize) -> Vec<String> {
    let line_budget = budget.saturating_sub(MAX_PRESERVED_HEADER_LEN + 1);
    let mut preserved: Vec<String> = Vec::new();
    let mut used = 0;

    for line in middle.iter().filter(|line| is_error_line(line)) {
        if preserved.len() >= MAX_PRESERVED_ERROR_LINES {
            break;
        }
        if used + line.len() < line_budget {
            used += line.len() + 1;
            preserved.push((*line).to_string());
            continue;
        }
        // Line does not fit whole: keep a truncated fragment if it is
        // still useful, then stop (nothing after can fit either).
        let room = line_budget.saturating_sub(used + 1);
        if room >= TRUNCATION_SUFFIX.len() + MIN_TRUNCATED_FRAGMENT {
            let keep = room - TRUNCATION_SUFFIX.len();
            let mut fragment = truncate_to_char_boundary(line, keep).to_string();
            fragment.push_str(TRUNCATION_SUFFIX);
            preserved.push(fragment);
        }
        break;
    }

    preserved
}

/// Truncate `s` to at most `max` bytes without splitting a UTF-8 char.
fn truncate_to_char_boundary(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Truncate `input` to head + tail with a marker, preserving error lines.
///
/// The preserved-error block is allocated from the same `max_chars` budget
/// (up to a quarter of it), then head and tail split the remainder, so the
/// assembled result never exceeds `max_chars`. Shrinking head/tail grows the
/// middle and can change which error lines are preserved, so the allocation
/// is refined over a few passes until the reserved cost stabilizes.
fn truncate_head_tail(input: &str, max_chars: usize) -> String {
    let lines: Vec<&str> = input.lines().collect();
    if lines.is_empty() {
        return input.to_string();
    }

    let preserved_budget = max_chars / PRESERVED_BUDGET_DIVISOR;

    let mut reserved = 0;
    let mut head_count = 0;
    let mut tail_count = 0;
    let mut preserved: Vec<String> = Vec::new();
    let mut converged = false;

    for _ in 0..MAX_BUDGET_PASSES {
        let char_budget = max_chars.saturating_sub(reserved + MARKER_RESERVE) / 2;
        let (head, tail) = fit_head_tail(&lines, char_budget);
        if head + tail >= lines.len() {
            return input.to_string();
        }
        let block = collect_preserved(&lines[head..lines.len() - tail], preserved_budget);
        let cost = preserved_block_cost(&block);
        (head_count, tail_count, preserved) = (head, tail, block);
        if cost <= reserved {
            converged = true;
            break;
        }
        reserved = cost;
    }

    if !converged {
        // Pathological input kept growing the reserved cost; fall back to the
        // worst-case reservation so the size guarantee still holds.
        reserved = preserved_budget;
        let char_budget = max_chars.saturating_sub(reserved + MARKER_RESERVE) / 2;
        let (head, tail) = fit_head_tail(&lines, char_budget);
        if head + tail >= lines.len() {
            return input.to_string();
        }
        preserved = collect_preserved(&lines[head..lines.len() - tail], preserved_budget);
        (head_count, tail_count) = (head, tail);
    }

    let head_lines = &lines[..head_count];
    let tail_lines = &lines[lines.len() - tail_count..];
    let omitted = lines.len() - head_count - tail_count - preserved.len();

    let mut result = String::with_capacity(max_chars);
    for line in head_lines {
        result.push_str(line);
        result.push('\n');
    }
    result.push_str(&format!("[… {} lines omitted …]\n", omitted));
    if !preserved.is_empty() {
        result.push_str(&preserved_header(preserved.len()));
        result.push('\n');
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

        let result = compact_generic(&input, 4000);
        assert_eq!(
            result.matches("ERROR unique diagnostic").count(),
            MAX_PRESERVED_ERROR_LINES
        );
        assert!(result.len() <= 4000);
    }

    #[test]
    fn test_preserved_error_lines_respect_max_chars() {
        // 25 long error lines in the middle: the preserved block must stay
        // within the budget, truncating individual lines as needed.
        let mut lines: Vec<String> = (0..600).map(|i| format!("padding line {}", i)).collect();
        for i in 0..25 {
            lines.insert(300 + i * 2, format!("error diagnostic {:0>1000}", i));
        }
        let input = lines.join("\n");

        let result = compact_generic(&input, 2000);
        assert!(
            result.len() <= 2000,
            "result of {} chars exceeds max_chars",
            result.len()
        );
        assert!(result.contains("preserved error line"));
        assert!(result.contains(TRUNCATION_SUFFIX));
    }

    #[test]
    fn test_preserved_block_empty_when_no_errors_in_middle() {
        let lines: Vec<String> = (0..1000).map(|i| format!("plain line {}", i)).collect();
        let input = lines.join("\n");

        let result = compact_generic(&input, 500);
        assert!(result.len() <= 500);
        assert!(result.contains("lines omitted"));
        assert!(!result.contains("preserved error line"));
    }

    #[test]
    fn test_truncate_to_char_boundary_multibyte() {
        let s = "ééééé"; // 10 bytes, 5 chars
        assert_eq!(truncate_to_char_boundary(s, 4), "éé");
        assert_eq!(truncate_to_char_boundary(s, 100), s);
    }
}
