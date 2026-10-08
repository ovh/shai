use serde::Deserialize;
use std::path::PathBuf;
use std::process::Command;

/// JSON output from measurement_harness
#[derive(Deserialize, Debug)]
#[allow(dead_code)]
struct Report {
    name: String,
    category: String,
    difficulty: String,
    model: String,
    goal: String,
    summary: ReportSummary,
    verification: Vec<VerifyResult>,
    total_duration_ms: u64,
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
struct ReportSummary {
    total_input: u32,
    total_output: u32,
    total_cached: u32,
    total_tool_calls: usize,
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
struct VerifyResult {
    command: String,
    passed: bool,
}

fn parse_args() -> Vec<String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: benchmark_runner <script.json> [script.json ...] | <directory>");
        eprintln!();
        eprintln!("Run measurement_harness for each script and aggregate results.");
        eprintln!("Results are written to benchmarks-<timestamp>/<task-name>/");
        std::process::exit(1);
    }
    args[1..].to_vec()
}

fn collect_scripts(args: &[String]) -> Vec<PathBuf> {
    let mut script_paths = Vec::new();
    for arg in args {
        let path = PathBuf::from(arg);
        if path.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&path) {
                let mut json_files: Vec<_> = entries
                    .filter_map(|e| e.ok())
                    .filter(|e| e.path().extension().map_or(false, |ext| ext == "json"))
                    .map(|e| e.path())
                    .collect();
                json_files.sort();
                script_paths.extend(json_files);
            }
        } else {
            script_paths.push(path);
        }
    }
    script_paths
}

fn build_harness() -> Result<(), String> {
    eprintln!("Compiling measurement_harness...");
    let output = Command::new("cargo")
        .args(["build", "--example", "measurement_harness"])
        .output()
        .map_err(|e| format!("Failed to run cargo build: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("cargo build failed:\n{}", stderr));
    }
    Ok(())
}

fn run_benchmark(script_path: &PathBuf, output_dir: &str) -> Option<Report> {
    let harness_bin = std::env::current_dir()
        .ok()?
        .join("target/debug/examples/measurement_harness");

    eprintln!("\n>>> Running {}", script_path.display());

    let output = Command::new(&harness_bin)
        .arg("--output-dir")
        .arg(output_dir)
        .arg(script_path)
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);

    let begin_marker = "<<<HARNESS_JSON_BEGIN>>>\n";
    let end_marker = "<<<HARNESS_JSON_END>>>";

    let json_str = {
        if let Some(start) = stdout.find(begin_marker) {
            let after_begin = start + begin_marker.len();
            if let Some(end) = stdout[after_begin..].find(end_marker) {
                Some(&stdout[after_begin..after_begin + end])
            } else {
                None
            }
        } else {
            None
        }
    };

    match json_str {
        Some(json) => match serde_json::from_str::<Report>(json) {
            Ok(report) => Some(report),
            Err(e) => {
                eprintln!("  Failed to parse JSON: {}", e);
                None
            }
        },
        None => {
            eprintln!("  Failed to find JSON markers in harness output");
            None
        }
    }
}

fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}

fn format_status(report: &Report) -> &'static str {
    if report.verification.is_empty() {
        "NO VERIFY"
    } else if report.verification.iter().all(|v| v.passed) {
        "PASS"
    } else {
        "FAIL"
    }
}

fn main() {
    let args = parse_args();
    let scripts = collect_scripts(&args);

    if scripts.is_empty() {
        eprintln!("No scripts found.");
        std::process::exit(1);
    }

    // Create a parent directory for this benchmark run
    let run_id = chrono::Utc::now().timestamp();
    let run_dir = format!("benchmarks-{}", run_id);
    std::fs::create_dir_all(&run_dir).expect("Failed to create benchmark output directory");
    eprintln!("Benchmark run directory: {}", run_dir);

    build_harness().expect("Failed to compile measurement_harness");

    eprintln!("Running {} benchmark(s)...\n", scripts.len());

    let mut reports = Vec::new();
    for (i, script) in scripts.iter().enumerate() {
        let task_name = script
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| format!("task-{}", i));
        let output_dir = format!("{}/{}", run_dir, task_name);

        eprintln!(
            "[{}/{}] {}",
            i + 1,
            scripts.len(),
            script.file_name().unwrap_or_default().to_string_lossy()
        );
        if let Some(report) = run_benchmark(script, &output_dir) {
            let total_tokens = report.summary.total_input + report.summary.total_output;
            eprintln!(
                "  {} {} | {} tokens | {:.1}s",
                format_status(&report),
                truncate_str(&report.name, 30),
                total_tokens,
                report.total_duration_ms as f64 / 1000.0,
            );
            reports.push(report);
        } else {
            eprintln!("  FAILED to produce output");
        }
    }

    // Final summary table
    println!(
        "\n{:<30} {:<12} {:<10} {:<8} {:>10} {:>10}",
        "Task", "Category", "Diff", "Status", "Tokens", "Duration"
    );
    println!("{}", "-".repeat(85));

    for report in &reports {
        let total_tokens = report.summary.total_input + report.summary.total_output;

        println!(
            "{:<30} {:<12} {:<10} {:<8} {:>10} {:>9.1}s",
            truncate_str(&report.name, 30),
            report.category,
            report.difficulty,
            format_status(report),
            total_tokens,
            report.total_duration_ms as f64 / 1000.0,
        );
    }

    // Aggregate stats
    if !reports.is_empty() {
        let total_tokens: u32 = reports
            .iter()
            .map(|r| r.summary.total_input + r.summary.total_output)
            .sum();
        let total_duration: u64 = reports.iter().map(|r| r.total_duration_ms).sum();
        let passed = reports
            .iter()
            .filter(|r| !r.verification.is_empty() && r.verification.iter().all(|v| v.passed))
            .count();
        let failed = reports
            .iter()
            .filter(|r| !r.verification.is_empty() && !r.verification.iter().all(|v| v.passed))
            .count();
        let no_verify = reports.iter().filter(|r| r.verification.is_empty()).count();

        println!(
            "\nTotal: {} passed, {} failed, {} no-verify | {} tokens | {:.1}s",
            passed,
            failed,
            no_verify,
            total_tokens,
            total_duration as f64 / 1000.0
        );
    }

    eprintln!("\nResults written to: {}/", run_dir);
}
