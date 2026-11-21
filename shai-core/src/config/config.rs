use crate::tools::mcp::McpConfig;
use json_comments::StripComments;
use reqwest::Url;
use serde::{Deserialize, Deserializer, Serialize};
use shai_llm::{LlmClient, ToolCallMethod};
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{collections::HashMap, path::PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub provider: String,
    pub env_vars: std::collections::HashMap<String, String>,
    pub model: String,
    pub tool_method: ToolCallMethod,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShaiConfig {
    pub providers: Vec<ProviderConfig>,
    pub selected_provider: usize,
    #[serde(default)]
    pub mcp_configs: HashMap<String, McpConfig>,
    /// How to handle a missing config or an anonymous OVH provider.
    /// Unset means the built-in default (`prompt`). Stored as a raw string so
    /// an unrecognized value can be rejected after load instead of failing
    /// the rest of the file (or silently becoming the default).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_fallback"
    )]
    pub anonymous_fallback: Option<String>,
}

fn deserialize_optional_fallback<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    // A missing key stays `None` via `#[serde(default)]`. An explicit JSON
    // `null` is malformed: store it as the token "null" so resolution fails
    // closed to `deny` instead of treating it as "unset, use prompt".
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(Some(match value {
        serde_json::Value::String(s) => s,
        other => other.to_string(),
    }))
}

impl ShaiConfig {
    pub async fn pull_from_url(url: Url) -> Result<Self, Box<dyn std::error::Error>> {
        let response = reqwest::get(url).await?;
        let config_json = response.text().await?;
        let config: ShaiConfig = serde_json::from_str(&config_json)?;
        Ok(config)
    }

    pub fn add_provider(
        &mut self,
        provider: String,
        env_vars: std::collections::HashMap<String, String>,
        model: String,
    ) -> usize {
        let provider_config = ProviderConfig {
            provider,
            env_vars,
            model,
            tool_method: ToolCallMethod::FunctionCall,
        };

        self.providers.push(provider_config);
        self.providers.len() - 1
    }

    pub fn is_duplicate_config(
        &self,
        provider_name: &str,
        env_vars: &std::collections::HashMap<String, String>,
        model: &str,
    ) -> bool {
        self.providers.iter().any(|provider_config| {
            provider_config.provider == provider_name
                && provider_config.env_vars == *env_vars
                && provider_config.model.eq(model)
        })
    }

    pub fn get_selected_provider(&self) -> Option<&ProviderConfig> {
        self.providers.get(self.selected_provider)
    }

    pub fn get_selected_provider_mut(&mut self) -> Option<&mut ProviderConfig> {
        self.providers.get_mut(self.selected_provider)
    }

    pub fn set_selected_provider(&mut self, index: usize) -> Result<(), String> {
        if index < self.providers.len() {
            self.selected_provider = index;
            Ok(())
        } else {
            Err(format!(
                "Provider index {} out of bounds (have {} providers)",
                index,
                self.providers.len()
            ))
        }
    }

