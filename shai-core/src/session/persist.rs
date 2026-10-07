// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: OVH SAS

use chrono::{DateTime, Utc};
use openai_dive::v1::resources::chat::ChatMessage;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, ErrorKind};
use std::path::PathBuf;
use tracing::{debug, error};
use uuid::Uuid;

/// Session data stored on disk
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionData {
    pub session_id: String,
    pub name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub trace: Vec<ChatMessage>,
}

const ADJECTIVES: &[&str] = &[
    "swift", "clever", "bright", "calm", "eager", "fancy", "gentle", "happy", "jolly", "lively",
    "merry", "noble", "proud", "silly", "witty", "brave", "cosmic", "dazzling", "frosty", "golden",
    "lucky", "magic", "noble", "vivid",
];

const NOUNS: &[&str] = &[
    "otter", "falcon", "panda", "tiger", "wolf", "eagle", "fox", "bear", "lynx", "raven", "swan",
    "hawk", "lion", "owl", "panther", "whale", "comet", "river", "meadow", "canyon", "forest",
    "summit", "harbor", "glacier",
];

/// Derive a short title from the first user message in the trace.
/// If the trace is empty or has no user text, falls back to a random name.
fn generate_session_name_from_trace(trace: &[ChatMessage]) -> Option<String> {
    let first_user_text = trace.iter().find_map(|msg| match msg {
        openai_dive::v1::resources::chat::ChatMessage::User { content, .. } => match content {
            openai_dive::v1::resources::chat::ChatMessageContent::Text(t)
                if !t.trim().is_empty() =>
            {
                Some(t.trim())
            }
            _ => None,
        },
        _ => None,
    });

    match first_user_text {
        Some(text) => {
            // Take first line, trim to a reasonable length
            let title = text.lines().next().unwrap_or(text);
            let title = title.trim();
            if title.is_empty() {
                None
            } else {
                // Truncate to 60 chars and add ellipsis if needed
                let max_len = 60;
                if title.chars().count() > max_len {
                    let truncated: String = title.chars().take(max_len).collect();
                    Some(format!("{}…", truncated))
                } else {
                    Some(title.to_string())
                }
            }
        }
        None => None,
    }
}

/// Generate a random fancy session name (adjective-noun).
fn generate_session_name() -> String {
    // Use thread-local RNG for simplicity; we don't need cryptographic randomness.
    use std::time::{SystemTime, UNIX_EPOCH};
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let seed = seed as u64;
    let adj = ADJECTIVES[(seed.wrapping_mul(2654435761) % ADJECTIVES.len() as u64) as usize];
    let noun = NOUNS[(seed.wrapping_mul(40503) % NOUNS.len() as u64) as usize];
    format!("{}-{}", adj, noun)
}

/// Handle session persistence to disk
pub struct SessionPersist;

type PersistError = Box<dyn std::error::Error + Send + Sync>;

/// Maximum length of a session id
const MAX_SESSION_ID_LEN: usize = 128;

/// Validate a session id before it is used in a filesystem path.
///
/// Session ids come from user-controlled inputs (HTTP path segments, CLI
/// arguments). Restricting them to `[A-Za-z0-9_-]` makes path traversal
/// structurally impossible. Generated ids (UUIDs, `resp_<uuid>`) comply.
fn validate_session_id(session_id: &str) -> Result<(), PersistError> {
    let valid = !session_id.is_empty()
        && session_id.len() <= MAX_SESSION_ID_LEN
        && session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if valid {
        Ok(())
    } else {
        // Generic message: do not echo the (potentially hostile) input back
        Err(io::Error::new(ErrorKind::InvalidInput, "invalid session id").into())
    }
}

impl SessionPersist {
    /// Check if session persistence is enabled via environment variable
    pub fn is_enabled() -> bool {
        std::env::var("SHAI_SESSION_PERSIST_ENABLE")
            .map(|v| v.to_lowercase() == "true")
            .unwrap_or(true)
    }

