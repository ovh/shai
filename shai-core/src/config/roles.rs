// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: OVH SAS

//! Named secondary models ("roles").
//!
//! A session runs on one primary model, chosen by `selected_provider`. The
//! optional `roles` section of the auth config names *additional* models that
//! opt-in features can use next to it:
//!
//! ```jsonc
//! {
//!   "providers": [ /* ... */ ],
//!   "selected_provider": 0,
//!   "roles": {
//!     "planner": "anthropic/claude-sonnet-4-5",
//!     "cheap": "ollama/qwen3:8b"
//!   }
//! }
//! ```
//!
//! Each value is a `<provider>/<model>` reference. The provider part must name
//! an entry of `providers`; the role inherits that entry's credentials and
//! base URL, and only swaps the model. A role never carries credentials of its
//! own.
//!
//! Resolution is deliberately strict:
//!
//! * it is synchronous and offline: nothing here talks to a provider;
//! * a role that is not configured resolves to `Ok(None)`, which callers treat
//!   as "feature off";
//! * anything else that cannot be resolved is an error. There is no fallback to
//!   the primary model, to another provider, or to the built-in default
//!   config.
//!
//! What resolution cannot know is whether the provider actually serves the
//! model: that would take a network call. A well-formed but wrong model name
//! is reported by the provider on first use, and nothing here substitutes
//! another model for it.

use super::config::{ProviderConfig, ShaiConfig};
use super::section::{RawSection, Repeated};
use serde::de::{MapAccess, Visitor};
use serde::Deserializer;
use shai_llm::client::IsolatedSettings;
use shai_llm::LlmClient;
use std::fmt;
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;

/// Longest accepted role name.
const MAX_ROLE_NAME_LEN: usize = 64;
/// Longest accepted provider name in a reference.
const MAX_PROVIDER_LEN: usize = 32;
/// Longest accepted model identifier.
const MAX_MODEL_LEN: usize = 200;
/// Longest piece of user input quoted back in an error message.
const MAX_ECHO_LEN: usize = 80;
/// Most names listed in an error message.
const MAX_LISTED: usize = 16;

/// Why a role or model reference could not be resolved.
///
/// Messages are bounded and name roles and providers. They never include a
/// value from a provider's `env_vars`, and never the value of a role read from
/// the config file: a malformed value may be something the user meant as a
/// credential.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RoleError {
    /// There is no config file. The built-in default config is never used to
    /// resolve a secondary model.
    #[error(
        "no config file found: secondary models need configured providers, run `shai auth` first"
    )]
    ConfigMissing,

    /// The config file exists but could not be read or parsed. `reason` says
    /// where, without quoting the file.
    #[error("the config file {path} could not be loaded ({reason}); fix it before using a secondary model")]
    ConfigInvalid { path: String, reason: String },

    /// The config file has more than one `roles` section. Neither is used.
    #[error("the config has more than one \"roles\" section; keep a single one")]
    RepeatedRoles,

    /// The `roles` section exists but is not a JSON object that can be read.
    #[error("the \"roles\" section of the config must be an object mapping a role name to \"<provider>/<model>\"")]
    MalformedRoles,

    /// The `roles` section defines the same role more than once.
    #[error("the \"roles\" section of the config defines role {role:?} more than once")]
    DuplicateRole { role: String },

    /// The `roles` section has a key that is not a valid role name.
    #[error("the \"roles\" section of the config has an invalid role name {key:?}: use 1 to {} lowercase letters, digits, '-' or '_'", MAX_ROLE_NAME_LEN)]
    InvalidRoleKey { key: String },

    /// A role name that is empty, too long, or uses other characters than
    /// lowercase letters, digits, `-` and `_`.
    #[error(
        "invalid role name {role:?}: use 1 to {} lowercase letters, digits, '-' or '_'",
        MAX_ROLE_NAME_LEN
    )]
    InvalidRoleName { role: String },

    /// A role whose value is not a plain string, for example an object that
    /// tries to carry its own credentials.
    #[error("role {role:?} must be a string of the form \"<provider>/<model>\"; a role takes its credentials from the provider it names and cannot define its own")]
    InvalidRoleEntry { role: String },

    /// A role whose value is not a well-formed `<provider>/<model>`. The value
    /// itself is deliberately left out.
    #[error("role {role:?} is not a valid \"<provider>/<model>\" reference: {reason}")]
    InvalidRoleReference { role: String, reason: &'static str },

    /// A reference given directly (not read from `roles`) that is not a
    /// well-formed `<provider>/<model>`.
    #[error("invalid model reference {reference:?}: {reason}")]
    InvalidModelRef {
        reference: String,
        reason: &'static str,
    },

    /// A target that is neither a usable role name nor a reference.
    #[error("{target:?} is neither a role name nor a \"<provider>/<model>\" reference")]
    InvalidTarget { target: String },

    /// A setting of the config file, such as `prewalk.into`, that should name
    /// a model and does not. The value itself is deliberately left out.
    #[error(
        "{setting:?} is neither a role name nor a valid \"<provider>/<model>\" reference: {reason}"
    )]
    InvalidSetting {
        setting: &'static str,
        reason: &'static str,
    },

    /// A setting of the config file that names a role which is not
    /// configured. The value itself is deliberately left out.
    #[error("{setting:?} names a role that is not configured (configured roles: {})", listed(.known))]
    UnknownRoleInSetting {
        setting: &'static str,
        known: Vec<String>,
    },

    /// A role that was asked for by name but is not configured.
    #[error("role {role:?} is not configured (configured roles: {}); to name a model directly, use \"<provider>/<model>\"", listed(.known))]
    UnknownRole { role: String, known: Vec<String> },

    /// A reference to a provider that has no entry in `providers`.
    #[error("{origin} names a provider that is not configured (configured providers: {})", listed(.configured))]
    UnknownProvider {
        origin: String,
        configured: Vec<String>,
    },

    /// The provider entry the reference points at cannot back a secondary
    /// model, for example because it has no API key of its own.
    #[error("{origin} cannot use provider {provider:?}: {reason}")]
    UnusableProvider {
        origin: String,
        provider: String,
        reason: String,
    },

    /// Several entries share the provider name and the reference does not
    /// single one out.
    #[error("{origin} is ambiguous: {count} {provider:?} providers are configured {}; name a model exactly one of them is configured for, or remove the entries you do not use", ambiguity(*.exact_model))]
    AmbiguousProvider {
        origin: String,
        provider: String,
        count: usize,
        /// True when all of them are configured for the requested model.
        exact_model: bool,
    },
}

/// Names for an error message: quoted, and cut to a bounded list.
fn listed(names: &[String]) -> String {
    if names.is_empty() {
        return "none".to_string();
    }
    let mut shown: Vec<String> = names
        .iter()
        .take(MAX_LISTED)
        .map(|name| format!("{:?}", echo(name)))
        .collect();
    if names.len() > MAX_LISTED {
        shown.push(format!("and {} more", names.len() - MAX_LISTED));
    }
    shown.join(", ")
}

fn ambiguity(exact_model: bool) -> &'static str {
    if exact_model {
        "for that model with different settings"
    } else {
        "that are not interchangeable, and none of them is configured for that model"
    }
}

/// `text` cut to a length that is reasonable to quote in an error message.
fn echo(text: &str) -> String {
    if text.chars().count() <= MAX_ECHO_LEN {
        text.to_string()
    } else {
        let cut: String = text.chars().take(MAX_ECHO_LEN).collect();
        format!("{cut}…")
    }
}

/// The `roles` section of the config file, kept as the JSON text it was
/// written as.
///
/// The section is not deserialized into a typed map on purpose. A typed map
/// would turn a wrong value inside `roles` into a failure to load the *whole*
/// config, and a config that fails to load makes the primary model fall back
/// to the built-in default. Keeping the text confines such a mistake to roles:
/// the rest of the config loads as it did when the key was unknown, the
/// section is written back unchanged on save, and resolving a role fails
/// closed. One difference from an unknown key remains: the bytes of the
/// section have to be valid UTF-8, or the file does not load.
///
/// A `roles` key that appears twice is confined in the same way: the file
/// loads, both copies are kept, and no role resolves until one is removed.
/// Only a JSON syntax error still stops the whole file from loading, as it
/// does anywhere else in it.
#[derive(Clone, Default, PartialEq)]
pub struct RolesConfig(RawSection);

