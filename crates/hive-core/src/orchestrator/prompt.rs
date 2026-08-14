//! Turns a room transcript into the per-agent view of the conversation.
//!
//! Every agent sees the same events but from its own perspective: its own
//! turns are assistant turns, everyone else's are user turns prefixed with the
//! speaker name. Without that prefix a model cannot tell two peers apart, since
//! the chat dialects have no concept of a named third participant.

use crate::model::{Agent, Message, Role, Room};
use crate::provider::ChatTurn;

/// House rules every agent receives, regardless of policy.
const ROOM_RULES: &str = "\
Rules of this room:
- Other participants are labelled `Name:` in the conversation. Address them by name.
- Speak only for yourself. Never write a turn on behalf of another participant.
- Do not prefix your own reply with your name; the room adds it.
- Be substantive and concise. Add something the previous speakers did not.";

/// Builds the system prompt for one agent's turn.
pub fn system_prompt(room: &Room, agent: &Agent, stance: Option<&str>) -> String {
    let mut prompt = format!(
        "You are {}, a participant in the group chat \"{}\".\n",
        agent.name, room.name
    );
    if !agent.persona.trim().is_empty() {
        prompt.push_str(&format!("\nYour role: {}\n", agent.persona.trim()));
    }
    if !room.topic.trim().is_empty() {
        prompt.push_str(&format!("\nTopic of the room: {}\n", room.topic.trim()));
    }
    prompt.push_str(&format!(
        "\nOther participants: {}\n",
        peer_list(room, agent)
    ));
    if let Some(stance) = stance {
        prompt.push_str(&format!("\nYour assigned stance: {stance}\n"));
    }
    prompt.push_str(&format!("\n{ROOM_RULES}\n"));
    prompt
}

fn peer_list(room: &Room, agent: &Agent) -> String {
    let peers: Vec<&str> = room
        .active_agents()
        .into_iter()
        .filter(|a| a.id != agent.id)
        .map(|a| a.name.as_str())
        .collect();
    if peers.is_empty() {
        return "none, you are answering alone".to_string();
    }
    peers.join(", ")
}

/// Projects the transcript into turns for one agent.
///
/// In [`crate::model::TurnPolicy::Parallel`] the other agents' contributions are withheld, so
/// every model answers the same input and the outputs stay comparable.
pub fn turns_for(room: &Room, agent: &Agent, history: &[Message]) -> Vec<ChatTurn> {
    let visible = history.iter().filter(|message| {
        room.policy.agents_see_each_other()
            || message.agent_id.is_none()
            || message.agent_id.as_deref() == Some(&agent.id)
    });

    let raw: Vec<ChatTurn> = visible.map(|message| project(message, agent)).collect();
    collapse(raw)
}

fn project(message: &Message, agent: &Agent) -> ChatTurn {
    let own = message.agent_id.as_deref() == Some(agent.id.as_str());
    match (message.role, own) {
        (Role::Assistant, true) => ChatTurn::assistant(message.content.as_str()),
        (Role::Assistant, false) => {
            ChatTurn::user(format!("{}: {}", message.speaker, message.content))
        }
        _ => ChatTurn::user(message.content.as_str()),
    }
}

/// Merges neighbouring turns of the same role and guarantees the conversation
/// opens with a user turn, which the Messages API requires.
fn collapse(turns: Vec<ChatTurn>) -> Vec<ChatTurn> {
    let mut merged: Vec<ChatTurn> = Vec::with_capacity(turns.len());
    for turn in turns {
        match merged.last_mut() {
            Some(last) if last.role == turn.role => {
                last.content.push_str("\n\n");
                last.content.push_str(&turn.content);
            }
            _ => merged.push(turn),
        }
    }
    if merged.first().is_some_and(|turn| turn.role != Role::User) {
        merged.insert(0, ChatTurn::user("(the conversation so far)"));
    }
    merged
}

