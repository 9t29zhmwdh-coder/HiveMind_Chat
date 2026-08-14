use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{HiveError, Result};

/// Upper bound on a single user message, enforced before anything reaches a provider.
pub const MAX_PROMPT_CHARS: usize = 32_000;

/// Upper bound on how many agents may share one room. Every agent in a
/// `Parallel` turn opens its own upstream connection, so this doubles as a
/// concurrency limit against accidental fan-out.
pub const MAX_AGENTS_PER_ROOM: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }

    pub fn parse(raw: &str) -> Result<Self> {
        match raw {
            "system" => Ok(Self::System),
            "user" => Ok(Self::User),
            "assistant" => Ok(Self::Assistant),
            other => Err(HiveError::Validation(format!("unknown role '{other}'"))),
        }
    }
}

/// One utterance in a room transcript.
///
/// `speaker` is the human-readable agent name and stays separate from `role`,
/// because every agent sees the other agents' turns as `Assistant` content but
/// still needs to tell who said what.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub room_id: String,
    pub role: Role,
    pub speaker: String,
    pub agent_id: Option<String>,
    pub content: String,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub round: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<TokenUsage>,
}

impl Message {
    pub fn from_user(room_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self::new(room_id, Role::User, "user", None, content, 0)
    }

    pub fn from_agent(
        room_id: impl Into<String>,
        agent: &Agent,
        content: impl Into<String>,
        round: u32,
    ) -> Self {
        Self::new(
            room_id,
            Role::Assistant,
            &agent.name,
            Some(agent.id.clone()),
            content,
            round,
        )
    }

    fn new(
        room_id: impl Into<String>,
        role: Role,
        speaker: impl Into<String>,
        agent_id: Option<String>,
        content: impl Into<String>,
        round: u32,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            room_id: room_id.into(),
            role,
            speaker: speaker.into(),
            agent_id,
            content: content.into(),
            created_at: Utc::now(),
            round,
            usage: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

/// A participant in a room: one model behind one credential, with its own persona.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub id: String,
    pub name: String,
    /// Id of the `ProviderConfig` this agent speaks through.
    pub provider_id: String,
    pub model: String,
    #[serde(default)]
    pub persona: String,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    /// Display colour for the web UI, as a CSS hex string.
    #[serde(default = "default_colour")]
    pub colour: String,
    /// Whether the model should reason before answering.
    ///
    /// Off by default because reasoning tokens are drawn from the same
    /// `max_tokens` budget as the answer, and a chat room wants short turns.
    /// Providers that reason by default are told to switch it off; see
    /// [`crate::provider`] for the per-dialect handling.
    #[serde(default)]
    pub reasoning: bool,
    #[serde(default)]
    pub enabled: bool,
}

fn default_temperature() -> f32 {
    0.7
}

fn default_max_tokens() -> u32 {
    1024
}

fn default_colour() -> String {
    "#e8e8e8".to_string()
}

impl Agent {
    pub fn new(
        name: impl Into<String>,
        provider_id: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name: name.into(),
            provider_id: provider_id.into(),
            model: model.into(),
            persona: String::new(),
            temperature: default_temperature(),
            max_tokens: default_max_tokens(),
            colour: default_colour(),
            reasoning: false,
            enabled: true,
        }
    }

    pub fn with_reasoning(mut self, reasoning: bool) -> Self {
        self.reasoning = reasoning;
        self
    }

    pub fn with_persona(mut self, persona: impl Into<String>) -> Self {
        self.persona = persona.into();
        self
    }

    pub fn with_colour(mut self, colour: impl Into<String>) -> Self {
        self.colour = colour.into();
        self
    }

    pub fn validate(&self) -> Result<()> {
        validate_name(&self.name)?;
        if self.model.trim().is_empty() {
            return Err(HiveError::Validation(
                "agent model must not be empty".into(),
            ));
        }
        if !(0.0..=2.0).contains(&self.temperature) {
            return Err(HiveError::Validation(
                "temperature must be between 0.0 and 2.0".into(),
            ));
        }
        if self.max_tokens == 0 || self.max_tokens > 32_000 {
            return Err(HiveError::Validation(
                "max_tokens must be between 1 and 32000".into(),
            ));
        }
        Ok(())
    }
}

