use std::io::{self, BufRead, Write};

/// Environment override for [`anonymous_fallback`](super::config::ShaiConfig::anonymous_fallback).
pub const ENV_VAR: &str = "SHAI_ANONYMOUS_FALLBACK";

/// Prompt shown on an interactive TTY when no auth config is present.
pub const FIRST_RUN_PROMPT: &str =
    "No config found (no 3rd-party API configured). Falling back to anonymous OVH. Continue? [Y/n] ";

/// One-line warning printed to stderr on non-interactive first run (prompt mode).
pub const FIRST_RUN_WARNING: &str =
    "Warning: no config found (no 3rd-party API configured); falling back to anonymous OVH.";

/// Printed when the user declines the first-run prompt.
pub const CANCEL_MESSAGE: &str = "Cancelled. Run `shai auth` to configure a provider.";

/// Exit code when anonymous fallback is refused (deny or an invalid setting).
pub const DENY_EXIT_CODE: i32 = 2;

/// Exit code when the user declines the first-run prompt.
pub const CANCEL_EXIT_CODE: i32 = 1;

/// How to behave when a session would use anonymous OVH.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnonymousFallbackMode {
    Prompt,
    Deny,
}

impl AnonymousFallbackMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Prompt => "prompt",
            Self::Deny => "deny",
        }
    }
}

/// Where the resolved mode came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnonymousFallbackSource {
    Default,
    Config,
    Env,
}

impl AnonymousFallbackSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Config => "config",
            Self::Env => "env",
        }
    }
}

/// Result of combining the config key, the env override, and the built-in default.
///
/// Precedence is tighten-only: `deny` from either source wins. The environment
/// can tighten `prompt` to `deny`, but cannot loosen a config `deny`. An
/// unknown or malformed value in either source resolves to `deny` and carries a
/// clear error; it never becomes `prompt`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnonymousFallbackResolution {
    pub mode: AnonymousFallbackMode,
    pub source: AnonymousFallbackSource,
    /// Present when config or env contained a value that was not `prompt` or `deny`.
    pub invalid: Option<String>,
}

impl AnonymousFallbackResolution {
    pub fn log_line(&self) -> String {
        format!(
            "anonymous fallback: {} (source: {})",
            self.mode.as_str(),
            self.source.as_str()
        )
    }

    pub fn log(&self) {
        tracing::info!("{}", self.log_line());
    }
}

/// Parse a single raw value. Surrounding whitespace is ignored. Unknown values
/// (including empty) are `None` so the caller can fail closed.
fn parse_mode(raw: &str) -> Option<AnonymousFallbackMode> {
    match raw.trim() {
        s if s.eq_ignore_ascii_case("prompt") => Some(AnonymousFallbackMode::Prompt),
        s if s.eq_ignore_ascii_case("deny") => Some(AnonymousFallbackMode::Deny),
        _ => None,
    }
}

fn invalid_value_message(which: &str, raw: &str) -> String {
    format!("Invalid {which} value \"{raw}\"; expected \"prompt\" or \"deny\".")
}

