use regex::Regex;

/// Lines matching this pattern indicate errors/failures that must be preserved.
const ERROR_LINE_PATTERN: &str = r"(?i)^\s*(error|failed|failure|panic|fatal|exception|traceback)";

/// Bash command-aware compaction.
///
/// Inspects the command and applies targeted reformatting:
/// - `cargo test` / `cargo t`: keep FAILED lines + summary.
/// - `cargo build` / `cargo check` / `cargo clippy`: keep error/warning lines.
/// - `pytest`: keep FAILED/ERROR sections + summary.
/// - `git status`: pass through (already compact).
/// - `git diff`: pass through (already diff-targeted).
/// - `grep`/`rg`: limit to first 50 matches.
/// - Default: fall through to generic compaction.
pub fn compact_bash(command: &str, output: &str, max_chars: usize) -> String {
    let cmd = command.trim();

    if cmd.starts_with("cargo test") || cmd.starts_with("cargo t ") {
        return compact_cargo_test(output, max_chars);
    }
    if cmd.starts_with("cargo build")
        || cmd.starts_with("cargo check")
        || cmd.starts_with("cargo clippy")
    {
        return compact_cargo_build(output, max_chars);
    }
    if cmd.starts_with("pytest") {
        return compact_pytest(output, max_chars);
    }
    if cmd.starts_with("grep") || cmd.starts_with("rg ") {
        return limit_matches(output, 50);
    }
    // git status, git diff — already compact, pass through
    output.to_string()
}

/// Extract failed test lines and the summary from `cargo test` output.
fn compact_cargo_test(output: &str, _max_chars: usize) -> String {
    let mut kept = Vec::new();
    let error_re = Regex::new(ERROR_LINE_PATTERN).unwrap();

    for line in output.lines() {
        let trimmed = line.trim();
        if error_re.is_match(trimmed)
            || trimmed.starts_with("test ")
            || trimmed.contains("test result:")
            || trimmed.contains("running ")
        {
            kept.push(line.to_string());
        }
    }

    if kept.is_empty() {
        return output.to_string();
    }
    kept.join("\n")
}

/// Extract error/warning blocks from `cargo build`/`clippy` output.
///
/// Parses the output into blocks separated by blank lines and keeps any
/// block that contains an `error` or `warning` line. This preserves the
/// full context around each issue: `-->` file locations, `note:`/`help:`
/// annotations, and code snippets.
///
/// If no error/warning lines are found (success case), returns the output
/// unchanged so the agent sees the `Finished` line and exit status.
fn compact_cargo_build(output: &str, max_chars: usize) -> String {
    if output.len() <= max_chars {
        return output.to_string();
    }

    let error_re = Regex::new(r"(?i)^\s*(error|warning)\b").unwrap();
    let blocks: Vec<&str> = output.split("\n\n").collect();
    let mut kept = Vec::new();

    for block in &blocks {
        let has_error = block.lines().any(|line| {
            let trimmed = line.trim();
            error_re.is_match(trimmed)
                || trimmed.starts_with("error[")
                || trimmed.starts_with("warning[")
        });
        if has_error {
            kept.push(*block);
        }
    }

    if kept.is_empty() {
        return output.to_string();
    }
    kept.join("\n\n")
}

/// Extract FAILED/ERROR sections from pytest output.
fn compact_pytest(output: &str, _max_chars: usize) -> String {
    let mut kept = Vec::new();
    let error_re = Regex::new(ERROR_LINE_PATTERN).unwrap();

    for line in output.lines() {
        let trimmed = line.trim();
        if error_re.is_match(trimmed) || trimmed.starts_with("===") || trimmed.starts_with("---") {
            kept.push(line.to_string());
        }
    }

    if kept.is_empty() {
        return output.to_string();
    }
    kept.join("\n")
}

/// Limit grep/rg output to first N matches, appending a marker if truncated.
fn limit_matches(output: &str, limit: usize) -> String {
    let lines: Vec<&str> = output.lines().collect();
    if lines.len() <= limit {
        return output.to_string();
    }

    let mut result = lines[..limit].join("\n");
    result.push_str(&format!(
        "\n… ({} more matches omitted)",
        lines.len() - limit
    ));
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cargo_test_keep_failures() {
        let output = "running tests\ntest foo ... ok\ntest bar ... FAILED\n\ntest result: FAILED. 1 passed; 1 failed\n";
        let result = compact_bash("cargo test", output, 8000);
        assert!(result.contains("test bar ... FAILED"));
        assert!(result.contains("test result:"));
    }

    #[test]
    fn test_cargo_build_keep_errors() {
        let output = "   Compiling foo\n\nerror[E0308]: mismatched types\nwarning: unused variable\nnote: stuff";
        let result = compact_bash("cargo build", output, 10);
        assert!(result.contains("error[E0308]"));
        assert!(result.contains("warning: unused variable"));
        assert!(result.contains("note: stuff"));
        assert!(!result.contains("Compiling"));
    }

    #[test]
    fn test_cargo_build_keep_error_location() {
        let output = "   Compiling foo v0.1.0\n\nerror[E0277]: the trait `Debug` is not implemented for `ThinkerDecision`\n  --> src/agent/actions/brain.rs:50:47\n   |\n50 |         let ThinkerDecision{message, flow, token_usage} = self.handle_brain_error(result).await?;\n   |                    ^^^^^^^^^^^^\n\nerror: aborting due to previous error";
        let result = compact_bash("cargo check", output, 10);
        assert!(result.contains("error[E0277]"));
        assert!(result.contains("--> src/agent/actions/brain.rs:50:47"));
        assert!(result.contains("error: aborting"));
        assert!(!result.contains("Compiling"));
    }

    #[test]
    fn test_cargo_build_short_output_passthrough() {
        let output = "   Compiling foo v0.1.10\n    Finished `dev` profile";
        let result = compact_bash("cargo check", output, 8000);
        assert_eq!(result, output);
    }

    #[test]
    fn test_cargo_build_preserves_blocks() {
        let output = "   Compiling foo v0.1.10\n    Finished `dev` profile\n\nerror[E0277]: missing field\n  --> src/main.rs:10:5\n   |\n10 |     let x = foo;\n   |         ^^^         missing field\n   |\nnote: consider adding\nhelp: try\n\nwarning: unused";
        let result = compact_bash("cargo check", output, 10);
        assert!(result.contains("error[E0277]"));
        assert!(result.contains("--> src/main.rs:10:5"));
        assert!(result.contains("note: consider adding"));
        assert!(result.contains("help: try"));
        assert!(result.contains("warning: unused"));
        assert!(!result.contains("Compiling"));
        assert!(!result.contains("Finished"));
    }

    #[test]
    fn test_pytest_keep_failures() {
        let output = "==== FAILURES ====\n---- test_foo ----\npassed = 1\n==== short test summary ====\nFAILED test_foo::test_bar";
        let result = compact_pytest(output, 8000);
        assert!(result.contains("FAILURES"));
        assert!(result.contains("FAILED"));
    }

    #[test]
    fn test_grep_limit() {
        let mut output = String::new();
        for i in 0..100 {
            output.push_str(&format!("match {}\n", i));
        }
        let result = limit_matches(&output, 10);
        assert!(result.contains("match 9"));
        assert!(result.contains("more matches omitted"));
    }

    #[test]
    fn test_git_status_passthrough() {
        let output = "On branch main\nnothing to commit";
        assert_eq!(compact_bash("git status", output, 8000), output);
    }
}
