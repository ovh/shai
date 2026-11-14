use super::bash::BashTool;
use super::structs::{resolve_timeout_secs, BashToolConfig, BashToolParams};
use crate::tools::{Tool, ToolCapability};
use serde_json::json;
use shai_llm::ToolDescription;
use std::collections::HashMap;
use std::time::Instant;

fn error_text(result: &crate::tools::types::ToolResult) -> String {
    match result {
        crate::tools::types::ToolResult::Error { error, .. } => error.clone(),
        other => format!("{:?}", other),
    }
}

/// Own working directory for tests that assert on exact output. The process
/// cwd is shared across tests, and runner tests `set_current_dir` into a
/// `TempDir` that is later deleted; a child inheriting that cwd prints
/// `shell-init: error retrieving current directory` on stderr, which the tool
/// appends to stdout. Keep the returned `TempDir` alive for the whole test.
fn isolated_dir() -> (tempfile::TempDir, Option<String>) {
    let dir = tempfile::TempDir::new().expect("temp working dir");
    let path = dir.path().to_string_lossy().into_owned();
    (dir, Some(path))
}

#[test]
fn test_bash_tool_permissions() {
    let tool = BashTool::new();
    let perms = tool.capabilities();
    assert!(perms.contains(&ToolCapability::Read));
    assert!(perms.contains(&ToolCapability::Write));
    assert!(perms.contains(&ToolCapability::Network));
    assert_eq!(perms.len(), 3);
}

#[tokio::test]
async fn test_bash_tool_creation() {
    let tool = BashTool::new();
    assert_eq!(&tool.name(), "bash");
    assert!(!tool.description().is_empty());
}

#[tokio::test]
async fn test_bash_tool_execution() {
    let tool = BashTool::new();
    let params = BashToolParams {
        command: "echo hello".to_string(),
        timeout: Some(30),
        working_dir: None,
        env: HashMap::new(),
    };

    let result = Tool::execute(&tool, params, None).await;
    assert!(result.is_success());
    if let crate::tools::types::ToolResult::Success { output, metadata } = result {
        assert!(output.contains("hello"));
        let metadata = metadata.unwrap();
        assert_eq!(metadata["exit_code"], json!(0));
        assert_eq!(metadata["success"], json!(true));
        assert_eq!(metadata["timeout"], json!(30));
    } else {
        panic!("Expected success result");
    }
}

#[tokio::test]
async fn test_timeout_keeps_partial_stdout() {
    let tool = BashTool::new();
    let params = BashToolParams {
        command: "echo PARTIAL_MARKER; sleep 5".to_string(),
        timeout: Some(1),
        working_dir: None,
        env: HashMap::new(),
    };

    let started = Instant::now();
    let result = Tool::execute(&tool, params, None).await;
    let elapsed = started.elapsed();
    assert!(
        elapsed.as_secs() < 4,
        "should have timed out well before sleep 5 finished, took {:?}",
        elapsed
    );
    assert!(
        result.is_error(),
        "expected timeout error, got {:?}",
        result
    );
    let err = error_text(&result);
    assert!(
        err.contains("killed after 1 seconds") || err.contains("killed after 1 second"),
        "missing kill message: {}",
        err
    );
    assert!(
        err.contains("PARTIAL_MARKER"),
        "partial stdout was lost: {}",
        err
    );
    assert!(
        err.contains("re-run with a larger `timeout`"),
        "missing hint: {}",
        err
    );
    assert!(
        !err.to_lowercase().contains("nohup"),
        "must not suggest nohup: {}",
        err
    );
    assert!(
        !err.contains("timeout: 0"),
        "must not tell the model to pass timeout:0: {}",
        err
    );
}

