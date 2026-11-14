use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Per-agent bash tool limits. Nested under `AgentConfig.tools.bash`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BashToolConfig {
    /// Applied when the tool call omits `timeout` (or when the model passes
    /// `0`, which is ignored — unlimited is operator-config only). Set to `0`
    /// here to disable the default timeout for this agent. Default: 300.
    #[serde(default = "default_bash_default_timeout_secs")]
    pub default_timeout_secs: u32,
    /// Hard cap on any model-requested timeout (including the default).
    /// Default: 1800. `0` means no cap (operator only).
    #[serde(default = "default_bash_max_timeout_secs")]
    pub max_timeout_secs: u32,
}

impl Default for BashToolConfig {
    fn default() -> Self {
        Self {
            default_timeout_secs: default_bash_default_timeout_secs(),
            max_timeout_secs: default_bash_max_timeout_secs(),
        }
    }
}

fn default_bash_default_timeout_secs() -> u32 {
    300
}

fn default_bash_max_timeout_secs() -> u32 {
    1800
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct BashToolParams {
    /// The bash command to execute
    pub command: String,
    /// Timeout in seconds. Omit to use the agent default (300s). Values are
    /// capped by `tools.bash.max_timeout_secs` (default 1800). `0` is **not**
    /// unlimited — it is ignored and the agent default applies (unlimited is
    /// operator-config only via `tools.bash.default_timeout_secs = 0`).
    pub timeout: Option<u32>,
    /// Working directory for command execution (optional)
    pub working_dir: Option<String>,
    /// Environment variables to set (optional)
    #[serde(default)]
    pub env: HashMap<String, String>,
}

/// Resolve the effective timeout for a bash call.
///
/// - `params_timeout == Some(0)` → **ignored** (model cannot opt out); treat as omit
/// - `params_timeout == Some(n)` → `min(n, max)` (or uncapped if operator max is 0)
/// - `params_timeout == None` → default; `default_timeout_secs == 0` → no limit (operator)
pub fn resolve_timeout_secs(params_timeout: Option<u32>, config: &BashToolConfig) -> Option<u32> {
    let requested = match params_timeout {
        // A model-facing `timeout: 0` must not disable the hang fix.
        Some(0) | None => {
            if config.default_timeout_secs == 0 {
                return None;
            }
            config.default_timeout_secs
        }
        Some(n) => n,
    };
    if config.max_timeout_secs == 0 {
        Some(requested)
    } else {
        Some(requested.min(config.max_timeout_secs))
    }
}

#[cfg(test)]
mod resolve_tests {
    use super::*;

    #[test]
    fn omitted_uses_default_capped_by_max() {
        let cfg = BashToolConfig::default();
        assert_eq!(resolve_timeout_secs(None, &cfg), Some(300));
        let cfg = BashToolConfig {
            default_timeout_secs: 5000,
            max_timeout_secs: 1800,
        };
        assert_eq!(resolve_timeout_secs(None, &cfg), Some(1800));
    }

    #[test]
    fn model_zero_uses_default_not_unlimited() {
        let cfg = BashToolConfig::default();
        assert_eq!(resolve_timeout_secs(Some(0), &cfg), Some(300));
        let cfg = BashToolConfig {
            default_timeout_secs: 60,
            max_timeout_secs: 1800,
        };
        assert_eq!(resolve_timeout_secs(Some(0), &cfg), Some(60));
    }

    #[test]
    fn explicit_capped_by_max() {
        let cfg = BashToolConfig::default();
        assert_eq!(resolve_timeout_secs(Some(60), &cfg), Some(60));
        assert_eq!(resolve_timeout_secs(Some(99999), &cfg), Some(1800));
    }

    #[test]
    fn operator_default_zero_means_unlimited_when_omitted() {
        let cfg = BashToolConfig {
            default_timeout_secs: 0,
            max_timeout_secs: 1800,
        };
        assert_eq!(resolve_timeout_secs(None, &cfg), None);
        // Model still cannot pass 0 to mean unlimited when operator default is 0:
        // Some(0) is treated as omit → still None (operator unlimited).
        assert_eq!(resolve_timeout_secs(Some(0), &cfg), None);
        assert_eq!(resolve_timeout_secs(Some(10), &cfg), Some(10));
    }
}