impl RolesConfig {
    /// True when the config has no `roles` section.
    pub fn is_unset(&self) -> bool {
        self.0.is_unset()
    }

    /// The section as found in the file.
    pub(crate) fn section(&self) -> &RawSection {
        &self.0
    }

    pub(crate) fn section_mut(&mut self) -> &mut RawSection {
        &mut self.0
    }

    /// Build a roles section from `(role, "<provider>/<model>")` pairs.
    /// No pairs means no section.
    pub fn from_pairs<I, K, V>(pairs: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        let entries: Vec<String> = pairs
            .into_iter()
            .map(|(role, reference)| {
                format!(
                    "{}:{}",
                    serde_json::Value::String(role.into()),
                    serde_json::Value::String(reference.into())
                )
            })
            .collect();
        if entries.is_empty() {
            return Self::default();
        }
        Self(RawSection::from_text(format!("{{{}}}", entries.join(","))).unwrap_or_default())
    }

    /// Names of the roles that are configured, sorted: the entries with a
    /// string value. Empty when the section is missing or cannot be used.
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .checked_entries()
            .unwrap_or_default()
            .into_iter()
            .filter(|(_, value)| value.is_string())
            .map(|(name, _)| name)
            .collect();
        names.sort();
        names
    }

    /// The entries of the section, once the section as a whole is known to be
    /// sound: an object whose keys are valid, distinct role names.
    ///
    /// One bad key condemns the whole section. A key that differs from a valid
    /// name by case or a stray space would otherwise never match, and the
    /// feature behind it would be off while its owner believes it is on.
    fn checked_entries(&self) -> Result<Vec<(String, serde_json::Value)>, RoleError> {
        let text = match self.0.text() {
            Ok(None) => return Ok(Vec::new()),
            Ok(Some(text)) => text,
            Err(Repeated) => return Err(RoleError::RepeatedRoles),
        };
        let entries = parse_entries(text)?;
        for (index, (name, _)) in entries.iter().enumerate() {
            if !is_role_name(name) {
                return Err(RoleError::InvalidRoleKey { key: echo(name) });
            }
            if entries[..index].iter().any(|(earlier, _)| earlier == name) {
                return Err(RoleError::DuplicateRole { role: name.clone() });
            }
        }
        Ok(entries)
    }

    /// The reference configured for `role`, if any.
    fn reference(&self, role: &str) -> Result<Option<String>, RoleError> {
        validate_role_name(role)?;
        let entries = self.checked_entries()?;
        match entries.into_iter().find(|(name, _)| name == role) {
            None => Ok(None),
            Some((_, serde_json::Value::String(reference))) => Ok(Some(reference)),
            // `null` included: a value was written, and it is not a
            // reference. Only a role that is not listed is turned off.
            Some(_) => Err(RoleError::InvalidRoleEntry {
                role: role.to_string(),
            }),
        }
    }
}

/// Read the section's text as a JSON object, keeping every entry in order,
/// duplicates included, so that they can be reported.
fn parse_entries(text: &str) -> Result<Vec<(String, serde_json::Value)>, RoleError> {
    struct Entries;

    impl<'de> Visitor<'de> for Entries {
        type Value = Vec<(String, serde_json::Value)>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("an object")
        }

        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut entries = Vec::new();
            while let Some(entry) = map.next_entry::<String, serde_json::Value>()? {
                entries.push(entry);
            }
            Ok(entries)
        }
    }

    let mut deserializer = serde_json::Deserializer::from_str(text);
    let entries = deserializer
        .deserialize_map(Entries)
        .map_err(|_| RoleError::MalformedRoles)?;
    deserializer.end().map_err(|_| RoleError::MalformedRoles)?;
    Ok(entries)
}

// Nothing of the section is printed: it has not been validated at this point.
impl fmt::Debug for RolesConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.is_unset() {
            "RolesConfig(unset)"
        } else {
            "RolesConfig(set)"
        })
    }
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_'
}

fn is_role_name(role: &str) -> bool {
    !role.is_empty() && role.len() <= MAX_ROLE_NAME_LEN && role.chars().all(is_name_char)
}

fn validate_role_name(role: &str) -> Result<(), RoleError> {
    if is_role_name(role) {
        Ok(())
    } else {
        Err(RoleError::InvalidRoleName { role: echo(role) })
    }
}

/// A parsed `<provider>/<model>` reference.
struct ModelRef<'a> {
    provider: &'a str,
    model: &'a str,
}

impl<'a> ModelRef<'a> {
    /// Parse `<provider>/<model>`, returning why it is malformed otherwise.
    ///
    /// The provider is everything before the first `/`, so model identifiers
    /// that contain slashes (`openrouter/anthropic/claude-sonnet-4`) work.
    /// Both parts are bounded, and the model is limited to visible ASCII: no
    /// whitespace, control or invisible characters, so that what is shown is
    /// what is sent.
    fn parse(reference: &'a str) -> Result<Self, &'static str> {
        let (provider, model) = reference
            .split_once('/')
            .ok_or("expected \"<provider>/<model>\"")?;
        if provider.is_empty() {
            return Err("the provider name is empty");
        }
        if provider.len() > MAX_PROVIDER_LEN || !provider.chars().all(is_name_char) {
            return Err(
                "the provider name must be at most 32 lowercase letters, digits, '-' or '_'",
            );
        }
        if model.is_empty() {
            return Err("the model name is empty");
        }
        if model.len() > MAX_MODEL_LEN {
            return Err("the model name is too long");
        }
        if !model.chars().all(|c| c.is_ascii_graphic()) {
            return Err("the model name may only contain visible ASCII characters, without spaces");
        }
        if model.starts_with('/') || model.split('/').any(|segment| segment == "..") {
            return Err("the model name must not start with '/' or contain a '..' segment");
        }
        Ok(Self { provider, model })
    }
}

/// A secondary model resolved against the configured providers.
///
/// Holds the provider entry it resolved to, including that entry's
/// credentials, so `Debug` is implemented by hand and prints the label only.
#[derive(Clone)]
pub struct ResolvedModel {
    provider: ProviderConfig,
    model: String,
}

impl ResolvedModel {
    /// Name of the provider entry this model resolved to.
    pub fn provider_name(&self) -> &str {
        &self.provider.provider
    }

    /// Model identifier to send to the provider.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// `<provider>/<model>`, safe to show to the user.
    pub fn label(&self) -> String {
        format!("{}/{}", self.provider.provider, self.model)
    }

    /// Build the client for this model and bind the model to it.
    ///
    /// Only the resolved provider entry is read. Unlike the primary model, a
    /// secondary model never picks a missing setting up from the process
    /// environment: the primary provider exports its own key and base URL
    /// there, and they must not leak into a client for another endpoint.
    ///
    /// No network I/O is performed.
    pub fn bind(&self) -> Result<BoundModel, RoleError> {
        let client =
            LlmClient::create_provider_isolated(&self.provider.provider, &self.provider.env_vars)
                .map_err(|e| RoleError::UnusableProvider {
                origin: format!("{:?}", self.label()),
                provider: self.provider.provider.clone(),
                reason: e.to_string(),
            })?;
        Ok(BoundModel::new(Arc::new(client), self.model.clone()))
    }
}

impl fmt::Debug for ResolvedModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResolvedModel")
            .field("provider", &self.provider.provider)
            .field("model", &self.model)
            .finish_non_exhaustive()
    }
}

/// A client together with the one model it is to be used with.
///
/// Secondary models are handed around in this form rather than as a bare
/// client because a bare client does not know its model: asking it for a
/// default (`LlmClient::default_model`) answers with the *primary* model, or
/// with whatever the provider lists first. Code that uses a secondary model
/// must send [`BoundModel::model`] and nothing else.
#[derive(Clone)]
pub struct BoundModel {
    llm: Arc<LlmClient>,
    model: String,
}

impl BoundModel {
    /// Bind `model` to `llm`.
    pub fn new(llm: Arc<LlmClient>, model: impl Into<String>) -> Self {
        Self {
            llm,
            model: model.into(),
        }
    }

