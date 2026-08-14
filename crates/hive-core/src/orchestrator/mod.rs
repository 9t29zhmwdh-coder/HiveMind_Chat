//! Runs a room's conversation according to its turn policy.
//!
//! The orchestrator owns no state of its own: it takes a room and a transcript,
//! emits [`SessionEvent`]s as the agents speak, and returns the messages that
//! were produced. Persistence and transport live elsewhere.

mod policy;
mod prompt;

use std::sync::Arc;

use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::Sender;

use crate::error::{HiveError, Result};
use crate::model::{validate_prompt, Agent, Message, Room, TokenUsage, TurnPolicy};
use crate::provider::{ChatChunk, ChatRequest, ModelProvider, ProviderRegistry};

pub use policy::{discussion_rounds, speaking_order, stance_for};

/// What the caller observes while a turn runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionEvent {
    /// The prompt as it entered the transcript. Emitted so every client shows
    /// the same opening message without having to invent one locally.
    UserMessage {
        message: Message,
    },
    TurnStarted {
        round: u32,
        rounds: u32,
        policy: TurnPolicy,
        speakers: Vec<String>,
    },
    AgentStarted {
        agent_id: String,
        agent_name: String,
        colour: String,
        round: u32,
    },
    AgentDelta {
        agent_id: String,
        text: String,
    },
    AgentCompleted {
        message: Message,
    },
    AgentFailed {
        agent_id: String,
        agent_name: String,
        error: String,
    },
    VoteCast {
        agent_id: String,
        agent_name: String,
        choice: String,
        rationale: String,
    },
    TurnCompleted {
        round: u32,
    },
    SessionCompleted {
        messages: usize,
        usage: TokenUsage,
    },
}

pub struct Orchestrator {
    registry: ProviderRegistry,
}

impl Orchestrator {
    pub fn new(registry: ProviderRegistry) -> Self {
        Self { registry }
    }

    /// Runs one user prompt through the room and returns everything that was said.
    ///
    /// The returned messages include the user prompt itself, so the caller can
    /// append the result to the transcript verbatim.
    pub async fn run(
        &self,
        room: &Room,
        history: &[Message],
        user_prompt: &str,
        events: &Sender<SessionEvent>,
    ) -> Result<Vec<Message>> {
        room.validate()?;
        validate_prompt(user_prompt)?;
        if room.active_agents().is_empty() {
            return Err(HiveError::Validation(
                "the room has no enabled agents".into(),
            ));
        }

        let mut transcript: Vec<Message> = history.to_vec();
        let opening = Message::from_user(&room.id, user_prompt);
        transcript.push(opening.clone());
        let _ = events
            .send(SessionEvent::UserMessage {
                message: opening.clone(),
            })
            .await;
        let mut produced = vec![opening];

        for round in 0..discussion_rounds(room) {
            let new_messages = self.run_round(room, &transcript, round + 1, events).await?;
            transcript.extend(new_messages.iter().cloned());
            produced.extend(new_messages);
        }

        if room.policy == TurnPolicy::Consensus {
            let votes = self.collect_votes(room, &transcript, events).await;
            transcript.extend(votes.iter().cloned());
            produced.extend(votes);
        }

        let usage = total_usage(&produced);
        let _ = events
            .send(SessionEvent::SessionCompleted {
                messages: produced.len(),
                usage,
            })
            .await;
        Ok(produced)
    }

    async fn run_round(
        &self,
        room: &Room,
        transcript: &[Message],
        round: u32,
        events: &Sender<SessionEvent>,
    ) -> Result<Vec<Message>> {
        let order = speaking_order(room, round - 1);
        let _ = events
            .send(SessionEvent::TurnStarted {
                round,
                rounds: discussion_rounds(room),
                policy: room.policy,
                speakers: order.iter().map(|a| a.name.clone()).collect(),
            })
            .await;

        let messages = match room.policy {
            TurnPolicy::Parallel => {
                self.run_parallel(room, transcript, &order, round, events)
                    .await
            }
            TurnPolicy::Moderated => self.run_moderated(room, transcript, round, events).await?,
            _ => {
                self.run_sequential(room, transcript, &order, round, events)
                    .await
            }
        };

        let _ = events.send(SessionEvent::TurnCompleted { round }).await;
        Ok(messages)
    }

