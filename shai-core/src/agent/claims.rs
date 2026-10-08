use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Match strategy for permission checking
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum MatchStrategy {
    /// Exact match - entire JSON must match exactly
    #[default]
    Exact,
    /// Partial match - permission fields must exist and match in tool call
    Partial,
    /// Glob match - each permission field is a regex pattern
    Glob,
}

/// A Permission represents a granted permission for a specific tool and parameter pattern
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Permission {
    pub tool_name: String,
    pub match_strategy: MatchStrategy,
    pub parameters: serde_json::Value,
    pub granted_at: DateTime<Utc>,
    pub session_only: bool,
    pub description: Option<String>,
}

impl Permission {
    pub fn new(
        tool_name: String,
        match_strategy: MatchStrategy,
        parameters: serde_json::Value,
        session_only: bool,
    ) -> Self {
        Self {
            tool_name,
            match_strategy,
            parameters,
            granted_at: Utc::now(),
            session_only,
            description: None,
        }
    }

    /// Add a permission

    pub fn with_description(mut self, description: String) -> Self {
        self.description = Some(description);
        self
    }

    /// Check if this permission matches the given tool call parameters
    pub fn matches(&self, tool_name: &str, call_params: &serde_json::Value) -> bool {
        if self.tool_name != tool_name {
            return false;
        }

        match self.match_strategy {
            MatchStrategy::Exact => self.matches_exact(call_params),
            MatchStrategy::Partial => self.matches_partial(call_params),
            MatchStrategy::Glob => self.matches_glob(call_params),
        }
    }

    fn matches_exact(&self, call_params: &serde_json::Value) -> bool {
        &self.parameters == call_params
    }

    fn matches_partial(&self, call_params: &serde_json::Value) -> bool {
        let Some(perm_obj) = self.parameters.as_object() else {
            return false;
        };
        let Some(call_obj) = call_params.as_object() else {
            return false;
        };

        for (key, perm_value) in perm_obj {
            match call_obj.get(key) {
                Some(call_value) if call_value == perm_value => continue,
                _ => return false,
            }
        }
        true
    }

    fn matches_glob(&self, call_params: &serde_json::Value) -> bool {
        let Some(perm_obj) = self.parameters.as_object() else {
            return false;
        };
        let Some(call_obj) = call_params.as_object() else {
            return false;
        };

        for (key, perm_pattern) in perm_obj {
            let Some(perm_pattern_str) = perm_pattern.as_str() else {
                continue;
            };

            let Ok(regex) = Regex::new(perm_pattern_str) else {
                continue;
            };

            match call_obj.get(key) {
                Some(call_value) => {
                    let call_str = match call_value {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    if !regex.is_match(&call_str) {
                        return false;
                    }
                }
                None => return false,
            }
        }
        true
    }
}

/// Permission Manager for storing and retrieving permissions
#[derive(Debug, Clone)]
pub struct ClaimManager {
    permissions: Vec<Permission>,
    config_file: Option<PathBuf>,
    sudo_mode: bool,
    deny_all: bool,
    active_prompts: Vec<String>,
}

impl ClaimManager {
    /// Create a new empty permission manager
    pub fn new() -> Self {
        Self {
            permissions: Vec::new(),
            config_file: None,
            sudo_mode: false,
            deny_all: false,
            active_prompts: Vec::new(),
        }
    }

    /// Create a permission manager that loads/saves from a file
    pub fn with_config_file(path: PathBuf) -> Self {
        Self {
            permissions: Vec::new(),
            config_file: Some(path),
            sudo_mode: false,
            deny_all: false,
            active_prompts: Vec::new(),
        }
    }

    /// Create a permission manager with sudo mode enabled
    pub fn with_sudo() -> Self {
        Self {
            permissions: Vec::new(),
            config_file: None,
            sudo_mode: true,
            deny_all: false,
            active_prompts: Vec::new(),
        }
    }

    /// Create a permission manager with config file and sudo mode
    pub fn with_config_file_and_sudo(path: PathBuf) -> Self {
        Self {
            permissions: Vec::new(),
            config_file: Some(path),
            sudo_mode: true,
            deny_all: false,
            active_prompts: Vec::new(),
        }
    }

    /// Enable sudo mode - bypasses all permission checks
    pub fn sudo(&mut self) {
        self.sudo_mode = true;
    }

    /// Disable sudo mode - re-enables permission checks
    pub fn no_sudo(&mut self) {
        self.sudo_mode = false;
    }

    /// Enable plan mode - denies all tool execution (read-only)
    pub fn plan_mode(&mut self) {
        self.deny_all = true;
    }

    /// Disable plan mode - re-enables tool execution
    pub fn no_plan_mode(&mut self) {
        self.deny_all = false;
    }

    /// Check if plan mode is enabled
    pub fn is_plan_mode(&self) -> bool {
        self.deny_all
    }

    /// Set active system prompts
    pub fn set_active_prompts(&mut self, prompts: Vec<String>) {
        self.active_prompts = prompts;
    }

    /// Get active system prompts
    pub fn active_prompts(&self) -> &[String] {
        &self.active_prompts
    }

