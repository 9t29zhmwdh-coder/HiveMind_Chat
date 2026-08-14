//! Model providers.
//!
//! A provider is one configured endpoint plus the dialect it speaks. Agents
//! reference providers by id, so several agents can share one credential and one
//! room can mix local and hosted models without the orchestrator knowing which
//! is which.

mod anthropic;
mod ollama;
mod openai;
mod sse;

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::Stream;
use serde::{Deserialize, Serialize};

use crate::config::{HiveConfig, ProviderConfig, ProviderKind};
use crate::error::{HiveError, Result};
use crate::model::Role;

pub use anthropic::AnthropicProvider;
pub use ollama::OllamaProvider;
pub use openai::OpenAiProvider;

/// One turn handed to a model. Deliberately narrower than [`crate::model::Message`]:
/// providers see roles and text, never room or agent identifiers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatTurn {
    pub role: Role,
    pub content: String,
}

impl ChatTurn {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub model: String,
    pub system: String,
    pub turns: Vec<ChatTurn>,
    pub temperature: f32,
    pub max_tokens: u32,
    /// See [`crate::model::Agent::reasoning`].
    pub reasoning: bool,
}

/// One piece of a streamed answer.
#[derive(Debug, Clone)]
pub enum ChatChunk {
    Delta(String),
    Done(crate::model::TokenUsage),
}

pub type ChatStream = Pin<Box<dyn Stream<Item = Result<ChatChunk>> + Send>>;

#[async_trait]
pub trait ModelProvider: Send + Sync {
    fn id(&self) -> &str;

    fn kind(&self) -> ProviderKind;

    /// Model identifiers the endpoint currently serves.
    async fn list_models(&self) -> Result<Vec<String>>;

    async fn chat(&self, request: ChatRequest) -> Result<ChatStream>;
}

/// Builds an HTTP client scoped to one provider.
///
/// The timeout is per request rather than per stream chunk, so a slow local
/// model streaming steadily is not cut off mid-answer.
pub(crate) fn build_client(config: &ProviderConfig) -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(config.timeout_secs.clamp(5, 600)))
        .user_agent(concat!("hivemind-chat/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| HiveError::provider(&config.id, e))
}

/// All configured providers, keyed by id.
#[derive(Clone, Default)]
pub struct ProviderRegistry {
    providers: HashMap<String, Arc<dyn ModelProvider>>,
}

impl ProviderRegistry {
    pub fn from_config(config: &HiveConfig) -> Result<Self> {
        config.validate()?;
        let mut providers = HashMap::new();
        for entry in &config.providers {
            providers.insert(entry.id.clone(), build_provider(entry)?);
        }
        Ok(Self { providers })
    }

    /// Registers a provider that does not come from the configuration file.
    ///
    /// This is how an embedder plugs in a dialect the crate does not ship, and
    /// how the integration tests drive the orchestrator without a network.
    pub fn insert(&mut self, provider: Arc<dyn ModelProvider>) {
        self.providers.insert(provider.id().to_string(), provider);
    }

    pub fn get(&self, id: &str) -> Result<Arc<dyn ModelProvider>> {
        self.providers
            .get(id)
            .cloned()
            .ok_or_else(|| HiveError::UnknownProvider(id.to_string()))
    }

    pub fn ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.providers.keys().cloned().collect();
        ids.sort();
        ids
    }

    pub fn len(&self) -> usize {
        self.providers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }
}

fn build_provider(config: &ProviderConfig) -> Result<Arc<dyn ModelProvider>> {
    let provider: Arc<dyn ModelProvider> = match config.kind {
        ProviderKind::Ollama => Arc::new(OllamaProvider::new(config)?),
        ProviderKind::Anthropic => Arc::new(AnthropicProvider::new(config)?),
        ProviderKind::OpenAi => Arc::new(OpenAiProvider::new(config)?),
    };
    Ok(provider)
}

/// Turns a non-success HTTP response into an error without echoing the body.
///
/// Provider error bodies have been observed to quote request headers, so only
/// the status line is surfaced to the caller and the body goes to the trace log.
pub(crate) async fn error_from_response(
    provider_id: &str,
    response: reqwest::Response,
) -> HiveError {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    tracing::debug!(provider = provider_id, %status, body = %body, "provider returned an error response");
    let hint = match status.as_u16() {
        401 | 403 => " (check the credential referenced by api_key_env)",
        404 => " (check the model name and base_url)",
        429 => " (rate limited by the provider)",
        _ => "",
    };
    HiveError::provider(provider_id, format!("HTTP {status}{hint}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProviderKind;

    #[test]
    fn registry_builds_every_configured_provider() {
        let config = HiveConfig {
            server: Default::default(),
            providers: vec![
                ProviderConfig::new("local", ProviderKind::Ollama),
                ProviderConfig::new("anthropic-main", ProviderKind::Anthropic)
                    .with_key_env("HIVEMIND_KEY_A"),
                ProviderConfig::new("groq", ProviderKind::OpenAi)
                    .with_base_url("https://api.groq.com/openai/v1")
                    .with_key_env("HIVEMIND_KEY_G"),
            ],
        };
        let registry = ProviderRegistry::from_config(&config).unwrap();
        assert_eq!(registry.len(), 3);
        assert_eq!(registry.ids(), vec!["anthropic-main", "groq", "local"]);
        assert!(registry.get("local").is_ok());
        assert!(matches!(
            registry.get("nope"),
            Err(HiveError::UnknownProvider(_))
        ));
    }

    #[test]
    fn registry_construction_fails_on_invalid_config() {
        let config = HiveConfig {
            server: Default::default(),
            providers: vec![ProviderConfig::new(
                "anthropic-main",
                ProviderKind::Anthropic,
            )],
        };
        assert!(ProviderRegistry::from_config(&config).is_err());
    }
}
