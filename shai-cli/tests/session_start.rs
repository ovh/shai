// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: OVH SAS

//! What the `shai` binary does when a session option cannot be honoured.
//!
//! Every case here ends before a session starts, so no model is contacted,
//! and every case has a config file, so no starter config is downloaded.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// One provider that is never contacted, and a role on it.
const PROVIDERS: &str = r#""providers": [{"provider": "ollama", "env_vars": {"OLLAMA_BASE_URL": "http://127.0.0.1:9/v1"}, "model": "local-a", "tool_method": "FunctionCall"}], "selected_provider": 0, "roles": {"cheap": "ollama/local-b"}"#;

struct Outcome {
    status: i32,
    stdout: String,
    stderr: String,
}

/// Run `shai` with `config` as its only config file, in an environment that
/// holds nothing of the machine it runs on.
fn shai(config_file: &str, config: &str, args: &[&str]) -> Outcome {
    let dir = tempfile::TempDir::new().unwrap();
    let config_home = dir.path().join("config");
    std::fs::create_dir_all(config_home.join("shai")).unwrap();
    std::fs::write(config_home.join("shai").join(config_file), config).unwrap();
    let work = dir.path().join("work");
    std::fs::create_dir_all(&work).unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_shai"))
        .args(args)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", dir.path().join("home"))
        .env("XDG_CONFIG_HOME", &config_home)
        .env("NO_COLOR", "1")
        // A request that should never be made fails here, without leaving
        // the machine.
        .env("HTTPS_PROXY", "http://127.0.0.1:9")
        .env("HTTP_PROXY", "http://127.0.0.1:9")
        .env("ALL_PROXY", "http://127.0.0.1:9")
        .current_dir(&work)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the shai binary should start");

    // None of these invocations may get as far as a session.
    let started = Instant::now();
    while child.try_wait().unwrap().is_none() {
        if started.elapsed() > Duration::from_secs(60) {
            let _ = child.kill();
            panic!("shai {args:?} did not exit");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = child.wait_with_output().unwrap();
    // Nothing was fetched over the config file or next to it: the config
    // directory holds what it held before.
    let left: Vec<(String, String)> = std::fs::read_dir(config_home.join("shai"))
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            let text = std::fs::read_to_string(entry.path()).unwrap_or_default();
            (entry.file_name().to_string_lossy().into_owned(), text)
        })
        .collect();
    assert_eq!(
        left,
        [(config_file.to_string(), config.to_string())],
        "no starter config may be fetched"
    );
    Outcome {
        status: output.status.code().expect("an exit status"),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn config(tail: &str) -> String {
    format!("{{{PROVIDERS}{tail}}}")
}

#[test]
fn a_target_that_does_not_resolve_stops_the_session_with_status_2() {
    for (args, expected) in [
        (
            &["-p", "hello", "--prewalk-into", "planner"][..],
            "cannot arm prewalk from --prewalk-into: role \"planner\" is not configured",
        ),
        (
            &["-p", "hello", "--prewalk-into", "anthropic/claude-x"][..],
            "names a provider that is not configured",
        ),
        (
            &["--prewalk-into", "planner", "agent", "reviewer"][..],
            "cannot arm prewalk from --prewalk-into",
        ),
    ] {
        let outcome = shai("auth.config.json", &config(""), args);
        assert_eq!(outcome.status, 2, "{args:?}: {}", outcome.stderr);
        assert!(
            outcome.stderr.contains(expected),
            "{args:?}: {}",
            outcome.stderr
        );
        assert!(outcome.stdout.is_empty(), "{args:?}: {}", outcome.stdout);
    }

    // The same from the config, where the value is named and not repeated.
    let outcome = shai(
        "auth.config.json",
        &config(r#", "prewalk": {"into": "sk-live-SECRETSECRET"}"#),
        &["-p", "hello"],
    );
    assert_eq!(outcome.status, 2, "{}", outcome.stderr);
    assert!(
        outcome
            .stderr
            .contains("cannot arm prewalk from the config: \"prewalk.into\""),
        "{}",
        outcome.stderr
    );
    assert!(!outcome.stderr.contains("SECRET"), "{}", outcome.stderr);
}

#[test]
fn a_prewalk_section_that_is_invalid_or_written_twice_stops_the_session() {
    for tail in [
        r#", "prewalk": "cheap""#,
        r#", "prewalk": {"in_to": "cheap"}"#,
        r#", "prewalk": {"into": "cheap"}, "prewalk": {"into": "cheap"}"#,
    ] {
        let outcome = shai("auth.config.json", &config(tail), &["-p", "hello"]);
        assert_eq!(outcome.status, 2, "{tail}: {}", outcome.stderr);
        assert!(
            outcome
                .stderr
                .contains("the \"prewalk\" section of the config is invalid"),
            "{tail}: {}",
            outcome.stderr
        );
        // The rest of the file still loads: no parse warning.
        assert!(
            !outcome.stderr.contains("Warning"),
            "{tail}: {}",
            outcome.stderr
        );
    }
}

#[test]
fn a_config_file_that_cannot_be_read_stops_the_session_and_nothing_else() {
    // A value in the wrong place, which the parser would quote.
    let broken = r#"{"providers": [], "selected_provider": "sk-live-SECRETSECRET"}"#;
    for file in ["auth.config.json", "auth.config"] {
        let outcome = shai(file, broken, &["-p", "hello"]);
        assert_eq!(outcome.status, 2, "{file}: {}", outcome.stderr);
        assert!(
            outcome.stderr.contains("could not be loaded") && outcome.stderr.contains(file),
            "{file}: {}",
            outcome.stderr
        );
        // Where it is broken, not what it holds.
        assert!(
            outcome.stderr.contains("line 1 column"),
            "{}",
            outcome.stderr
        );
        assert!(!outcome.stderr.contains("SECRET"), "{}", outcome.stderr);
        assert!(outcome.stdout.is_empty(), "{}", outcome.stdout);

        // Commands that start no session are not held up by it.
        let outcome = shai(file, broken, &["--list-tools"]);
        assert_eq!(outcome.status, 0, "{file}: {}", outcome.stderr);
        assert!(outcome.stderr.contains("todo_write"), "{}", outcome.stderr);
        assert!(!outcome.stderr.contains("SECRET"), "{}", outcome.stderr);
    }
}

#[test]
fn the_flag_is_refused_where_no_session_starts() {
    for args in [
        &["--prewalk-into", "cheap", "--list-tools"][..],
        &["--prewalk-into", "cheap", "--version"],
        &["--prewalk-into", "cheap", "agent", "list"],
        &["--prewalk-into", "cheap", "auth"],
        &["--prewalk-into", "cheap", "list", "agent"],
        &["--prewalk-into", "cheap", "import"],
    ] {
        let outcome = shai("auth.config.json", &config(""), args);
        assert_eq!(outcome.status, 2, "{args:?}: {}", outcome.stderr);
        assert!(
            outcome
                .stderr
                .contains("--prewalk-into only applies to commands that start an agent session"),
            "{args:?}: {}",
            outcome.stderr
        );
        assert!(outcome.stdout.is_empty(), "{args:?}: {}", outcome.stdout);
    }

    // Without the flag, and with a config section that is fine, they run.
    let outcome = shai(
        "auth.config.json",
        &config(r#", "prewalk": {"into": "cheap"}"#),
        &["--version"],
    );
    assert_eq!(outcome.status, 0, "{}", outcome.stderr);
    assert!(
        outcome.stdout.contains("shai version"),
        "{}",
        outcome.stdout
    );
}
