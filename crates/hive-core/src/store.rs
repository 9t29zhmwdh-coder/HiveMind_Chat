//! SQLite persistence for rooms, agents and transcripts.
//!
//! Every public method runs the actual query on a blocking thread, so the
//! server's async runtime is never blocked by disk IO.

use std::path::Path;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

use crate::error::{HiveError, Result};
use crate::model::{Agent, Message, Role, Room, TokenUsage, TurnPolicy};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS rooms (
    id           TEXT PRIMARY KEY,
    name         TEXT NOT NULL,
    topic        TEXT NOT NULL DEFAULT '',
    policy       TEXT NOT NULL,
    rounds       INTEGER NOT NULL DEFAULT 1,
    moderator_id TEXT,
    created_at   TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS agents (
    id          TEXT PRIMARY KEY,
    room_id     TEXT NOT NULL REFERENCES rooms(id) ON DELETE CASCADE,
    position    INTEGER NOT NULL,
    name        TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    model       TEXT NOT NULL,
    persona     TEXT NOT NULL DEFAULT '',
    temperature REAL NOT NULL,
    max_tokens  INTEGER NOT NULL,
    colour      TEXT NOT NULL,
    reasoning   INTEGER NOT NULL DEFAULT 0,
    enabled     INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS messages (
    id            TEXT PRIMARY KEY,
    room_id       TEXT NOT NULL REFERENCES rooms(id) ON DELETE CASCADE,
    role          TEXT NOT NULL,
    speaker       TEXT NOT NULL,
    agent_id      TEXT,
    content       TEXT NOT NULL,
    created_at    TEXT NOT NULL,
    round         INTEGER NOT NULL DEFAULT 0,
    input_tokens  INTEGER,
    output_tokens INTEGER
);

CREATE INDEX IF NOT EXISTS idx_messages_room ON messages(room_id, created_at);
CREATE INDEX IF NOT EXISTS idx_agents_room ON agents(room_id, position);
"#;

/// A room without its transcript, for list views.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomSummary {
    pub id: String,
    pub name: String,
    pub topic: String,
    pub policy: TurnPolicy,
    pub agents: usize,
    pub messages: usize,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct Store {
    connection: Arc<Mutex<Connection>>,
}

impl Store {
    /// Opens (and if needed creates) the database at `path`.
    pub fn open(path: &Path) -> Result<Self> {
        let connection = Connection::open(path)?;
        Self::prepare(&connection)?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    /// An in-memory database, used by the tests and by `--ephemeral` runs.
    pub fn in_memory() -> Result<Self> {
        let connection = Connection::open_in_memory()?;
        Self::prepare(&connection)?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    fn prepare(connection: &Connection) -> Result<()> {
        // WAL keeps a reader from blocking the writer, which matters as soon as
        // the web UI polls the transcript while a turn is still streaming.
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.execute_batch(SCHEMA)?;
        Ok(())
    }

    /// Runs `work` on the blocking pool with the shared connection.
    async fn with_connection<T, F>(&self, work: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T> + Send + 'static,
    {
        let connection = Arc::clone(&self.connection);
        tokio::task::spawn_blocking(move || {
            let mut guard = connection
                .lock()
                .map_err(|_| HiveError::Storage("the database lock was poisoned".into()))?;
            work(&mut guard)
        })
        .await
        .map_err(|e| HiveError::Storage(format!("database task failed: {e}")))?
    }

    pub async fn save_room(&self, room: &Room) -> Result<()> {
        room.validate()?;
        let room = room.clone();
        self.with_connection(move |connection| {
            let tx = connection.transaction()?;
            write_room(&tx, &room)?;
            tx.execute("DELETE FROM agents WHERE room_id = ?1", params![room.id])?;
            for (position, agent) in room.agents.iter().enumerate() {
                write_agent(&tx, &room.id, position, agent)?;
            }
            tx.commit()?;
            Ok(())
        })
        .await
    }

    pub async fn load_room(&self, room_id: &str) -> Result<Room> {
        let room_id = room_id.to_string();
        self.with_connection(move |connection| {
            let mut room = connection
                .query_row(
                    "SELECT * FROM rooms WHERE id = ?1",
                    params![room_id],
                    read_room,
                )
                .optional()?
                .ok_or_else(|| HiveError::UnknownRoom(room_id.clone()))?;
            room.agents = read_agents(connection, &room_id)?;
            Ok(room)
        })
        .await
    }

    pub async fn list_rooms(&self) -> Result<Vec<RoomSummary>> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT r.id, r.name, r.topic, r.policy, r.created_at,
                        (SELECT COUNT(*) FROM agents a WHERE a.room_id = r.id),
                        (SELECT COUNT(*) FROM messages m WHERE m.room_id = r.id)
                 FROM rooms r ORDER BY r.created_at DESC",
            )?;
            let rows = statement.query_map([], |row| {
                Ok(RoomSummary {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    topic: row.get(2)?,
                    policy: policy_from_row(row, 3)?,
                    created_at: row.get(4)?,
                    agents: row.get::<_, i64>(5)? as usize,
                    messages: row.get::<_, i64>(6)? as usize,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .await
    }

    pub async fn delete_room(&self, room_id: &str) -> Result<()> {
        let room_id = room_id.to_string();
        self.with_connection(move |connection| {
            let affected =
                connection.execute("DELETE FROM rooms WHERE id = ?1", params![room_id])?;
            match affected {
                0 => Err(HiveError::UnknownRoom(room_id)),
                _ => Ok(()),
            }
        })
        .await
    }

    pub async fn append_messages(&self, messages: &[Message]) -> Result<()> {
        let messages = messages.to_vec();
        self.with_connection(move |connection| {
            let tx = connection.transaction()?;
            for message in &messages {
                write_message(&tx, message)?;
            }
            tx.commit()?;
            Ok(())
        })
        .await
    }

    /// The newest `limit` messages of a room, oldest first.
    pub async fn load_messages(&self, room_id: &str, limit: u32) -> Result<Vec<Message>> {
        let room_id = room_id.to_string();
        self.with_connection(move |connection| {
            let mut statement = connection.prepare(
                // `rowid` is aliased explicitly: a bare `SELECT *` does not carry
                // it into the outer query, and it is what breaks ties between
                // messages written within the same timestamp resolution.
                "SELECT * FROM (
                     SELECT *, rowid AS seq FROM messages WHERE room_id = ?1
                     ORDER BY created_at DESC, seq DESC LIMIT ?2
                 ) ORDER BY created_at ASC, seq ASC",
            )?;
            let rows = statement.query_map(params![room_id, limit], read_message)?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .await
    }

    pub async fn clear_messages(&self, room_id: &str) -> Result<()> {
        let room_id = room_id.to_string();
        self.with_connection(move |connection| {
            connection.execute("DELETE FROM messages WHERE room_id = ?1", params![room_id])?;
            Ok(())
        })
        .await
    }
}

fn write_room(connection: &Connection, room: &Room) -> rusqlite::Result<usize> {
    connection.execute(
        "INSERT INTO rooms (id, name, topic, policy, rounds, moderator_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(id) DO UPDATE SET
             name = excluded.name, topic = excluded.topic, policy = excluded.policy,
             rounds = excluded.rounds, moderator_id = excluded.moderator_id",
        params![
            room.id,
            room.name,
            room.topic,
            room.policy.as_str(),
            room.rounds,
            room.moderator_id,
            room.created_at
        ],
    )
}

fn write_agent(
    connection: &Connection,
    room_id: &str,
    position: usize,
    agent: &Agent,
) -> rusqlite::Result<usize> {
    connection.execute(
        "INSERT INTO agents (id, room_id, position, name, provider_id, model, persona,
                             temperature, max_tokens, colour, reasoning, enabled)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            agent.id,
            room_id,
            position as i64,
            agent.name,
            agent.provider_id,
            agent.model,
            agent.persona,
            agent.temperature,
            agent.max_tokens,
            agent.colour,
            agent.reasoning,
            agent.enabled
        ],
    )
}

fn write_message(connection: &Connection, message: &Message) -> rusqlite::Result<usize> {
    connection.execute(
        "INSERT OR REPLACE INTO messages
             (id, room_id, role, speaker, agent_id, content, created_at, round, input_tokens, output_tokens)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            message.id,
            message.room_id,
            message.role.as_str(),
            message.speaker,
            message.agent_id,
            message.content,
            message.created_at,
            message.round,
            message.usage.map(|u| u.input_tokens),
            message.usage.map(|u| u.output_tokens)
        ],
    )
}

fn read_room(row: &Row) -> rusqlite::Result<Room> {
    Ok(Room {
        id: row.get("id")?,
        name: row.get("name")?,
        topic: row.get("topic")?,
        policy: policy_from_row(row, "policy")?,
        rounds: row.get("rounds")?,
        moderator_id: row.get("moderator_id")?,
        agents: Vec::new(),
        created_at: row.get("created_at")?,
    })
}

fn read_agents(connection: &Connection, room_id: &str) -> rusqlite::Result<Vec<Agent>> {
    let mut statement =
        connection.prepare("SELECT * FROM agents WHERE room_id = ?1 ORDER BY position")?;
    let rows = statement.query_map(params![room_id], |row| {
        Ok(Agent {
            id: row.get("id")?,
            name: row.get("name")?,
            provider_id: row.get("provider_id")?,
            model: row.get("model")?,
            persona: row.get("persona")?,
            temperature: row.get("temperature")?,
            max_tokens: row.get("max_tokens")?,
            colour: row.get("colour")?,
            reasoning: row.get("reasoning")?,
            enabled: row.get("enabled")?,
        })
    })?;
    rows.collect()
}

fn read_message(row: &Row) -> rusqlite::Result<Message> {
    let input: Option<u32> = row.get("input_tokens")?;
    let output: Option<u32> = row.get("output_tokens")?;
    Ok(Message {
        id: row.get("id")?,
        room_id: row.get("room_id")?,
        role: role_from_row(row, "role")?,
        speaker: row.get("speaker")?,
        agent_id: row.get("agent_id")?,
        content: row.get("content")?,
        created_at: row.get("created_at")?,
        round: row.get("round")?,
        usage: input
            .zip(output)
            .map(|(input_tokens, output_tokens)| TokenUsage {
                input_tokens,
                output_tokens,
            }),
    })
}

/// Maps a stored enum back, turning an unknown value into a column error so the
/// caller sees which row is corrupt instead of a silent default.
fn policy_from_row<I: rusqlite::RowIndex + Copy>(
    row: &Row,
    index: I,
) -> rusqlite::Result<TurnPolicy> {
    let raw: String = row.get(index)?;
    TurnPolicy::parse(&raw).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

fn role_from_row<I: rusqlite::RowIndex + Copy>(row: &Row, index: I) -> rusqlite::Result<Role> {
    let raw: String = row.get(index)?;
    Role::parse(&raw).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_room() -> Room {
        let mut room = Room::new("Lab", TurnPolicy::Debate);
        room.topic = "SQLite or Postgres".to_string();
        room.rounds = 3;
        room.agents.push(
            Agent::new("Scout", "local", "llama3:8b")
                .with_persona("You favour simplicity.")
                .with_colour("#8ab4f8"),
        );
        room.agents
            .push(Agent::new("Vera", "anthropic-main", "claude-opus-5").with_reasoning(true));
        room
    }

    #[tokio::test]
    async fn rooms_round_trip_with_their_agents() {
        let store = Store::in_memory().unwrap();
        let room = sample_room();
        store.save_room(&room).await.unwrap();

        let loaded = store.load_room(&room.id).await.unwrap();
        assert_eq!(loaded.name, "Lab");
        assert_eq!(loaded.policy, TurnPolicy::Debate);
        assert_eq!(loaded.rounds, 3);
        assert_eq!(loaded.agents.len(), 2);
        assert_eq!(loaded.agents[0].name, "Scout");
        assert_eq!(loaded.agents[0].persona, "You favour simplicity.");
        assert!(loaded.agents[1].reasoning);
    }

    #[tokio::test]
    async fn saving_twice_replaces_the_agent_list_instead_of_duplicating_it() {
        let store = Store::in_memory().unwrap();
        let mut room = sample_room();
        store.save_room(&room).await.unwrap();

        room.agents.pop();
        store.save_room(&room).await.unwrap();

        assert_eq!(store.load_room(&room.id).await.unwrap().agents.len(), 1);
    }

    #[tokio::test]
    async fn transcripts_are_returned_oldest_first_and_capped() {
        let store = Store::in_memory().unwrap();
        let room = sample_room();
        store.save_room(&room).await.unwrap();

        let messages: Vec<Message> = (0..5)
            .map(|i| Message::from_user(&room.id, format!("message {i}")))
            .collect();
        store.append_messages(&messages).await.unwrap();

        let loaded = store.load_messages(&room.id, 3).await.unwrap();
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded[0].content, "message 2");
        assert_eq!(loaded[2].content, "message 4");
    }

    #[tokio::test]
    async fn token_usage_survives_a_round_trip() {
        let store = Store::in_memory().unwrap();
        let room = sample_room();
        store.save_room(&room).await.unwrap();

        let mut message = Message::from_agent(&room.id, &room.agents[0], "Answer", 2);
        message.usage = Some(TokenUsage {
            input_tokens: 42,
            output_tokens: 7,
        });
        store.append_messages(&[message]).await.unwrap();

        let loaded = &store.load_messages(&room.id, 10).await.unwrap()[0];
        assert_eq!(loaded.usage.unwrap().input_tokens, 42);
        assert_eq!(loaded.round, 2);
    }

    #[tokio::test]
    async fn deleting_a_room_removes_its_transcript() {
        let store = Store::in_memory().unwrap();
        let room = sample_room();
        store.save_room(&room).await.unwrap();
        store
            .append_messages(&[Message::from_user(&room.id, "hello")])
            .await
            .unwrap();

        store.delete_room(&room.id).await.unwrap();
        assert!(store.load_room(&room.id).await.is_err());
        assert!(store.load_messages(&room.id, 10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn deleting_an_unknown_room_is_an_error() {
        let store = Store::in_memory().unwrap();
        assert!(matches!(
            store.delete_room("nope").await,
            Err(HiveError::UnknownRoom(_))
        ));
    }

    #[tokio::test]
    async fn summaries_count_agents_and_messages() {
        let store = Store::in_memory().unwrap();
        let room = sample_room();
        store.save_room(&room).await.unwrap();
        store
            .append_messages(&[Message::from_user(&room.id, "hello")])
            .await
            .unwrap();

        let summaries = store.list_rooms().await.unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].agents, 2);
        assert_eq!(summaries[0].messages, 1);
    }
}