    pub fn config_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
        let config_dir = std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|_| {
                dirs::home_dir()
                    .map(|home| home.join(".config"))
                    .ok_or("Could not find home directory")
            })?;

        let shai_config_dir = config_dir.join("shai");
        std::fs::create_dir_all(&shai_config_dir)?;

        Ok(shai_config_dir.join("auth.config.json"))
    }

    fn legacy_config_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
        let config_dir = std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|_| {
                dirs::home_dir()
                    .map(|home| home.join(".config"))
                    .ok_or("Could not find home directory")
            })?;

        Ok(config_dir.join("shai").join("auth.config"))
    }

    pub fn load() -> Result<ShaiConfig, Box<dyn std::error::Error>> {
        Ok(Self::try_load()?)
    }

    fn validate_provider(&mut self) {
        if self.providers.is_empty() || self.selected_provider >= self.providers.len() {
            self.selected_provider = 0;
        }
    }

    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let config_path = Self::config_path()?;
        let content = serde_json::to_string_pretty(self)?;
        fs::write(&config_path, content)?;

        // Set file permissions to 600 (user read/write only) on Unix systems
        #[cfg(unix)]
        {
            let mut perms = fs::metadata(&config_path)?.permissions();
            perms.set_mode(0o600);
            fs::set_permissions(&config_path, perms)?;
        }

        Ok(())
    }

    pub fn exists() -> bool {
        Self::config_path()
            .map(|path| path.exists())
            .unwrap_or(false)
    }

    /// Load the config, falling back to the default one.
    ///
    /// Returns a warning message only when a config file exists but could not
    /// be loaded; a missing file is the normal first-run case.
    pub fn load_or_default() -> (ShaiConfig, Option<String>) {
        match Self::load() {
            Ok(config) => (config, None),
            Err(e) => {
                let warning =
                    Self::exists().then(|| format!("Failed to load config, using default: {}", e));
                (ShaiConfig::default(), warning)
            }
        }
    }

    /// Set environment variables from the currently selected provider
    pub fn set_env_vars(&self) {
        if let Some(provider_config) = self.get_selected_provider() {
            for (name, value) in &provider_config.env_vars {
                std::env::set_var(name, value);
            }
            // Set model-specific environment variable if model is specified
            std::env::set_var("SHAI_MODEL", &provider_config.model);
            // Set provider-specific environment variable
            std::env::set_var("SHAI_PROVIDER", &provider_config.provider);
        }
    }

    pub fn remove_provider(&mut self, index: usize) -> Result<ProviderConfig, String> {
        if index >= self.providers.len() {
            return Err(format!(
                "Provider index {} out of bounds (have {} providers)",
                index,
                self.providers.len()
            ));
        }

        if self.providers.len() == 1 {
            return Err("Cannot remove the last provider".to_string());
        }

        let removed = self.providers.remove(index);

        // Adjust selected_provider if needed
        if self.selected_provider >= self.providers.len() {
            self.selected_provider = self.providers.len() - 1;
        } else if self.selected_provider > index {
            self.selected_provider -= 1;
        }

        Ok(removed)
    }

    pub fn list_providers(&self) -> Vec<(usize, &str, &str)> {
        self.providers
            .iter()
            .enumerate()
            .map(|(i, config)| (i, config.provider.as_str(), config.model.as_str()))
            .collect()
    }

    pub fn find_providers_by_type(&self, provider_type: &str) -> Vec<usize> {
        self.providers
            .iter()
            .enumerate()
            .filter_map(|(i, config)| {
                if config.provider == provider_type {
                    Some(i)
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn add_mcp_config(&mut self, name: String, config: McpConfig) -> Option<McpConfig> {
        self.mcp_configs.insert(name, config)
    }

    pub fn remove_mcp_config(&mut self, name: &str) -> Option<McpConfig> {
        self.mcp_configs.remove(name)
    }

    pub fn get_mcp_config(&self, name: &str) -> Option<&McpConfig> {
        self.mcp_configs.get(name)
    }

    pub fn list_mcp_configs(&self) -> Vec<(String, String)> {
        self.mcp_configs
            .iter()
            .map(|(name, config)| {
                let description = match config {
                    McpConfig::Stdio { command, .. } => format!("stdio: {}", command),
                    McpConfig::Http { url, auth } => {
                        if auth.is_some() {
                            format!("http: {} (authenticated)", url)
                        } else {
                            format!("http: {}", url)
                        }
                    }
                    McpConfig::Sse { url } => format!("sse: {}", url),
                };
                (name.clone(), description)
            })
            .collect()
    }

    pub fn has_mcp_config(&self, name: &str) -> bool {
        self.mcp_configs.contains_key(name)
    }

    /// True when the selected provider is OVHcloud with no API key in the
    /// config or in the `OVH_API_KEY` environment variable.
    pub fn selected_is_anonymous_ovh(&self) -> bool {
        let Some(provider) = self.get_selected_provider() else {
            return false;
        };
        if provider.provider != "ovhcloud" {
            return false;
        }
        if provider_has_api_key(provider) {
            return false;
        }
        !matches!(std::env::var("OVH_API_KEY"), Ok(value) if !value.trim().is_empty())
    }

    /// Load the auth config, distinguishing a missing file from one that is
    /// there but cannot be read or parsed.
    pub fn try_load() -> Result<ShaiConfig, ConfigLoadError> {
        let config_path = Self::config_path().map_err(|e| ConfigLoadError::Invalid {
            path: "auth.config.json".to_string(),
            reason: e.to_string(),
        })?;
        let legacy_path = Self::legacy_config_path().map_err(|e| ConfigLoadError::Invalid {
            path: "auth.config".to_string(),
            reason: e.to_string(),
        })?;
        Self::try_load_from(&config_path, &legacy_path)
    }

    /// [`try_load`] from the two paths a config file may occupy.
    pub fn try_load_from(
        config_path: &std::path::Path,
        legacy_path: &std::path::Path,
    ) -> Result<ShaiConfig, ConfigLoadError> {
        for path in [config_path, legacy_path] {
            match fs::symlink_metadata(path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(ConfigLoadError::Invalid {
                        path: path.display().to_string(),
                        reason: format!("read error: {}", error.kind()),
                    });
                }
                Ok(_) => return Self::load_from_existing_path(path),
            }
        }
        Err(ConfigLoadError::Missing)
    }

    fn load_from_existing_path(path: &std::path::Path) -> Result<ShaiConfig, ConfigLoadError> {
        let invalid = |reason: String| ConfigLoadError::Invalid {
            path: path.display().to_string(),
            reason,
        };
        let content_bytes =
            fs::read(path).map_err(|e| invalid(format!("read error: {}", e.kind())))?;
        let content_stripped = StripComments::new(&content_bytes[..]);
        match serde_json::from_reader::<_, ShaiConfig>(content_stripped) {
            Ok(mut config) => {
                Self::validate_provider(&mut config);
                Ok(config)
            }
            Err(error) => Err(invalid(describe_load_error(&error))),
        }
    }

    /// The config the primary model runs on, honouring `anonymous_fallback`.
    ///
    /// A missing file falls back to the built-in default only in `prompt`
    /// mode. `deny`, an invalid setting, or a file that exists but cannot be
    /// read or parsed is an error, never a silent fallback to anonymous OVH.
    pub fn primary() -> Result<ShaiConfig, PrimaryConfigError> {
        let env_setting = crate::config::anonymous_fallback::env_anonymous_fallback();
        Self::primary_from_with_env(Self::try_load(), env_setting.as_deref())
    }

    /// [`primary`] with an explicit environment override, for tests.
    pub fn primary_from_with_env(
        config: Result<ShaiConfig, ConfigLoadError>,
        env_setting: Option<&str>,
    ) -> Result<ShaiConfig, PrimaryConfigError> {
        use crate::config::anonymous_fallback::{
            deny_message, resolve_anonymous_fallback, AnonymousFallbackMode, FallbackSituation,
        };

        match config {
            Ok(config) => {
                let resolution =
                    resolve_anonymous_fallback(config.anonymous_fallback.as_deref(), env_setting);
                if let Some(message) = resolution.invalid {
                    return Err(PrimaryConfigError::Other(message));
                }
                if resolution.mode == AnonymousFallbackMode::Deny
                    && config.selected_is_anonymous_ovh()
                {
                    return Err(PrimaryConfigError::Other(deny_message(
                        FallbackSituation::AnonymousOvh,
                    )));
                }
                Ok(config)
            }
            Err(ConfigLoadError::Missing) => {
                let resolution = resolve_anonymous_fallback(None, env_setting);
                if let Some(message) = resolution.invalid {
                    return Err(PrimaryConfigError::Other(message));
                }
                match resolution.mode {
                    AnonymousFallbackMode::Deny => Err(PrimaryConfigError::Missing),
                    AnonymousFallbackMode::Prompt => Ok(ShaiConfig::default()),
                }
            }
            Err(ConfigLoadError::Invalid { path, reason }) => {
                Err(PrimaryConfigError::Invalid { path, reason })
            }
        }
    }
}

/// Why the auth config file could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigLoadError {
    /// There is no auth config file (and no legacy file).
    #[error("config file does not exist")]
    Missing,
    /// The file is there but could not be read or parsed. `reason` never
    /// repeats a value from the file.
    #[error("the config file {path} could not be loaded ({reason})")]
    Invalid { path: String, reason: String },
}

/// Why [`ShaiConfig::primary`] could not load or allow a config.
///
/// Every variant is a refused start: front-ends map this to process exit code 2.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PrimaryConfigError {
    /// There is no config file and `anonymous_fallback` is `deny`.
    #[error("No config found (no 3rd-party API configured). Anonymous fallback is disabled. Run `shai auth` to configure a provider.")]
    Missing,
    /// The config file exists but could not be read or parsed.
    #[error("the config file {path} could not be loaded ({reason})")]
    Invalid { path: String, reason: String },
    /// The setting refused startup (for example `anonymous_fallback: deny` with
    /// anonymous OVH, or an invalid `anonymous_fallback` value).
    #[error("{0}")]
    Other(String),
}

