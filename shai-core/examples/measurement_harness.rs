use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use shai_core::agent::events::AgentEventHandler;
use shai_core::agent::{
    Agent, AgentController, AgentEvent, AgentRequest, AgentResponse, UserResponse,
};
use shai_core::runners::coder::coder::coder;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Input script format
#[derive(Deserialize)]
struct Script {
    name: Option<String>,
    category: Option<String>,
    difficulty: Option<String>,
    goal: String,
    #[serde(default)]
    prompts: Vec<String>,
    /// Optional shell command to set up a fixture directory.
    /// The harness creates a temp dir in the current working directory,
    /// runs this command there, then changes CWD to the result.
    setup: Option<String>,
    /// Optional verification commands to run after the agent completes.
    /// Each command is run sequentially; the agent passes if all exit codes match expected_exit_code (default 0).
    #[serde(default)]
    verify: Vec<VerifyCommand>,
}

#[derive(Deserialize, Clone)]
struct VerifyCommand {
    command: String,
    #[serde(default)]
    expected_exit_code: Option<i32>,
}

/// Result of a single verification command
#[derive(Serialize, Clone, Debug)]
struct VerificationResult {
    command: String,
    exit_code: Option<i32>,
    passed: bool,
    duration_ms: u64,
    stdout: String,
    stderr: String,
}

/// Per-turn metrics
#[derive(Serialize, Clone, Debug)]
struct TurnMetrics {
    turn: usize,
    prompt: String,
    input_tokens: u32,
    output_tokens: u32,
    cached_tokens: u32,
    tool_calls: Vec<ToolCallMetrics>,
    duration_ms: u64,
}

/// Per-tool-call metrics
#[derive(Serialize, Clone, Debug)]
struct ToolCallMetrics {
    tool_name: String,
    original_bytes: u64,
    compacted_bytes: u64,
    duration_ms: u64,
}

/// Session summary
#[derive(Serialize, Debug)]
struct SessionSummary {
    total_input: u32,
    total_output: u32,
    total_cached: u32,
    cache_hit_rate: f64,
    total_tool_calls: usize,
    total_tool_output_bytes: u64,
    total_compacted_bytes: u64,
    compaction_ratio: f64,
}

/// Full report
#[derive(Serialize, Debug)]
struct Report {
    session_id: String,
    name: String,
    category: String,
    difficulty: String,
    model: String,
    goal: String,
    fixture: Option<String>,
    turns: Vec<TurnMetrics>,
    summary: SessionSummary,
    verification: Vec<VerificationResult>,
    total_duration_ms: u64,
}

/// Shared state for metrics collection
struct MetricsState {
    current_turn: usize,
    turns: Vec<TurnMetrics>,
    turn_start: Option<Instant>,
    tool_calls: Vec<ToolCallMetrics>,
    pending_input_tokens: u32,
    pending_output_tokens: u32,
    pending_cached_tokens: u32,
}

/// Event handler that captures metrics from agent events
struct MetricsHandler {
    state: Arc<Mutex<MetricsState>>,
    controller: AgentController,
}

impl MetricsHandler {
    fn new(state: Arc<Mutex<MetricsState>>, controller: AgentController) -> Self {
        Self { state, controller }
    }
}

#[async_trait::async_trait]
impl AgentEventHandler for MetricsHandler {
    async fn handle_event(&self, event: AgentEvent) {
        match event {
            AgentEvent::TokenUsage {
                input_tokens,
                output_tokens,
                cached_tokens,
            } => {
                let mut s = self.state.lock().unwrap();
                s.pending_input_tokens += input_tokens;
                s.pending_output_tokens += output_tokens;
                s.pending_cached_tokens += cached_tokens;
            }
            AgentEvent::ToolCallCompleted {
                duration,
                call,
                result,
                original_bytes,
                compacted_bytes,
            } => {
                let _ = result;
                let mut s = self.state.lock().unwrap();
                s.tool_calls.push(ToolCallMetrics {
                    tool_name: call.tool_name,
                    original_bytes: original_bytes as u64,
                    compacted_bytes: compacted_bytes as u64,
                    duration_ms: duration.num_milliseconds() as u64,
                });
            }
            AgentEvent::UserInputRequired { request_id, .. } => {
                let _ = self
                    .controller
                    .response_user_query(request_id, UserResponse::Cancel)
                    .await;
            }
            _ => {
                eprintln!("⚡ Event: {:?}", event);
            }
        }
    }
}