    /// Get the folder path for session storage
    pub fn folder() -> PathBuf {
        std::env::var("SHAI_SESSION_PERSIST_FOLDER")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(".shai/sessions"))
    }

    /// Get the file path for a specific session, after validating the id.
    ///
    /// Defense in depth: beyond the charset validation, the joined path is
    /// checked to remain inside the session folder.
    fn session_file_path(session_id: &str) -> Result<PathBuf, PersistError> {
        validate_session_id(session_id)?;
        let folder = Self::folder();
        let path = folder.join(format!("{}.json", session_id));
        if !path.starts_with(&folder) {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "session id escapes session folder",
            )
            .into());
        }
        Ok(path)
    }

    /// Save a session to disk (atomic write using temp file)
    pub fn save_session(session_id: &str, trace: Vec<ChatMessage>) -> Result<(), PersistError> {
        if !Self::is_enabled() {
            return Ok(());
        }

        // Validate up-front so hostile ids never reach the filesystem,
        // including the empty-trace deletion branch below
        let file_path = Self::session_file_path(session_id)?;

        // Don't keep empty trace files around — delete if one already exists
        if trace.is_empty() {
            if file_path.exists() {
                let _ = fs::remove_file(&file_path);
            }
            return Ok(());
        }

        let folder = Self::folder();

        // Create directory if it doesn't exist
        if let Err(e) = fs::create_dir_all(&folder) {
            error!("Failed to create session directory: {}", e);
            return Err(e.into());
        }

        // Load existing data to preserve created_at and name, or create new
        let (created_at, updated_at, name) = if file_path.exists() {
            match fs::read_to_string(&file_path) {
                Ok(content) => match serde_json::from_str::<SessionData>(&content) {
                    Ok(existing) => (existing.created_at, Utc::now(), existing.name),
                    Err(_) => (Utc::now(), Utc::now(), None),
                },
                Err(_) => (Utc::now(), Utc::now(), None),
            }
        } else {
            // For new sessions, derive a title from the first user message,
            // falling back to a random name if none found.
            let name =
                generate_session_name_from_trace(&trace).unwrap_or_else(generate_session_name);
            (Utc::now(), Utc::now(), Some(name))
        };

        let session_data = SessionData {
            session_id: session_id.to_string(),
            name,
            created_at,
            updated_at,
            trace,
        };

        // Serialize to JSON
        let json = serde_json::to_string_pretty(&session_data)?;

        // Atomic write: write to temp file, then rename
        let temp_path = folder.join(format!("{}.tmp", Uuid::new_v4()));
        fs::write(&temp_path, json)?;
        fs::rename(&temp_path, &file_path)?;

        debug!("Session saved to disk: {}", file_path.display());
        Ok(())
    }

    /// Load a single session from disk by session_id
    /// Returns the session data if found, or an error if not found or failed to load
    pub fn load_session(session_id: &str) -> Result<SessionData, PersistError> {
        if !Self::is_enabled() {
            return Err(io::Error::other("Session persistence is not enabled").into());
        }

        let file_path = Self::session_file_path(session_id)?;

        // If file doesn't exist, return error
        if !file_path.exists() {
            debug!("Session file does not exist: {}", file_path.display());
            return Err(io::Error::new(
                ErrorKind::NotFound,
                format!("Session file not found: {}", session_id),
            )
            .into());
        }

        // Read and parse the session file
        let content = fs::read_to_string(&file_path)?;
        let session_data: SessionData = serde_json::from_str(&content)?;

        debug!("Loaded session from disk: {}", session_id);
        Ok(session_data)
    }

    /// Delete a session file from disk
    pub fn delete_session(session_id: &str) {
        if !Self::is_enabled() {
            return;
        }

        let Ok(file_path) = Self::session_file_path(session_id) else {
            debug!("Refusing to delete session with invalid id");
            return;
        };

        if file_path.exists() {
            match fs::remove_file(&file_path) {
                Ok(_) => debug!("Deleted session file: {}", file_path.display()),
                Err(e) => error!("Failed to delete session file {:?}: {}", file_path, e),
            }
        }
    }

    /// List all saved sessions IDs from disk
    pub fn list_sessions() -> Result<Vec<SessionData>, PersistError> {
        if !Self::is_enabled() {
            return Err(io::Error::other("Session persistence is not enabled").into());
        }

        let folder = Self::folder();
        if !folder.exists() {
            return Ok(Vec::new());
        }

        let mut sessions = Vec::new();
        for entry in fs::read_dir(&folder)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                match fs::read_to_string(&path) {
                    Ok(content) => {
                        if let Ok(session_data) = serde_json::from_str::<SessionData>(&content) {
                            sessions.push(session_data);
                        }
                    }
                    Err(e) => {
                        error!("Failed to read session file {:?}: {}", path, e);
                    }
                }
            }
        }

        // Filter out sessions with empty traces
        let mut sessions: Vec<SessionData> = sessions
            .into_iter()
            .filter(|s| !s.trace.is_empty())
            .collect();

        // Sort by updated_at descending (most recent first)
        sessions.sort_by_key(|b| std::cmp::Reverse(b.updated_at));
        Ok(sessions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_session_id_accepts_generated_formats() {
        assert!(validate_session_id(&Uuid::new_v4().to_string()).is_ok());
        assert!(validate_session_id(&format!("resp_{}", Uuid::new_v4())).is_ok());
        assert!(validate_session_id("abc-DEF_123").is_ok());
    }

    #[test]
    fn test_validate_session_id_rejects_hostile_ids() {
        let too_long = "x".repeat(MAX_SESSION_ID_LEN + 1);
        for id in [
            "../evil",
            "..",
            ".",
            "",
            "a/b",
            "a\\b",
            "foo.bar",
            "foo:bar",
            "id with space",
            &too_long,
        ] {
            assert!(validate_session_id(id).is_err(), "should reject {:?}", id);
        }
    }

    // Single test touches SHAI_SESSION_PERSIST_FOLDER to avoid env-var races
    // between parallel tests.
    #[test]
    fn test_session_persist_roundtrip_and_traversal() {
        let temp = tempfile::TempDir::new().unwrap();
        let prev_folder = std::env::var("SHAI_SESSION_PERSIST_FOLDER").ok();
        let prev_enable = std::env::var("SHAI_SESSION_PERSIST_ENABLE").ok();
        std::env::set_var("SHAI_SESSION_PERSIST_FOLDER", temp.path());
        std::env::set_var("SHAI_SESSION_PERSIST_ENABLE", "true");

        let id = Uuid::new_v4().to_string();
        let trace = vec![ChatMessage::User {
            content: openai_dive::v1::resources::chat::ChatMessageContent::Text(
                "hello".to_string(),
            ),
            name: None,
        }];

        // save -> load roundtrip
        SessionPersist::save_session(&id, trace.clone()).unwrap();
        let loaded = SessionPersist::load_session(&id).unwrap();
        assert_eq!(loaded.session_id, id);
        assert_eq!(loaded.trace.len(), 1);

        // delete removes the session
        SessionPersist::delete_session(&id);
        assert!(SessionPersist::load_session(&id).is_err());

        // traversal attempts are rejected and never touch the filesystem
        let outside = temp.path().parent().unwrap().join("evil.json");
        assert!(SessionPersist::save_session("../evil", trace.clone()).is_err());
        assert!(!outside.exists());
        assert!(!temp.path().join("evil.json").exists());
        assert!(SessionPersist::load_session("../../etc/passwd").is_err());
        SessionPersist::delete_session("../evil"); // no-op, must not panic

        // empty-trace save with a hostile id must not delete anything either
        assert!(SessionPersist::save_session("../evil", Vec::new()).is_err());

        // Cleanup
        match prev_folder {
            Some(v) => std::env::set_var("SHAI_SESSION_PERSIST_FOLDER", v),
            None => std::env::remove_var("SHAI_SESSION_PERSIST_FOLDER"),
        }
        match prev_enable {
            Some(v) => std::env::set_var("SHAI_SESSION_PERSIST_ENABLE", v),
            None => std::env::remove_var("SHAI_SESSION_PERSIST_ENABLE"),
        }
    }
}
