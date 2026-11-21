use super::lenient::Lenient;
use super::prewalk::PrewalkConfig;
use super::roles::RolesConfig;
use crate::tools::mcp::McpConfig;
use json_comments::StripComments;
use reqwest::Url;
use serde::de::{self, IgnoredAny, MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use shai_llm::{LlmClient, ToolCallMethod};
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::{collections::HashMap, path::PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub provider: String,
    pub env_vars: std::collections::HashMap<String, String>,
    pub model: String,
    pub tool_method: ToolCallMethod,
}

#[derive(Debug, Clone)]
pub struct ShaiConfig {
    pub providers: Vec<ProviderConfig>,
    pub selected_provider: usize,
    pub mcp_configs: HashMap<String, McpConfig>,
    /// Named secondary models, see [`crate::config::roles`].
    pub roles: RolesConfig,
    /// One-shot handoff to another model, see [`crate::config::prewalk`].
    pub prewalk: Lenient<PrewalkConfig>,
}

// Reading and writing are spelled out instead of derived for the sake of the
// optional sections (see `super::section`): a section the file gives twice
// has to survive loading and saving as it is, which a derived field cannot
// do. The other keys of the object are read as the derive read them:
// `providers` and `selected_provider` are required, `mcp_configs` is
// optional, none of the three may be repeated, and unknown keys are ignored.
// A config has to be a JSON object: the array of positional values that the
// derive also happened to accept is not read.
impl Serialize for ShaiConfig {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("providers", &self.providers)?;
        map.serialize_entry("selected_provider", &self.selected_provider)?;
        map.serialize_entry("mcp_configs", &self.mcp_configs)?;
        self.roles.section().write("roles", &mut map)?;
        self.prewalk.section().write("prewalk", &mut map)?;
        map.end()
    }
}

impl<'de> Deserialize<'de> for ShaiConfig {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ConfigVisitor;

        /// Read the value of a key that may appear only once.
        fn once<'de, A: MapAccess<'de>, T: Deserialize<'de>>(
            slot: &mut Option<T>,
            field: &'static str,
            map: &mut A,
        ) -> Result<(), A::Error> {
            if slot.is_some() {
                return Err(de::Error::duplicate_field(field));
            }
            *slot = Some(map.next_value()?);
            Ok(())
        }

        impl<'de> Visitor<'de> for ConfigVisitor {
            type Value = ShaiConfig;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a config object")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<ShaiConfig, A::Error> {
                let mut providers = None;
                let mut selected_provider = None;
                let mut mcp_configs = None;
                let mut roles = RolesConfig::default();
                let mut prewalk = Lenient::unset();

                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "providers" => once(&mut providers, "providers", &mut map)?,
                        "selected_provider" => {
                            once(&mut selected_provider, "selected_provider", &mut map)?
                        }
                        "mcp_configs" => once(&mut mcp_configs, "mcp_configs", &mut map)?,
                        // Optional sections: every copy is kept as text.
                        "roles" => roles.section_mut().push(map.next_value()?),
                        "prewalk" => prewalk.section_mut().push(map.next_value()?),
                        _ => {
                            map.next_value::<IgnoredAny>()?;
                        }
                    }
                }

                Ok(ShaiConfig {
                    providers: providers.ok_or_else(|| de::Error::missing_field("providers"))?,
                    selected_provider: selected_provider
                        .ok_or_else(|| de::Error::missing_field("selected_provider"))?,
                    mcp_configs: mcp_configs.unwrap_or_default(),
                    roles,
                    prewalk,
                })
            }
        }

        deserializer.deserialize_map(ConfigVisitor)
    }
}

impl ShaiConfig {
    pub async fn pull_from_url(url: Url) -> Result<Self, Box<dyn std::error::Error>> {
        let response = reqwest::get(url).await?;
        let config_json = response.text().await?;
        Self::from_pulled_json(&config_json)
    }

    /// Parse a config that was downloaded rather than written by the user.
    ///
    /// Sections that send a session's data to additional models are opt-in,
    /// and only the user's own file may opt in: they are dropped here.
    fn from_pulled_json(config_json: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let mut config: ShaiConfig = serde_json::from_str(config_json)?;
        config.roles = RolesConfig::default();
        config.prewalk = Lenient::unset();
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

    pub(crate) fn legacy_config_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
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
        Self::load_from_paths(&Self::config_path()?, &Self::legacy_config_path()?)
    }

    /// Load the config from `config_path`, or from `legacy_path` when the
    /// former does not exist. A missing file is an error, not a default.
    pub(crate) fn load_from_paths(
        config_path: &Path,
        legacy_path: &Path,
    ) -> Result<ShaiConfig, Box<dyn std::error::Error>> {
        for path in [config_path, legacy_path] {
            if path.exists() {
                let content_bytes = fs::read(path)?;
                let content_stripped = StripComments::new(&content_bytes[..]);
                let mut config: ShaiConfig = serde_json::from_reader(content_stripped)?;
                Self::validate_provider(&mut config);
                return Ok(config);
            }
        }

        Err("config file does not exist".into())
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
            roles: RolesConfig::default(),
            prewalk: Lenient::unset(),
        }
    }
}

