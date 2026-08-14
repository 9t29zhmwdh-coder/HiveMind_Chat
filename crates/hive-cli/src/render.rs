//! Renders orchestrator events as a readable terminal transcript.

use hive_core::{SessionEvent, TurnPolicy};

const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RED: &str = "\x1b[31m";
const RESET: &str = "\x1b[0m";

#[derive(Default)]
pub struct Renderer {
    /// Whether the current speaker's line is still open, so the next event
    /// knows if it has to break the line first.
    speaking: bool,
    /// Whether to hold turns back until they are complete.
    ///
    /// A terminal has one cursor. When several agents answer at once their
    /// deltas arrive interleaved, and streaming them live produces one
    /// unreadable block with no way to tell who wrote what. Those turns are
    /// printed whole instead.
    buffered: bool,
}

impl Renderer {
    pub fn handle(&mut self, event: &SessionEvent) {
        match event {
            SessionEvent::UserMessage { message } => {
                self.close_line();
                println!("{BOLD}you:{RESET} {}", message.content);
            }
            SessionEvent::TurnStarted {
                round,
                rounds,
                policy,
                speakers,
            } => {
                self.close_line();
                self.buffered = *policy == TurnPolicy::Parallel && speakers.len() > 1;
                println!(
                    "{DIM}round {round}/{rounds} · {} · {}{RESET}",
                    policy.as_str(),
                    speakers.join(" → ")
                );
            }
            SessionEvent::AgentStarted { agent_name, .. } => {
                if self.buffered {
                    return;
                }
                self.close_line();
                print!("{BOLD}{agent_name}:{RESET} ");
                self.speaking = true;
            }
            SessionEvent::AgentDelta { text, .. } => {
                if !self.buffered {
                    print!("{text}");
                }
            }
            SessionEvent::AgentCompleted { message } => {
                self.close_line();
                if self.buffered {
                    println!("{BOLD}{}:{RESET} {}\n", message.speaker, message.content);
                }
            }
            SessionEvent::AgentFailed {
                agent_name, error, ..
            } => {
                self.close_line();
                println!("{RED}{agent_name} could not answer: {error}{RESET}");
            }
            SessionEvent::VoteCast {
                agent_name,
                choice,
                rationale,
                ..
            } => {
                self.close_line();
                println!(
                    "{BOLD}{agent_name} votes:{RESET} {choice}{}",
                    suffix(rationale)
                );
            }
            SessionEvent::TurnCompleted { .. } => self.close_line(),
            SessionEvent::SessionCompleted { messages, usage } => {
                self.close_line();
                println!(
                    "{DIM}{messages} messages · {} input tokens · {} output tokens{RESET}",
                    usage.input_tokens, usage.output_tokens
                );
            }
        }
    }

    fn close_line(&mut self) {
        if self.speaking {
            println!();
            self.speaking = false;
        }
    }
}

fn suffix(rationale: &str) -> String {
    match rationale.trim().is_empty() {
        true => String::new(),
        false => format!(" ({})", rationale.trim()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hive_core::{Agent, Message};

    fn turn_started(policy: TurnPolicy, speakers: &[&str]) -> SessionEvent {
        SessionEvent::TurnStarted {
            round: 1,
            rounds: 1,
            policy,
            speakers: speakers.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    fn started(name: &str) -> SessionEvent {
        SessionEvent::AgentStarted {
            agent_id: "a".into(),
            agent_name: name.into(),
            colour: "#fff".into(),
            round: 1,
        }
    }

    #[test]
    fn a_rationale_is_appended_only_when_present() {
        assert_eq!(suffix("  "), "");
        assert_eq!(suffix(" it is simpler "), " (it is simpler)");
    }

    #[test]
    fn sequential_policies_stream_live() {
        let mut renderer = Renderer::default();
        renderer.handle(&turn_started(TurnPolicy::RoundRobin, &["Ada", "Ben"]));
        assert!(!renderer.buffered);

        renderer.handle(&started("Ada"));
        assert!(
            renderer.speaking,
            "a sequential turn should open a live line"
        );
    }

    #[test]
    fn concurrent_speakers_are_held_back_until_complete() {
        let mut renderer = Renderer::default();
        renderer.handle(&turn_started(TurnPolicy::Parallel, &["Ada", "Ben"]));
        assert!(renderer.buffered);

        renderer.handle(&started("Ada"));
        // No live line is opened, so interleaved deltas cannot mix on it.
        assert!(!renderer.speaking);
    }

    #[test]
    fn a_lone_parallel_speaker_still_streams() {
        let mut renderer = Renderer::default();
        renderer.handle(&turn_started(TurnPolicy::Parallel, &["Ada"]));
        assert!(
            !renderer.buffered,
            "one speaker cannot interleave with anyone"
        );
    }

    #[test]
    fn the_mode_is_re_evaluated_for_every_turn() {
        let mut renderer = Renderer::default();
        renderer.handle(&turn_started(TurnPolicy::Parallel, &["Ada", "Ben"]));
        assert!(renderer.buffered);
        renderer.handle(&turn_started(TurnPolicy::Debate, &["Ada", "Ben"]));
        assert!(!renderer.buffered);
    }

    #[test]
    fn every_event_kind_is_handled_without_panicking() {
        let mut renderer = Renderer::default();
        let agent = Agent::new("Ada", "local", "m1");
        let events = [
            turn_started(TurnPolicy::Debate, &["Ada"]),
            SessionEvent::UserMessage {
                message: Message::from_user("room", "Hello"),
            },
            started("Ada"),
            SessionEvent::AgentDelta {
                agent_id: "a".into(),
                text: "hello".into(),
            },
            SessionEvent::AgentCompleted {
                message: Message::from_agent("room", &agent, "Answer", 1),
            },
            SessionEvent::AgentFailed {
                agent_id: "a".into(),
                agent_name: "Ada".into(),
                error: "timeout".into(),
            },
            SessionEvent::VoteCast {
                agent_id: "a".into(),
                agent_name: "Ada".into(),
                choice: "SQLite".into(),
                rationale: String::new(),
            },
            SessionEvent::TurnCompleted { round: 1 },
            SessionEvent::SessionCompleted {
                messages: 3,
                usage: Default::default(),
            },
        ];
        for event in &events {
            renderer.handle(event);
        }
    }
}