#[tokio::test]
async fn test_omitted_timeout_uses_default_from_config() {
    let tool = BashTool::with_config(BashToolConfig {
        default_timeout_secs: 1,
        max_timeout_secs: 1800,
    });
    let params = BashToolParams {
        command: "echo BEFORE; sleep 5".to_string(),
        timeout: None,
        working_dir: None,
        env: HashMap::new(),
    };
    let result = Tool::execute(&tool, params, None).await;
    assert!(result.is_error());
    let err = error_text(&result);
    assert!(err.contains("killed after 1"), "{}", err);
    assert!(err.contains("BEFORE"), "partial output lost: {}", err);
}

#[tokio::test]
async fn test_model_timeout_zero_uses_default_not_unlimited() {
    // A model passing timeout:0 must not disable the hang fix.
    let tool = BashTool::with_config(BashToolConfig {
        default_timeout_secs: 1,
        max_timeout_secs: 1800,
    });
    let params = BashToolParams {
        command: "echo ZERO_IGNORED; sleep 5".to_string(),
        timeout: Some(0),
        working_dir: None,
        env: HashMap::new(),
    };
    let started = Instant::now();
    let result = Tool::execute(&tool, params, None).await;
    assert!(
        started.elapsed().as_secs() < 4,
        "timeout:0 from model must not run unlimited"
    );
    assert!(result.is_error(), "{:?}", result);
    let err = error_text(&result);
    assert!(err.contains("killed after 1"), "{}", err);
    assert!(err.contains("ZERO_IGNORED"), "{}", err);
}

#[tokio::test]
async fn test_max_timeout_caps_huge_value() {
    let tool = BashTool::with_config(BashToolConfig {
        default_timeout_secs: 300,
        max_timeout_secs: 1,
    });
    let params = BashToolParams {
        command: "echo CAPPED; sleep 5".to_string(),
        timeout: Some(99999),
        working_dir: None,
        env: HashMap::new(),
    };
    let started = Instant::now();
    let result = Tool::execute(&tool, params, None).await;
    assert!(started.elapsed().as_secs() < 4);
    assert!(result.is_error());
    let err = error_text(&result);
    assert!(err.contains("killed after 1"), "{}", err);
    assert!(err.contains("CAPPED"), "{}", err);
    if let crate::tools::types::ToolResult::Error { metadata, .. } = result {
        assert_eq!(metadata.unwrap()["timeout"], json!(1));
    }
}

#[tokio::test]
async fn test_drain_buffer_bounded_under_flood() {
    // Flooding stdout must not grow the drain buffer without bound.
    let tool = BashTool::with_config(BashToolConfig {
        default_timeout_secs: 5,
        max_timeout_secs: 5,
    });
    let params = BashToolParams {
        command: r#"python3 -c "import sys; sys.stdout.buffer.write(b'x'*200000); sys.stdout.buffer.flush(); import time; time.sleep(5)""#.to_string(),
        timeout: Some(1),
        working_dir: None,
        env: HashMap::new(),
    };
    let started = Instant::now();
    let result = Tool::execute(&tool, params, None).await;
    assert!(
        started.elapsed().as_secs() < 4,
        "flood+timeout should still finish quickly"
    );
    assert!(result.is_error(), "{:?}", result);
    let err = error_text(&result);
    assert!(err.contains("killed after"), "{}", err);
    // Assert truncation actually happened on the flood.
    assert!(
        err.contains("truncated: earlier output omitted"),
        "flood >64KiB must report truncation: {}",
        &err[..err.len().min(300)]
    );
}

