// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: OVH SAS

//! Configuration of prewalk, the one-shot handoff of a session to another
//! model (see [`crate::agent::prewalk`]).
//!
//! Prewalk is off unless a target is named, either on the command line or in
//! the auth config:
//!
//! ```jsonc
//! "prewalk": { "into": "cheap" }
//! ```
//!
//! The target is a role name or a `<provider>/<model>` reference, resolved
//! with the same rules as any secondary model (see [`super::roles`]). A
//! target that is named but cannot be resolved is an error: the session must
//! not start as if prewalk had not been asked for.

use super::config::ShaiConfig;
use super::roles::{BoundModel, RoleError};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The `prewalk` section of the auth config.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrewalkConfig {
    /// Role name or `<provider>/<model>` to hand the session off to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub into: Option<String>,
}

/// Why a prewalk target that was asked for cannot be armed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PrewalkSetupError {
    /// The `prewalk` section of the config does not have the expected shape,
    /// or the config has more than one.
    #[error("the \"prewalk\" section of the config is invalid: expected a single {{\"into\": \"<role or provider/model>\"}}")]
    InvalidSection,

    /// The target is empty.
    #[error("the prewalk target given by {source_name} is empty")]
    EmptyTarget { source_name: &'static str },

    /// The target does not resolve to a usable model.
    #[error("cannot arm prewalk from {source_name}: {error}")]
    Unresolved {
        source_name: &'static str,
        #[source]
        error: RoleError,
    },
}

/// Where a prewalk target was named.
const FLAG: &str = "--prewalk-into";
const CONFIG: &str = "the config";
/// The config setting that names the target.
const SETTING: &str = "prewalk.into";

