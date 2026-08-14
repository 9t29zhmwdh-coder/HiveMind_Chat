//! Speaking order and stance assignment per turn policy.

use crate::model::{Agent, Room, TurnPolicy};

/// Stances handed out in a debate, cycled over the participants.
const STANCES: [&str; 3] = [
    "argue in favour, and defend the position under challenge",
    "argue against, and probe the weakest part of the previous argument",
    "stay neutral: weigh both sides and name what is still unresolved",
];

/// The order agents speak in for one round.
///
/// The starting position rotates with the round so the same agent does not
/// always set the frame, which measurably skews later contributions.
pub fn speaking_order(room: &Room, round: u32) -> Vec<Agent> {
    let agents = room.active_agents();
    if agents.is_empty() {
        return Vec::new();
    }
    let offset = (round as usize) % agents.len();
    agents
        .iter()
        .cycle()
        .skip(offset)
        .take(agents.len())
        .map(|a| (*a).clone())
        .collect()
}

/// The stance an agent argues from, if the policy assigns one.
pub fn stance_for(policy: TurnPolicy, position: usize) -> Option<&'static str> {
    match policy {
        TurnPolicy::Debate => Some(STANCES[position % STANCES.len()]),
        _ => None,
    }
}

/// How many discussion rounds run before the policy's closing step.
pub fn discussion_rounds(room: &Room) -> u32 {
    match room.policy {
        // Every agent answers the same prompt once; more rounds would just
        // repeat an identical request.
        TurnPolicy::Parallel => 1,
        _ => room.rounds,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room_with(names: &[&str]) -> Room {
        let mut room = Room::new("Lab", TurnPolicy::RoundRobin);
        for name in names {
            room.agents.push(Agent::new(*name, "local", "llama3:8b"));
        }
        room
    }

    #[test]
    fn speaking_order_rotates_per_round() {
        let room = room_with(&["A", "B", "C"]);
        let names = |round| {
            speaking_order(&room, round)
                .into_iter()
                .map(|a| a.name)
                .collect::<Vec<_>>()
        };
        assert_eq!(names(0), vec!["A", "B", "C"]);
        assert_eq!(names(1), vec!["B", "C", "A"]);
        assert_eq!(names(3), vec!["A", "B", "C"]);
    }

    #[test]
    fn disabled_agents_are_left_out() {
        let mut room = room_with(&["A", "B"]);
        room.agents[0].enabled = false;
        let order = speaking_order(&room, 0);
        assert_eq!(order.len(), 1);
        assert_eq!(order[0].name, "B");
    }

    #[test]
    fn empty_rooms_produce_no_order() {
        assert!(speaking_order(&room_with(&[]), 0).is_empty());
    }

    #[test]
    fn stances_are_assigned_only_in_debates() {
        assert!(stance_for(TurnPolicy::RoundRobin, 0).is_none());
        assert!(stance_for(TurnPolicy::Debate, 0)
            .unwrap()
            .contains("in favour"));
        assert!(stance_for(TurnPolicy::Debate, 1)
            .unwrap()
            .contains("against"));
        assert_eq!(
            stance_for(TurnPolicy::Debate, 3),
            stance_for(TurnPolicy::Debate, 0)
        );
    }

    #[test]
    fn parallel_policy_runs_exactly_one_round() {
        let mut room = room_with(&["A", "B"]);
        room.rounds = 5;
        room.policy = TurnPolicy::Parallel;
        assert_eq!(discussion_rounds(&room), 1);
        room.policy = TurnPolicy::Debate;
        assert_eq!(discussion_rounds(&room), 5);
    }
}