fn parse_args() -> (String, bool, Option<String>) {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: measurement_harness [--clean] [--output-dir <dir>] <script.json>");
        eprintln!();
        eprintln!("Options:");
        eprintln!("  --clean             Remove fixture directory after run (default: keep)");
        eprintln!(
            "  --output-dir <dir>  Use <dir> as the fixture directory instead of generating one"
        );
        eprintln!();
        eprintln!("Script format:");
        eprintln!("  {{");
        eprintln!("    \"name\": \"task-name\",");
        eprintln!("    \"category\": \"refactor\",");
        eprintln!("    \"difficulty\": \"easy\",");
        eprintln!("    \"goal\": \"Initial goal/prompt for the agent\",");
        eprintln!("    \"prompts\": [\"Turn 2 prompt\", \"Turn 3 prompt\", ...],");
        eprintln!("    \"setup\": \"sh -c 'curl ... | tar xz && cd crate-0.1.0'\",");
        eprintln!("    \"verify\": [{{\"command\": \"cargo check\", \"expected_exit_code\": 0}}]");
        eprintln!("  }}");
        std::process::exit(1);
    }

    let mut clean = false;
    let mut output_dir: Option<String> = None;
    let mut script_path = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--clean" => clean = true,
            "--output-dir" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("Error: --output-dir requires a value");
                    std::process::exit(1);
                }
                output_dir = Some(args[i].clone());
            }
            _ => script_path = Some(args[i].clone()),
        }
        i += 1;
    }

    let script_path = script_path.expect("script.json path is required");
    (script_path, clean, output_dir)
}

fn setup_fixture_with_cwd(
    setup_cmd: &str,
    output_dir: Option<&str>,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let fixture_dir = if let Some(dir) = output_dir {
        PathBuf::from(dir)
    } else {
        let ts = Utc::now().timestamp();
        PathBuf::from(format!("shai-harness-{}", ts))
    };

    fs::create_dir_all(&fixture_dir)?;
    eprintln!("░ Fixture: {}", fixture_dir.display());

    let cmd_with_pwd = format!("{} && pwd", setup_cmd);
    let output = Command::new("sh")
        .arg("-c")
        .arg(&cmd_with_pwd)
        .current_dir(&fixture_dir)
        .output()
        .map_err(|e| format!("Failed to run setup: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "Setup command failed (exit {}):\n{}",
            output
                .status
                .code()
                .map_or("?".to_string(), |c| c.to_string()),
            stderr
        )
        .into());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let cwd_str = stdout.trim().to_string();

    let working_dir = if cwd_str.starts_with('/') {
        PathBuf::from(cwd_str)
    } else {
        fixture_dir.join(cwd_str)
    };

    eprintln!("░ Working dir: {}", working_dir.display());
    Ok(working_dir)
}

fn run_verification(
    verify_commands: &[VerifyCommand],
    working_dir: &std::path::Path,
) -> Vec<VerificationResult> {
    if verify_commands.is_empty() {
        return Vec::new();
    }

    eprintln!("\n░ Running verification...");
    let mut results = Vec::new();

    for cmd in verify_commands {
        let start = Instant::now();
        let output = Command::new("sh")
            .arg("-c")
            .arg(&cmd.command)
            .current_dir(working_dir)
            .output();
        let duration_ms = start.elapsed().as_millis() as u64;

        match output {
            Ok(output) => {
                let exit_code = output.status.code();
                let expected = cmd.expected_exit_code.unwrap_or(0);
                let passed = exit_code == Some(expected);
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();

                eprintln!(
                    "  {} {} ({})",
                    if passed { "✓" } else { "✗" },
                    cmd.command,
                    if passed {
                        "passed".to_string()
                    } else {
                        format!("exit code {:?} != {}", exit_code, expected)
                    }
                );

                results.push(VerificationResult {
                    command: cmd.command.clone(),
                    exit_code,
                    passed,
                    duration_ms,
                    stdout,
                    stderr,
                });
            }
            Err(e) => {
                eprintln!("  ✗ {} failed to execute: {}", cmd.command, e);
                results.push(VerificationResult {
                    command: cmd.command.clone(),
                    exit_code: None,
                    passed: false,
                    duration_ms,
                    stdout: String::new(),
                    stderr: e.to_string(),
                });
            }
        }
    }

    results
}

