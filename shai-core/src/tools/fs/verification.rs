use crate::config::agent::VerificationConfig;
use std::path::Path;
use std::time::Duration;
use tokio::process::Command;
use tracing::warn;

/// Placeholder in configured commands, replaced with each edited file.
const FILES_PLACEHOLDER: &str = "{files}";

/// Cap on per-file verification runs for a single language.
const MAX_FILES_PER_LANGUAGE: usize = 10;

/// Maps file extensions to language identifiers used in `VerificationConfig.commands`.
fn language_for_extension(ext: &str) -> Option<&'static str> {
    match ext {
        "rs" => Some("rust"),
        "go" => Some("go"),
        "py" => Some("python"),
        "ts" | "tsx" => Some("typescript"),
        "js" | "mjs" | "cjs" => Some("javascript"),
        "pl" => Some("perl"),
        "rb" => Some("ruby"),
        "sh" | "bash" => Some("bash"),
        "php" => Some("php"),
        "lua" => Some("lua"),
        _ => None,
    }
}

fn language_for_file(path: &str) -> Option<&'static str> {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .and_then(language_for_extension)
}

/// Run a single verification command and return diagnostic text if any.
///
/// A nonzero exit status is always reported, even when the command produced
/// no output. Spawn failures (binary not found, permission errors) are logged
/// as warnings and treated as "no diagnostics".
async fn run_command(
    args: &[String],
    label: &str,
    working_dir: &Option<String>,
    timeout_secs: u64,
) -> Option<String> {
    let mut cmd = Command::new(&args[0]);
    cmd.args(&args[1..]);

    if let Some(dir) = working_dir {
        cmd.current_dir(dir);
    }

    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    match tokio::time::timeout(Duration::from_secs(timeout_secs), cmd.output()).await {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let combined = format!("{}{}", stdout, stderr);

            let header = if output.status.success() {
                format!("=== Verification ({}) ===", label)
            } else {
                format!(
                    "=== Verification ({}) exited with code {} ===",
                    label,
                    output.status.code().unwrap_or(-1)
                )
            };

            if !combined.trim().is_empty() {
                Some(format!("{}\n{}", header, combined.trim()))
            } else if !output.status.success() {
                Some(format!("{} (no output)", header))
            } else {
                None
            }
        }
        Ok(Err(e)) => {
            warn!("Verification command '{}' failed: {}", args.join(" "), e);
            None
        }
        Err(_) => {
            warn!(
                "Verification command '{}' timed out after {}s",
                args.join(" "),
                timeout_secs
            );
            Some(format!(
                "=== Verification ({}) timed out after {}s ===",
                label, timeout_secs
            ))
        }
    }
}

