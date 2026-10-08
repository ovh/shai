use super::structs::BashToolParams;
use crate::tools::{tool, ToolResult};
use serde_json::json;
use std::collections::HashMap;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, BufReader};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

pub struct BashTool;

impl Default for BashTool {
    fn default() -> Self {
        Self::new()
    }
}

impl BashTool {
    pub fn new() -> Self {
        Self
    }

    async fn kill_process_group(child: &mut tokio::process::Child) {
        #[cfg(unix)]
        {
            // Try to kill the entire process group
            if let Some(pid) = child.id() {
                unsafe {
                    // Kill the process group (negative PID kills the group)
                    libc::kill(-(pid as i32), libc::SIGTERM);

                    // Give it a moment to terminate gracefully
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

                    // Force kill if still running
                    libc::kill(-(pid as i32), libc::SIGKILL);
                }
            }
        }

        // Fallback: kill just the immediate child
        let _ = child.kill().await;
        let _ = child.wait().await;
    }

    async fn execute_command(
        &self,
        params: &BashToolParams,
        cancel_token: Option<CancellationToken>,
    ) -> Result<(String, String, i32), Box<dyn std::error::Error + Send + Sync>> {
        // Validate command is not empty
        if params.command.trim().is_empty() {
            return Err("Command cannot be empty".into());
        }

        // Create the command with process group
        let mut cmd = Command::new("bash");
        cmd.args(["-c", &params.command]);

        // Set working directory if specified
        if let Some(working_dir) = &params.working_dir {
            cmd.current_dir(working_dir);
        }

        // Set environment variables
        for (key, value) in &params.env {
            cmd.env(key, value);
        }

        // Configure stdio
        cmd.stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null());

        // Spawn the process
        #[cfg(unix)]
        cmd.process_group(0);

        let mut child = cmd.spawn()?;

        // Read output asynchronously (needed to prevent blocking on full buffers)
        let stdout = child.stdout.take().ok_or("Failed to capture stdout")?;
        let stdout_task = tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            let mut output = String::new();
            reader.read_to_string(&mut output).await?;
            Ok::<String, std::io::Error>(output)
        });
        let stderr = child.stderr.take().ok_or("Failed to capture stderr")?;
        let stderr_task = tokio::spawn(async move {
            let mut reader = BufReader::new(stderr);
            let mut output = String::new();
            reader.read_to_string(&mut output).await?;
            Ok::<String, std::io::Error>(output)
        });

        // Optionable Future
        let cancel_future = async {
            if let Some(token) = cancel_token {
                token.cancelled().await;
            } else {
                std::future::pending::<()>().await;
            }
        };
        let timeout_future = async {
            if let Some(timeout_secs) = params.timeout {
                tokio::time::sleep(Duration::from_secs(timeout_secs as u64)).await;
            } else {
                std::future::pending::<()>().await;
            }
        };

        // Wait for process completion, cancellation, or timeout
        tokio::select! {
            // Process completed normally
            wait_result = child.wait() => {
                let exit_status = wait_result?;
                let stdout_output = stdout_task.await??;
                let stderr_output = stderr_task.await??;
                let exit_code = exit_status.code().unwrap_or(-1);
                Ok((stdout_output, stderr_output, exit_code))
            }
            // Cancellation requested
            _ = cancel_future => {
                stdout_task.abort();
                stderr_task.abort();
                Self::kill_process_group(&mut child).await;
                Err("Command was cancelled by user".into())
            }
            // Timeout occurred
            _ = timeout_future => {
                stdout_task.abort();
                stderr_task.abort();
                Self::kill_process_group(&mut child).await;
                Err(format!("Command timed out after {} seconds", params.timeout.unwrap()).into())
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
                if let Some(timeout_val) = params.timeout {
                    metadata.insert("timeout".to_string(), json!(timeout_val));
                } else {
                    metadata.insert("timeout".to_string(), json!("none"));
                }
                metadata.insert("success".to_string(), json!(exit_code == 0));

                if let Some(working_dir) = &params.working_dir {
                    metadata.insert("working_dir".to_string(), json!(working_dir));
                }

                if !params.env.is_empty() {
                    metadata.insert("env_vars".to_string(), json!(params.env));
                }

                // Include stderr info if present
                let has_stderr = !stderr.is_empty();
                if has_stderr {
                    metadata.insert("has_stderr".to_string(), json!(true));
                    metadata.insert("stderr_length".to_string(), json!(stderr.len()));
                }

                // Prepare error message if needed
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

                // Combine stdout and stderr for output
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
                if let Some(timeout_val) = params.timeout {
                    metadata.insert("timeout".to_string(), json!(timeout_val));
                } else {
                    metadata.insert("timeout".to_string(), json!("none"));
                }
                metadata.insert("success".to_string(), json!(false));

                ToolResult::Error {
                    error: e.to_string(),
                    metadata: Some(metadata),
                }
            }
        }
    }
}