    /// Every agent answers the same input concurrently.
    async fn run_parallel(
        &self,
        room: &Room,
        transcript: &[Message],
        order: &[Agent],
        round: u32,
        events: &Sender<SessionEvent>,
    ) -> Vec<Message> {
        let attempts = order
            .iter()
            .map(|agent| self.speak(room, agent, transcript, None, None, round, events));
        futures::future::join_all(attempts)
            .await
            .into_iter()
            .flatten()
            .collect()
    }

    /// Agents speak one after another, each seeing what came before.
    async fn run_sequential(
        &self,
        room: &Room,
        transcript: &[Message],
        order: &[Agent],
        round: u32,
        events: &Sender<SessionEvent>,
    ) -> Vec<Message> {
        let mut local = transcript.to_vec();
        let mut produced = Vec::new();
        for (position, agent) in order.iter().enumerate() {
            let stance = stance_for(room.policy, position);
            if let Some(message) = self
                .speak(room, agent, &local, stance, None, round, events)
                .await
            {
                local.push(message.clone());
                produced.push(message);
            }
        }
        produced
    }

    /// The moderator picks one speaker per round.
    async fn run_moderated(
        &self,
        room: &Room,
        transcript: &[Message],
        round: u32,
        events: &Sender<SessionEvent>,
    ) -> Result<Vec<Message>> {
        let moderator_id = room.moderator_id.as_deref().ok_or_else(|| {
            HiveError::Validation("the moderated policy requires a moderator".into())
        })?;
        let moderator = room.agent(moderator_id)?.clone();
        let candidates: Vec<Agent> = room
            .active_agents()
            .into_iter()
            .filter(|a| a.id != moderator.id)
            .cloned()
            .collect();
        if candidates.is_empty() {
            return Err(HiveError::Validation(
                "a moderated room needs at least one agent besides the moderator".into(),
            ));
        }

        let chosen = self
            .pick_speaker(room, &moderator, transcript, &candidates, round)
            .await;
        let message = self
            .speak(room, &chosen, transcript, None, None, round, events)
            .await;
        Ok(message.into_iter().collect())
    }

    /// Asks the moderator who speaks next, falling back to rotation.
    ///
    /// A moderator that answers with something unparseable must not stall the
    /// room, so an unmatched answer degrades to the round-robin position.
    async fn pick_speaker(
        &self,
        room: &Room,
        moderator: &Agent,
        transcript: &[Message],
        candidates: &[Agent],
        round: u32,
    ) -> Agent {
        let names: Vec<&str> = candidates.iter().map(|a| a.name.as_str()).collect();
        let instruction = prompt::moderator_instruction(&names);
        let answer = self
            .complete(room, moderator, transcript, Some(instruction.as_str()))
            .await
            .unwrap_or_default()
            .to_lowercase();

        candidates
            .iter()
            .find(|a| answer.contains(&a.name.to_lowercase()))
            .cloned()
            .unwrap_or_else(|| candidates[(round as usize - 1) % candidates.len()].clone())
    }

    /// Closing round of the consensus policy.
    ///
    /// A vote is a result, not a chat turn: it is collected without streaming
    /// and reported only as [`SessionEvent::VoteCast`], so clients render the
    /// decision once instead of showing the raw `VOTE:`/`WHY:` text as well.
    /// The message still enters the transcript, where the export needs it.
    async fn collect_votes(
        &self,
        room: &Room,
        transcript: &[Message],
        events: &Sender<SessionEvent>,
    ) -> Vec<Message> {
        let instruction = prompt::vote_instruction("the positions discussed above");
        let mut produced = Vec::new();
        for agent in room.active_agents() {
            match self
                .complete(room, agent, transcript, Some(instruction.as_str()))
                .await
            {
                Ok(answer) if !answer.trim().is_empty() => {
                    let (choice, rationale) = parse_vote(&answer);
                    let _ = events
                        .send(SessionEvent::VoteCast {
                            agent_id: agent.id.clone(),
                            agent_name: agent.name.clone(),
                            choice,
                            rationale,
                        })
                        .await;
                    produced.push(Message::from_agent(
                        &room.id,
                        agent,
                        answer.trim(),
                        room.rounds,
                    ));
                }
                Ok(_) => {}
                Err(error) => {
                    let _ = events
                        .send(SessionEvent::AgentFailed {
                            agent_id: agent.id.clone(),
                            agent_name: agent.name.clone(),
                            error: error.to_string(),
                        })
                        .await;
                }
            }
        }
        produced
    }

