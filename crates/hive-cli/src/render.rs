//! Renders orchestrator events as a readable terminal transcript.

use hive_core::SessionEvent;

const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RED: &str = "\x1b[31m";
const RESET: &str = "\x1b[0m";

#[derive(Default)]
pub struct Renderer {
    /// Whether the current speaker's line is still open, so the next event
    /// knows if it has to break the line first.
    speaking: bool,
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
                println!(
                    "{DIM}round {round}/{rounds} · {} · {}{RESET}",
                    policy.as_str(),
                    speakers.join(" → ")
                );
            }
            SessionEvent::AgentStarted { agent_name, .. } => {
                self.close_line();
                print!("{BOLD}{agent_name}:{RESET} ");
                self.speaking = true;
            }
            SessionEvent::AgentDelta { text, .. } => print!("{text}"),
            SessionEvent::AgentCompleted { .. } => self.close_line(),
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
    use hive_core::TurnPolicy;

    #[test]
    fn a_rationale_is_appended_only_when_present() {
        assert_eq!(suffix("  "), "");
        assert_eq!(suffix(" it is simpler "), " (it is simpler)");
    }

    #[test]
    fn the_renderer_tracks_whether_a_line_is_open() {
        let mut renderer = Renderer::default();
        assert!(!renderer.speaking);

        renderer.handle(&SessionEvent::AgentStarted {
            agent_id: "a".into(),
            agent_name: "Scout".into(),
            colour: "#fff".into(),
            round: 1,
        });
        assert!(renderer.speaking);

        renderer.handle(&SessionEvent::TurnCompleted { round: 1 });
        assert!(!renderer.speaking);
    }

    #[test]
    fn every_event_kind_is_handled_without_panicking() {
        let mut renderer = Renderer::default();
        let events = [
            SessionEvent::TurnStarted {
                round: 1,
                rounds: 2,
                policy: TurnPolicy::Debate,
                speakers: vec!["Scout".into()],
            },
            SessionEvent::AgentDelta {
                agent_id: "a".into(),
                text: "hello".into(),
            },
            SessionEvent::AgentFailed {
                agent_id: "a".into(),
                agent_name: "Scout".into(),
                error: "timeout".into(),
            },
            SessionEvent::VoteCast {
                agent_id: "a".into(),
                agent_name: "Scout".into(),
                choice: "SQLite".into(),
                rationale: String::new(),
            },
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
