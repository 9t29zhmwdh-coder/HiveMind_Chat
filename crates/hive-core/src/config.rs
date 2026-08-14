use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{HiveError, Result};
use crate::secrets::SecretRef;

/// How many provider entries one instance accepts. Each cloud entry is a
/// separate credential, so this is the ceiling on how many distinct accounts can
/// meet in a single room.
pub const MAX_PROVIDERS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    /// Local Ollama daemon, no credential required.
    Ollama,
    /// Anthropic Messages API.
    Anthropic,
    /// Any endpoint speaking the OpenAI chat completions dialect, which covers
    /// LM Studio, vLLM, llama.cpp, Groq, Together and OpenAI itself.
    OpenAi,
}

impl ProviderKind {
    pub fn default_base_url(self) -> &'static str {
        match self {
            Self::Ollama => "http://127.0.0.1:11434",
            Self::Anthropic => "https://api.anthropic.com",
            Self::OpenAi => "https://api.openai.com/v1",
        }
    }

    pub fn requires_credential(self) -> bool {
        !matches!(self, Self::Ollama)
    }
}

/// One configured endpoint. Note that `api_key_env` holds a variable *name*,
/// never a key, and that the whole struct is safe to serialise to the web UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub id: String,
    #[serde(default)]
    pub label: String,
    pub kind: ProviderKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
}

fn default_timeout_secs() -> u64 {
    120
}

impl ProviderConfig {
    pub fn new(id: impl Into<String>, kind: ProviderKind) -> Self {
        Self {
            id: id.into(),
            label: String::new(),
            kind,
            base_url: None,
            api_key_env: None,
            timeout_secs: default_timeout_secs(),
        }
    }

    pub fn with_key_env(mut self, var_name: impl Into<String>) -> Self {
        self.api_key_env = Some(var_name.into());
        self
    }

    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    pub fn resolved_base_url(&self) -> String {
        let raw = self
            .base_url
            .clone()
            .unwrap_or_else(|| self.kind.default_base_url().to_string());
        raw.trim_end_matches('/').to_string()
    }

    pub fn secret_ref(&self) -> Result<Option<SecretRef>> {
        match &self.api_key_env {
            Some(name) => SecretRef::new(name).map(Some),
            None => Ok(None),
        }
    }

    /// True when the endpoint is reachable without a key leaving the machine.
    pub fn is_local(&self) -> bool {
        let url = self.resolved_base_url();
        url.contains("127.0.0.1") || url.contains("localhost") || url.contains("::1")
    }

    pub fn validate(&self) -> Result<()> {
        self.validate_id()?;
        self.validate_base_url()?;
        if self.kind.requires_credential() && self.api_key_env.is_none() {
            return Err(HiveError::Config(format!(
                "provider '{}' needs an api_key_env entry naming the environment variable that holds its key",
                self.id
            )));
        }
        self.secret_ref().map(|_| ())
    }

    fn validate_id(&self) -> Result<()> {
        let valid = !self.id.is_empty()
            && self.id.len() <= 48
            && self
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
        if valid {
            return Ok(());
        }
        Err(HiveError::Config(format!(
            "provider id '{}' must be lowercase letters, digits, hyphen or underscore",
            self.id
        )))
    }

    /// Only http and https are accepted, which keeps `file://` and other schemes
    /// out of a URL that is later used to build outbound requests.
    fn validate_base_url(&self) -> Result<()> {
        let url = self.resolved_base_url();
        if url.starts_with("http://") || url.starts_with("https://") {
            return Ok(());
        }
        Err(HiveError::Config(format!(
            "provider '{}' has base_url '{url}', expected an http:// or https:// address",
            self.id
        )))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default = "default_db_path")]
    pub database: String,
    /// Extra origins allowed to call the API. Empty means same-origin only,
    /// which is the right default when the server also ships the web UI.
    #[serde(default)]
    pub allowed_origins: Vec<String>,
}

fn default_bind() -> String {
    "127.0.0.1:8750".to_string()
}