/// Instruction appended when an agent is asked to cast its final vote.
pub fn vote_instruction(options: &str) -> String {
    format!(
        "The discussion is over. State your final position in exactly two lines:\n\
         Line 1: `VOTE: <your choice>`, where the choice is one of: {options}\n\
         Line 2: `WHY: <one sentence>`"
    )
}

/// Instruction for the moderator's speaker selection.
pub fn moderator_instruction(candidates: &[&str]) -> String {
    format!(
        "You are moderating. Decide who should speak next and answer with that \
         name and nothing else. Choose one of: {}",
        candidates.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TurnPolicy;

    fn room_with_two() -> Room {
        let mut room = Room::new("Lab", TurnPolicy::RoundRobin);
        room.topic = "SQLite or Postgres".to_string();
        room.agents
            .push(Agent::new("Scout", "local", "llama3:8b").with_persona("You favour simplicity."));
        room.agents.push(Agent::new("Vera", "local", "gemma4"));
        room
    }

    #[test]
    fn system_prompt_names_peers_and_persona() {
        let room = room_with_two();
        let prompt = system_prompt(&room, &room.agents[0], None);
        assert!(prompt.contains("You are Scout"));
        assert!(prompt.contains("You favour simplicity."));
        assert!(prompt.contains("Other participants: Vera"));
        assert!(prompt.contains("SQLite or Postgres"));
    }

    #[test]
    fn stance_is_included_only_when_assigned() {
        let room = room_with_two();
        assert!(!system_prompt(&room, &room.agents[0], None).contains("assigned stance"));
        assert!(system_prompt(&room, &room.agents[0], Some("in favour")).contains("in favour"));
    }

    #[test]
    fn own_turns_are_assistant_and_peers_are_labelled() {
        let room = room_with_two();
        let scout = &room.agents[0];
        let vera = &room.agents[1];
        let history = vec![
            Message::from_user(&room.id, "Which database?"),
            Message::from_agent(&room.id, scout, "SQLite is enough.", 1),
            Message::from_agent(&room.id, vera, "Postgres scales further.", 1),
        ];

        let turns = turns_for(&room, scout, &history);
        assert_eq!(turns[0].role, Role::User);
        assert_eq!(turns[1].role, Role::Assistant);
        assert_eq!(turns[1].content, "SQLite is enough.");
        assert_eq!(turns[2].content, "Vera: Postgres scales further.");
    }

    #[test]
    fn parallel_policy_hides_peer_contributions() {
        let mut room = room_with_two();
        room.policy = TurnPolicy::Parallel;
        let scout = room.agents[0].clone();
        let vera = room.agents[1].clone();
        let history = vec![
            Message::from_user(&room.id, "Which database?"),
            Message::from_agent(&room.id, &vera, "Postgres scales further.", 1),
        ];

        let turns = turns_for(&room, &scout, &history);
        assert_eq!(turns.len(), 1);
        assert!(!turns[0].content.contains("Vera"));
    }

    #[test]
    fn consecutive_same_role_turns_are_merged() {
        let room = room_with_two();
        let scout = room.agents[0].clone();
        let vera = room.agents[1].clone();
        let history = vec![
            Message::from_user(&room.id, "Which database?"),
            Message::from_agent(&room.id, &vera, "Postgres.", 1),
            Message::from_user(&room.id, "Why?"),
        ];

        let turns = turns_for(&room, &scout, &history);
        assert_eq!(turns.len(), 1);
        assert!(turns[0].content.contains("Vera: Postgres."));
        assert!(turns[0].content.contains("Why?"));
    }

    #[test]
    fn transcript_starting_with_an_agent_gets_a_user_opener() {
        let room = room_with_two();
        let scout = room.agents[0].clone();
        let history = vec![Message::from_agent(&room.id, &scout, "I will start.", 1)];
        let turns = turns_for(&room, &scout, &history);
        assert_eq!(turns[0].role, Role::User);
        assert_eq!(turns[1].role, Role::Assistant);
    }
}