/// Describe a load failure without quoting the file's contents. Serde's own
/// message can include a value from the file (credentials, for example).
fn describe_load_error(error: &serde_json::Error) -> String {
    let kind = match error.classify() {
        serde_json::error::Category::Syntax | serde_json::error::Category::Eof => {
            "JSON syntax error"
        }
        serde_json::error::Category::Data => "unexpected value",
        serde_json::error::Category::Io => "read error",
    };
    format!(
        "{} at line {} column {}",
        kind,
        error.line(),
        error.column()
    )
}

/// Whether an API key is set on the provider. Only the presence of a
/// non-empty `*_API_KEY` value is checked, never the value itself.
pub fn provider_has_api_key(provider: &ProviderConfig) -> bool {
    provider
        .env_vars
        .iter()
        .any(|(name, value)| name.ends_with("_API_KEY") && !value.trim().is_empty())
}

impl Default for ShaiConfig {
    fn default() -> Self {
        Self {
            // default to ovhcloud qwen3 in anonymous mode
            providers: vec![ProviderConfig {
                provider: "ovhcloud".to_string(),
                env_vars: HashMap::from([(
                    String::from("OVH_BASE_URL"),
                    String::from(
                        "https://qwen-3-32b.endpoints.kepler.ai.cloud.ovh.net/api/openai_compat/v1",
                    ),
                )]),
                model: "Qwen3-32B".to_string(),
                tool_method: ToolCallMethod::FunctionCall,
            }],
            selected_provider: 0,
            mcp_configs: HashMap::new(),
            anonymous_fallback: None,
        }
    }
}