fn default_db_path() -> String {
    "hivemind.db".to_string()
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: default_bind(),
            database: default_db_path(),
            allowed_origins: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HiveConfig {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub providers: Vec<ProviderConfig>,
}

impl HiveConfig {
    /// A config that works on a fresh machine: local Ollama, nothing else.
    pub fn local_default() -> Self {
        let mut ollama = ProviderConfig::new("local", ProviderKind::Ollama);
        ollama.label = "Ollama (local)".to_string();
        Self {
            server: ServerConfig::default(),
            providers: vec![ollama],
        }
    }

    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .map_err(|e| HiveError::Config(format!("cannot read {}: {e}", path.display())))?;
        let config: Self = toml::from_str(&raw)
            .map_err(|e| HiveError::Config(format!("cannot parse {}: {e}", path.display())))?;
        config.validate()?;
        Ok(config)
    }

    /// Loads the config, falling back to a local-only default when the file does
    /// not exist yet. A malformed file is still an error: silently replacing it
    /// would hide a typo in a provider entry.
    pub fn load_or_default(path: &Path) -> Result<Self> {
        if path.exists() {
            return Self::load(path);
        }
        Ok(Self::local_default())
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        let raw = toml::to_string_pretty(self)
            .map_err(|e| HiveError::Config(format!("cannot serialise config: {e}")))?;
        std::fs::write(path, raw)
            .map_err(|e| HiveError::Config(format!("cannot write {}: {e}", path.display())))
    }

    pub fn provider(&self, id: &str) -> Result<&ProviderConfig> {
        self.providers
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| HiveError::UnknownProvider(id.to_string()))
    }

    pub fn validate(&self) -> Result<()> {
        if self.providers.len() > MAX_PROVIDERS {
            return Err(HiveError::Config(format!(
                "at most {MAX_PROVIDERS} providers are supported, found {}",
                self.providers.len()
            )));
        }
        for provider in &self.providers {
            provider.validate()?;
        }
        self.validate_unique_ids()
    }

    fn validate_unique_ids(&self) -> Result<()> {
        let mut seen = std::collections::HashSet::new();
        for provider in &self.providers {
            if !seen.insert(&provider.id) {
                return Err(HiveError::Config(format!(
                    "duplicate provider id '{}'",
                    provider.id
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_provider_without_key_reference_is_rejected() {
        let provider = ProviderConfig::new("anthropic-main", ProviderKind::Anthropic);
        assert!(provider.validate().is_err());
        assert!(provider
            .with_key_env("HIVEMIND_KEY_ANTHROPIC")
            .validate()
            .is_ok());
    }

    #[test]
    fn ollama_needs_no_credential() {
        assert!(ProviderConfig::new("local", ProviderKind::Ollama)
            .validate()
            .is_ok());
    }

    #[test]
    fn non_http_base_urls_are_rejected() {
        let provider =
            ProviderConfig::new("local", ProviderKind::Ollama).with_base_url("file:///etc/passwd");
        assert!(provider.validate().is_err());
    }

    #[test]
    fn duplicate_provider_ids_are_rejected() {
        let config = HiveConfig {
            server: ServerConfig::default(),
            providers: vec![
                ProviderConfig::new("local", ProviderKind::Ollama),
                ProviderConfig::new("local", ProviderKind::Ollama),
            ],
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn provider_limit_is_enforced() {
        let providers = (0..MAX_PROVIDERS + 1)
            .map(|i| ProviderConfig::new(format!("p{i}"), ProviderKind::Ollama))
            .collect();
        let config = HiveConfig {
            server: ServerConfig::default(),
            providers,
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn serialised_config_contains_variable_names_not_keys() {
        let config = HiveConfig {
            server: ServerConfig::default(),
            providers: vec![
                ProviderConfig::new("anthropic-main", ProviderKind::Anthropic)
                    .with_key_env("HIVEMIND_KEY_ANTHROPIC"),
            ],
        };
        let raw = toml::to_string(&config).unwrap();
        assert!(raw.contains("HIVEMIND_KEY_ANTHROPIC"));
        assert!(!raw.contains("sk-"));
    }

    #[test]
    fn trailing_slash_in_base_url_is_normalised() {
        let provider = ProviderConfig::new("groq", ProviderKind::OpenAi)
            .with_base_url("https://api.groq.com/openai/v1/")
            .with_key_env("HIVEMIND_KEY_GROQ");
        assert_eq!(
            provider.resolved_base_url(),
            "https://api.groq.com/openai/v1"
        );
    }

    #[test]
    fn local_detection_covers_loopback_addresses() {
        assert!(ProviderConfig::new("local", ProviderKind::Ollama).is_local());
        let remote = ProviderConfig::new("groq", ProviderKind::OpenAi)
            .with_base_url("https://api.groq.com/openai/v1")
            .with_key_env("HIVEMIND_KEY_GROQ");
        assert!(!remote.is_local());
    }
}