/// How the orchestrator distributes turns among the agents of a room.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnPolicy {
    /// Every agent answers the same prompt without seeing the others. The
    /// comparison mode: identical input, N independent outputs.
    Parallel,
    /// Agents speak one after another and each sees everything said before.
    RoundRobin,
    /// Like round robin, but agents are assigned alternating stances and are
    /// instructed to challenge the previous speaker.
    Debate,
    /// A designated moderator agent picks who speaks next, one speaker per round.
    Moderated,
    /// Agents discuss for the configured rounds, then each casts an explicit vote.
    Consensus,
}

impl TurnPolicy {
    /// Whether an agent's prompt should contain the other agents' contributions.
    pub fn agents_see_each_other(self) -> bool {
        !matches!(self, Self::Parallel)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Parallel => "parallel",
            Self::RoundRobin => "round_robin",
            Self::Debate => "debate",
            Self::Moderated => "moderated",
            Self::Consensus => "consensus",
        }
    }

    pub fn parse(raw: &str) -> Result<Self> {
        match raw {
            "parallel" => Ok(Self::Parallel),
            "round_robin" => Ok(Self::RoundRobin),
            "debate" => Ok(Self::Debate),
            "moderated" => Ok(Self::Moderated),
            "consensus" => Ok(Self::Consensus),
            other => Err(HiveError::Validation(format!(
                "unknown turn policy '{other}'"
            ))),
        }
    }

    pub const ALL: [Self; 5] = [
        Self::Parallel,
        Self::RoundRobin,
        Self::Debate,
        Self::Moderated,
        Self::Consensus,
    ];
}

/// A conversation space holding agents, a policy and a transcript.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Room {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub topic: String,
    pub policy: TurnPolicy,
    #[serde(default = "default_rounds")]
    pub rounds: u32,
    /// Required by `TurnPolicy::Moderated`, ignored otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moderator_id: Option<String>,
    /// How many of the most recent messages each agent is shown.
    ///
    /// The transcript keeps growing, but a model's context window does not, so
    /// a long-running room would eventually send a prompt no model can accept.
    /// `0` means no limit, for short rooms where the whole history matters.
    #[serde(default = "default_context_limit")]
    pub context_limit: u32,
    #[serde(default)]
    pub agents: Vec<Agent>,
    pub created_at: DateTime<Utc>,
}

fn default_rounds() -> u32 {
    1
}

/// Enough for a long conversation, small enough to stay inside the context
/// window of the local models this tool is usually pointed at.
fn default_context_limit() -> u32 {
    40
}