    /// The client to send requests with.
    pub fn llm(&self) -> &Arc<LlmClient> {
        &self.llm
    }

    /// The model to name in every request.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Name of the provider behind the client.
    pub fn provider(&self) -> &str {
        self.llm.provider_name()
    }

    /// `<provider>/<model>`, safe to show to the user.
    pub fn label(&self) -> String {
        format!("{}/{}", self.provider(), self.model)
    }
}

impl fmt::Debug for BoundModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BoundModel({})", self.label())
    }
}

/// Where a reference came from, which decides how errors describe it.
enum Origin<'a> {
    /// The value of a role in the config file: named, never quoted.
    Role(&'a str),
    /// Given directly, by the user or by a caller: quoted, cut to size.
    Direct,
    /// The value of another setting of the config file: the setting is
    /// named, its value never quoted.
    Setting(&'static str),
}

/// Say why a config file failed to load without repeating what it contains.
///
/// The parser's own message can quote the offending value, and a config file
/// holds credentials; the position is enough to find the mistake.
fn describe_load_error(error: &(dyn std::error::Error + 'static)) -> String {
    if let Some(json) = error.downcast_ref::<serde_json::Error>() {
        let kind = match json.classify() {
            serde_json::error::Category::Syntax | serde_json::error::Category::Eof => {
                "JSON syntax error"
            }
            serde_json::error::Category::Data => "unexpected value",
            serde_json::error::Category::Io => "read error",
        };
        format!("{} at line {} column {}", kind, json.line(), json.column())
    } else if let Some(io) = error.downcast_ref::<std::io::Error>() {
        format!("read error: {}", io.kind())
    } else {
        "unreadable".to_string()
    }
}

impl ShaiConfig {
    /// Why the user's config file cannot be loaded, if it is there and does
    /// not load: its path and what is wrong with it, without repeating
    /// anything the file holds. `None` when the file loads or does not exist.
    pub fn load_problem() -> Option<(String, String)> {
        match Self::load_for_roles() {
            Err(RoleError::ConfigInvalid { path, reason }) => Some((path, reason)),
            _ => None,
        }
    }

    /// Load the user's config for resolving secondary models.
    ///
    /// Unlike [`ShaiConfig::get_llm`] and [`ShaiConfig::load_or_default`], this
    /// never substitutes the built-in default config: without a readable
    /// config file there is nothing a role could legitimately refer to.
    pub fn load_for_roles() -> Result<ShaiConfig, RoleError> {
        // Where the config would be is unknown when there is no home
        // directory, or when the config directory does not exist and cannot
        // be created. Either way there is no config file: that is a missing
        // config, not one that is there and cannot be read.
        match (Self::config_path(), Self::legacy_config_path()) {
            (Ok(config_path), Ok(legacy_path)) => {
                Self::load_for_roles_from(&config_path, &legacy_path)
            }
            _ => Err(RoleError::ConfigMissing),
        }
    }

    pub(crate) fn load_for_roles_from(
        config_path: &Path,
        legacy_path: &Path,
    ) -> Result<ShaiConfig, RoleError> {
        // The file that counts is the first of the two that is there. One
        // that is there but cannot be reached (a link to nothing, a directory
        // that cannot be searched) is not a missing file: what it asks for is
        // unknown, which is for the caller to refuse.
        for path in [config_path, legacy_path] {
            let invalid = |reason: String| RoleError::ConfigInvalid {
                path: path.display().to_string(),
                reason,
            };
            match std::fs::symlink_metadata(path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(invalid(format!("read error: {}", error.kind()))),
                Ok(_) => {
                    return Self::load_from_paths(path, path)
                        .map_err(|e| invalid(describe_load_error(e.as_ref())));
                }
            }
        }
        Err(RoleError::ConfigMissing)
    }

    /// Resolve a named role.
    ///
    /// Returns `Ok(None)` when the role is not configured, which callers must
    /// treat as "the feature is off". A role that is configured but cannot be
    /// resolved is an error, never a fallback.
    pub fn resolve_role(&self, role: &str) -> Result<Option<ResolvedModel>, RoleError> {
        match self.roles.reference(role)? {
            Some(reference) => self.resolve(&reference, Origin::Role(role)).map(Some),
            None => Ok(None),
        }
    }

    /// Resolve a `<provider>/<model>` reference against `providers`.
    ///
    /// The reference must single out one way to reach the model:
    ///
    /// * if entries of that provider are configured for exactly that model,
    ///   it is one of those, and they must be usable and agree with each
    ///   other; an entry that cannot back a secondary model (no API key of
    ///   its own, for example) is skipped only when another entry for the
    ///   same model can;
    /// * otherwise every entry of that provider must be usable and hold the
    ///   same settings, so that the choice cannot change where the
    ///   conversation and the credentials are sent.
    ///
    /// Anything else is an error rather than a guess.
    pub fn resolve_model_ref(&self, reference: &str) -> Result<ResolvedModel, RoleError> {
        self.resolve(reference, Origin::Direct)
    }

    /// Resolve a target given by the user: either a role name or a
    /// `<provider>/<model>` reference.
    ///
    /// The user asked for this model explicitly, so a role that is not
    /// configured is an error here rather than "off".
    pub fn resolve_model_target(&self, target: &str) -> Result<ResolvedModel, RoleError> {
        if target.contains('/') {
            return self.resolve_model_ref(target);
        }
        if !is_role_name(target) {
            return Err(RoleError::InvalidTarget {
                target: echo(target),
            });
        }
        self.resolve_role(target)?
            .ok_or_else(|| RoleError::UnknownRole {
                role: target.to_string(),
                known: self.roles.names(),
            })
    }

    /// Resolve a target read from a setting of the config file, such as
    /// `prewalk.into`: either a role name or a `<provider>/<model>` reference.
    ///
    /// Works like [`ShaiConfig::resolve_model_target`], except that errors
    /// name the setting instead of quoting its value. Like the value of a
    /// role, it comes from the file and may be something the user meant as a
    /// credential.
    pub fn resolve_configured_target(
        &self,
        setting: &'static str,
        target: &str,
    ) -> Result<ResolvedModel, RoleError> {
        if target.contains('/') {
            return self.resolve(target, Origin::Setting(setting));
        }
        if !is_role_name(target) {
            return Err(RoleError::InvalidSetting {
                setting,
                reason: "a role name uses lowercase letters, digits, '-' and '_'",
            });
        }
        self.resolve_role(target)?
            .ok_or_else(|| RoleError::UnknownRoleInSetting {
                setting,
                known: self.roles.names(),
            })
    }

    fn resolve(&self, reference: &str, origin: Origin<'_>) -> Result<ResolvedModel, RoleError> {
        let parsed = ModelRef::parse(reference).map_err(|reason| match origin {
            Origin::Role(role) => RoleError::InvalidRoleReference {
                role: role.to_string(),
                reason,
            },
            Origin::Direct => RoleError::InvalidModelRef {
                reference: echo(reference),
                reason,
            },
            Origin::Setting(setting) => RoleError::InvalidSetting { setting, reason },
        })?;
        let origin = match origin {
            Origin::Role(role) => format!("role {role:?}"),
            Origin::Direct => format!("{:?}", echo(reference)),
            Origin::Setting(setting) => format!("{setting:?}"),
        };

        let named: Vec<&ProviderConfig> = self
            .providers
            .iter()
            .filter(|p| p.provider == parsed.provider)
            .collect();
        if named.is_empty() {
            let mut configured: Vec<String> =
                self.providers.iter().map(|p| p.provider.clone()).collect();
            configured.sort();
            configured.dedup();
            return Err(RoleError::UnknownProvider { origin, configured });
        }
        // From here on the provider part is known to be a configured name.
        let provider = parsed.provider.to_string();

        // What an isolated client would be built from, per entry, or why it
        // cannot be built.
        let settings = |entry: &ProviderConfig| {
            LlmClient::isolated_settings(&entry.provider, &entry.env_vars)
                .map_err(|e| e.to_string())
        };

        let exact: Vec<&ProviderConfig> = named
            .iter()
            .copied()
            .filter(|entry| entry.model == parsed.model)
            .collect();
        let exact_model = !exact.is_empty();

        let chosen = if exact_model {
            // Entries configured for this very model. One that cannot be
            // used is passed over only in favour of another one of them,
            // never in favour of an entry for a different model.
            let mut usable = Vec::new();
            let mut first_reason = None;
            for entry in &exact {
                match settings(entry) {
                    Ok(entry_settings) => usable.push((*entry, entry_settings)),
                    Err(reason) => {
                        first_reason.get_or_insert(reason);
                    }
                }
            }
            let Some((first, first_settings)) = usable.first() else {
                return Err(RoleError::UnusableProvider {
                    origin,
                    provider,
                    reason: first_reason.unwrap_or_default(),
                });
            };
            if usable
                .iter()
                .any(|(_, entry_settings)| entry_settings != first_settings)
            {
                return Err(RoleError::AmbiguousProvider {
                    origin,
                    provider,
                    count: usable.len(),
                    exact_model,
                });
            }
            *first
        } else {
            // No entry is configured for this model: any entry of the
            // provider would have to do, so they must all be the same.
            let first = named[0];
            let first_settings = match settings(first) {
                Ok(first_settings) => first_settings,
                Err(reason) if named.len() == 1 => {
                    return Err(RoleError::UnusableProvider {
                        origin,
                        provider,
                        reason,
                    });
                }
                Err(_) => {
                    return Err(RoleError::AmbiguousProvider {
                        origin,
                        provider,
                        count: named.len(),
                        exact_model,
                    });
                }
            };
            if named[1..]
                .iter()
                .any(|entry| settings(entry).as_ref() != Ok(&first_settings))
            {
                return Err(RoleError::AmbiguousProvider {
                    origin,
                    provider,
                    count: named.len(),
                    exact_model,
                });
            }
            first
        };

        Ok(ResolvedModel {
            provider: chosen.clone(),
            model: parsed.model.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shai_llm::ToolCallMethod;
    use std::collections::HashMap;

    const SECRET: &str = "sk-test-do-not-print-0123456789";

    fn provider(name: &str, model: &str, env: &[(&str, &str)]) -> ProviderConfig {
        ProviderConfig {
            provider: name.to_string(),
            env_vars: env
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            model: model.to_string(),
            tool_method: ToolCallMethod::FunctionCall,
        }
    }

    fn config(providers: Vec<ProviderConfig>, roles: &[(&str, &str)]) -> ShaiConfig {
        ShaiConfig {
            providers,
            selected_provider: 0,
            mcp_configs: HashMap::new(),
            roles: RolesConfig::from_pairs(roles.iter().copied()),
            prewalk: Default::default(),
        }
    }

    fn openai() -> ProviderConfig {
        provider("openai", "gpt-4o", &[("OPENAI_API_KEY", SECRET)])
    }

    /// The text of a config file with one keyed `openai` provider and the
    /// given `roles` section.
    fn file_with_roles(roles: &str) -> String {
        format!(
            r#"{{
                "providers": [
                    {{"provider": "openai", "env_vars": {{"OPENAI_API_KEY": "k"}}, "model": "gpt-4o", "tool_method": "FunctionCall"}}
                ],
                "selected_provider": 0,
                "roles": {roles}
            }}"#
        )
    }

    fn parse_with_roles(roles: &str) -> ShaiConfig {
        serde_json::from_str(&file_with_roles(roles))
            .expect("the config must load whatever the roles section holds")
    }

    fn label(result: Result<ResolvedModel, RoleError>) -> String {
        result.expect("should resolve").label()
    }

    // ---- parsing -----------------------------------------------------------

    #[test]
    fn model_ref_splits_on_the_first_slash() {
        let parsed = ModelRef::parse("anthropic/claude-sonnet-4-5").unwrap();
        assert_eq!(
            (parsed.provider, parsed.model),
            ("anthropic", "claude-sonnet-4-5")
        );

        // Model identifiers may contain slashes, colons, dots and capitals.
        let parsed = ModelRef::parse("openrouter/anthropic/claude-sonnet-4").unwrap();
        assert_eq!(
            (parsed.provider, parsed.model),
            ("openrouter", "anthropic/claude-sonnet-4")
        );
        assert_eq!(
            ModelRef::parse("ollama/qwen3:8b").unwrap().model,
            "qwen3:8b"
        );
        assert_eq!(
            ModelRef::parse("ovhcloud/Meta-Llama-3_3-70B-Instruct")
                .unwrap()
                .model,
            "Meta-Llama-3_3-70B-Instruct"
        );
    }

    #[test]
    fn model_ref_rejects_malformed_references() {
        let long_model = format!("openai/{}", "m".repeat(MAX_MODEL_LEN + 1));
        let long_provider = format!("{}/gpt-4o", "p".repeat(MAX_PROVIDER_LEN + 1));
        for bad in [
            "",
            "gpt-4o",
            "/gpt-4o",
            "openai/",
            "/",
            " openai/gpt-4o",
            "openai/gpt-4o ",
            "openai/gpt 4o",
            "openai/gpt-4o\n",
            "openai/gpt\u{0}4o",
            // Invisible and direction-changing characters.
            "openai/gpt-4o\u{200b}",
            "openai/gpt-4o\u{202e}inim",
            "openai/gpt-4\u{f6}",
            // Provider names are short, plain lowercase identifiers.
            "open ai/gpt-4o",
            "open.ai/gpt-4o",
            "OpenAI/gpt-4o",
            "openai:key@host/gpt-4o",
            long_provider.as_str(),
            // Nothing path-like in the model.
            "openai//gpt-4o",
            "openai/../gpt-4o",
            "openai/a/../b",
            long_model.as_str(),
        ] {
            assert!(ModelRef::parse(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn roles_section_is_optional_and_round_trips() {
        // A config written before roles existed still loads, with no roles,
        // and saving it does not add the key.
        let json = r#"{"providers":[],"selected_provider":0,"mcp_configs":{}}"#;
        let without: ShaiConfig = serde_json::from_str(json).unwrap();
        assert!(without.roles.is_unset());
        assert!(without.roles.names().is_empty());
        assert_eq!(serde_json::to_string(&without).unwrap(), json);

        let with = parse_with_roles(
            r#"{"planner": "anthropic/claude-sonnet-4-5", "cheap": "ollama/qwen3:8b"}"#,
        );
        assert_eq!(with.roles.names(), ["cheap", "planner"]);

        // Roles survive a save and reload, as `shai auth` does, as written.
        let saved = serde_json::to_string(&with).unwrap();
        assert!(
            saved.ends_with(
                r#""roles":{"planner": "anthropic/claude-sonnet-4-5", "cheap": "ollama/qwen3:8b"}}"#
            ),
            "{saved}"
        );
        let reloaded: ShaiConfig = serde_json::from_str(&saved).unwrap();
        assert_eq!(reloaded.roles, with.roles);
        let pretty = serde_json::to_string_pretty(&with).unwrap();
        let reloaded: ShaiConfig = serde_json::from_str(&pretty).unwrap();
        assert_eq!(reloaded.roles.names(), ["cheap", "planner"]);

        assert!(parse_with_roles("null").roles.is_unset());
        assert!(RolesConfig::from_pairs(Vec::<(String, String)>::new()).is_unset());
    }

    #[test]
    fn roles_are_read_from_the_config_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("auth.config.json");
        std::fs::write(
            &path,
            r#"{
                // comments are allowed in the config file
                "providers": [
                    {"provider": "ollama", "env_vars": {"OLLAMA_BASE_URL": "http://127.0.0.1:11434/v1"}, "model": "llama3", "tool_method": "FunctionCall"}
                ],
                "selected_provider": 0,
                "roles": {"cheap": "ollama/qwen3:8b"}
            }"#,
        )
        .unwrap();

        let loaded =
            ShaiConfig::load_for_roles_from(&path, &dir.path().join("auth.config")).unwrap();
        assert_eq!(
            loaded.resolve_role("cheap").unwrap().unwrap().label(),
            "ollama/qwen3:8b"
        );
    }

    #[test]
    fn the_legacy_config_file_is_only_a_fallback() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("auth.config.json");
        let legacy = dir.path().join("auth.config");
        let file = |role: &str| {
            format!(
                r#"{{"providers": [], "selected_provider": 0, "roles": {{"{role}": "openai/gpt-4o"}}}}"#
            )
        };

        std::fs::write(&legacy, file("legacy")).unwrap();
        let loaded = ShaiConfig::load_for_roles_from(&path, &legacy).unwrap();
        assert_eq!(loaded.roles.names(), ["legacy"]);

        std::fs::write(&path, file("current")).unwrap();
        let loaded = ShaiConfig::load_for_roles_from(&path, &legacy).unwrap();
        assert_eq!(loaded.roles.names(), ["current"]);
        let loaded = ShaiConfig::load_from_paths(&path, &legacy).unwrap();
        assert_eq!(loaded.roles.names(), ["current"]);
    }

    #[test]
    fn missing_or_broken_config_file_is_an_error_not_a_default() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("auth.config.json");
        let legacy = dir.path().join("auth.config");

        // No file: never the built-in default config.
        assert_eq!(
            ShaiConfig::load_for_roles_from(&path, &legacy).unwrap_err(),
            RoleError::ConfigMissing
        );

        // A file that does not parse: an error that names the file and does
        // not send the user to a wizard that would overwrite it.
        std::fs::write(
            &path,
            r#"{"providers": [], "selected_provider": 0, "roles": {"a": "b/c",}}"#,
        )
        .unwrap();
        let error = ShaiConfig::load_for_roles_from(&path, &legacy).unwrap_err();
        assert!(
            matches!(error, RoleError::ConfigInvalid { .. }),
            "{error:?}"
        );
        let message = error.to_string();
        assert!(message.contains("auth.config.json"), "{message}");
        assert!(message.contains("JSON syntax error at line 1"), "{message}");
        assert!(!message.contains("shai auth"), "{message}");
    }

    #[test]
    fn a_config_load_error_does_not_quote_the_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("auth.config.json");
        let legacy = dir.path().join("auth.config");
        // The parser's own message would quote each of these values.
        for body in [
            format!(
                r#"{{"providers": [{{"provider": "openai", "env_vars": "{SECRET}", "model": "m", "tool_method": "FunctionCall"}}], "selected_provider": 0}}"#
            ),
            format!(
                r#"{{"providers": [{{"provider": "openai", "env_vars": {{}}, "model": "m", "tool_method": "{SECRET}"}}], "selected_provider": 0}}"#
            ),
            format!(r#"{{"providers": [], "selected_provider": "{SECRET}"}}"#),
        ] {
            std::fs::write(&path, body).unwrap();
            let error = ShaiConfig::load_for_roles_from(&path, &legacy).unwrap_err();
            let shown = format!("{error} {error:?}");
            assert!(shown.contains("unexpected value at line 1"), "{shown}");
            assert!(!shown.contains(SECRET), "{shown}");
        }
    }

    // ---- resolution --------------------------------------------------------

    #[test]
    fn unset_role_means_the_feature_is_off() {
        let cfg = config(vec![openai()], &[("planner", "openai/gpt-4o-mini")]);
        assert!(cfg.resolve_role("cheap").unwrap().is_none());

        // No roles section at all, including in the built-in default config.
        let cfg = config(vec![openai()], &[]);
        assert!(cfg.resolve_role("planner").unwrap().is_none());
        assert!(ShaiConfig::default()
            .resolve_role("planner")
            .unwrap()
            .is_none());

        // A null entry is not the same as no entry: something was written,
        // and it is not a reference.
        let cfg = parse_with_roles(r#"{"planner": null}"#);
        assert!(matches!(
            cfg.resolve_role("planner"),
            Err(RoleError::InvalidRoleEntry { .. })
        ));
        assert!(matches!(
            cfg.resolve_model_target("planner"),
            Err(RoleError::InvalidRoleEntry { .. })
        ));
    }

    #[test]
    fn role_takes_credentials_from_the_provider_it_names() {
        let cfg = config(
            vec![
                provider("ovhcloud", "Qwen3-32B", &[("OVH_API_KEY", "ovh-key")]),
                openai(),
            ],
            &[("planner", "openai/gpt-4o-mini")],
        );

        let planner = cfg.resolve_role("planner").unwrap().expect("configured");
        assert_eq!(planner.provider_name(), "openai");
        // The role swaps the model; the entry's own model is not used.
        assert_eq!(planner.model(), "gpt-4o-mini");
        assert_eq!(planner.label(), "openai/gpt-4o-mini");
        assert_eq!(planner.provider.env_vars, cfg.providers[1].env_vars);

        let bound = planner.bind().expect("client");
        assert_eq!(bound.provider(), "openai");
        assert_eq!(bound.model(), "gpt-4o-mini");
        assert_eq!(bound.label(), "openai/gpt-4o-mini");
    }

    #[test]
    fn unknown_provider_fails_closed() {
        let cfg = config(
            vec![
                provider("ovhcloud", "Qwen3-32B", &[("OVH_API_KEY", "ovh-key")]),
                openai(),
            ],
            &[("planner", "anthropic/claude-sonnet-4-5")],
        );

        // Not the selected provider, not any other provider: an error.
        let error = cfg.resolve_role("planner").unwrap_err();
        assert_eq!(
            error,
            RoleError::UnknownProvider {
                origin: "role \"planner\"".to_string(),
                configured: vec!["openai".to_string(), "ovhcloud".to_string()],
            }
        );
        let message = error.to_string();
        assert!(message.contains("role \"planner\""), "{message}");
        assert!(message.contains("\"openai\", \"ovhcloud\""), "{message}");
        assert!(!message.contains(SECRET), "{message}");

        let error = cfg
            .resolve_model_ref("mistral/mistral-small-latest")
            .unwrap_err();
        assert!(
            matches!(error, RoleError::UnknownProvider { .. }),
            "{error:?}"
        );
        // What the user typed is quoted back to them.
        assert!(
            error.to_string().contains("mistral/mistral-small-latest"),
            "{error}"
        );
    }

    #[test]
    fn built_in_default_provider_never_backs_a_role() {
        // The default config ships one anonymous OVHcloud entry. Naming
        // another provider must not end up there...
        let default = ShaiConfig::default();
        assert!(matches!(
            default.resolve_model_ref("openai/gpt-4o"),
            Err(RoleError::UnknownProvider { .. })
        ));
        // ...and naming it explicitly is refused too, for its own model or
        // any other: secondary models never use anonymous access.
        let model = default.providers[0].model.clone();
        for reference in [format!("ovhcloud/{model}"), "ovhcloud/Other".to_string()] {
            let error = default.resolve_model_ref(&reference).unwrap_err();
            assert!(
                matches!(error, RoleError::UnusableProvider { .. }),
                "{reference}: {error:?}"
            );
            assert!(error.to_string().contains("OVH_API_KEY"), "{error}");
        }
    }

    #[test]
    fn an_entry_that_cannot_back_a_role_is_never_replaced_by_another_endpoint() {
        // A keyless local server next to a keyed remote one.
        let local = ("OPENAI_COMPATIBLE_BASE_URL", "http://127.0.0.1:8080/v1");
        let remote = ("OPENAI_COMPATIBLE_BASE_URL", "https://b.example.com/v1");
        let cfg = config(
            vec![
                provider("openai_compatible", "local-llm", &[local]),
                provider(
                    "openai_compatible",
                    "big-model",
                    &[remote, ("OPENAI_COMPATIBLE_API_KEY", "key-b")],
                ),
            ],
            &[
                ("private", "openai_compatible/local-llm"),
                ("other", "openai_compatible/third-model"),
                ("big", "openai_compatible/big-model"),
            ],
        );

        // The role names the local entry's model. That entry cannot be used,
        // and the conversation must not go to the remote host instead.
        let error = cfg.resolve_role("private").unwrap_err();
        assert!(
            matches!(error, RoleError::UnusableProvider { .. }),
            "{error:?}"
        );
        assert!(
            error.to_string().contains("OPENAI_COMPATIBLE_API_KEY"),
            "{error}"
        );

        // A model neither entry is configured for: the two are not
        // interchangeable, so there is nothing to pick.
        let error = cfg.resolve_role("other").unwrap_err();
        assert!(
            matches!(
                error,
                RoleError::AmbiguousProvider {
                    count: 2,
                    exact_model: false,
                    ..
                }
            ),
            "{error:?}"
        );

        // The entry that is configured for its model and usable still works.
        let big = cfg.resolve_role("big").unwrap().unwrap();
        assert_eq!(big.provider.env_vars, cfg.providers[1].env_vars);
    }

    #[test]
    fn a_keyless_starter_entry_next_to_a_keyed_one() {
        // What a first run followed by `shai auth` leaves behind: the
        // built-in anonymous entry next to a keyed one.
        let mut cfg = ShaiConfig::default();
        let starter_model = cfg.providers[0].model.clone();
        cfg.providers.push(provider(
            "ovhcloud",
            "Mistral-Nemo",
            &[("OVH_API_KEY", "ovh-key")],
        ));

        // The model the keyed entry is configured for resolves to it.
        let resolved = cfg.resolve_model_ref("ovhcloud/Mistral-Nemo").unwrap();
        assert_eq!(resolved.provider.env_vars, cfg.providers[1].env_vars);
        resolved.bind().expect("client");

        // The starter entry's own model is refused: it has no key, and its
        // endpoint is not the keyed entry's.
        let error = cfg
            .resolve_model_ref(&format!("ovhcloud/{starter_model}"))
            .unwrap_err();
        assert!(
            matches!(error, RoleError::UnusableProvider { .. }),
            "{error:?}"
        );

        // Any other model is ambiguous until the unused entry is removed.
        let error = cfg.resolve_model_ref("ovhcloud/Another-Model").unwrap_err();
        assert!(
            matches!(error, RoleError::AmbiguousProvider { count: 2, .. }),
            "{error:?}"
        );
        assert!(
            error
                .to_string()
                .contains("remove the entries you do not use"),
            "{error}"
        );
        cfg.providers.remove(0);
        assert_eq!(
            label(cfg.resolve_model_ref("ovhcloud/Another-Model")),
            "ovhcloud/Another-Model"
        );

        // A keyed entry for the very model of the starter entry is used.
        let mut cfg = ShaiConfig::default();
        cfg.providers.push(provider(
            "ovhcloud",
            &starter_model,
            &[("OVH_API_KEY", "ovh-key")],
        ));
        let resolved = cfg
            .resolve_model_ref(&format!("ovhcloud/{starter_model}"))
            .unwrap();
        assert_eq!(resolved.provider.env_vars, cfg.providers[1].env_vars);
    }

    #[test]
    fn malformed_reference_in_a_role_fails_closed_without_echoing_it() {
        let leaky = format!("openai:{SECRET}@api.example.com/gpt-4o");
        let url = format!("https://user:{SECRET}@host/v1/gpt-4o");
        // A pasted key can look like a provider name.
        let as_provider = format!("{SECRET}/gpt-4o");
        let long_provider = format!("{}/gpt-4o", "a".repeat(10_000));
        let cfg = config(
            vec![openai()],
            &[
                ("empty", "openai/"),
                ("bare", "gpt-4o"),
                ("spaced", "openai/gpt 4o"),
                ("leaky", &leaky),
                ("url", &url),
                ("pasted", SECRET),
                ("as-provider", &as_provider),
                ("long", &long_provider),
            ],
        );
        for role in [
            "empty",
            "bare",
            "spaced",
            "leaky",
            "url",
            "pasted",
            "as-provider",
            "long",
        ] {
            let error = cfg.resolve_role(role).unwrap_err();
            assert!(
                matches!(
                    error,
                    RoleError::InvalidRoleReference { .. } | RoleError::UnknownProvider { .. }
                ),
                "{role}: {error:?}"
            );
            // The role is named; its value is not repeated.
            let shown = format!("{error} {error:?}");
            assert!(shown.contains(role), "{shown}");
            assert!(!shown.contains(SECRET), "{shown}");
            assert!(!shown.contains("gpt 4o"), "{shown}");
            assert!(shown.len() < 600, "{role}: {}", shown.len());
        }
    }

    #[test]
    fn direct_reference_errors_quote_a_bounded_amount() {
        let cfg = config(vec![openai()], &[]);
        let long_model = format!("openai/{}", "x".repeat(10_000));
        let long_provider = format!("{}/m", "a".repeat(10_000));
        // Well-formed, as long as allowed, and naming an unknown provider.
        let long_valid = format!("{}/{}", "p".repeat(32), "m".repeat(200));
        for reference in [long_model, long_provider, long_valid] {
            let message = cfg.resolve_model_ref(&reference).unwrap_err().to_string();
            assert!(message.len() < 400, "{}", message.len());
        }

        let message = cfg
            .resolve_model_target(&"y".repeat(10_000))
            .unwrap_err()
            .to_string();
        assert!(message.len() < 400, "{}", message.len());

        // Lists of names are bounded too.
        let many: Vec<ProviderConfig> = (0..100)
            .map(|i| provider(&format!("provider-{i}"), "m", &[]))
            .collect();
        let message = config(many, &[])
            .resolve_model_ref("openai/gpt-4o")
            .unwrap_err()
            .to_string();
        assert!(message.contains("and 84 more"), "{message}");
        assert!(message.len() < 600, "{}", message.len());
    }

    #[test]
    fn invalid_role_names_are_rejected() {
        let cfg = config(vec![openai()], &[("planner", "openai/gpt-4o-mini")]);
        let too_long = "r".repeat(MAX_ROLE_NAME_LEN + 1);
        for bad in [
            "",
            "plan ner",
            "planner ",
            "Planner",
            "../x",
            "r\u{f4}le",
            "planner\u{200b}",
            too_long.as_str(),
        ] {
            assert!(
                matches!(
                    cfg.resolve_role(bad),
                    Err(RoleError::InvalidRoleName { .. })
                ),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn a_bad_key_condemns_the_whole_roles_section() {
        // A key that is almost right would otherwise never match, leaving the
        // feature off while the config looks like it is on.
        for (roles, key) in [
            (
                r#"{"Planner": "openai/gpt-4o-mini", "cheap": "openai/gpt-4o-mini"}"#,
                "Planner",
            ),
            (
                r#"{"planner ": "openai/gpt-4o-mini", "cheap": "openai/gpt-4o-mini"}"#,
                "planner ",
            ),
            (
                r#"{"": "openai/gpt-4o-mini", "cheap": "openai/gpt-4o-mini"}"#,
                "",
            ),
        ] {
            let cfg = parse_with_roles(roles);
            let expected = RoleError::InvalidRoleKey {
                key: key.to_string(),
            };
            for role in ["planner", "cheap", "unset"] {
                assert_eq!(cfg.resolve_role(role).unwrap_err(), expected, "{roles}");
            }
            // The message points at the config, not at the name asked for.
            let error = cfg.resolve_model_target("cheap").unwrap_err();
            assert!(error.to_string().contains("\"roles\" section"), "{error}");
            assert!(cfg.roles.names().is_empty());
        }
    }

    #[test]
    fn a_role_defined_twice_is_refused_rather_than_guessed() {
        let cfg = parse_with_roles(
            r#"{"reviewer": "openai/gpt-4o-mini", "cheap": "openai/gpt-4o", "reviewer": "openai/o4-mini"}"#,
        );
        let expected = RoleError::DuplicateRole {
            role: "reviewer".to_string(),
        };
        assert_eq!(cfg.resolve_role("reviewer").unwrap_err(), expected);
        // The section as a whole is unreliable, not just that role.
        assert_eq!(cfg.resolve_role("cheap").unwrap_err(), expected);
        assert_eq!(cfg.resolve_role("unset").unwrap_err(), expected);

        // Saving keeps what the user wrote, so it can be fixed by hand.
        let saved = serde_json::to_string(&cfg).unwrap();
        assert_eq!(saved.matches(r#""reviewer""#).count(), 2, "{saved}");
    }

    #[test]
    fn inline_credentials_are_never_accepted() {
        let cfg = parse_with_roles(&format!(
            r#"{{
                "planner": {{"provider": "openai", "model": "gpt-4o-mini", "api_key": "{SECRET}"}},
                "cheap": "openai/gpt-4o-mini"
            }}"#
        ));
        // The rest of the config loaded: a bad role must not take the primary
        // provider down with it.
        assert_eq!(cfg.providers.len(), 1);

        let error = cfg.resolve_role("planner").unwrap_err();
        assert_eq!(
            error,
            RoleError::InvalidRoleEntry {
                role: "planner".to_string()
            }
        );
        assert!(!error.to_string().contains(SECRET));
        assert_eq!(format!("{:?}", cfg.roles), "RolesConfig(set)");
        // Asked for by name: still an error, never "off".
        assert_eq!(cfg.resolve_model_target("planner").unwrap_err(), error);

        // It is not listed as configured, and a well-formed neighbour works.
        assert_eq!(cfg.roles.names(), ["cheap"]);
        assert!(cfg.resolve_role("cheap").unwrap().is_some());
    }

    #[test]
    fn no_value_in_roles_can_stop_the_config_from_loading() {
        let deep = format!("{}{}", "[".repeat(200), "]".repeat(200));
        let deep_object = format!(r#"{{"planner": {deep}}}"#);
        for roles in [
            // The wrong shape altogether.
            r#"5"#,
            r#"-1.5"#,
            r#""openai/gpt-4o""#,
            r#"["openai/gpt-4o", 2]"#,
            r#"true"#,
            // Valid JSON that a typed section would reject outright.
            r#"{"planner": 1e999}"#,
            r#"{"planner": "\ud800"}"#,
            deep.as_str(),
            deep_object.as_str(),
        ] {
            let cfg = parse_with_roles(roles);
            // The primary provider is intact...
            assert_eq!(cfg.get_selected_provider().unwrap().provider, "openai");
            // ...and every role fails closed.
            assert_eq!(
                cfg.resolve_role("planner").unwrap_err(),
                RoleError::MalformedRoles,
                "{}",
                &roles[..roles.len().min(40)]
            );
            assert!(cfg.resolve_model_target("planner").is_err());
            assert!(cfg.roles.names().is_empty());
            assert!(!cfg.roles.is_unset());

            // Saving writes the section back as it was.
            let saved = serde_json::to_string(&cfg).unwrap();
            assert!(saved.ends_with(&format!(r#""roles":{roles}}}"#)));

            // The same holds when the file is read from disk, comments and all.
            let dir = tempfile::TempDir::new().unwrap();
            let path = dir.path().join("auth.config.json");
            std::fs::write(&path, format!("// mine\n{}", file_with_roles(roles))).unwrap();
            let loaded = ShaiConfig::load_from_paths(&path, &dir.path().join("none")).unwrap();
            assert_eq!(loaded.roles, cfg.roles);
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_config_file_that_is_there_but_cannot_be_reached_is_not_a_missing_one() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("auth.config.json");
        let legacy = dir.path().join("auth.config");
        // A legacy file that would load fine must not stand in for it.
        std::fs::write(
            &legacy,
            r#"{"providers": [], "selected_provider": 0, "roles": {"legacy": "openai/gpt-4o"}}"#,
        )
        .unwrap();

        // A link to nothing: there is a config file, and it cannot be read.
        std::os::unix::fs::symlink(dir.path().join("gone.json"), &path).unwrap();
        let error = ShaiConfig::load_for_roles_from(&path, &legacy).unwrap_err();
        assert!(
            matches!(&error, RoleError::ConfigInvalid { path, .. } if path.ends_with("auth.config.json")),
            "{error:?}"
        );

        // The same for a file that is not JSON text at all.
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, b"{\"providers\": [], \"selected_provider\": 0, \"roles\": {\"a\": \"openai/qq\xE8qq\"}}").unwrap();
        let error = ShaiConfig::load_for_roles_from(&path, &legacy).unwrap_err();
        assert!(
            matches!(error, RoleError::ConfigInvalid { .. }),
            "{error:?}"
        );
        assert!(!error.to_string().contains("qq"), "{error}");

        // With the file gone, the legacy one counts again.
        std::fs::remove_file(&path).unwrap();
        let loaded = ShaiConfig::load_for_roles_from(&path, &legacy).unwrap();
        assert_eq!(loaded.roles.names(), ["legacy"]);
    }

    #[test]
    fn a_repeated_roles_section_loads_and_fails_closed() {
        // Before roles existed the key was unknown, and ignored however many
        // times it was written. Writing it twice must still not stop the file
        // from loading.
        let file = r#"{
            // pasted twice
            "roles": {"planner": "openai/gpt-4o-mini"},
            "providers": [
                {"provider": "openai", "env_vars": {"OPENAI_API_KEY": "k"}, "model": "gpt-4o", "tool_method": "FunctionCall"}
            ],
            "selected_provider": 0,
            "roles": {"planner": "openai/o4-mini"}
        }"#;
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("auth.config.json");
        let none = dir.path().join("none");
        std::fs::write(&path, file).unwrap();

        let cfg = ShaiConfig::load_from_paths(&path, &none).unwrap();
        assert_eq!(cfg.get_selected_provider().unwrap().provider, "openai");

        // Neither copy is used: taking one of them would be a guess.
        assert!(!cfg.roles.is_unset());
        assert!(cfg.roles.names().is_empty());
        for result in [
            cfg.resolve_role("planner").map(|_| ()),
            cfg.resolve_role("other").map(|_| ()),
            cfg.resolve_model_target("planner").map(|_| ()),
        ] {
            assert_eq!(result.unwrap_err(), RoleError::RepeatedRoles);
        }
        // A reference that does not go through `roles` is not affected.
        assert_eq!(
            label(cfg.resolve_model_ref("openai/gpt-4o")),
            "openai/gpt-4o"
        );

        // The same through the loader the features use.
        let loaded = ShaiConfig::load_for_roles_from(&path, &none).unwrap();
        assert_eq!(
            loaded.resolve_role("planner").unwrap_err(),
            RoleError::RepeatedRoles
        );

        // Saving keeps both copies: the mistake stays visible in the file and
        // the section stays unusable until one of them is removed.
        let saved = serde_json::to_string(&cfg).unwrap();
        assert_eq!(saved.matches(r#""roles":"#).count(), 2, "{saved}");
        let reloaded: ShaiConfig = serde_json::from_str(&saved).unwrap();
        assert_eq!(reloaded.roles, cfg.roles);
        assert_eq!(
            reloaded.resolve_role("planner").unwrap_err(),
            RoleError::RepeatedRoles
        );

        // A copy that is `null` does not make the other one usable.
        for roles in [
            r#""roles": null, "roles": {"a": "openai/x"}"#,
            r#""roles": {"a": "openai/x"}, "roles": null"#,
        ] {
            let cfg: ShaiConfig = serde_json::from_str(&format!(
                r#"{{"providers": [], "selected_provider": 0, {roles}}}"#
            ))
            .unwrap();
            assert_eq!(cfg.resolve_role("a").unwrap_err(), RoleError::RepeatedRoles);
        }
    }

    #[test]
    fn same_provider_twice_resolves_only_when_unambiguous() {
        let host_a = ("OPENAI_COMPATIBLE_BASE_URL", "https://a.example.com/v1");
        let host_b = ("OPENAI_COMPATIBLE_BASE_URL", "https://b.example.com/v1");
        let key_a = ("OPENAI_COMPATIBLE_API_KEY", "key-a");
        let key_b = ("OPENAI_COMPATIBLE_API_KEY", "key-b");
        let cfg = config(
            vec![
                provider("openai_compatible", "model-a", &[host_a, key_a]),
                provider("openai_compatible", "model-b", &[host_b, key_b]),
            ],
            &[],
        );

        // A model one entry is configured for picks that entry's endpoint.
        let b = cfg.resolve_model_ref("openai_compatible/model-b").unwrap();
        assert_eq!(b.provider.env_vars, cfg.providers[1].env_vars);

        // Any other model could belong to either host: refuse to guess.
        let error = cfg
            .resolve_model_ref("openai_compatible/model-c")
            .unwrap_err();
        assert_eq!(
            error,
            RoleError::AmbiguousProvider {
                origin: "\"openai_compatible/model-c\"".to_string(),
                provider: "openai_compatible".to_string(),
                count: 2,
                exact_model: false,
            }
        );
        assert!(
            error.to_string().contains(
                "that are not interchangeable, and none of them is configured for that model"
            ),
            "{error}"
        );

        // Same provider and model on two different endpoints: ambiguous too,
        // and the message says what is actually the case.
        let cfg = config(
            vec![
                provider("openai_compatible", "model-a", &[host_a, key_a]),
                provider("openai_compatible", "model-a", &[host_b, key_b]),
            ],
            &[("planner", "openai_compatible/model-a")],
        );
        let error = cfg.resolve_role("planner").unwrap_err();
        assert!(
            matches!(
                error,
                RoleError::AmbiguousProvider {
                    count: 2,
                    exact_model: true,
                    ..
                }
            ),
            "{error:?}"
        );
        assert!(
            error
                .to_string()
                .contains("are configured for that model with different settings"),
            "{error}"
        );
    }

    #[test]
    fn entries_that_build_the_same_client_are_not_ambiguous() {
        let host_a = ("OPENAI_COMPATIBLE_BASE_URL", "https://a.example.com/v1");
        let host_b = ("OPENAI_COMPATIBLE_BASE_URL", "https://b.example.com/v1");
        let key = ("OPENAI_COMPATIBLE_API_KEY", "key-a");
        // They differ by model, by a setting the provider does not read, and
        // by a blank value: none of that changes the client.
        let cfg = config(
            vec![
                provider("openai_compatible", "model-a", &[host_a, key]),
                provider(
                    "openai_compatible",
                    "model-b",
                    &[host_a, key, ("UNRELATED", "x"), ("OPENAI_API_KEY", "")],
                ),
            ],
            &[],
        );
        let c = cfg.resolve_model_ref("openai_compatible/model-c").unwrap();
        assert_eq!(c.model(), "model-c");

        // Two identical entries for the model, next to a different third:
        // the model singles the pair out, and the pair agrees.
        let cfg = config(
            vec![
                provider("openai_compatible", "other", &[host_b, key]),
                provider("openai_compatible", "model-a", &[host_a, key]),
                provider("openai_compatible", "model-a", &[host_a, key]),
            ],
            &[],
        );
        let a = cfg.resolve_model_ref("openai_compatible/model-a").unwrap();
        assert_eq!(a.provider.env_vars, cfg.providers[1].env_vars);
        // Without the model to single them out, the third makes it ambiguous.
        assert!(matches!(
            cfg.resolve_model_ref("openai_compatible/model-c"),
            Err(RoleError::AmbiguousProvider { count: 3, .. })
        ));
    }

    #[test]
    fn explicit_target_is_a_role_name_or_a_reference() {
        let cfg = config(vec![openai()], &[("cheap", "openai/gpt-4o-mini")]);

        assert_eq!(
            label(cfg.resolve_model_target("cheap")),
            "openai/gpt-4o-mini"
        );
        assert_eq!(
            label(cfg.resolve_model_target("openai/o4-mini")),
            "openai/o4-mini"
        );

        // Asked for by name, so "not configured" is an error rather than "off".
        let error = cfg.resolve_model_target("planner").unwrap_err();
        assert_eq!(
            error,
            RoleError::UnknownRole {
                role: "planner".to_string(),
                known: vec!["cheap".to_string()],
            }
        );
        assert!(cfg.resolve_model_target("mistral/small").is_err());

        // A model typed without its provider gets pointed at the right form,
        // whether or not it could have been a role name.
        for target in ["gpt-4o", "gpt-4.1"] {
            let error = cfg.resolve_model_target(target).unwrap_err();
            assert!(
                matches!(
                    error,
                    RoleError::UnknownRole { .. } | RoleError::InvalidTarget { .. }
                ),
                "{target}: {error:?}"
            );
            assert!(
                error.to_string().contains("<provider>/<model>"),
                "{target}: {error}"
            );
        }
    }

    #[test]
    fn incomplete_provider_entry_cannot_back_a_role() {
        let cfg = config(
            vec![provider("anthropic", "claude-sonnet-4-5", &[])],
            &[("planner", "anthropic/claude-haiku-4-5")],
        );
        let error = cfg.resolve_role("planner").unwrap_err();
        assert!(
            matches!(error, RoleError::UnusableProvider { .. }),
            "{error:?}"
        );
        let message = error.to_string();
        assert!(message.contains("role \"planner\""), "{message}");
        assert!(message.contains("ANTHROPIC_API_KEY"), "{message}");

        // An ollama entry has to say where it points.
        let cfg = config(vec![provider("ollama", "llama3", &[])], &[]);
        let error = cfg.resolve_model_ref("ollama/qwen3:8b").unwrap_err();
        assert!(error.to_string().contains("OLLAMA_BASE_URL"), "{error}");
    }

    #[test]
    fn debug_output_never_contains_credentials() {
        let cfg = config(vec![openai()], &[("planner", "openai/gpt-4o-mini")]);
        let planner = cfg.resolve_role("planner").unwrap().unwrap();
        let bound = planner.bind().unwrap();
        for debug in [format!("{planner:?}"), format!("{bound:?}")] {
            assert!(debug.contains("gpt-4o-mini"), "{debug}");
            assert!(!debug.contains(SECRET), "{debug}");
        }
    }

    // ---- no network during resolution ---------------------------------------

    /// Resolving a role and binding its client must not contact the provider:
    /// no model listing, no probe. Every entry points at a local mock server
    /// that fails the test if it receives anything at all.
    #[tokio::test]
    async fn resolution_makes_no_network_call() {
        let mut server = mockito::Server::new_async().await;
        let mut guards = Vec::new();
        for method in ["GET", "POST", "HEAD", "OPTIONS", "PUT", "DELETE", "PATCH"] {
            guards.push(
                server
                    .mock(method, mockito::Matcher::Any)
                    .expect(0)
                    .create_async()
                    .await,
            );
        }

        let base_url = format!("{}/v1", server.url());
        let cfg = config(
            vec![
                provider(
                    "openai_compatible",
                    "model-a",
                    &[
                        ("OPENAI_COMPATIBLE_API_KEY", "k"),
                        ("OPENAI_COMPATIBLE_BASE_URL", &base_url),
                    ],
                ),
                provider("ollama", "llama3", &[("OLLAMA_BASE_URL", &base_url)]),
                provider(
                    "ovhcloud",
                    "Qwen3-32B",
                    &[("OVH_API_KEY", "k"), ("OVH_BASE_URL", &base_url)],
                ),
            ],
            &[
                ("planner", "openai_compatible/model-b"),
                ("cheap", "ollama/qwen3:8b"),
                ("third", "ovhcloud/some-model"),
                ("broken", "anthropic/claude-sonnet-4-5"),
            ],
        );

        let mut bound = Vec::new();
        for role in ["planner", "cheap", "third"] {
            let resolved = cfg.resolve_role(role).unwrap().expect("configured");
            bound.push(resolved.bind().expect("client"));
            bound.push(cfg.resolve_model_target(role).unwrap().bind().unwrap());
        }
        assert!(cfg.resolve_role("broken").is_err());
        assert!(cfg.resolve_role("unset").unwrap().is_none());
        bound.push(
            cfg.resolve_model_target("ollama/another")
                .unwrap()
                .bind()
                .unwrap(),
        );

        // Give anything started in the background a chance to reach the
        // server before the guards are checked.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        for guard in guards {
            guard.assert_async().await;
        }
        drop(bound);
    }

    /// The guard above is only meaningful if a request would trip it.
    #[tokio::test]
    async fn the_network_guard_detects_a_request() {
        let mut server = mockito::Server::new_async().await;
        let hit = server
            .mock("GET", mockito::Matcher::Any)
            .with_status(200)
            .with_body(r#"{"object":"list","data":[]}"#)
            .expect(1)
            .create_async()
            .await;

        let base_url = format!("{}/v1", server.url());
        let cfg = config(
            vec![provider(
                "ollama",
                "llama3",
                &[("OLLAMA_BASE_URL", &base_url)],
            )],
            &[("cheap", "ollama/qwen3:8b")],
        );
        let bound = cfg.resolve_role("cheap").unwrap().unwrap().bind().unwrap();
        // An explicit request does reach the server the entry points at.
        let _ = bound.llm().models().await;
        hit.assert_async().await;
    }
}
