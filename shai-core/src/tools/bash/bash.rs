use super::structs::{resolve_timeout_secs, BashToolConfig, BashToolParams};
use crate::tools::{tool, ToolResult};
use serde_json::json;
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, BufReader};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

/// How much of the captured stdout/stderr to include in a timeout error.
const PARTIAL_OUTPUT_TAIL_CHARS: usize = 8_000;
/// Hard cap on captured stdout/stderr bytes in drain_pipe (keep a tail).
pub(crate) const DRAIN_BUFFER_MAX_BYTES: usize = 64 * 1024;
/// Prefixed onto decoded output when the drain buffer dropped earlier bytes.
pub(crate) fn drain_truncation_marker() -> String {
    format!(
        "…[truncated: earlier output omitted, keeping last {} bytes]…\n",
        DRAIN_BUFFER_MAX_BYTES
    )
}

struct DrainBuf {
    bytes: Vec<u8>,
    truncated: bool,
}

impl DrainBuf {
    fn new() -> Self {
        Self {
            bytes: Vec::new(),
            truncated: false,
        }
    }
}

pub struct BashTool {
    config: BashToolConfig,
}

impl Default for BashTool {
    fn default() -> Self {
        Self::new()
    }
}

impl BashTool {
    pub fn new() -> Self {
        Self {
            config: BashToolConfig::default(),
        }
    }

    pub fn with_config(config: BashToolConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &BashToolConfig {
        &self.config
    }

    async fn kill_process_group(child: &mut tokio::process::Child) {
        #[cfg(unix)]
        {
            if let Some(pid) = child.id() {
                unsafe {
                    libc::kill(-(pid as i32), libc::SIGTERM);
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    libc::kill(-(pid as i32), libc::SIGKILL);
                }
            }
        }

        let _ = child.kill().await;
        let _ = child.wait().await;
    }

    /// Read stdout/stderr into a shared byte buffer so a timeout/cancel can
    /// still report what was emitted before the process was killed.
    /// Bytes are decoded once at snapshot time so multibyte UTF-8 characters
    /// split across read chunks are not corrupted.
    async fn drain_pipe<R: tokio::io::AsyncRead + Unpin + Send + 'static>(
        reader: R,
        buf: Arc<Mutex<DrainBuf>>,
    ) {
        let mut reader = BufReader::new(reader);
        let mut chunk = [0u8; 8192];
        loop {
            match reader.read(&mut chunk).await {
                Ok(0) => break,
                Ok(n) => {
                    if let Ok(mut guard) = buf.lock() {
                        guard.bytes.extend_from_slice(&chunk[..n]);
                        // Bound growth: keep only the trailing DRAIN_BUFFER_MAX_BYTES
                        // so a flood cannot exhaust memory.
                        if guard.bytes.len() > DRAIN_BUFFER_MAX_BYTES {
                            let excess = guard.bytes.len() - DRAIN_BUFFER_MAX_BYTES;
                            // Never start the kept tail mid-character: also drop any
                            // UTF-8 continuation bytes (0b10xxxxxx) at the new head.
                            let skip = guard.bytes[excess..]
                                .iter()
                                .take(3)
                                .take_while(|b| (**b & 0xC0) == 0x80)
                                .count();
                            guard.bytes.drain(..excess + skip);
                            guard.truncated = true;
                        }
                    }
                }
                Err(_) => break,
            }
        }
    }

    fn snapshot(buf: &Arc<Mutex<DrainBuf>>) -> String {
        let guard = match buf.lock() {
            Ok(g) => g,
            Err(_) => return String::new(),
        };
        let body = String::from_utf8_lossy(&guard.bytes);
        if guard.truncated {
            format!("{}{}", drain_truncation_marker(), body)
        } else {
            body.into_owned()
        }
    }

    /// Wait briefly for readers after kill; abort any that are still running so
    /// we do not leak JoinHandles/pipe tasks when a descendant holds the fd.
    async fn finish_readers(
        stdout_task: &mut tokio::task::JoinHandle<()>,
        stderr_task: &mut tokio::task::JoinHandle<()>,
        wait: Duration,
    ) {
        let drain = async {
            let _ = (&mut *stdout_task).await;
            let _ = (&mut *stderr_task).await;
        };
        if tokio::time::timeout(wait, drain).await.is_err() {
            stdout_task.abort();
            stderr_task.abort();
            let _ = stdout_task.await;
            let _ = stderr_task.await;
        }
    }

    /// Tail for timeout errors, but keep the drain truncation marker visible
    /// so the agent knows the head was dropped.
    fn tail_keep_marker(s: &str) -> String {
        let marker = drain_truncation_marker();
        match s.strip_prefix(marker.as_str()) {
            Some(rest) => format!(
                "{}{}",
                marker,
                Self::tail_chars(rest, PARTIAL_OUTPUT_TAIL_CHARS)
            ),
            None => Self::tail_chars(s, PARTIAL_OUTPUT_TAIL_CHARS),
        }
    }