/// Resolve the mode from optional raw config and env values.
///
/// `None` means the key or variable is unset. `Some("")` or any other
/// unrecognized string is invalid and resolves to [`AnonymousFallbackMode::Deny`].
pub fn resolve_anonymous_fallback(
    config: Option<&str>,
    env: Option<&str>,
) -> AnonymousFallbackResolution {
    let config_parsed = config.map(|raw| (raw, parse_mode(raw)));
    let env_parsed = env.map(|raw| (raw, parse_mode(raw)));

    let config_invalid = matches!(&config_parsed, Some((_, None)));
    let env_invalid = matches!(&env_parsed, Some((_, None)));

    if config_invalid || env_invalid {
        let mut parts = Vec::new();
        if let Some((raw, None)) = config_parsed {
            parts.push(invalid_value_message(
                "anonymous_fallback in auth config",
                raw,
            ));
        }
        if let Some((raw, None)) = env_parsed {
            parts.push(invalid_value_message(ENV_VAR, raw));
        }
        let source = if env_invalid {
            AnonymousFallbackSource::Env
        } else {
            AnonymousFallbackSource::Config
        };
        return AnonymousFallbackResolution {
            mode: AnonymousFallbackMode::Deny,
            source,
            invalid: Some(parts.join(" ")),
        };
    }

    let config_mode = config_parsed.and_then(|(_, mode)| mode);
    let env_mode = env_parsed.and_then(|(_, mode)| mode);

    match (config_mode, env_mode) {
        (Some(AnonymousFallbackMode::Deny), _) => AnonymousFallbackResolution {
            mode: AnonymousFallbackMode::Deny,
            source: AnonymousFallbackSource::Config,
            invalid: None,
        },
        (Some(AnonymousFallbackMode::Prompt), Some(AnonymousFallbackMode::Deny)) => {
            AnonymousFallbackResolution {
                mode: AnonymousFallbackMode::Deny,
                source: AnonymousFallbackSource::Env,
                invalid: None,
            }
        }
        (Some(AnonymousFallbackMode::Prompt), _) => AnonymousFallbackResolution {
            mode: AnonymousFallbackMode::Prompt,
            source: AnonymousFallbackSource::Config,
            invalid: None,
        },
        (None, Some(AnonymousFallbackMode::Deny)) => AnonymousFallbackResolution {
            mode: AnonymousFallbackMode::Deny,
            source: AnonymousFallbackSource::Env,
            invalid: None,
        },
        (None, Some(AnonymousFallbackMode::Prompt)) => AnonymousFallbackResolution {
            mode: AnonymousFallbackMode::Prompt,
            source: AnonymousFallbackSource::Env,
            invalid: None,
        },
        (None, None) => AnonymousFallbackResolution {
            mode: AnonymousFallbackMode::Prompt,
            source: AnonymousFallbackSource::Default,
            invalid: None,
        },
    }
}

/// Read [`ENV_VAR`] from the process environment. `None` means unset.
/// A present but non-Unicode value is returned as an invalid placeholder.
pub fn env_anonymous_fallback() -> Option<String> {
    match std::env::var(ENV_VAR) {
        Ok(value) => Some(value),
        Err(std::env::VarError::NotPresent) => None,
        Err(std::env::VarError::NotUnicode(_)) => Some("\u{FFFD}".to_string()),
    }
}

/// Why a session might use anonymous OVH.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackSituation {
    /// Auth config is missing or could not be loaded.
    MissingConfig,
    /// Auth config loaded, but the selected provider is anonymous OVH.
    AnonymousOvh,
    /// A non-anonymous provider is configured, or a named agent supplies one.
    Configured,
}

/// How the process can talk to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackInteraction {
    /// Interactive TUI with a TTY — confirm on first run in prompt mode.
    InteractiveTty,
    /// Headless, serve, or non-TTY stdin — warn on first run in prompt mode.
    NonInteractive,
}

/// Outcome of the anonymous-fallback gate. Callers must not start a session
/// or send a network request on [`FallbackGate::Refuse`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FallbackGate {
    Proceed {
        resolution: AnonymousFallbackResolution,
        first_run_warning: Option<&'static str>,
    },
    NeedConfirm {
        resolution: AnonymousFallbackResolution,
        prompt: &'static str,
    },
    Refuse {
        resolution: AnonymousFallbackResolution,
        message: String,
        exit_code: i32,
    },
}

pub fn deny_message(situation: FallbackSituation) -> String {
    match situation {
        FallbackSituation::AnonymousOvh => {
            "The selected provider is anonymous OVH. Anonymous fallback is disabled. Run `shai auth` to configure a provider."
                .to_string()
        }
        _ => {
            "No config found (no 3rd-party API configured). Anonymous fallback is disabled. Run `shai auth` to configure a provider."
                .to_string()
        }
    }
}

