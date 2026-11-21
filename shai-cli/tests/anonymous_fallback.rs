// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: OVH SAS

//! Binary checks for `anonymous_fallback`: an unreadable auth file must exit 2
//! in every mode without quoting the file, and env `deny` must not pull or
//! write a first-run config.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

struct Outcome {
    status: i32,
    stdout: String,
    stderr: String,
}

fn shai(
    config_home: &std::path::Path,
    home: &std::path::Path,
    extra_env: &[(&str, &str)],
    args: &[&str],
) -> Outcome {
    let mut command = Command::new(env!("CARGO_BIN_EXE_shai"));
    command
        .args(args)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", config_home)
        .env("NO_COLOR", "1")
        .env("HTTPS_PROXY", "http://127.0.0.1:9")
        .env("HTTP_PROXY", "http://127.0.0.1:9")
        .env("ALL_PROXY", "http://127.0.0.1:9")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in extra_env {
        command.env(k, v);
    }

    let mut child = command.spawn().expect("the shai binary should start");

    let started = Instant::now();
    while child.try_wait().unwrap().is_none() {
        if started.elapsed() > Duration::from_secs(20) {
            let _ = child.kill();
            panic!("shai {args:?} did not exit");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = child.wait_with_output().unwrap();
    Outcome {
        status: output.status.code().expect("an exit status"),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn assert_invalid_file_exits_2(env: &[(&str, &str)]) {
    let dir = tempfile::TempDir::new().unwrap();
    let config_home = dir.path().join("config");
    let shai_dir = config_home.join("shai");
    std::fs::create_dir_all(&shai_dir).unwrap();
    let broken = r#"{"providers":[],"selected_provider":"sk-live-SECRETSECRET"}"#;
    std::fs::write(shai_dir.join("auth.config.json"), broken).unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();

    let outcome = shai(&config_home, &home, env, &["-p", "hello"]);
    assert_eq!(
        outcome.status, 2,
        "env={env:?} stdout={} stderr={}",
        outcome.stdout, outcome.stderr
    );
    assert!(
        outcome.stderr.contains("could not be loaded")
            && outcome.stderr.contains("auth.config.json"),
        "stderr={}",
        outcome.stderr
    );
    assert!(
        outcome.stderr.contains("unexpected value at line 1")
            || outcome.stderr.contains("JSON syntax error"),
        "stderr={}",
        outcome.stderr
    );
    assert!(
        !outcome.stderr.contains("SECRET") && !outcome.stderr.contains("sk-live"),
        "stderr leaked file contents: {}",
        outcome.stderr
    );
    assert!(
        !outcome.stdout.contains("SECRET") && !outcome.stdout.contains("sk-live"),
        "stdout leaked file contents: {}",
        outcome.stdout
    );
    // The broken file must not be replaced by a pulled starter config.
    let left = std::fs::read_to_string(shai_dir.join("auth.config.json")).unwrap();
    assert_eq!(left, broken);
}

#[test]
fn invalid_auth_config_exits_2_under_deny() {
    assert_invalid_file_exits_2(&[("SHAI_ANONYMOUS_FALLBACK", "deny")]);
}

#[test]
fn invalid_auth_config_exits_2_under_prompt() {
    assert_invalid_file_exits_2(&[("SHAI_ANONYMOUS_FALLBACK", "prompt")]);
    // Default (unset) is also prompt.
    assert_invalid_file_exits_2(&[]);
}

#[test]
fn env_deny_does_not_write_auth_config_on_version() {
    let dir = tempfile::TempDir::new().unwrap();
    let config_home = dir.path().join("config");
    let home = dir.path().join("home");
    std::fs::create_dir_all(&config_home).unwrap();
    std::fs::create_dir_all(&home).unwrap();

    let outcome = shai(
        &config_home,
        &home,
        &[("SHAI_ANONYMOUS_FALLBACK", "deny")],
        &["--version"],
    );
    assert_eq!(
        outcome.status, 0,
        "stdout={} stderr={}",
        outcome.stdout, outcome.stderr
    );
    assert!(
        outcome.stdout.contains("shai version") || outcome.stdout.contains("version"),
        "stdout={}",
        outcome.stdout
    );
    let auth = config_home.join("shai").join("auth.config.json");
    assert!(!auth.exists(), "env deny must not write {}", auth.display());
}