    fn tail_chars(s: &str, max: usize) -> String {
        if s.chars().count() <= max {
            return s.to_string();
        }
        let skip = s.chars().count().saturating_sub(max);
        let tailed: String = s.chars().skip(skip).collect();
        format!("…[truncated]…{}", tailed)
    }

    fn format_timeout_error(timeout_secs: u32, stdout: &str, stderr: &str) -> String {
        let mut parts = vec![format!(
            "Command killed after {} seconds. If it legitimately needs longer, re-run with a larger `timeout` (capped by tools.bash.max_timeout_secs).",
            timeout_secs
        )];
        let out = Self::tail_keep_marker(stdout);
        let err = Self::tail_keep_marker(stderr);
        if !out.is_empty() {
            parts.push(format!("--- partial stdout ---\n{}", out));
        }
        if !err.is_empty() {
            parts.push(format!("--- partial stderr ---\n{}", err));
        }
        if out.is_empty() && err.is_empty() {
            parts.push("(no output captured before kill)".to_string());
        }
        parts.join("\n")
    }

    async fn execute_command(
        &self,
        params: &BashToolParams,
        cancel_token: Option<CancellationToken>,
    ) -> Result<(String, String, i32), Box<dyn std::error::Error + Send + Sync>> {
        if params.command.trim().is_empty() {
            return Err("Command cannot be empty".into());
        }

        let effective_timeout = resolve_timeout_secs(params.timeout, &self.config);

        let mut cmd = Command::new("bash");
        cmd.args(["-c", &params.command]);

        if let Some(working_dir) = &params.working_dir {
            cmd.current_dir(working_dir);
        }

        for (key, value) in &params.env {
            cmd.env(key, value);
        }

        cmd.stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null());

        #[cfg(unix)]
        cmd.process_group(0);

        let mut child = cmd.spawn()?;

        let stdout_buf = Arc::new(Mutex::new(DrainBuf::new()));
        let stderr_buf = Arc::new(Mutex::new(DrainBuf::new()));

        let stdout = child.stdout.take().ok_or("Failed to capture stdout")?;
        let stderr = child.stderr.take().ok_or("Failed to capture stderr")?;
        let mut stdout_task = {
            let buf = stdout_buf.clone();
            tokio::spawn(async move {
                Self::drain_pipe(stdout, buf).await;
            })
        };
        let mut stderr_task = {
            let buf = stderr_buf.clone();
            tokio::spawn(async move {
                Self::drain_pipe(stderr, buf).await;
            })
        };

        let cancel_future = async {
            if let Some(token) = cancel_token {
                token.cancelled().await;
            } else {
                std::future::pending::<()>().await;
            }
        };
        let timeout_future = async {
            if let Some(timeout_secs) = effective_timeout {
                tokio::time::sleep(Duration::from_secs(timeout_secs as u64)).await;
                Some(timeout_secs)
            } else {
                std::future::pending::<Option<u32>>().await
            }
        };

        tokio::select! {
            wait_result = child.wait() => {
                let exit_status = wait_result?;
                // Process exited: wait for readers to finish draining the pipes.
                // Still bound the wait so a leaked descendant cannot hang us forever.
                Self::finish_readers(&mut stdout_task, &mut stderr_task, Duration::from_millis(200)).await;
                let exit_code = exit_status.code().unwrap_or(-1);
                Ok((Self::snapshot(&stdout_buf), Self::snapshot(&stderr_buf), exit_code))
            }
            _ = cancel_future => {
                Self::kill_process_group(&mut child).await;
                Self::finish_readers(&mut stdout_task, &mut stderr_task, Duration::from_millis(200)).await;
                Err("Command was cancelled by user".into())
            }
            timed_out = timeout_future => {
                let timeout_secs = timed_out.expect("timeout_future only completes with Some");
                Self::kill_process_group(&mut child).await;
                Self::finish_readers(&mut stdout_task, &mut stderr_task, Duration::from_millis(200)).await;
                let stdout_output = Self::snapshot(&stdout_buf);
                let stderr_output = Self::snapshot(&stderr_buf);
                Err(Self::format_timeout_error(timeout_secs, &stdout_output, &stderr_output).into())
            }
        }
    }
}

#[tool(name = "bash", description = r#"
Executes shell commands within the user's environment. This tool is powerful and requires careful handling to ensure safety and predictability. It is your primary tool for compiling code, running tests, and managing version control with git.

SECURITY WARNING:
 - You are operating in a live user environment without a sandbox.
 - NEVER execute commands that could have unintended consequences, such as deleting files (rm), modifying system-wide configurations, or installing software without explicit, step-by-step user consent.
- When in doubt, ask the user for confirmation before proceeding with any command that modifies the file system.