/// Decide whether to start, confirm, or refuse.
///
/// Invalid settings always refuse with exit 2, even if a provider is already
/// configured. `deny` refuses only when the session would use anonymous OVH.
pub fn evaluate_anonymous_fallback_gate(
    resolution: AnonymousFallbackResolution,
    situation: FallbackSituation,
    interaction: FallbackInteraction,
) -> FallbackGate {
    if let Some(message) = resolution.invalid.clone() {
        return FallbackGate::Refuse {
            resolution,
            message,
            exit_code: DENY_EXIT_CODE,
        };
    }

    if situation == FallbackSituation::Configured {
        return FallbackGate::Proceed {
            resolution,
            first_run_warning: None,
        };
    }

    match resolution.mode {
        AnonymousFallbackMode::Deny => FallbackGate::Refuse {
            resolution,
            message: deny_message(situation),
            exit_code: DENY_EXIT_CODE,
        },
        AnonymousFallbackMode::Prompt => match (situation, interaction) {
            (FallbackSituation::MissingConfig, FallbackInteraction::InteractiveTty) => {
                FallbackGate::NeedConfirm {
                    resolution,
                    prompt: FIRST_RUN_PROMPT,
                }
            }
            (FallbackSituation::MissingConfig, FallbackInteraction::NonInteractive) => {
                FallbackGate::Proceed {
                    resolution,
                    first_run_warning: Some(FIRST_RUN_WARNING),
                }
            }
            (FallbackSituation::AnonymousOvh, _) | (FallbackSituation::Configured, _) => {
                FallbackGate::Proceed {
                    resolution,
                    first_run_warning: None,
                }
            }
        },
    }
}

