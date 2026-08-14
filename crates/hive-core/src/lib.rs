//! Orchestration core for HiveMind Chat.
//!
//! A [`Room`] holds [`Agent`]s, each bound to a configured provider. The
//! [`Orchestrator`] runs a user prompt through the room according to its
//! [`TurnPolicy`] and streams [`SessionEvent`]s while the agents answer.
//!
//! ```no_run
//! use hive_core::{Agent, HiveConfig, Orchestrator, ProviderRegistry, Room, TurnPolicy};
//!
//! # async fn example() -> hive_core::Result<()> {
//! let config = HiveConfig::local_default();
//! let registry = ProviderRegistry::from_config(&config)?;
//!
//! let mut room = Room::new("Design review", TurnPolicy::Debate);
//! room.agents.push(Agent::new("Scout", "local", "llama3:8b"));
//! room.agents.push(Agent::new("Vera", "local", "gemma4"));
//!
//! let (tx, mut rx) = tokio::sync::mpsc::channel(64);
//! let orchestrator = Orchestrator::new(registry);
//! orchestrator.run(&room, &[], "Should we ship this?", &tx).await?;
//! while let Some(event) = rx.recv().await {
//!     println!("{event:?}");
//! }
//! # Ok(())
//! # }
//! ```

pub mod config;
pub mod error;
pub mod model;
pub mod orchestrator;
pub mod provider;
pub mod secrets;
pub mod store;

pub use config::{HiveConfig, ProviderConfig, ProviderKind, ServerConfig, MAX_PROVIDERS};
pub use error::{HiveError, Result};
pub use model::{
    Agent, Message, Role, Room, TokenUsage, TurnPolicy, MAX_AGENTS_PER_ROOM, MAX_PROMPT_CHARS,
};
pub use orchestrator::{Orchestrator, SessionEvent};
pub use provider::{ChatChunk, ChatRequest, ChatTurn, ModelProvider, ProviderRegistry};
pub use secrets::{redact, SecretRef};
pub use store::{RoomSummary, Store};

/// The crate version, surfaced by the server's health endpoint.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