    /// Check if sudo mode is enabled
    pub fn is_sudo(&self) -> bool {
        self.sudo_mode
    }

    /// Add a permission
    pub fn add_permission(&mut self, permission: Permission) {
        self.permissions.push(permission);
    }

    /// Check if a tool call is permitted
    pub fn is_permitted(&self, tool_name: &str, parameters: &serde_json::Value) -> bool {
        // Sudo mode bypasses all permission checks
        if self.sudo_mode {
            return true;
        }

        // Plan mode denies all tool execution
        if self.deny_all {
            return false;
        }

        self.permissions
            .iter()
            .any(|perm| perm.matches(tool_name, parameters))
    }

    /// Get all permissions for a specific tool
    pub fn get_permissions_for_tool(&self, tool_name: &str) -> Vec<&Permission> {
        self.permissions
            .iter()
            .filter(|perm| perm.tool_name == tool_name)
            .collect()
    }

    /// Remove session-only permissions (called when session ends)
    pub fn clear_session_permissions(&mut self) {
        self.permissions.retain(|perm| !perm.session_only);
    }

    /// Clear all permissions
    pub fn clear(&mut self) {
        self.permissions.clear();
    }

    /// Get number of permissions
    pub fn len(&self) -> usize {
        self.permissions.len()
    }

    /// Check if manager has no permissions
    pub fn is_empty(&self) -> bool {
        self.permissions.is_empty()
    }

    /// Get all permissions (for debugging/inspection)
    pub fn get_all_permissions(&self) -> &[Permission] {
        &self.permissions
    }

    /// Save permissions to JSON file (if config file is set)
    pub fn save_to_file(&self) -> Result<(), PermissionError> {
        if let Some(path) = &self.config_file {
            let persistent_permissions: Vec<&Permission> = self
                .permissions
                .iter()
                .filter(|perm| !perm.session_only)
                .collect();

            let json_str = serde_json::to_string_pretty(&persistent_permissions)
                .map_err(PermissionError::Serialization)?;

            std::fs::write(path, json_str).map_err(PermissionError::FileAccess)?;

            Ok(())
        } else {
            Err(PermissionError::NoConfigFile)
        }
    }

    /// Load permissions from JSON file (if config file is set)
    pub fn load_from_file(&mut self) -> Result<(), PermissionError> {
        if let Some(path) = &self.config_file {
            if !path.exists() {
                return Ok(()); // No file means no permissions, which is fine
            }

            let json_str = std::fs::read_to_string(path).map_err(PermissionError::FileAccess)?;

            let loaded_permissions: Vec<Permission> =
                serde_json::from_str(&json_str).map_err(PermissionError::Serialization)?;

            // Only load non-session permissions from file
            for permission in loaded_permissions {
                if !permission.session_only {
                    self.permissions.push(permission);
                }
            }

            Ok(())
        } else {
            Err(PermissionError::NoConfigFile)
        }
    }
}

impl Default for ClaimManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Errors that can occur in permission management
#[derive(Debug, thiserror::Error)]
pub enum PermissionError {
    #[error("Permission validation failed: {0}")]
    ValidationFailed(String),

    #[error("Tool '{0}' not found")]
    ToolNotFound(String),

    #[error("Permission type mismatch for tool '{0}'")]
    TypeMismatch(String),

    #[error("No config file specified")]
    NoConfigFile,