/// Ask the first-run question. Enter or `y`/`yes` continues; `n`/`no` cancels.
/// Other answers re-prompt. EOF cancels.
pub fn confirm_anonymous_fallback(
    input: &mut impl BufRead,
    output: &mut impl Write,
    prompt: &str,
) -> io::Result<bool> {
    loop {
        write!(output, "{prompt}")?;
        output.flush()?;
        let mut line = String::new();
        let n = input.read_line(&mut line)?;
        if n == 0 {
            return Ok(false);
        }
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.eq_ignore_ascii_case("y")
            || trimmed.eq_ignore_ascii_case("yes")
        {
            return Ok(true);
        }
        if trimmed.eq_ignore_ascii_case("n") || trimmed.eq_ignore_ascii_case("no") {
            return Ok(false);
        }
        writeln!(output, "Please answer Y or n.")?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn res(
        config: Option<&str>,
        env: Option<&str>,
        mode: AnonymousFallbackMode,
        source: AnonymousFallbackSource,
        invalid: bool,
    ) {
        let got = resolve_anonymous_fallback(config, env);
        assert_eq!(got.mode, mode, "config={config:?} env={env:?}");
        assert_eq!(got.source, source, "config={config:?} env={env:?}");
        assert_eq!(
            got.invalid.is_some(),
            invalid,
            "config={config:?} env={env:?} invalid={:?} log={}",
            got.invalid,
            got.log_line()
        );
        if invalid {
            assert_eq!(got.mode, AnonymousFallbackMode::Deny);
            let msg = got.invalid.as_deref().unwrap();
            assert!(msg.contains("expected \"prompt\" or \"deny\""), "{msg}");
            assert!(!msg.to_ascii_lowercase().contains("prompt mode"));
        }
        assert_eq!(
            got.log_line(),
            format!(
                "anonymous fallback: {} (source: {})",
                mode.as_str(),
                source.as_str()
            )
        );
    }

    #[test]
    fn resolve_precedence_matrix() {
        use AnonymousFallbackMode::*;
        use AnonymousFallbackSource::*;

        // default
        res(None, None, Prompt, Default, false);

        // config only
        res(Some("prompt"), None, Prompt, Config, false);
        res(Some("deny"), None, Deny, Config, false);
        res(Some("PROMPT"), None, Prompt, Config, false);
        res(Some(" Deny "), None, Deny, Config, false);

        // env only
        res(None, Some("prompt"), Prompt, Env, false);
        res(None, Some("deny"), Deny, Env, false);
        res(None, Some("prompt"), Prompt, Env, false);

        // both, same
        res(Some("prompt"), Some("prompt"), Prompt, Config, false);
        res(Some("deny"), Some("deny"), Deny, Config, false);

        // tighten-only: env may tighten prompt to deny, never loosen config deny
        res(Some("prompt"), Some("deny"), Deny, Env, false);
        res(Some("deny"), Some("prompt"), Deny, Config, false);
    }

    #[test]
    fn resolve_invalid_values_are_deny_never_prompt() {
        use AnonymousFallbackMode::*;
        use AnonymousFallbackSource::*;

        res(Some("allow"), None, Deny, Config, true);
        res(Some(""), None, Deny, Config, true);
        res(Some("true"), None, Deny, Config, true);
        res(Some("1"), None, Deny, Config, true);
        res(None, Some("allow"), Deny, Env, true);
        res(None, Some(""), Deny, Env, true);
        res(None, Some("maybe"), Deny, Env, true);

        // invalid env wins the source tag when both are invalid
        let both = resolve_anonymous_fallback(Some("nope"), Some("nope"));
        assert_eq!(both.mode, Deny);
        assert_eq!(both.source, Env);
        assert!(both
            .invalid
            .as_ref()
            .unwrap()
            .contains("anonymous_fallback"));
        assert!(both.invalid.as_ref().unwrap().contains(ENV_VAR));

        // invalid config + env prompt still deny (cannot loosen / cannot default to prompt)
        let tightened = resolve_anonymous_fallback(Some("bogus"), Some("prompt"));
        assert_eq!(tightened.mode, Deny);
        assert!(tightened.invalid.is_some());

        // valid config prompt + invalid env → deny, never prompt
        let env_bad = resolve_anonymous_fallback(Some("prompt"), Some("bogus"));
        assert_eq!(env_bad.mode, Deny);
        assert_eq!(env_bad.source, Env);
        assert!(env_bad.invalid.is_some());

        // valid config deny + invalid env → deny
        let deny_plus_bad = resolve_anonymous_fallback(Some("deny"), Some("bogus"));
        assert_eq!(deny_plus_bad.mode, Deny);
        assert!(deny_plus_bad.invalid.is_some());
    }

    fn gate(
        config: Option<&str>,
        env: Option<&str>,
        situation: FallbackSituation,
        interaction: FallbackInteraction,
    ) -> FallbackGate {
        evaluate_anonymous_fallback_gate(
            resolve_anonymous_fallback(config, env),
            situation,
            interaction,
        )
    }

    #[test]
    fn gate_configured_provider_proceeds_for_valid_modes() {
        for (config, env) in [
            (None, None),
            (Some("prompt"), None),
            (Some("deny"), None),
            (None, Some("deny")),
            (Some("deny"), Some("prompt")),
        ] {
            match gate(
                config,
                env,
                FallbackSituation::Configured,
                FallbackInteraction::InteractiveTty,
            ) {
                FallbackGate::Proceed {
                    first_run_warning, ..
                } => assert_eq!(first_run_warning, None),
                other => panic!("expected proceed, got {other:?}"),
            }
        }
    }

    #[test]
    fn gate_invalid_refuses_even_when_configured() {
        match gate(
            Some("allow"),
            None,
            FallbackSituation::Configured,
            FallbackInteraction::InteractiveTty,
        ) {
            FallbackGate::Refuse {
                exit_code, message, ..
            } => {
                assert_eq!(exit_code, DENY_EXIT_CODE);
                assert!(message.contains("expected \"prompt\" or \"deny\""));
            }
            other => panic!("expected refuse, got {other:?}"),
        }
    }

    #[test]
    fn gate_prompt_tty_first_run_confirms() {
        match gate(
            None,
            None,
            FallbackSituation::MissingConfig,
            FallbackInteraction::InteractiveTty,
        ) {
            FallbackGate::NeedConfirm { prompt, resolution } => {
                assert_eq!(prompt, FIRST_RUN_PROMPT);
                assert_eq!(resolution.mode, AnonymousFallbackMode::Prompt);
                assert_eq!(resolution.source, AnonymousFallbackSource::Default);
            }
            other => panic!("expected confirm, got {other:?}"),
        }
    }

    #[test]
    fn gate_prompt_headless_first_run_warns_and_continues() {
        match gate(
            None,
            None,
            FallbackSituation::MissingConfig,
            FallbackInteraction::NonInteractive,
        ) {
            FallbackGate::Proceed {
                first_run_warning, ..
            } => assert_eq!(first_run_warning, Some(FIRST_RUN_WARNING)),
            other => panic!("expected proceed with warning, got {other:?}"),
        }
    }

    #[test]
    fn gate_prompt_does_not_reprompt_saved_anonymous_ovh() {
        match gate(
            None,
            None,
            FallbackSituation::AnonymousOvh,
            FallbackInteraction::InteractiveTty,
        ) {
            FallbackGate::Proceed {
                first_run_warning, ..
            } => assert_eq!(first_run_warning, None),
            other => panic!("expected silent proceed, got {other:?}"),
        }
    }

    #[test]
    fn gate_deny_refuses_missing_and_anonymous_without_network_side_effects() {
        for situation in [
            FallbackSituation::MissingConfig,
            FallbackSituation::AnonymousOvh,
        ] {
            match gate(
                Some("deny"),
                None,
                situation,
                FallbackInteraction::NonInteractive,
            ) {
                FallbackGate::Refuse {
                    exit_code,
                    message,
                    resolution,
                } => {
                    assert_eq!(exit_code, DENY_EXIT_CODE);
                    assert_eq!(resolution.mode, AnonymousFallbackMode::Deny);
                    assert!(message.contains("Anonymous fallback is disabled"));
                    assert!(message.contains("shai auth"));
                }
                other => panic!("expected refuse for {situation:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn confirm_accepts_enter_and_yes() {
        for input in ["\n", "y\n", "Y\n", "yes\n", " YES \n"] {
            let mut inp = Cursor::new(input.as_bytes().to_vec());
            let mut out = Vec::new();
            let accepted =
                confirm_anonymous_fallback(&mut inp, &mut out, FIRST_RUN_PROMPT).expect("confirm");
            assert!(accepted, "input={input:?}");
            let text = String::from_utf8(out).unwrap();
            assert!(text.contains(FIRST_RUN_PROMPT), "{text}");
            assert!(!text.contains(FIRST_RUN_WARNING));
        }
    }

    #[test]
    fn confirm_declines_n_and_eof() {
        for input in ["n\n", "N\n", "no\n", ""] {
            let mut inp = Cursor::new(input.as_bytes().to_vec());
            let mut out = Vec::new();
            let accepted =
                confirm_anonymous_fallback(&mut inp, &mut out, FIRST_RUN_PROMPT).expect("confirm");
            assert!(!accepted, "input={input:?}");
        }
    }

    #[test]
    fn confirm_reprompts_on_garbage() {
        let mut inp = Cursor::new(b"maybe\ny\n".to_vec());
        let mut out = Vec::new();
        let accepted =
            confirm_anonymous_fallback(&mut inp, &mut out, FIRST_RUN_PROMPT).expect("confirm");
        assert!(accepted);
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("Please answer Y or n."), "{text}");
        assert_eq!(text.matches(FIRST_RUN_PROMPT).count(), 2);
    }
}