    /// Streams one agent's turn, reporting failure as an event rather than an error.
    #[allow(clippy::too_many_arguments)]
    async fn speak(
        &self,
        room: &Room,
        agent: &Agent,
        transcript: &[Message],
        stance: Option<&str>,
        instruction: Option<&str>,
        round: u32,
        events: &Sender<SessionEvent>,
    ) -> Option<Message> {
        let _ = events
            .send(SessionEvent::AgentStarted {
                agent_id: agent.id.clone(),
                agent_name: agent.name.clone(),
                colour: agent.colour.clone(),
                round,
            })
            .await;

        match self
            .stream_turn(room, agent, transcript, stance, instruction, round, events)
            .await
        {
            Ok(message) => {
                let _ = events
                    .send(SessionEvent::AgentCompleted {
                        message: message.clone(),
                    })
                    .await;
                Some(message)
            }
            Err(error) => {
                tracing::warn!(agent = %agent.name, %error, "agent turn failed");
                let _ = events
                    .send(SessionEvent::AgentFailed {
                        agent_id: agent.id.clone(),
                        agent_name: agent.name.clone(),
                        error: error.to_string(),
                    })
                    .await;
                None
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn stream_turn(
        &self,
        room: &Room,
        agent: &Agent,
        transcript: &[Message],
        stance: Option<&str>,
        instruction: Option<&str>,
        round: u32,
        events: &Sender<SessionEvent>,
    ) -> Result<Message> {
        let (provider, request) = self.prepare(room, agent, transcript, stance, instruction)?;
        let mut stream = provider.chat(request).await?;
        let mut text = String::new();
        let mut usage = TokenUsage::default();

        while let Some(chunk) = stream.next().await {
            match chunk? {
                ChatChunk::Delta(delta) => {
                    text.push_str(&delta);
                    let _ = events
                        .send(SessionEvent::AgentDelta {
                            agent_id: agent.id.clone(),
                            text: delta,
                        })
                        .await;
                }
                ChatChunk::Done(counts) => usage = counts,
            }
        }

        if text.trim().is_empty() {
            return Err(HiveError::provider(
                &agent.provider_id,
                "the model returned no text",
            ));
        }
        let mut message = Message::from_agent(&room.id, agent, text.trim(), round);
        message.usage = Some(usage);
        Ok(message)
    }

    /// Runs a turn without streaming it, for internal decisions such as the
    /// moderator's choice that the room should not see as chat output.
    async fn complete(
        &self,
        room: &Room,
        agent: &Agent,
        transcript: &[Message],
        instruction: Option<&str>,
    ) -> Result<String> {
        let (provider, request) = self.prepare(room, agent, transcript, None, instruction)?;
        let mut stream = provider.chat(request).await?;
        let mut text = String::new();
        while let Some(chunk) = stream.next().await {
            if let ChatChunk::Delta(delta) = chunk? {
                text.push_str(&delta);
            }
        }
        Ok(text)
    }

    fn prepare(
        &self,
        room: &Room,
        agent: &Agent,
        transcript: &[Message],
        stance: Option<&str>,
        instruction: Option<&str>,
    ) -> Result<(Arc<dyn ModelProvider>, ChatRequest)> {
        let provider = self.registry.get(&agent.provider_id)?;
        let mut turns = prompt::turns_for(room, agent, transcript);
        if let Some(instruction) = instruction {
            turns.push(crate::provider::ChatTurn::user(instruction));
        }
        let request = ChatRequest {
            model: agent.model.clone(),
            system: prompt::system_prompt(room, agent, stance),
            turns,
            temperature: agent.temperature,
            max_tokens: agent.max_tokens,
            reasoning: agent.reasoning,
        };
        Ok((provider, request))
    }
}

fn total_usage(messages: &[Message]) -> TokenUsage {
    messages
        .iter()
        .filter_map(|m| m.usage)
        .fold(TokenUsage::default(), |mut acc, u| {
            acc.input_tokens += u.input_tokens;
            acc.output_tokens += u.output_tokens;
            acc
        })
}

/// Extracts the `VOTE:` / `WHY:` pair from a closing statement.
///
/// Models drift from the requested format, so anything unparseable falls back
/// to the raw text rather than dropping the vote.
fn parse_vote(content: &str) -> (String, String) {
    let mut choice = String::new();
    let mut rationale = String::new();
    for line in content.lines() {
        let trimmed = line.trim().trim_start_matches(['*', '-', '#', ' ']);
        if let Some(rest) = strip_label(trimmed, "vote:") {
            choice = rest.to_string();
        } else if let Some(rest) = strip_label(trimmed, "why:") {
            rationale = rest.to_string();
        }
    }
    if choice.is_empty() {
        choice = content
            .lines()
            .next()
            .unwrap_or_default()
            .trim()
            .to_string();
    }
    (choice, rationale)
}

fn strip_label<'a>(line: &'a str, label: &str) -> Option<&'a str> {
    let lowered = line.to_lowercase();
    lowered
        .starts_with(label)
        .then(|| line[label.len()..].trim().trim_matches(['*', '`']).trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vote_lines_are_extracted() {
        let (choice, why) = parse_vote("VOTE: SQLite\nWHY: it removes an operational dependency.");
        assert_eq!(choice, "SQLite");
        assert_eq!(why, "it removes an operational dependency.");
    }

    #[test]
    fn markdown_decorated_votes_are_still_parsed() {
        let (choice, why) = parse_vote("- **VOTE:** Postgres\n- **WHY:** it scales further.");
        assert_eq!(choice, "Postgres");
        assert_eq!(why, "it scales further.");
    }

    #[test]
    fn unparseable_votes_fall_back_to_the_first_line() {
        let (choice, why) = parse_vote("I would keep SQLite for now.");
        assert_eq!(choice, "I would keep SQLite for now.");
        assert!(why.is_empty());
    }

    #[test]
    fn usage_is_summed_across_messages() {
        let room_id = "room";
        let agent = Agent::new("Scout", "local", "llama3:8b");
        let mut first = Message::from_agent(room_id, &agent, "a", 1);
        first.usage = Some(TokenUsage {
            input_tokens: 10,
            output_tokens: 5,
        });
        let mut second = Message::from_agent(room_id, &agent, "b", 1);
        second.usage = Some(TokenUsage {
            input_tokens: 3,
            output_tokens: 7,
        });

        let total = total_usage(&[first, second, Message::from_user(room_id, "q")]);
        assert_eq!(total.input_tokens, 13);
        assert_eq!(total.output_tokens, 12);
    }

    #[tokio::test]
    async fn empty_rooms_are_rejected_before_any_provider_call() {
        let orchestrator = Orchestrator::new(ProviderRegistry::default());
        let room = Room::new("Empty", TurnPolicy::RoundRobin);
        let (tx, _rx) = tokio::sync::mpsc::channel(4);
        let result = orchestrator.run(&room, &[], "Hello", &tx).await;
        assert!(matches!(result, Err(HiveError::Validation(_))));
    }

    #[tokio::test]
    async fn blank_prompts_are_rejected() {
        let orchestrator = Orchestrator::new(ProviderRegistry::default());
        let mut room = Room::new("Lab", TurnPolicy::RoundRobin);
        room.agents.push(Agent::new("Scout", "local", "llama3:8b"));
        let (tx, _rx) = tokio::sync::mpsc::channel(4);
        assert!(orchestrator.run(&room, &[], "   ", &tx).await.is_err());
    }
}