    #[error("File access error: {0}")]
    FileAccess(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Deserialization error: unknown permission type '{0}'")]
    UnknownPermissionType(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permission_manager_creation() {
        let manager = ClaimManager::new();
        assert!(manager.is_empty());
        assert_eq!(manager.len(), 0);
        // assert!(manager.get_restricted_tools().is_empty());
    }

    #[test]
    fn test_permission_manager_with_config_file() {
        let path = PathBuf::from("/tmp/test_permissions.json");
        let manager = ClaimManager::with_config_file(path.clone());
        assert_eq!(manager.config_file, Some(path));
    }

    #[test]
    fn test_permission_manager_operations() {
        let mut manager = ClaimManager::new();

        // Test empty state
        assert!(!manager.is_permitted("bash", &serde_json::json!({})));
        assert!(manager.get_permissions_for_tool("bash").is_empty());

        // Test clear
        manager.clear();
        assert!(manager.is_empty());
    }

    #[test]
    fn test_exact_match() {
        let permission = Permission::new(
            "test_tool".to_string(),
            MatchStrategy::Exact,
            serde_json::json!({"path": "/home/user/file.txt", "action": "read"}),
            false,
        );

        // Should match exact parameters
        assert!(permission.matches(
            "test_tool",
            &serde_json::json!({
                "path": "/home/user/file.txt",
                "action": "read"
            })
        ));

        // Should not match different parameters
        assert!(!permission.matches(
            "test_tool",
            &serde_json::json!({
                "path": "/home/user/other.txt",
                "action": "read"
            })
        ));

        // Should not match different tool
        assert!(!permission.matches(
            "other_tool",
            &serde_json::json!({
                "path": "/home/user/file.txt",
                "action": "read"
            })
        ));
    }

    #[test]
    fn test_partial_match() {
        let permission = Permission::new(
            "test_tool".to_string(),
            MatchStrategy::Partial,
            serde_json::json!({"action": "read"}),
            false,
        );

        // Should match when required fields are present
        assert!(permission.matches(
            "test_tool",
            &serde_json::json!({
                "path": "/home/user/file.txt",
                "action": "read",
                "extra": "ignored"
            })
        ));

        // Should not match when required field has different value
        assert!(!permission.matches(
            "test_tool",
            &serde_json::json!({
                "path": "/home/user/file.txt",
                "action": "write"
            })
        ));

        // Should not match when required field is missing
        assert!(!permission.matches(
            "test_tool",
            &serde_json::json!({
                "path": "/home/user/file.txt"
            })
        ));
    }

    #[test]
    fn test_glob_match() {
        let permission = Permission::new(
            "test_tool".to_string(),
            MatchStrategy::Glob,
            serde_json::json!({"path": r"/home/user/.*\.txt"}),
            false,
        );

        // Should match regex pattern
        assert!(permission.matches(
            "test_tool",
            &serde_json::json!({
                "path": "/home/user/file.txt"
            })
        ));

        assert!(permission.matches(
            "test_tool",
            &serde_json::json!({
                "path": "/home/user/another.txt"
            })
        ));

        // Should not match non-matching pattern
        assert!(!permission.matches(
            "test_tool",
            &serde_json::json!({
                "path": "/home/user/file.doc"
            })
        ));

        assert!(!permission.matches(
            "test_tool",
            &serde_json::json!({
                "path": "/other/user/file.txt"
            })
        ));
    }

    #[test]
    fn test_claim_manager_permission_checking() {
        let mut manager = ClaimManager::new();

        // Add exact match permission
        manager.add_permission(Permission::new(
            "file_tool".to_string(),
            MatchStrategy::Exact,
            serde_json::json!({"path": "/home/user/allowed.txt", "action": "read"}),
            false,
        ));

        // Add partial match permission
        manager.add_permission(Permission::new(
            "file_tool".to_string(),
            MatchStrategy::Partial,
            serde_json::json!({"action": "write"}),
            true, // session only
        ));

        // Test exact match works
        assert!(manager.is_permitted(
            "file_tool",
            &serde_json::json!({
                "path": "/home/user/allowed.txt",
                "action": "read"
            })
        ));

        // Test partial match works
        assert!(manager.is_permitted(
            "file_tool",
            &serde_json::json!({
                "path": "/any/path.txt",
                "action": "write",
                "extra": "data"
            })
        ));

        // Test no permission case
        assert!(!manager.is_permitted(
            "file_tool",
            &serde_json::json!({
                "path": "/forbidden/file.txt",
                "action": "delete"
            })
        ));

        // Test session cleanup
        assert_eq!(manager.len(), 2);
        manager.clear_session_permissions();
        assert_eq!(manager.len(), 1); // Only persistent permission remains
    }

    #[test]
    fn test_sudo_mode() {
        let mut manager = ClaimManager::new();

        // Initially no sudo mode
        assert!(!manager.is_sudo());

        // Without permissions, should deny
        assert!(!manager.is_permitted(
            "any_tool",
            &serde_json::json!({
                "any": "parameters"
            })
        ));

        // Enable sudo mode
        manager.sudo();
        assert!(manager.is_sudo());

        // With sudo mode, should allow everything
        assert!(manager.is_permitted(
            "any_tool",
            &serde_json::json!({
                "any": "parameters"
            })
        ));

        assert!(manager.is_permitted(
            "another_tool",
            &serde_json::json!({
                "dangerous": "action"
            })
        ));

        // Disable sudo mode
        manager.no_sudo();
        assert!(!manager.is_sudo());

        // Back to normal permission checking
        assert!(!manager.is_permitted(
            "any_tool",
            &serde_json::json!({
                "any": "parameters"
            })
        ));
    }

    #[test]
    fn test_builder_patterns() {
        // Test normal creation
        let manager = ClaimManager::new();
        assert!(!manager.is_sudo());
        assert!(manager.config_file.is_none());

        // Test with config file
        let path = PathBuf::from("/tmp/test.json");
        let manager = ClaimManager::with_config_file(path.clone());
        assert!(!manager.is_sudo());
        assert_eq!(manager.config_file, Some(path));

        // Test with sudo
        let manager = ClaimManager::with_sudo();
        assert!(manager.is_sudo());
        assert!(manager.config_file.is_none());

        // Test with both config file and sudo
        let path = PathBuf::from("/tmp/test2.json");
        let manager = ClaimManager::with_config_file_and_sudo(path.clone());
        assert!(manager.is_sudo());
        assert_eq!(manager.config_file, Some(path));
    }

    #[test]
    fn test_permission_manager_clone() {
        let manager = ClaimManager::new();
        let cloned = manager.clone();
        assert_eq!(manager.len(), cloned.len());
        assert_eq!(manager.config_file, cloned.config_file);
    }
}
