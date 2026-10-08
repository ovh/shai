use crate::config::agent::VerificationConfig;
use std::path::Path;
use std::time::Duration;
use tokio::process::Command;
use tracing::warn;

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

/// Runs configured verification commands for the languages present among `edited_files`.
///
/// Returns `None` when there are no diagnostics (clean) or `Some(String)` with the
/// combined output when the verifier produced output. Failures (binary not found,
/// timeout, permission errors) are logged as warnings and treated as "no diagnostics".
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
        let ext = Path::new(file_path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        if let Some(lang) = language_for_extension(ext) {
            languages.insert(lang);
        }
    }

    if languages.is_empty() {
        return None;
    }

    let mut diagnostics = Vec::new();

    for lang in &languages {
        let command = match config.commands.get(*lang) {
            Some(cmd) => cmd,
            None => continue,
        };

        if command.is_empty() {
            continue;
        }

        let mut cmd = Command::new(&command[0]);
        cmd.args(&command[1..]);

        if let Some(dir) = working_dir {
            cmd.current_dir(dir);
        }

        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let timeout = Duration::from_secs(config.timeout_secs);
        let result = tokio::time::timeout(timeout, cmd.output()).await;

        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let combined = format!("{}{}", stdout, stderr);
                if !combined.trim().is_empty() {
                    diagnostics.push(format!(
                        "=== Verification ({}) ===\n{}",
                        lang,
                        combined.trim()
                    ));
                }
            }
            Ok(Err(e)) => {
                warn!("Verification command '{}' failed: {}", command.join(" "), e);
            }
            Err(_) => {
                warn!(
                    "Verification command '{}' timed out after {}s",
                    command.join(" "),
                    config.timeout_secs
                );
                diagnostics.push(format!(
                    "=== Verification ({}) timed out after {}s ===",
                    lang, config.timeout_secs
                ));
            }
        }
    }

    if diagnostics.is_empty() {
        None
    } else {
        Some(diagnostics.join("\n\n"))
    }
}
