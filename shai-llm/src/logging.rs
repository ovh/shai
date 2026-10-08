use crate::provider::LlmError;
use openai_dive::v1::resources::chat::ChatCompletionParameters;
use std::path::PathBuf;
use tracing::{debug, warn};

/// Log a failed LLM request to a file for debugging
///
/// Configuration via environment variables:
/// - `SHAI_LLM_ERR_LOGGING_ENABLED`: Set to "true" to enable error logging (default: false)
/// - `SHAI_LLM_ERR_FOLDER`: Directory for error logs (default: `.shai/llm/errors/`)
pub fn log_llm_error(request: &ChatCompletionParameters, error: &LlmError, provider_name: &str) {
    // Check if error logging is enabled
    let enabled = std::env::var("SHAI_LLM_LOGGING_ENABLED")
        .map(|v| v.to_lowercase() == "true")
        .unwrap_or(false);

    if !enabled {
        return;
    }

    // Get log directory from env or use default
    let log_dir = std::env::var("SHAI_LLM_LOGGING_FOLDER")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(".shai/logs/"));

    // Create directory if it doesn't exist
    if let Err(e) = std::fs::create_dir_all(&log_dir) {
        warn!(target: "shai_llm::logging", "Failed to create error log directory: {}", e);
        return;
    }

    // Generate filename with timestamp
    let timestamp = chrono::Utc::now();
    let filename = format!(
        "error_{}_{}.log",
        timestamp.format("%Y%m%d_%H%M%S"),
        timestamp.format("%3f") // milliseconds
    );
    let log_path = log_dir.join(filename);

    // Build log content
    let mut log_content = String::new();

    // Header
    log_content.push_str("=== LLM Request Error Log ===\n");
    log_content.push_str(&format!("Timestamp: {}\n", timestamp.to_rfc3339()));
    log_content.push_str(&format!("Provider: {}\n", provider_name));
    log_content.push_str(&format!("Model: {}\n", request.model));

    // Request section
    log_content.push_str("\n=== REQUEST ===\n");
    match serde_json::to_string_pretty(request) {
        Ok(json) => log_content.push_str(&json),
        Err(e) => log_content.push_str(&format!("Failed to serialize request: {}", e)),
    }
    log_content.push('\n');

    // Error section
    log_content.push_str("\n=== ERROR ===\n");
    log_content.push_str(&format!("{}\n", error));

    // Write to file
    if let Err(e) = std::fs::write(&log_path, log_content) {
        warn!(target: "shai_llm::logging", "Failed to write error log to {}: {}", log_path.display(), e);
    } else {
        debug!(target: "shai_llm::logging", "LLM error logged to: {}", log_path.display());
    }
}