/// Decide what, if anything, a new session should prewalk into.
///
/// `flag` is the value of `--prewalk-into` and wins over the config.
/// `config` is the outcome of [`ShaiConfig::load_for_roles`]: the user's own
/// config, never the built-in default.
///
/// Returns `Ok(None)` when prewalk was not asked for. When it was, the result
/// is either a target ready to be armed or an error; there is no third
/// outcome in which the session quietly runs without it. Nothing here
/// performs network I/O.
pub fn resolve_prewalk(
    flag: Option<&str>,
    config: Result<&ShaiConfig, &RoleError>,
) -> Result<Option<BoundModel>, PrewalkSetupError> {
    let (target, source_name) = match flag {
        Some(target) => (target.to_string(), FLAG),
        None => {
            // Without a flag there is nothing to resolve unless the config
            // asks for it. A config file that is missing asks for nothing;
            // one that exists and cannot be read is the caller's to refuse
            // (see `SessionOptions::resolve` in shai-cli).
            let Ok(config) = config else {
                return Ok(None);
            };
            let section = config
                .prewalk
                .get()
                .map_err(|_| PrewalkSetupError::InvalidSection)?;
            match section.and_then(|section| section.into) {
                Some(target) => (target, CONFIG),
                None => return Ok(None),
            }
        }
    };

    if target.trim().is_empty() {
        return Err(PrewalkSetupError::EmptyTarget { source_name });
    }
    let unresolved = |error| PrewalkSetupError::Unresolved { source_name, error };
    let config = config.map_err(|error| unresolved(error.clone()))?;
    // What was typed on the command line may be quoted back in an error.
    // What was read from the file may not: the setting is named instead.
    let resolved = if source_name == FLAG {
        config.resolve_model_target(&target)
    } else {
        config.resolve_configured_target(SETTING, &target)
    };
    let bound = resolved
        .and_then(|resolved| resolved.bind())
        .map_err(unresolved)?;
    Ok(Some(bound))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(json_tail: &str) -> ShaiConfig {
        serde_json::from_str(&format!(
            r#"{{
                "providers": [
                    {{"provider": "openai", "env_vars": {{"OPENAI_API_KEY": "k"}}, "model": "gpt-4o", "tool_method": "FunctionCall"}},
                    {{"provider": "ollama", "env_vars": {{"OLLAMA_BASE_URL": "http://127.0.0.1:9/v1"}}, "model": "llama3", "tool_method": "FunctionCall"}}
                ],
                "selected_provider": 0,
                "roles": {{"cheap": "ollama/qwen3:8b"}}
                {json_tail}
            }}"#
        ))
        .expect("config should load")
    }

    fn label(result: Result<Option<BoundModel>, PrewalkSetupError>) -> Option<String> {
        result.expect("should resolve").map(|bound| bound.label())
    }

    #[test]
    fn off_by_default() {
        let cfg = config("");
        assert!(cfg.prewalk.is_unset());
        assert_eq!(label(resolve_prewalk(None, Ok(&cfg))), None);

        // An empty section, or one without a target, is off too.
        for tail in [
            r#", "prewalk": {}"#,
            r#", "prewalk": {"into": null}"#,
            r#", "prewalk": null"#,
        ] {
            assert_eq!(
                label(resolve_prewalk(None, Ok(&config(tail)))),
                None,
                "{tail}"
            );
        }

        // No usable config and no flag: nothing was asked for.
        assert_eq!(
            label(resolve_prewalk(None, Err(&RoleError::ConfigMissing))),
            None
        );
    }

    #[test]
    fn config_names_a_role_or_a_reference() {
        let cfg = config(r#", "prewalk": {"into": "cheap"}"#);
        assert_eq!(
            label(resolve_prewalk(None, Ok(&cfg))).as_deref(),
            Some("ollama/qwen3:8b")
        );

        let cfg = config(r#", "prewalk": {"into": "openai/gpt-4o-mini"}"#);
        assert_eq!(
            label(resolve_prewalk(None, Ok(&cfg))).as_deref(),
            Some("openai/gpt-4o-mini")
        );
    }

    #[test]
    fn flag_wins_over_config() {
        let cfg = config(r#", "prewalk": {"into": "cheap"}"#);
        assert_eq!(
            label(resolve_prewalk(Some("openai/o4-mini"), Ok(&cfg))).as_deref(),
            Some("openai/o4-mini")
        );
        // Including over a config section that is broken.
        let cfg = config(r#", "prewalk": {"in_to": "cheap"}"#);
        assert_eq!(
            label(resolve_prewalk(Some("cheap"), Ok(&cfg))).as_deref(),
            Some("ollama/qwen3:8b")
        );
    }

    #[test]
    fn unresolvable_target_refuses_to_arm() {
        let cfg = config("");
        for target in [
            "planner",            // role that is not configured
            "anthropic/claude-x", // provider that is not configured
            "gpt-4o-mini",        // neither a role nor a reference
            "openai/",            // malformed
        ] {
            let error = resolve_prewalk(Some(target), Ok(&cfg)).unwrap_err();
            assert!(
                matches!(
                    error,
                    PrewalkSetupError::Unresolved {
                        source_name: "--prewalk-into",
                        ..
                    }
                ),
                "{target}: {error:?}"
            );
            assert!(error
                .to_string()
                .starts_with("cannot arm prewalk from --prewalk-into: "));
        }
    }

    #[test]
    fn an_unresolvable_target_from_the_config_is_named_not_quoted() {
        // The value comes from the file and may be a credential pasted into
        // the wrong place: the error names the setting and leaves it out.
        for (value, expected) in [
            ("planner-x9", "names a role that is not configured"),
            ("sk-live-SECRETSECRET", "is neither a role name nor a valid"),
            (
                "anthropic/claude-x9",
                "names a provider that is not configured",
            ),
            ("openai/", "is neither a role name nor a valid"),
            ("OpenAI/gpt-x9", "is neither a role name nor a valid"),
        ] {
            let cfg = config(&format!(r#", "prewalk": {{"into": "{value}"}}"#));
            let error = resolve_prewalk(None, Ok(&cfg)).unwrap_err();
            assert!(
                matches!(
                    error,
                    PrewalkSetupError::Unresolved {
                        source_name: "the config",
                        ..
                    }
                ),
                "{value}: {error:?}"
            );
            let message = error.to_string();
            assert!(
                message.starts_with("cannot arm prewalk from the config: \"prewalk.into\" "),
                "{message}"
            );
            assert!(message.contains(expected), "{value}: {message}");
            for part in [value, "x9", "SECRET", "OpenAI"] {
                if value.contains(part) {
                    assert!(!message.contains(part), "{value}: {message}");
                }
            }
            assert!(!format!("{error:?}").contains(value), "{error:?}");
        }
    }

    #[test]
    fn empty_target_is_an_error_not_off() {
        let cfg = config(r#", "prewalk": {"into": ""}"#);
        assert!(matches!(
            resolve_prewalk(None, Ok(&cfg)),
            Err(PrewalkSetupError::EmptyTarget { .. })
        ));
        assert!(matches!(
            resolve_prewalk(Some("  "), Ok(&cfg)),
            Err(PrewalkSetupError::EmptyTarget { .. })
        ));
    }

    #[test]
    fn flag_without_a_usable_config_is_an_error() {
        // The built-in default config must never stand in for the user's.
        let error = resolve_prewalk(Some("ovhcloud/Qwen3-32B"), Err(&RoleError::ConfigMissing))
            .unwrap_err();
        assert!(matches!(
            error,
            PrewalkSetupError::Unresolved {
                error: RoleError::ConfigMissing,
                ..
            }
        ));
    }

    #[test]
    fn invalid_section_is_an_error_and_does_not_break_the_config() {
        for tail in [
            r#", "prewalk": "cheap""#,
            r#", "prewalk": {"in_to": "cheap"}"#,
            r#", "prewalk": {"into": 5}"#,
            r#", "prewalk": ["cheap"]"#,
            // Written twice, as after a careless paste.
            r#", "prewalk": {"into": "cheap"}, "prewalk": {"into": "cheap"}"#,
            r#", "prewalk": null, "prewalk": {"into": "cheap"}"#,
        ] {
            let cfg = config(tail);
            // The primary provider still loads...
            assert_eq!(cfg.get_selected_provider().unwrap().provider, "openai");
            assert!(!cfg.prewalk.is_unset(), "{tail}");
            // ...and prewalk refuses to guess what was meant.
            assert_eq!(
                resolve_prewalk(None, Ok(&cfg)).unwrap_err(),
                PrewalkSetupError::InvalidSection,
                "{tail}"
            );
        }
    }

    #[test]
    fn section_survives_a_save_and_is_absent_when_unset() {
        let cfg = config(r#", "prewalk": {"into": "cheap"}"#);
        let saved = serde_json::to_string(&cfg).unwrap();
        assert!(
            saved.ends_with(r#""prewalk":{"into": "cheap"}}"#),
            "{saved}"
        );
        let reloaded: ShaiConfig = serde_json::from_str(&saved).unwrap();
        assert_eq!(reloaded.prewalk, cfg.prewalk);

        let saved = serde_json::to_string(&config("")).unwrap();
        assert!(!saved.contains("prewalk"), "{saved}");

        // A section written twice is written back twice: it stays invalid,
        // and visibly so, until one copy is removed.
        let twice = config(r#", "prewalk": {"into": "cheap"}, "prewalk": {"into": "cheap"}"#);
        let saved = serde_json::to_string(&twice).unwrap();
        assert_eq!(saved.matches(r#""prewalk":"#).count(), 2, "{saved}");
        let reloaded: ShaiConfig = serde_json::from_str(&saved).unwrap();
        assert_eq!(
            resolve_prewalk(None, Ok(&reloaded)).unwrap_err(),
            PrewalkSetupError::InvalidSection
        );
    }
}