Timeouts:
- Default timeout is 300 seconds when `timeout` is omitted (configurable via agent `tools.bash.default_timeout_secs`).
- Pass `timeout` (seconds) to override; values above `tools.bash.max_timeout_secs` (default 1800) are capped.
- `timeout: 0` is ignored (uses the agent default). Unlimited runs are operator-config only (`tools.bash.default_timeout_secs = 0`).
- On timeout the process is killed and any partial stdout/stderr captured so far is returned.

Development Workflow:
- Compiling and Building: Frequently use this tool to run build commands (e.g., make, npm run build, cargo build) to validate your changes and ensure the code compiles successfully.
- Running Tests: After making changes, always run the project's test suite (e.g., npm test, pytest, cargo test) to verify that your changes haven't introduced any regressions.

Usage Guidelines:
- this tool always runs from the same path. If you need to execute command in another directory, chain the commands with && for instance "cd subcrate && cargo test"
- Do NOT use bash for file operations that have dedicated tools:
  - Use `read` instead of `cat`, `less`, `head`, `tail`, or `bat`
  - Use `find` instead of `grep` or `find` commands
  - Use `ls` instead of `ls` or `dir` commands
  - Use `edit` instead of `sed`, `awk`, or `perl` for file modifications
  - Use `write` instead of redirect operators (`>`, `>>`)
- Use bash only for compiling, testing, running scripts, git operations, and other commands without a dedicated tool.
- Always provide a clear, concise description of the command's purpose for the user.
- Chain commands using && to ensure that subsequent commands only run if the previous ones succeed.
- Enclose file paths and arguments in double quotes (") to handle spaces and special characters correctly.

Examples:
- Good: cargo build (Compiles the project)
- Good: npm test (Runs the test suite)
- Good: git status (Checks the repository status)
- Good: git add . && git commit -m "feat: Implement the new feature" (Stages and commits changes)
- DANGEROUS: rm -rf / (Deletes the root directory)
- DANGEROUS: curl http://example.com/install.sh | sh (Executes a script from the internet without inspection)
"#, capabilities = [ToolCapability::Read, ToolCapability::Write, ToolCapability::Network])]
impl BashTool {
    async fn execute(
        &self,
        params: BashToolParams,
        cancel_token: Option<CancellationToken>,
    ) -> ToolResult {
        let start_time = Instant::now();
        let effective_timeout = resolve_timeout_secs(params.timeout, &self.config);

        match self.execute_command(&params, cancel_token).await {
            Ok((stdout, stderr, exit_code)) => {
                let execution_time = start_time.elapsed();
                let mut metadata = HashMap::new();

                metadata.insert("command".to_string(), json!(params.command));
                metadata.insert("exit_code".to_string(), json!(exit_code));
                metadata.insert(
                    "execution_time_ms".to_string(),
                    json!(execution_time.as_millis()),
                );
                match effective_timeout {
                    Some(t) => metadata.insert("timeout".to_string(), json!(t)),
                    None => metadata.insert("timeout".to_string(), json!("none")),
                };
                metadata.insert("success".to_string(), json!(exit_code == 0));

                if let Some(working_dir) = &params.working_dir {
                    metadata.insert("working_dir".to_string(), json!(working_dir));
                }

                if !params.env.is_empty() {
                    metadata.insert("env_vars".to_string(), json!(params.env));
                }

                let has_stderr = !stderr.is_empty();
                if has_stderr {
                    metadata.insert("has_stderr".to_string(), json!(true));
                    metadata.insert("stderr_length".to_string(), json!(stderr.len()));
                }

                let error_message = if exit_code != 0 && has_stderr {
                    Some(format!(
                        "Command failed with exit code {}: {}",
                        exit_code, stderr
                    ))
                } else if exit_code != 0 {
                    Some(format!("Command failed with exit code {}", exit_code))
                } else {
                    None
                };

                let output = if stderr.is_empty() {
                    stdout
                } else if stdout.is_empty() {
                    stderr
                } else {
                    format!("{}\n--- STDERR ---\n{}", stdout, stderr)
                };

                if exit_code == 0 {
                    ToolResult::Success {
                        output,
                        metadata: Some(metadata),
                    }
                } else {
                    ToolResult::Error {
                        error: error_message.unwrap_or_else(|| {
                            format!("Command failed with exit code {}", exit_code)
                        }),
                        metadata: Some(metadata),
                    }
                }
            }
            Err(e) => {
                let execution_time = start_time.elapsed();
                let mut metadata = HashMap::new();

                metadata.insert("command".to_string(), json!(params.command));
                metadata.insert(
                    "execution_time_ms".to_string(),
                    json!(execution_time.as_millis()),
                );
                match effective_timeout {
                    Some(t) => metadata.insert("timeout".to_string(), json!(t)),
                    None => metadata.insert("timeout".to_string(), json!("none")),
                };
                metadata.insert("success".to_string(), json!(false));
                metadata.insert(
                    "timed_out".to_string(),
                    json!(e.to_string().contains("killed after")),
                );

                ToolResult::Error {
                    error: e.to_string(),
                    metadata: Some(metadata),
                }
            }
        }
    }
}