/// Copy bundled AGENTS.md and skills into the fixture directory so the agent
/// has access to project context and skills during benchmark runs.
fn inject_context(fixture_dir: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    // Copy AGENTS.md
    let agents_src = manifest_dir.join("examples/AGENTS.md");
    if agents_src.exists() {
        fs::copy(&agents_src, fixture_dir.join("AGENTS.md"))?;
        eprintln!("░ Injected AGENTS.md");
    }

    // Copy all skills from examples/skills/ into <fixture>/.shai/skills/
    let skills_src = manifest_dir.join("examples/skills");
    let skills_dst = fixture_dir.join(".shai").join("skills");
    fs::create_dir_all(&skills_dst)?;

    let mut injected = 0;
    for entry in fs::read_dir(&skills_src)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            let src_skill = entry.path().join("SKILL.md");
            if src_skill.exists() {
                let dest_dir = skills_dst.join(entry.file_name());
                fs::create_dir_all(&dest_dir)?;
                fs::copy(&src_skill, dest_dir.join("SKILL.md"))?;
                injected += 1;
            }
        }
    }
    eprintln!("░ Injected {} skill(s)", injected);

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (script_path, clean, output_dir) = parse_args();
    let script_content = fs::read_to_string(&script_path)?;
    let script: Script = serde_json::from_str(&script_content)?;

    let goal = script.goal.clone();
    let prompts = script.prompts.clone();
    let setup_cmd = script.setup.clone();
    let script_name = script.name.clone().unwrap_or_else(|| {
        std::path::Path::new(&script_path)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string())
    });
    let script_category = script
        .category
        .clone()
        .unwrap_or_else(|| "unknown".to_string());
    let script_difficulty = script
        .difficulty
        .clone()
        .unwrap_or_else(|| "unknown".to_string());

    let fixture_dir = if let Some(ref cmd) = setup_cmd {
        Some(setup_fixture_with_cwd(cmd, output_dir.as_deref())?)
    } else {
        None
    };

    if let Some(ref working_dir) = fixture_dir {
        std::env::set_current_dir(working_dir)
            .map_err(|e| format!("Failed to change to fixture dir: {}", e))?;
        eprintln!("░ CWD: {}", std::env::current_dir()?.display());
    }

    if let Some(ref working_dir) = fixture_dir {
        eprintln!("░ Initializing git repo...");
        fs::write(
            working_dir.join(".gitignore"),
            "runtime.log\nsession.log\ndiff.patch\n",
        )?;

        let _ = Command::new("git")
            .args(["init"])
            .current_dir(working_dir)
            .output();
        let _ = Command::new("git")
            .args(["add", "-A"])
            .current_dir(working_dir)
            .output();

        let commit = Command::new("git")
            .args([
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-m",
                "baseline",
                "--allow-empty",
            ])
            .env("GIT_AUTHOR_NAME", "harness")
            .env("GIT_AUTHOR_EMAIL", "harness@harness")
            .env("GIT_COMMITTER_NAME", "harness")
            .env("GIT_COMMITTER_EMAIL", "harness@harness")
            .current_dir(working_dir)
            .output();

        if commit.map_or(false, |c| c.status.success()) {
            eprintln!("░ Git baseline committed");
        } else {
            eprintln!("⚠ Failed to commit git baseline");
        }

        inject_context(working_dir)?;
    }

    {
        let log_dir = fixture_dir
            .as_ref()
            .map(|d| d.as_path())
            .unwrap_or_else(|| std::path::Path::new("."));
        let log_writer = tracing_appender::rolling::never(log_dir, "runtime.log");
        let _ = tracing_subscriber::registry()
            .with(tracing_subscriber::EnvFilter::new(
                "shai_core=debug,brain::coder=debug,agent=debug,misc=debug",
            ))
            .with(
                tracing_subscriber::fmt::layer()
                    .with_writer(log_writer)
                    .with_ansi(false),
            )
            .try_init();
    }

    let session_start = Instant::now();

    eprintln!("░ Measurement harness");
    eprintln!("░ Task: {}", script_name);
    eprintln!("░ Goal: {}", goal);
    eprintln!("░ Prompts: {}", prompts.len());

    let (llm, model) = shai_core::config::config::ShaiConfig::get_llm()
        .await
        .map_err(|e| {
            format!(
                "Failed to get LLM from config: {}. Run `shai auth` first.",
                e
            )
        })?;
    eprintln!("░ Model: {} ({})", model, llm.provider().name());

    let shared_state = Arc::new(Mutex::new(MetricsState {
        current_turn: 1,
        turns: Vec::new(),
        turn_start: None,
        tool_calls: Vec::new(),
        pending_input_tokens: 0,
        pending_output_tokens: 0,
        pending_cached_tokens: 0,
    }));

    let mut agent = coder(Arc::new(llm), model.clone());
    if let Some(ref dir) = fixture_dir {
        agent.tool_ctx.working_dir = Some(dir.to_string_lossy().to_string());
    }
    let mut controller = agent.controller();
    let handler = MetricsHandler::new(shared_state.clone(), controller.clone());
    let mut agent = agent.with_event_handler(handler);

    let handle = tokio::spawn(async move { agent.run().await });

    match controller.send(AgentRequest::Sudo(Some(true))).await {
        Ok(AgentResponse::SudoStatus { enabled }) => eprintln!("░ Sudo enabled: {}", enabled),
        Ok(_) => eprintln!("⚠ Sudo returned unexpected response"),
        Err(e) => eprintln!("⚠ Failed to enable sudo: {}", e),
    }

    let turn_timeout_ms: u64 = std::env::var("HARNESS_TURN_TIMEOUT_MS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(120_000);

    let state_start = shared_state.clone();
    let start_turn = || {
        let mut s = state_start.lock().unwrap();
        s.turn_start = Some(Instant::now());
    };

    let state_flush = shared_state.clone();
    let flush_turn = |prompt: String| {
        let mut s = state_flush.lock().unwrap();
        let start = s.turn_start.take();
        let tool_calls = std::mem::take(&mut s.tool_calls);
        let input_tokens = std::mem::take(&mut s.pending_input_tokens);
        let output_tokens = std::mem::take(&mut s.pending_output_tokens);
        let cached_tokens = std::mem::take(&mut s.pending_cached_tokens);
        let turn = s.current_turn;
        let duration_ms = start
            .map(|inst| inst.elapsed().as_millis() as u64)
            .unwrap_or(0);
        s.turns.push(TurnMetrics {
            turn,
            prompt,
            input_tokens,
            output_tokens,
            cached_tokens,
            tool_calls,
            duration_ms,
        });
        s.current_turn += 1;
    };

    start_turn();
    eprintln!("\n> [Turn 0] {}", goal);
    let _ = controller.send_user_input(goal.clone()).await;
    match controller.wait_turn(Some(turn_timeout_ms)).await {
        Ok(()) => {}
        Err(e) => eprintln!("⚠ Turn 0 timed out: {}", e),
    }
    flush_turn(goal.clone());

    for (i, prompt) in prompts.iter().enumerate() {
        start_turn();
        eprintln!("> [Turn {}] {}", i + 1, prompt);
        let _ = controller.send_user_input(prompt.clone()).await;
        match controller.wait_turn(Some(turn_timeout_ms)).await {
            Ok(()) => {}
            Err(e) => eprintln!("⚠ Turn {} timed out: {}", i + 1, e),
        }
        flush_turn(prompt.clone());
    }

    let _ = controller.drop().await;
    let _ = handle.await??;

    let total_duration_ms = session_start.elapsed().as_millis() as u64;

    let turns = shared_state.lock().unwrap().turns.clone();

    let total_input: u32 = turns.iter().map(|t| t.input_tokens).sum();
    let total_output: u32 = turns.iter().map(|t| t.output_tokens).sum();
    let total_cached: u32 = turns.iter().map(|t| t.cached_tokens).sum();
    let total_tool_calls: usize = turns.iter().map(|t| t.tool_calls.len()).sum();
    let total_tool_output_bytes: u64 = turns
        .iter()
        .map(|t| t.tool_calls.iter().map(|tc| tc.original_bytes).sum::<u64>())
        .sum();
    let total_compacted_bytes: u64 = turns
        .iter()
        .map(|t| {
            t.tool_calls
                .iter()
                .map(|tc| tc.compacted_bytes)
                .sum::<u64>()
        })
        .sum();
    let cache_hit_rate = if total_input > 0 {
        total_cached as f64 / total_input as f64
    } else {
        0.0
    };
    let compaction_ratio = if total_tool_output_bytes > 0 {
        total_compacted_bytes as f64 / total_tool_output_bytes as f64
    } else {
        1.0
    };

    let fixture_path = fixture_dir
        .as_ref()
        .map(|p| p.to_string_lossy().to_string());

    let verification_results = if let Some(ref working_dir) = fixture_dir {
        run_verification(&script.verify, working_dir)
    } else {
        Vec::new()
    };

    let report = Report {
        session_id: format!("session-{}", Utc::now().timestamp()),
        name: script_name.clone(),
        category: script_category.clone(),
        difficulty: script_difficulty.clone(),
        model,
        goal,
        fixture: fixture_path,
        turns,
        summary: SessionSummary {
            total_input,
            total_output,
            total_cached,
            cache_hit_rate,
            total_tool_calls,
            total_tool_output_bytes,
            total_compacted_bytes,
            compaction_ratio,
        },
        verification: verification_results.clone(),
        total_duration_ms,
    };

    let json = serde_json::to_string_pretty(&report)?;
    println!("<<<HARNESS_JSON_BEGIN>>>\n{}\n<<<HARNESS_JSON_END>>>", json);

    eprintln!("\n░ Session complete");
    eprintln!(
        "░ Total input tokens:  {} ({} cached, {:.1}% hit rate)",
        total_input,
        total_cached,
        cache_hit_rate * 100.0
    );
    eprintln!("░ Total output tokens: {}", total_output);
    eprintln!(
        "░ Total tool calls: {} ({} bytes → {} compacted, {:.1}% reduction)",
        total_tool_calls,
        total_tool_output_bytes,
        total_compacted_bytes,
        (1.0 - compaction_ratio) * 100.0
    );
    eprintln!("░ Duration: {:.1}s", total_duration_ms as f64 / 1000.0);

    if !verification_results.is_empty() {
        let all_passed = verification_results.iter().all(|r| r.passed);
        eprintln!(
            "░ Verification: {}",
            if all_passed { "PASSED" } else { "FAILED" }
        );
        for vr in &verification_results {
            eprintln!(
                "  {} {} (exit {:?})",
                if vr.passed { "✓" } else { "✗" },
                vr.command,
                vr.exit_code
            );
        }
    }

    if let Some(ref working_dir) = fixture_dir {
        let diff_output = Command::new("git")
            .args(["diff"])
            .current_dir(working_dir)
            .output();

        if let Ok(diff) = diff_output {
            fs::write(working_dir.join("diff.patch"), &diff.stdout)?;
            eprintln!("░ Diff written to diff.patch ({} bytes)", diff.stdout.len());
        }

        let verify_summary = if verification_results.is_empty() {
            String::from("No verification commands\n")
        } else {
            let mut s = String::new();
            for vr in &verification_results {
                s.push_str(&format!(
                    "  {} {} (exit {:?})\n",
                    if vr.passed { "✓" } else { "✗" },
                    vr.command,
                    vr.exit_code,
                ));
            }
            s
        };

        let session_log = format!(
            "Task: {}\nCategory: {}\nDifficulty: {}\nSession: {}\nModel: {}\nGoal: {}\n\nTotal input tokens:  {} ({} cached, {:.1}% hit rate)\nTotal output tokens: {}\nTotal tool calls: {} ({} bytes → {} compacted, {:.1}% reduction)\nDuration: {:.1}s\n\n--- Verification ---\n{}\n--- JSON Report ---\n{}",
            script_name,
            script_category,
            script_difficulty,
            report.session_id,
            report.model,
            report.goal,
            total_input,
            total_cached,
            cache_hit_rate * 100.0,
            total_output,
            total_tool_calls,
            total_tool_output_bytes,
            total_compacted_bytes,
            (1.0 - compaction_ratio) * 100.0,
            total_duration_ms as f64 / 1000.0,
            verify_summary,
            json,
        );
        fs::write(working_dir.join("session.log"), &session_log)?;
        eprintln!("░ Session log written to session.log");
    }

    if let Some(ref dir) = fixture_dir {
        if clean {
            eprintln!("░ Cleaning fixture: {}", dir.display());
            let _ = fs::remove_dir_all(dir);
        } else {
            eprintln!("░ Fixture kept: {}", dir.display());
        }
    }

    Ok(())
}