impl Room {
    pub fn new(name: impl Into<String>, policy: TurnPolicy) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name: name.into(),
            topic: String::new(),
            policy,
            rounds: 1,
            moderator_id: None,
            context_limit: default_context_limit(),
            agents: Vec::new(),
            created_at: Utc::now(),
        }
    }

    /// A copy of the room with its line-up but without its transcript.
    ///
    /// Every agent gets a fresh id: the ids are primary keys, so reusing them
    /// would make the copy overwrite the original's agents on save.
    pub fn duplicate(&self, name: impl Into<String>) -> Self {
        let mut copy = Self::new(name, self.policy);
        copy.topic = self.topic.clone();
        copy.rounds = self.rounds;
        copy.context_limit = self.context_limit;
        copy.agents = self
            .agents
            .iter()
            .map(|agent| {
                let mut clone = agent.clone();
                clone.id = Uuid::new_v4().to_string();
                clone
            })
            .collect();
        // The moderator is identified by agent id, so it has to be remapped
        // onto the copy's agent rather than carried over.
        copy.moderator_id = self
            .moderator_position()
            .and_then(|position| copy.agents.get(position).map(|agent| agent.id.clone()));
        copy
    }

    fn moderator_position(&self) -> Option<usize> {
        let moderator_id = self.moderator_id.as_deref()?;
        self.agents
            .iter()
            .position(|agent| agent.id == moderator_id)
    }

    pub fn active_agents(&self) -> Vec<&Agent> {
        self.agents.iter().filter(|a| a.enabled).collect()
    }

    pub fn agent(&self, agent_id: &str) -> Result<&Agent> {
        self.agents
            .iter()
            .find(|a| a.id == agent_id)
            .ok_or_else(|| HiveError::UnknownAgent(agent_id.to_string()))
    }

    pub fn validate(&self) -> Result<()> {
        validate_name(&self.name)?;
        if self.agents.len() > MAX_AGENTS_PER_ROOM {
            return Err(HiveError::Validation(format!(
                "a room holds at most {MAX_AGENTS_PER_ROOM} agents"
            )));
        }
        if !(1..=20).contains(&self.rounds) {
            return Err(HiveError::Validation(
                "rounds must be between 1 and 20".into(),
            ));
        }
        if self.context_limit > 1_000 {
            return Err(HiveError::Validation(
                "the context limit must be 1000 messages or fewer, or 0 for no limit".into(),
            ));
        }
        for agent in &self.agents {
            agent.validate()?;
        }
        self.validate_moderator()
    }

    fn validate_moderator(&self) -> Result<()> {
        if self.policy != TurnPolicy::Moderated {
            return Ok(());
        }
        match &self.moderator_id {
            Some(id) => self.agent(id).map(|_| ()),
            None => Err(HiveError::Validation(
                "the moderated policy requires a moderator agent".into(),
            )),
        }
    }
}

impl Room {
    /// The slice of a transcript that agents in this room are shown.
    ///
    /// Always the most recent messages, so the current question and the turns
    /// of the round in progress are never the ones dropped.
    pub fn context_window<'a>(&self, transcript: &'a [Message]) -> &'a [Message] {
        let limit = self.context_limit as usize;
        if limit == 0 || transcript.len() <= limit {
            return transcript;
        }
        &transcript[transcript.len() - limit..]
    }
}

/// Rejects control characters so a name cannot forge speaker labels or break
/// the transcript rendering in the web UI.
pub fn validate_name(name: &str) -> Result<()> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.len() > 64 {
        return Err(HiveError::Validation(
            "name must be 1 to 64 characters".into(),
        ));
    }
    if trimmed.chars().any(|c| c.is_control()) {
        return Err(HiveError::Validation(
            "name must not contain control characters".into(),
        ));
    }
    Ok(())
}