#[tokio::test]
async fn test_drain_truncation_keeps_utf8_char_boundary() {
    // 30000 x 3-byte '€' = 90000 bytes; excess (24464) is not a
    // multiple of 3, so a naive cut would start mid-character.
    let (_wd_guard, wd) = isolated_dir();
    let tool = BashTool::new();
    let params = BashToolParams {
        command: r#"python3 -c "import sys; sys.stdout.buffer.write('\u20ac'.encode()*30000); sys.stdout.buffer.flush()""#.to_string(),
        timeout: Some(30),
        working_dir: wd,
        env: HashMap::new(),
    };
    let result = Tool::execute(&tool, params, None).await;
    assert!(result.is_success(), "{:?}", result);
    if let crate::tools::types::ToolResult::Success { output, .. } = result {
        let body = output
            .strip_prefix(super::bash::drain_truncation_marker().as_str())
            .expect("successful output over cap must carry the truncation marker");
        assert!(!body.contains('\u{FFFD}'), "tail started mid-character");
        assert!(body.chars().all(|c| c == '€'));
        assert!(body.len() <= 64 * 1024);
    }
}

#[tokio::test]
async fn test_drain_truncation_marker_on_success_over_cap() {
    // A successful command >64KiB must keep the tail, drop the head,
    // and surface a clear truncation marker (not silent).
    let (_wd_guard, wd) = isolated_dir();
    let tool = BashTool::new();
    let params = BashToolParams {
        command: r#"python3 -c "
import sys
head = b'HEAD_UNIQUE_MARKER'
tail = b'TAIL_UNIQUE_MARKER'
# 80 KiB of filler between head and tail so the 64 KiB cap must drop the head.
sys.stdout.buffer.write(head + b'x'*(80*1024) + tail)
sys.stdout.buffer.flush()
""#
        .to_string(),
        timeout: Some(30),
        working_dir: wd,
        env: HashMap::new(),
    };
    let result = Tool::execute(&tool, params, None).await;
    assert!(result.is_success(), "{:?}", result);
    if let crate::tools::types::ToolResult::Success { output, .. } = result {
        assert!(
            output.contains("truncated: earlier output omitted"),
            "missing truncation marker: {}",
            &output[..output.len().min(200)]
        );
        assert!(
            output.contains("TAIL_UNIQUE_MARKER"),
            "tail of flood must be kept"
        );
        assert!(
            !output.contains("HEAD_UNIQUE_MARKER"),
            "head must have been dropped by the 64 KiB cap"
        );
        // Marker + at most 64KiB body (UTF-8 ASCII here) — allow small slack for marker.
        assert!(
            output.len() <= 64 * 1024 + 200,
            "output should stay near the cap, got {}",
            output.len()
        );
    }
}

#[test]
fn test_resolve_timeout_helpers_exported() {
    let cfg = BashToolConfig::default();
    assert_eq!(resolve_timeout_secs(None, &cfg), Some(300));
    assert_eq!(resolve_timeout_secs(Some(0), &cfg), Some(300));
}

#[tokio::test]
async fn test_utf8_multibyte_split_across_8kb_boundary() {
    // Force a 3-byte UTF-8 codepoint (€ = e2 82 ac) to straddle the
    // 8192-byte read boundary so per-chunk decoding would corrupt it.
    let (_wd_guard, wd) = isolated_dir();
    let tool = BashTool::new();
    let params = BashToolParams {
        command: r#"python3 -c "
import sys
# 8191 ASCII bytes fill the first read leaving 1 byte free; then a 3-byte euro
# starts in that last byte and finishes in the next read.
sys.stdout.buffer.write(b'a'*8191 + '\u20ac'.encode('utf-8')*2000)
sys.stdout.buffer.flush()
""#
        .to_string(),
        timeout: Some(15),
        working_dir: wd,
        env: HashMap::new(),
    };
    let result = Tool::execute(&tool, params, None).await;
    assert!(result.is_success(), "{:?}", result);
    if let crate::tools::types::ToolResult::Success { output, .. } = result {
        assert!(
            !output.contains('\u{FFFD}'),
            "replacement char present: corrupted UTF-8 across chunk boundary"
        );
        assert!(output.starts_with(&"a".repeat(8191)));
        let euros: String = output.chars().skip(8191).collect();
        assert_eq!(euros.chars().count(), 2000);
        assert!(euros.chars().all(|c| c == '€'), "unexpected char after pad");
    }
}