/// Runs configured verification commands for the languages present among `edited_files`.
///
/// Commands containing the `{files}` placeholder run once per edited file of
/// that language (several verifiers — `bash -n`, `ruby -c`, `perl -c`, `php -l` —
/// only check the *first* file argument, so batching files would silently skip
/// the rest). Commands without the placeholder run once, unchanged — suited to
/// project-scoped verifiers like `cargo check` or `go build ./...`.
///
/// Returns `None` when there are no diagnostics (clean) or `Some(String)` with the
/// combined output when a verifier produced output or a nonzero exit status.
pub async fn run_verification(
    edited_files: &[String],
    working_dir: &Option<String>,
    config: &VerificationConfig,
) -> Option<String> {
    if !config.enabled || edited_files.is_empty() {
        return None;
    }

    // Collect unique languages from file extensions
    let mut languages = std::collections::HashSet::new();
    for file_path in edited_files {
        if let Some(lang) = language_for_file(file_path) {
            languages.insert(lang);
        }
    }

    if languages.is_empty() {
        return None;
    }

    let mut diagnostics = Vec::new();

    for lang in &languages {
        let Some(command) = config.commands.get(*lang) else {
            continue;
        };
        if command.is_empty() {
            continue;
        }

        if command.iter().any(|arg| arg == FILES_PLACEHOLDER) {
            let lang_files: Vec<&String> = edited_files
                .iter()
                .filter(|f| language_for_file(f) == Some(*lang))
                .collect();

            for file in lang_files.iter().take(MAX_FILES_PER_LANGUAGE) {
                let args: Vec<String> = command
                    .iter()
                    .flat_map(|arg| {
                        if arg == FILES_PLACEHOLDER {
                            vec![file.to_string()]
                        } else {
                            vec![arg.clone()]
                        }
                    })
                    .collect();
                let label = format!("{}: {}", lang, file);
                if let Some(diag) =
                    run_command(&args, &label, working_dir, config.timeout_secs).await
                {
                    diagnostics.push(diag);
                }
            }

            if lang_files.len() > MAX_FILES_PER_LANGUAGE {
                diagnostics.push(format!(
                    "=== Verification ({}): {} more file(s) not verified ===",
                    lang,
                    lang_files.len() - MAX_FILES_PER_LANGUAGE
                ));
            }
        } else if let Some(diag) =
            run_command(command, lang, working_dir, config.timeout_secs).await
        {
            diagnostics.push(diag);
        }
    }

    if diagnostics.is_empty() {
        None
    } else {
        Some(diagnostics.join("\n\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn config_with(commands: &[(&str, &[&str])]) -> VerificationConfig {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        for (lang, cmd) in commands {
            map.insert(
                lang.to_string(),
                cmd.iter().map(|s| s.to_string()).collect(),
            );
        }
        VerificationConfig {
            enabled: true,
            timeout_secs: 10,
            commands: map,
        }
    }

    #[tokio::test]
    async fn test_nonzero_exit_reported_without_output() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("a.py");
        std::fs::write(&file, "x = 1\n").unwrap();

        let config = config_with(&[("python", &["false"])]);
        let result = run_verification(
            &[file.to_string_lossy().to_string()],
            &Some(dir.path().to_string_lossy().to_string()),
            &config,
        )
        .await;

        let text = result.expect("nonzero exit must produce diagnostics");
        assert!(text.contains("exited with code"), "{}", text);
    }

    #[tokio::test]
    async fn test_zero_exit_no_output_is_clean() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("a.py");
        std::fs::write(&file, "x = 1\n").unwrap();

        let config = config_with(&[("python", &["true"])]);
        let result = run_verification(
            &[file.to_string_lossy().to_string()],
            &Some(dir.path().to_string_lossy().to_string()),
            &config,
        )
        .await;

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_files_placeholder_runs_per_edited_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let a = dir.path().join("a.py");
        let b = dir.path().join("b.py");
        std::fs::write(&a, "alpha_content\n").unwrap();
        std::fs::write(&b, "beta_content\n").unwrap();

        let config = config_with(&[("python", &["cat", "{files}"])]);
        let result = run_verification(
            &[
                a.to_string_lossy().to_string(),
                b.to_string_lossy().to_string(),
            ],
            &Some(dir.path().to_string_lossy().to_string()),
            &config,
        )
        .await;

        let text = result.expect("cat output must produce diagnostics");
        assert!(text.contains("alpha_content"), "{}", text);
        assert!(text.contains("beta_content"), "{}", text);
    }

    #[tokio::test]
    async fn test_placeholder_only_receives_matching_language_files() {
        let dir = tempfile::TempDir::new().unwrap();
        let py = dir.path().join("a.py");
        let rb = dir.path().join("b.rb");
        std::fs::write(&py, "python_marker\n").unwrap();
        std::fs::write(&rb, "ruby_marker\n").unwrap();

        // Only python has a command; the ruby file must not leak into it
        let config = config_with(&[("python", &["cat", "{files}"])]);
        let result = run_verification(
            &[
                py.to_string_lossy().to_string(),
                rb.to_string_lossy().to_string(),
            ],
            &Some(dir.path().to_string_lossy().to_string()),
            &config,
        )
        .await;

        let text = result.expect("diagnostics expected");
        assert!(text.contains("python_marker"), "{}", text);
        assert!(!text.contains("ruby_marker"), "{}", text);
    }

    #[tokio::test]
    async fn test_no_placeholder_gets_no_file_args() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("a.py");
        std::fs::write(&file, "some_content\n").unwrap();

        let config = config_with(&[("python", &["echo", "hello"])]);
        let result = run_verification(
            &[file.to_string_lossy().to_string()],
            &Some(dir.path().to_string_lossy().to_string()),
            &config,
        )
        .await;

        let text = result.expect("echo output must produce diagnostics");
        assert!(text.contains("hello"), "{}", text);
        assert!(!text.contains("some_content"), "{}", text);
    }

    #[tokio::test]
    async fn test_disabled_or_no_files_returns_none() {
        let mut config = config_with(&[("python", &["false"])]);
        config.enabled = false;
        assert!(run_verification(&["a.py".to_string()], &None, &config)
            .await
            .is_none());

        let config = config_with(&[("python", &["false"])]);
        assert!(run_verification(&[], &None, &config).await.is_none());
    }
}