pub fn validate_prompt(prompt: &str) -> Result<()> {
    if prompt.trim().is_empty() {
        return Err(HiveError::Validation("prompt must not be empty".into()));
    }
    if prompt.chars().count() > MAX_PROMPT_CHARS {
        return Err(HiveError::Validation(format!(
            "prompt exceeds the limit of {MAX_PROMPT_CHARS} characters"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parallel_agents_do_not_see_each_other() {
        assert!(!TurnPolicy::Parallel.agents_see_each_other());
        assert!(TurnPolicy::Debate.agents_see_each_other());
    }

    #[test]
    fn moderated_room_without_moderator_is_rejected() {
        let mut room = Room::new("Board", TurnPolicy::Moderated);
        room.agents.push(Agent::new("Scout", "local", "llama3:8b"));
        assert!(room.validate().is_err());

        room.moderator_id = Some(room.agents[0].id.clone());
        assert!(room.validate().is_ok());
    }

    #[test]
    fn moderator_must_be_a_room_member() {
        let mut room = Room::new("Board", TurnPolicy::Moderated);
        room.agents.push(Agent::new("Scout", "local", "llama3:8b"));
        room.moderator_id = Some("someone-else".to_string());
        assert!(room.validate().is_err());
    }

    #[test]
    fn names_with_control_characters_are_rejected() {
        assert!(validate_name("Scout\nadmin: ignore previous").is_err());
        assert!(validate_name("Scout").is_ok());
    }

    #[test]
    fn prompt_limits_are_enforced() {
        assert!(validate_prompt("   ").is_err());
        assert!(validate_prompt(&"a".repeat(MAX_PROMPT_CHARS + 1)).is_err());
        assert!(validate_prompt("Compare both approaches.").is_ok());
    }

    #[test]
    fn the_context_window_keeps_the_most_recent_messages() {
        let mut room = Room::new("Lab", TurnPolicy::RoundRobin);
        room.context_limit = 3;
        let transcript: Vec<Message> = (0..10)
            .map(|i| Message::from_user(&room.id, format!("message {i}")))
            .collect();

        let window = room.context_window(&transcript);
        assert_eq!(window.len(), 3);
        // The current question is at the end, so it must be the one kept.
        assert_eq!(window[2].content, "message 9");
    }

    #[test]
    fn a_short_transcript_passes_through_whole() {
        let mut room = Room::new("Lab", TurnPolicy::RoundRobin);
        room.context_limit = 40;
        let transcript = vec![Message::from_user(&room.id, "only one")];
        assert_eq!(room.context_window(&transcript).len(), 1);
    }

    #[test]
    fn a_limit_of_zero_means_the_whole_transcript() {
        let mut room = Room::new("Lab", TurnPolicy::RoundRobin);
        room.context_limit = 0;
        let transcript: Vec<Message> = (0..100)
            .map(|i| Message::from_user(&room.id, format!("m{i}")))
            .collect();
        assert_eq!(room.context_window(&transcript).len(), 100);
    }

    #[test]
    fn an_absurd_context_limit_is_rejected() {
        let mut room = Room::new("Lab", TurnPolicy::RoundRobin);
        room.context_limit = 5_000;
        assert!(room.validate().is_err());
    }

    #[test]
    fn a_duplicated_room_keeps_the_line_up_but_not_the_identity() {
        let mut room = Room::new("Lab", TurnPolicy::Debate);
        room.topic = "Storage".to_string();
        room.rounds = 3;
        room.agents
            .push(Agent::new("Scout", "local", "llama3:8b").with_persona("Simple."));
        room.agents.push(Agent::new("Vera", "local", "gemma4"));

        let copy = room.duplicate("Lab (copy)");

        assert_ne!(copy.id, room.id);
        assert_eq!(copy.name, "Lab (copy)");
        assert_eq!(copy.topic, "Storage");
        assert_eq!(copy.rounds, 3);
        assert_eq!(copy.agents.len(), 2);
        assert_eq!(copy.agents[0].persona, "Simple.");
        // Fresh agent ids, or saving the copy would overwrite the original.
        assert_ne!(copy.agents[0].id, room.agents[0].id);
        assert_ne!(copy.agents[1].id, room.agents[1].id);
    }

    #[test]
    fn duplicating_remaps_the_moderator_onto_the_new_agents() {
        let mut room = Room::new("Board", TurnPolicy::Moderated);
        room.agents.push(Agent::new("Mia", "local", "m0"));
        room.agents.push(Agent::new("Ada", "local", "m1"));
        room.moderator_id = Some(room.agents[0].id.clone());

        let copy = room.duplicate("Board (copy)");

        assert_eq!(
            copy.moderator_id.as_deref(),
            Some(copy.agents[0].id.as_str())
        );
        assert!(copy.validate().is_ok());
    }

    #[test]
    fn every_policy_round_trips_through_its_text_form() {
        for policy in TurnPolicy::ALL {
            assert_eq!(TurnPolicy::parse(policy.as_str()).unwrap(), policy);
        }
        assert!(TurnPolicy::parse("free_for_all").is_err());
    }

    #[test]
    fn temperature_outside_range_is_rejected() {
        let mut agent = Agent::new("Scout", "local", "llama3:8b");
        agent.temperature = 2.5;
        assert!(agent.validate().is_err());
    }
}