impl ShaiConfig {
    pub async fn get_llm() -> Result<(LlmClient, String), Box<dyn std::error::Error>> {
        let config = ShaiConfig::load().unwrap_or_else(|e| {
            tracing::warn!("Failed to load config, using default: {}", e);
            ShaiConfig::default()
        });
        config.get_llm_from_config().await
    }

    /// Build the LLM client and default model from this already-loaded config.
    /// Unlike [`ShaiConfig::get_llm`], this never loads the file again and never
    /// falls back to [`ShaiConfig::default`].
    pub async fn get_llm_from_config(
        &self,
    ) -> Result<(LlmClient, String), Box<dyn std::error::Error>> {
        self.set_env_vars();

        let llm = if let Some(provider_config) = self.get_selected_provider() {
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

    #[test]
    fn config_keys_are_read_and_written_as_before_roles_existed() {
        for (json, expected) in [
            (r#"{"selected_provider": 0}"#, "missing field `providers`"),
            (r#"{"providers": []}"#, "missing field `selected_provider`"),
            (
                r#"{"providers": [], "providers": [], "selected_provider": 0}"#,
                "duplicate field `providers`",
            ),
            (
                r#"{"providers": [], "selected_provider": 0, "selected_provider": 1}"#,
                "duplicate field `selected_provider`",
            ),
            (
                r#"{"providers": [], "selected_provider": 0, "mcp_configs": {}, "mcp_configs": {}}"#,
                "duplicate field `mcp_configs`",
            ),
            (
                r#"{"providers": [], "selected_provider": "0"}"#,
                "invalid type",
            ),
            (
                r#"{"providers": [], "selected_provider": 0, "mcp_configs": null}"#,
                "invalid type",
            ),
        ] {
            let error = serde_json::from_str::<ShaiConfig>(json)
                .unwrap_err()
                .to_string();
            assert!(error.contains(expected), "{json}: {error}");
        }

        // Unknown keys are ignored however often they appear, and
        // `mcp_configs` is optional.
        let config: ShaiConfig = serde_json::from_str(
            r#"{"future": 1, "providers": [], "future": {"a": [1, 2]}, "selected_provider": 3}"#,
        )
        .unwrap();
        assert!(config.providers.is_empty());
        assert_eq!(config.selected_provider, 3);
        assert!(config.mcp_configs.is_empty());

        // Without roles, the file that is written has the keys it always had.
        assert_eq!(
            serde_json::to_string(&config).unwrap(),
            r#"{"providers":[],"selected_provider":3,"mcp_configs":{}}"#
        );
        let null_roles: ShaiConfig =
            serde_json::from_str(r#"{"providers": [], "selected_provider": 3, "roles": null}"#)
                .unwrap();
        assert_eq!(
            serde_json::to_string(&null_roles).unwrap(),
            serde_json::to_string(&config).unwrap()
        );
    }

    #[test]
    fn pulled_config_cannot_turn_roles_on() {
        let pulled = ShaiConfig::from_pulled_json(
            r#"{
                "providers": [
                    {"provider": "ollama", "env_vars": {}, "model": "llama3", "tool_method": "FunctionCall"}
                ],
                "selected_provider": 0,
                "roles": {"planner": "ollama/other"},
                "prewalk": {"into": "planner"}
            }"#,
        )
        .unwrap();
        assert_eq!(pulled.providers.len(), 1);
        assert!(pulled.roles.is_unset());
        assert!(pulled.prewalk.is_unset());
        assert!(pulled.resolve_role("planner").unwrap().is_none());
    }

    #[tokio::test]
    async fn pull_from_url_drops_roles() {
        let mut server = mockito::Server::new_async().await;
        let served = server
            .mock("GET", "/starter.config")
            .with_status(200)
            .with_body(
                r#"{
                    "providers": [
                        {"provider": "ollama", "env_vars": {"OLLAMA_BASE_URL": "http://127.0.0.1:9/v1"}, "model": "llama3", "tool_method": "FunctionCall"}
                    ],
                    "selected_provider": 0,
                    "roles": {"planner": "ollama/other"},
                    "prewalk": {"into": "planner"}
                }"#,
            )
            .expect(1)
            .create_async()
            .await;

        let url = format!("{}/starter.config", server.url()).parse().unwrap();
        let pulled = ShaiConfig::pull_from_url(url).await.unwrap();
        served.assert_async().await;
        assert_eq!(pulled.providers.len(), 1);
        assert!(pulled.roles.is_unset());
        assert!(pulled.prewalk.is_unset());
        assert!(pulled.resolve_role("planner").unwrap().is_none());
    }
}
