use crate::config::agent::CompactionConfig;

use super::bash_aware::compact_bash;
use super::generic::compact_generic;

/// Compact tool output before it enters the trace.
///
/// Returns the compacted string. If compaction is disabled, returns the
/// original output unchanged.
pub fn compact_tool_result(
    tool_name: &str,
    tool_metadata: Option<&std::collections::HashMap<String, serde_json::Value>>,
    output: &str,
    config: &CompactionConfig,
) -> String {
    if !config.enabled {
        return output.to_string();
    }

    // Tier B: bash command-aware compaction
    let after_bash = if tool_name == "bash" {
        let command = tool_metadata
            .and_then(|m| m.get("command"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        compact_bash(command, output, config.max_output_chars)
    } else {
        output.to_string()
    };

    // Tier A: generic compaction (ANSI strip, dedup, head/tail truncate)
    compact_generic(&after_bash, config.max_output_chars)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn default_config() -> CompactionConfig {
        CompactionConfig::default()
    }

    #[test]
    fn test_disabled_returns_unchanged() {
        let mut config = default_config();
        config.enabled = false;
        let result = compact_tool_result("bash", None, "\x1b[31mhello\x1b[0m", &config);
        assert_eq!(result, "\x1b[31mhello\x1b[0m");
    }

    #[test]
    fn test_bash_compaction_strips_ansi() {
        let mut metadata = HashMap::new();
        metadata.insert("command".to_string(), serde_json::json!("echo hi"));
        let result = compact_tool_result(
            "bash",
            Some(&metadata),
            "\x1b[32mok\x1b[0m",
            &default_config(),
        );
        assert_eq!(result, "ok");
    }

    #[test]
    fn test_non_bash_strips_ansi() {
        let result = compact_tool_result("ls", None, "\x1b[31mhello\x1b[0m", &default_config());
        assert_eq!(result, "hello");
    }
}