impl ShaiConfig {
    pub async fn get_llm() -> Result<(LlmClient, String), Box<dyn std::error::Error>> {
        let config = ShaiConfig::primary()?;

        config.set_env_vars();

        let llm = if let Some(provider_config) = config.get_selected_provider() {
            LlmClient::create_provider(&provider_config.provider, &provider_config.env_vars)
                .map_err(|e| {
                    format!(
                        "Failed to create {} client: {}",
                        provider_config.provider, e
                    )
                })?
        } else {
            return Err("No provider configured".into());
        };

        let model = llm
            .default_model()
            .await
            .map_err(|_| "no Model available")?;
        Ok((llm, model))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::anonymous_fallback::{
        resolve_anonymous_fallback, AnonymousFallbackMode, AnonymousFallbackSource,
    };

    #[test]
    fn missing_anonymous_fallback_key_is_unset() {
        let json = r#"{
            "providers": [],
            "selected_provider": 0
        }"#;
        let config: ShaiConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.anonymous_fallback, None);
        let resolution = resolve_anonymous_fallback(config.anonymous_fallback.as_deref(), None);
        assert_eq!(resolution.mode, AnonymousFallbackMode::Prompt);
        assert_eq!(resolution.source, AnonymousFallbackSource::Default);
        assert!(resolution.invalid.is_none());
    }

    #[test]
    fn anonymous_fallback_key_round_trip_and_invalid() {
        let json = r#"{
            "providers": [],
            "selected_provider": 0,
            "anonymous_fallback": "deny"
        }"#;
        let config: ShaiConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.anonymous_fallback.as_deref(), Some("deny"));
        let out = serde_json::to_value(&config).unwrap();
        assert_eq!(out["anonymous_fallback"], "deny");

        let invalid: ShaiConfig = serde_json::from_str(
            r#"{
            "providers": [],
            "selected_provider": 0,
            "anonymous_fallback": "allow"
        }"#,
        )
        .unwrap();
        let resolution = resolve_anonymous_fallback(invalid.anonymous_fallback.as_deref(), None);
        assert_eq!(resolution.mode, AnonymousFallbackMode::Deny);
        assert!(resolution.invalid.is_some());

        let numeric: ShaiConfig = serde_json::from_str(
            r#"{
            "providers": [],
            "selected_provider": 0,
            "anonymous_fallback": 1
        }"#,
        )
        .unwrap();
        assert_eq!(numeric.anonymous_fallback.as_deref(), Some("1"));
        let resolution = resolve_anonymous_fallback(numeric.anonymous_fallback.as_deref(), None);
        assert_eq!(resolution.mode, AnonymousFallbackMode::Deny);
        assert!(resolution.invalid.is_some());
    }

    #[test]
    fn default_config_is_anonymous_ovh_without_key() {
        let config = ShaiConfig::default();
        assert!(config.selected_is_anonymous_ovh());
        assert_eq!(config.anonymous_fallback, None);
    }

    #[test]
    fn selected_is_anonymous_ovh_false_for_keyed_or_other_providers() {
        let mut keyed = ShaiConfig::default();
        keyed.providers[0]
            .env_vars
            .insert("OVH_API_KEY".into(), "x".into());
        assert!(!keyed.selected_is_anonymous_ovh());

        let mut other = ShaiConfig::default();
        other.providers[0].provider = "openai".into();
        other.providers[0].env_vars.clear();
        assert!(!other.selected_is_anonymous_ovh());
    }

    #[test]
    fn omit_key_when_serializing_default() {
        let json = serde_json::to_value(ShaiConfig::default()).unwrap();
        assert!(json.get("anonymous_fallback").is_none());
    }

    #[test]
    fn explicit_null_anonymous_fallback_is_malformed_and_denies() {
        let config: ShaiConfig = serde_json::from_str(
            r#"{"providers":[],"selected_provider":0,"anonymous_fallback":null}"#,
        )
        .unwrap();
        assert_eq!(config.anonymous_fallback.as_deref(), Some("null"));
        let resolution = resolve_anonymous_fallback(config.anonymous_fallback.as_deref(), None);
        assert_eq!(resolution.mode, AnonymousFallbackMode::Deny);
        assert_eq!(resolution.source, AnonymousFallbackSource::Config);
        assert!(resolution.invalid.is_some());
    }

    #[test]
    fn primary_refuses_an_invalid_file_under_prompt_and_deny() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("auth.config.json");
        let legacy = dir.path().join("auth.config");
        // A value in the wrong place, which serde would quote in its own message.
        std::fs::write(
            &path,
            r#"{"providers":[],"selected_provider":"sk-live-SECRETSECRET"}"#,
        )
        .unwrap();

        for env in [None, Some("prompt"), Some("deny")] {
            let error =
                ShaiConfig::primary_from_with_env(ShaiConfig::try_load_from(&path, &legacy), env)
                    .unwrap_err();
            assert!(
                matches!(error, PrimaryConfigError::Invalid { .. }),
                "env={env:?}: {error:?}"
            );
            let message = error.to_string();
            assert!(message.contains("auth.config.json"), "{message}");
            assert!(message.contains("could not be loaded"), "{message}");
            assert!(message.contains("unexpected value at line 1"), "{message}");
            assert!(!message.contains("SECRET"), "{message}");
            assert!(!message.contains("sk-live"), "{message}");
        }
    }

    #[test]
    fn primary_missing_file_follows_the_anonymous_fallback_setting() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("auth.config.json");
        let legacy = dir.path().join("auth.config");

        let ok = ShaiConfig::primary_from_with_env(ShaiConfig::try_load_from(&path, &legacy), None)
            .unwrap();
        assert!(ok.selected_is_anonymous_ovh());

        let err = ShaiConfig::primary_from_with_env(
            ShaiConfig::try_load_from(&path, &legacy),
            Some("deny"),
        )
        .unwrap_err();
        assert!(matches!(err, PrimaryConfigError::Missing), "{err:?}");
    }
}
