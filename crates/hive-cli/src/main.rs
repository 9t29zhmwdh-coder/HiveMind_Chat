//! Terminal client for HiveMind Chat.
//!
//! Talks to the same database and orchestrator as the server, so a room can be
//! set up, run and exported without starting one.

mod render;
mod rooms;

use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use hive_core::{HiveConfig, Orchestrator, ProviderRegistry, Store, TurnPolicy};
use tokio::sync::mpsc;

#[derive(Parser, Debug)]
#[command(
    name = "hive",
    version,
    about = "Run and inspect HiveMind Chat rooms from the terminal"
)]
struct Args {
    /// Path to the TOML configuration file.
    #[arg(
        long,
        default_value = "hivemind.toml",
        env = "HIVEMIND_CONFIG",
        global = true
    )]
    config: PathBuf,

    /// SQLite database file, overriding the configuration file.
    #[arg(long, env = "HIVEMIND_DATABASE", global = true)]
    database: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// List configured providers and whether their credential resolves.
    Providers,
    /// List the models a provider currently serves.
    Models {
        /// Provider id from the configuration file, for example `local`.
        provider_id: String,
    },
    /// List the rooms in the database.
    Rooms,
    /// Create a room.
    NewRoom {
        /// Name of the room.
        name: String,
        /// One of: parallel, round_robin, debate, moderated, consensus.
        #[arg(long, default_value = "round_robin")]
        policy: String,
        /// What the room is about. Included in every agent's prompt.
        #[arg(long, default_value = "")]
        topic: String,
        /// How many times each agent speaks per prompt.
        #[arg(long, default_value_t = 1)]
        rounds: u32,
    },
    /// Add an agent to a room.
    AddAgent {
        /// Room to add the agent to.
        room_id: String,
        /// Display name, used as the speaker label in the transcript.
        name: String,
        /// Provider id from the configuration file.
        provider_id: String,
        /// Model name as the provider serves it, for example `llama3:8b`.
        model: String,
        /// How this agent should behave and what it should argue for.
        #[arg(long, default_value = "")]
        persona: String,
        /// Let the model reason before answering. Slower, and the reasoning is
        /// drawn from the same token budget as the answer.
        #[arg(long)]
        reasoning: bool,
    },
    /// Show a room and its transcript.
    Show {
        /// Room to show.
        room_id: String,
        /// How many of the most recent messages to print.
        #[arg(long, default_value_t = 50)]
        limit: u32,
    },
    /// Send a prompt to a room and stream the conversation.
    Chat {
        /// Room to talk to.
        room_id: String,
        /// What to ask the room.
        prompt: String,
    },
    /// Print the transcript as Markdown.
    Export {
        /// Room to export.
        room_id: String,
        /// How many of the most recent messages to include.
        #[arg(long, default_value_t = 500)]
        limit: u32,
    },
    /// Copy a room's line-up into a new room, without its transcript.
    DuplicateRoom {
        /// Room whose line-up should be copied.
        room_id: String,
        /// Name for the copy. Defaults to the original plus "(copy)".
        #[arg(long)]
        name: Option<String>,
    },
    /// Delete a room and its transcript.
    DeleteRoom {
        /// Room to delete. This cannot be undone.
        room_id: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let config = HiveConfig::load_or_default(&args.config)
        .with_context(|| format!("cannot load {}", args.config.display()))?;
    let database = args
        .database
        .unwrap_or_else(|| PathBuf::from(&config.server.database));
    let store =
        Store::open(&database).with_context(|| format!("cannot open {}", database.display()))?;

    match args.command {
        Command::Providers => rooms::list_providers(&config),
        Command::Models { provider_id } => list_models(&config, &provider_id).await,
        Command::Rooms => rooms::list_rooms(&store).await,
        Command::NewRoom {
            name,
            policy,
            topic,
            rounds,
        } => rooms::create_room(&store, &name, &policy, &topic, rounds).await,
        Command::AddAgent {
            room_id,
            name,
            provider_id,
            model,
            persona,
            reasoning,
        } => {
            rooms::add_agent(
                &store,
                &config,
                &room_id,
                &name,
                &provider_id,
                &model,
                &persona,
                reasoning,
            )
            .await
        }
        Command::Show { room_id, limit } => rooms::show_room(&store, &room_id, limit).await,
        Command::Chat { room_id, prompt } => chat(&store, &config, &room_id, &prompt).await,
        Command::Export { room_id, limit } => rooms::export(&store, &room_id, limit).await,
        Command::DuplicateRoom { room_id, name } => {
            rooms::duplicate_room(&store, &room_id, name.as_deref()).await
        }
        Command::DeleteRoom { room_id } => rooms::delete_room(&store, &room_id).await,
    }
}

async fn list_models(config: &HiveConfig, provider_id: &str) -> Result<()> {
    let registry = ProviderRegistry::from_config(config)?;
    let mut models = registry.get(provider_id)?.list_models().await?;
    models.sort();
    for model in models {
        println!("{model}");
    }
    Ok(())
}

/// Runs one prompt through a room, printing the conversation as it streams.
async fn chat(store: &Store, config: &HiveConfig, room_id: &str, prompt: &str) -> Result<()> {
    let room = store.load_room(room_id).await?;
    let history = store.load_messages(room_id, 200).await?;
    let orchestrator = Orchestrator::new(ProviderRegistry::from_config(config)?);

    let (tx, mut rx) = mpsc::channel(256);
    let printer = tokio::spawn(async move {
        let mut renderer = render::Renderer::default();
        while let Some(event) = rx.recv().await {
            renderer.handle(&event);
            let _ = std::io::stdout().flush();
        }
    });

    let produced = orchestrator.run(&room, &history, prompt, &tx).await;
    drop(tx);
    let _ = printer.await;

    store.append_messages(&produced?).await?;
    Ok(())
}

/// Parses a policy name, listing the valid ones when it does not match.
pub(crate) fn parse_policy(raw: &str) -> Result<TurnPolicy> {
    TurnPolicy::parse(raw).map_err(|_| {
        let known: Vec<&str> = TurnPolicy::ALL.iter().map(|p| p.as_str()).collect();
        anyhow::anyhow!(
            "unknown policy '{raw}'. Valid policies: {}",
            known.join(", ")
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_policies_parse() {
        assert_eq!(parse_policy("debate").unwrap(), TurnPolicy::Debate);
        assert_eq!(parse_policy("round_robin").unwrap(), TurnPolicy::RoundRobin);
    }

    #[test]
    fn an_unknown_policy_lists_the_valid_ones() {
        let error = parse_policy("shouting_match").unwrap_err().to_string();
        assert!(error.contains("shouting_match"));
        assert!(error.contains("consensus"));
    }
}
