//! Room, agent and provider subcommands.

use anyhow::{Context, Result};
use hive_core::{Agent, HiveConfig, ProviderRegistry, Room, Store};

/// Palette handed out to new agents so the web UI can tell them apart.
const PALETTE: [&str; 6] = [
    "#e8b339", "#4f9dde", "#59c08a", "#d97757", "#b07be0", "#5bc0be",
];

pub fn list_providers(config: &HiveConfig) -> Result<()> {
    if config.providers.is_empty() {
        println!("No providers configured.");
        return Ok(());
    }
    for provider in &config.providers {
        let credential = match provider.secret_ref()?.as_ref() {
            Some(secret) if secret.is_available() => format!("{} resolved", secret.var_name()),
            Some(secret) => format!("{} MISSING", secret.var_name()),
            None => "no credential needed".to_string(),
        };
        println!(
            "{:<20} {:<10} {:<40} {credential}",
            provider.id,
            format!("{:?}", provider.kind).to_lowercase(),
            provider.resolved_base_url()
        );
    }
    Ok(())
}

pub async fn list_rooms(store: &Store) -> Result<()> {
    let rooms = store.list_rooms().await?;
    if rooms.is_empty() {
        println!("No rooms yet. Create one with `hive new-room <name>`.");
        return Ok(());
    }
    for room in rooms {
        println!(
            "{}  {:<24} {:<12} {} agents, {} messages",
            room.id,
            room.name,
            room.policy.as_str(),
            room.agents,
            room.messages
        );
    }
    Ok(())
}

pub async fn create_room(
    store: &Store,
    name: &str,
    policy: &str,
    topic: &str,
    rounds: u32,
    context_limit: u32,
) -> Result<()> {
    let mut room = Room::new(name, crate::parse_policy(policy)?);
    room.topic = topic.to_string();
    room.rounds = rounds;
    room.context_limit = context_limit;
    store.save_room(&room).await?;
    println!("{}", room.id);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn add_agent(
    store: &Store,
    config: &HiveConfig,
    room_id: &str,
    name: &str,
    provider_id: &str,
    model: &str,
    persona: &str,
    reasoning: bool,
) -> Result<()> {
    let registry = ProviderRegistry::from_config(config)?;
    registry
        .get(provider_id)
        .with_context(|| format!("provider '{provider_id}' is not configured"))?;

    let mut room = store.load_room(room_id).await?;
    let colour = PALETTE[room.agents.len() % PALETTE.len()];
    let agent = Agent::new(name, provider_id, model)
        .with_persona(persona)
        .with_colour(colour)
        .with_reasoning(reasoning);
    room.agents.push(agent);
    store.save_room(&room).await?;

    println!("{name} joined {} ({} agents)", room.name, room.agents.len());
    Ok(())
}

pub async fn show_room(store: &Store, room_id: &str, limit: u32) -> Result<()> {
    let room = store.load_room(room_id).await?;
    let window = match room.context_limit {
        0 => "whole transcript".to_string(),
        limit => format!("last {limit} messages"),
    };
    println!(
        "{} · {} · {} round(s) · context: {window}",
        room.name,
        room.policy.as_str(),
        room.rounds
    );
    if !room.topic.trim().is_empty() {
        println!("Topic: {}", room.topic);
    }
    for agent in &room.agents {
        let state = if agent.enabled { "" } else { " (disabled)" };
        println!(
            "  - {} · {} · {}{state}",
            agent.name, agent.provider_id, agent.model
        );
    }

    let messages = store.load_messages(room_id, limit).await?;
    println!();
    for message in messages {
        println!("{}: {}\n", message.speaker, message.content);
    }
    Ok(())
}

pub async fn export(store: &Store, room_id: &str, limit: u32) -> Result<()> {
    let room = store.load_room(room_id).await?;
    let messages = store.load_messages(room_id, limit).await?;
    println!("# {}\n", room.name);
    if !room.topic.trim().is_empty() {
        println!("**Topic:** {}\n", room.topic);
    }
    for message in messages {
        println!(
            "### {} · {}\n\n{}\n",
            message.speaker,
            message.created_at.format("%Y-%m-%d %H:%M:%S UTC"),
            message.content
        );
    }
    Ok(())
}

pub async fn duplicate_room(store: &Store, room_id: &str, name: Option<&str>) -> Result<()> {
    let source = store.load_room(room_id).await?;
    let title = match name {
        Some(name) => name.to_string(),
        None => format!("{} (copy)", source.name),
    };
    let copy = source.duplicate(title);
    store.save_room(&copy).await?;
    println!("{}", copy.id);
    Ok(())
}

pub async fn delete_room(store: &Store, room_id: &str) -> Result<()> {
    store.delete_room(room_id).await?;
    println!("deleted {room_id}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hive_core::TurnPolicy;

    #[tokio::test]
    async fn agents_are_given_distinct_colours_until_the_palette_wraps() {
        let store = Store::in_memory().unwrap();
        let config = HiveConfig::local_default();
        let room = Room::new("Lab", TurnPolicy::RoundRobin);
        store.save_room(&room).await.unwrap();

        for index in 0..3 {
            add_agent(
                &store,
                &config,
                &room.id,
                &format!("Agent{index}"),
                "local",
                "llama3:8b",
                "",
                false,
            )
            .await
            .unwrap();
        }

        let loaded = store.load_room(&room.id).await.unwrap();
        let colours: Vec<&str> = loaded.agents.iter().map(|a| a.colour.as_str()).collect();
        assert_eq!(colours.len(), 3);
        assert_eq!(colours[0], PALETTE[0]);
        assert_eq!(colours[2], PALETTE[2]);
    }

    #[tokio::test]
    async fn adding_an_agent_with_an_unknown_provider_fails_before_saving() {
        let store = Store::in_memory().unwrap();
        let config = HiveConfig::local_default();
        let room = Room::new("Lab", TurnPolicy::RoundRobin);
        store.save_room(&room).await.unwrap();

        let result = add_agent(
            &store, &config, &room.id, "Ghost", "nowhere", "x", "", false,
        )
        .await;
        assert!(result.is_err());
        assert!(store.load_room(&room.id).await.unwrap().agents.is_empty());
    }

    #[tokio::test]
    async fn creating_a_room_persists_its_policy_and_rounds() {
        let store = Store::in_memory().unwrap();
        create_room(&store, "Lab", "consensus", "Databases", 3, 25)
            .await
            .unwrap();

        let summaries = store.list_rooms().await.unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].policy, TurnPolicy::Consensus);

        let room = store.load_room(&summaries[0].id).await.unwrap();
        assert_eq!(room.rounds, 3);
        assert_eq!(room.topic, "Databases");
        assert_eq!(room.context_limit, 25);
    }

    #[tokio::test]
    async fn creating_a_room_with_an_unknown_policy_fails() {
        let store = Store::in_memory().unwrap();
        assert!(create_room(&store, "Lab", "shouting", "", 1, 40)
            .await
            .is_err());
        assert!(store.list_rooms().await.unwrap().is_empty());
    }
}
