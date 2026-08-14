//! End-to-end tests for the turn policies.
//!
//! Every test drives the real orchestrator against scripted providers, so the
//! speaking order, the prompt each agent receives, the transcript and the event
//! stream are all exercised for real.

mod support;

use std::sync::Arc;

use hive_core::{Agent, Message, Orchestrator, ProviderRegistry, Room, SessionEvent, TurnPolicy};
use support::{Reply, ScriptedProvider, Seen};
use tokio::sync::mpsc;

/// Runs a prompt through a room and returns the transcript and every event.
async fn run(
    room: &Room,
    registry: ProviderRegistry,
    prompt: &str,
) -> (Vec<Message>, Vec<SessionEvent>) {
    let orchestrator = Orchestrator::new(registry);
    let (tx, mut rx) = mpsc::channel(512);
    let collector = tokio::spawn(async move {
        let mut events = Vec::new();
        while let Some(event) = rx.recv().await {
            events.push(event);
        }
        events
    });

    let messages = orchestrator
        .run(room, &[], prompt, &tx)
        .await
        .expect("the turn failed");
    drop(tx);
    (messages, collector.await.expect("the collector panicked"))
}

fn registry_with(providers: &[Arc<ScriptedProvider>]) -> ProviderRegistry {
    let mut registry = ProviderRegistry::default();
    for provider in providers {
        registry.insert(provider.clone());
    }
    registry
}

fn room_with(policy: TurnPolicy, agents: Vec<Agent>) -> Room {
    let mut room = Room::new("Lab", policy);
    room.topic = "Storage layer".to_string();
    room.agents = agents;
    room
}

fn speakers(messages: &[Message]) -> Vec<&str> {
    messages
        .iter()
        .map(|message| message.speaker.as_str())
        .collect()
}

#[tokio::test]
async fn round_robin_lets_each_agent_see_what_came_before() {
    let alpha = ScriptedProvider::always("alpha", "Alpha says use SQLite.");
    let beta = ScriptedProvider::always("beta", "Beta disagrees.");
    let room = room_with(
        TurnPolicy::RoundRobin,
        vec![
            Agent::new("Ada", "alpha", "m1"),
            Agent::new("Ben", "beta", "m2"),
        ],
    );

    let (messages, _) = run(
        &room,
        registry_with(&[alpha.clone(), beta.clone()]),
        "Which store?",
    )
    .await;

    assert_eq!(speakers(&messages), vec!["user", "Ada", "Ben"]);

    // Ben's request must contain Ada's turn, labelled with her name.
    let bens_view = beta.requests()[0].transcript();
    assert!(
        bens_view.contains("Ada: Alpha says use SQLite."),
        "{bens_view}"
    );
    // Ada spoke first, so she cannot have seen Ben.
    assert!(!alpha.requests()[0].transcript().contains("Ben:"));
}

#[tokio::test]
async fn parallel_withholds_the_other_answers() {
    let alpha = ScriptedProvider::always("alpha", "Alpha answer.");
    let beta = ScriptedProvider::always("beta", "Beta answer.");
    let room = room_with(
        TurnPolicy::Parallel,
        vec![
            Agent::new("Ada", "alpha", "m1"),
            Agent::new("Ben", "beta", "m2"),
        ],
    );

    let (messages, _) = run(
        &room,
        registry_with(&[alpha.clone(), beta.clone()]),
        "Which store?",
    )
    .await;

    assert_eq!(messages.len(), 3);
    for provider in [&alpha, &beta] {
        let view = provider.requests()[0].transcript();
        assert!(view.contains("Which store?"));
        assert!(
            !view.contains("Ada:"),
            "a peer answer leaked into a parallel turn: {view}"
        );
        assert!(
            !view.contains("Ben:"),
            "a peer answer leaked into a parallel turn: {view}"
        );
    }
}

#[tokio::test]
async fn parallel_runs_one_round_regardless_of_the_configured_count() {
    let alpha = ScriptedProvider::always("alpha", "Answer.");
    let mut room = room_with(TurnPolicy::Parallel, vec![Agent::new("Ada", "alpha", "m1")]);
    room.rounds = 5;

    run(
        &room,
        registry_with(std::slice::from_ref(&alpha)),
        "Which store?",
    )
    .await;

    assert_eq!(alpha.call_count(), 1);
}

#[tokio::test]
async fn debate_assigns_opposing_stances() {
    let alpha = ScriptedProvider::always("alpha", "In favour.");
    let beta = ScriptedProvider::always("beta", "Against.");
    let room = room_with(
        TurnPolicy::Debate,
        vec![
            Agent::new("Ada", "alpha", "m1"),
            Agent::new("Ben", "beta", "m2"),
        ],
    );

    run(
        &room,
        registry_with(&[alpha.clone(), beta.clone()]),
        "Which store?",
    )
    .await;

    assert!(alpha.requests()[0].system.contains("in favour"));
    assert!(beta.requests()[0].system.contains("against"));
}

#[tokio::test]
async fn several_rounds_rotate_who_opens() {
    let alpha = ScriptedProvider::always("alpha", "A.");
    let beta = ScriptedProvider::always("beta", "B.");
    let mut room = room_with(
        TurnPolicy::RoundRobin,
        vec![
            Agent::new("Ada", "alpha", "m1"),
            Agent::new("Ben", "beta", "m2"),
        ],
    );
    room.rounds = 2;

    let (messages, _) = run(&room, registry_with(&[alpha, beta]), "Which store?").await;

    // The opener rotates so the same agent does not always frame the round.
    assert_eq!(
        speakers(&messages),
        vec!["user", "Ada", "Ben", "Ben", "Ada"]
    );
}

#[tokio::test]
async fn consensus_collects_a_vote_from_every_agent() {
    let alpha = ScriptedProvider::new(
        "alpha",
        vec![
            Reply::Text("I favour SQLite.".into()),
            Reply::Text("VOTE: SQLite\nWHY: fewer moving parts.".into()),
        ],
    );
    let beta = ScriptedProvider::new(
        "beta",
        vec![
            Reply::Text("I favour Postgres.".into()),
            Reply::Text("VOTE: Postgres\nWHY: it scales.".into()),
        ],
    );
    let room = room_with(
        TurnPolicy::Consensus,
        vec![
            Agent::new("Ada", "alpha", "m1"),
            Agent::new("Ben", "beta", "m2"),
        ],
    );

    let (messages, events) = run(&room, registry_with(&[alpha, beta]), "Which store?").await;

    let votes: Vec<(&str, &str)> = events
        .iter()
        .filter_map(|event| match event {
            SessionEvent::VoteCast {
                agent_name, choice, ..
            } => Some((agent_name.as_str(), choice.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(votes, vec![("Ada", "SQLite"), ("Ben", "Postgres")]);

    // The vote is a result rather than a chat turn, so it is not streamed as
    // deltas, but it does stay in the transcript for the export.
    assert_eq!(messages.len(), 5);
    assert!(messages
        .iter()
        .any(|m| m.content.starts_with("VOTE: SQLite")));
    assert!(!events.iter().any(|event| matches!(
        event,
        SessionEvent::AgentDelta { text, .. } if text.contains("VOTE")
    )));
}

#[tokio::test]
async fn a_moderator_picks_the_next_speaker() {
    let moderator = ScriptedProvider::always("mod", "Ben");
    let alpha = ScriptedProvider::always("alpha", "Ada speaks.");
    let beta = ScriptedProvider::always("beta", "Ben speaks.");

    let mods = Agent::new("Mia", "mod", "m0");
    let mut room = room_with(
        TurnPolicy::Moderated,
        vec![
            mods.clone(),
            Agent::new("Ada", "alpha", "m1"),
            Agent::new("Ben", "beta", "m2"),
        ],
    );
    room.moderator_id = Some(mods.id.clone());

    let (messages, _) = run(
        &room,
        registry_with(&[moderator, alpha.clone(), beta.clone()]),
        "Which store?",
    )
    .await;

    assert_eq!(speakers(&messages), vec!["user", "Ben"]);
    assert_eq!(beta.call_count(), 1);
    assert_eq!(alpha.call_count(), 0);
}

#[tokio::test]
async fn an_unusable_moderator_answer_falls_back_to_rotation() {
    let moderator = ScriptedProvider::always("mod", "I am not sure, perhaps nobody.");
    let alpha = ScriptedProvider::always("alpha", "Ada speaks.");

    let mods = Agent::new("Mia", "mod", "m0");
    let mut room = room_with(
        TurnPolicy::Moderated,
        vec![mods.clone(), Agent::new("Ada", "alpha", "m1")],
    );
    room.moderator_id = Some(mods.id.clone());

    let (messages, _) = run(&room, registry_with(&[moderator, alpha]), "Which store?").await;

    // The room must keep moving rather than stall on an unparseable answer.
    assert_eq!(speakers(&messages), vec!["user", "Ada"]);
}

#[tokio::test]
async fn a_failing_agent_is_reported_and_the_room_continues() {
    let broken = ScriptedProvider::new("broken", vec![Reply::Failure("upstream is down".into())]);
    let healthy = ScriptedProvider::always("healthy", "I can still answer.");
    let room = room_with(
        TurnPolicy::RoundRobin,
        vec![
            Agent::new("Ada", "broken", "m1"),
            Agent::new("Ben", "healthy", "m2"),
        ],
    );

    let (messages, events) = run(&room, registry_with(&[broken, healthy]), "Which store?").await;

    assert_eq!(speakers(&messages), vec!["user", "Ben"]);
    let failures: Vec<&str> = events
        .iter()
        .filter_map(|event| match event {
            SessionEvent::AgentFailed { agent_name, .. } => Some(agent_name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(failures, vec!["Ada"]);
}

#[tokio::test]
async fn an_empty_answer_counts_as_a_failure() {
    let silent = ScriptedProvider::new("silent", vec![Reply::Empty]);
    let room = room_with(
        TurnPolicy::RoundRobin,
        vec![Agent::new("Ada", "silent", "m1")],
    );

    let (messages, events) = run(&room, registry_with(&[silent]), "Which store?").await;

    assert_eq!(speakers(&messages), vec!["user"]);
    assert!(events
        .iter()
        .any(|event| matches!(event, SessionEvent::AgentFailed { .. })));
}

#[tokio::test]
async fn disabled_agents_are_skipped() {
    let alpha = ScriptedProvider::always("alpha", "A.");
    let beta = ScriptedProvider::always("beta", "B.");
    let mut agents = vec![
        Agent::new("Ada", "alpha", "m1"),
        Agent::new("Ben", "beta", "m2"),
    ];
    agents[1].enabled = false;
    let room = room_with(TurnPolicy::RoundRobin, agents);

    let (messages, _) = run(&room, registry_with(&[alpha, beta.clone()]), "Which store?").await;

    assert_eq!(speakers(&messages), vec!["user", "Ada"]);
    assert_eq!(beta.call_count(), 0);
}

#[tokio::test]
async fn the_system_prompt_carries_persona_topic_and_peers() {
    let alpha = ScriptedProvider::always("alpha", "A.");
    let room = room_with(
        TurnPolicy::RoundRobin,
        vec![
            Agent::new("Ada", "alpha", "m1").with_persona("You favour simplicity."),
            Agent::new("Ben", "alpha", "m2"),
        ],
    );

    run(
        &room,
        registry_with(std::slice::from_ref(&alpha)),
        "Which store?",
    )
    .await;

    let system = &alpha.requests()[0].system;
    assert!(system.contains("You are Ada"));
    assert!(system.contains("You favour simplicity."));
    assert!(system.contains("Storage layer"));
    assert!(system.contains("Other participants: Ben"));
}

#[tokio::test]
async fn deltas_are_streamed_and_add_up_to_the_stored_message() {
    let alpha = ScriptedProvider::always("alpha", "One two three four.");
    let room = room_with(
        TurnPolicy::RoundRobin,
        vec![Agent::new("Ada", "alpha", "m1")],
    );

    let (messages, events) = run(&room, registry_with(&[alpha]), "Which store?").await;

    let streamed: String = events
        .iter()
        .filter_map(|event| match event {
            SessionEvent::AgentDelta { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(streamed.trim(), "One two three four.");
    assert_eq!(messages[1].content, "One two three four.");
    assert!(
        events
            .iter()
            .filter(|e| matches!(e, SessionEvent::AgentDelta { .. }))
            .count()
            > 1
    );
}

#[tokio::test]
async fn a_model_that_labels_its_own_answer_is_corrected() {
    // Smaller models copy the `Name:` shape they see for their peers.
    let alpha = ScriptedProvider::always("alpha", "Ada: I favour SQLite.");
    let room = room_with(
        TurnPolicy::RoundRobin,
        vec![Agent::new("Ada", "alpha", "m1")],
    );

    let (messages, _events) = run(
        &room,
        registry_with(std::slice::from_ref(&alpha)),
        "Which store?",
    )
    .await;

    assert_eq!(messages[1].content, "I favour SQLite.");
    assert_eq!(messages[1].speaker, "Ada");

    // The label must not reach the stream either, or a client that renders
    // deltas live shows it before the finished message replaces it.
    let streamed: String = _events
        .iter()
        .filter_map(|event| match event {
            SessionEvent::AgentDelta { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(streamed, "I favour SQLite.");
}

#[tokio::test]
async fn the_session_reports_the_summed_token_usage() {
    let alpha = ScriptedProvider::always("alpha", "A.");
    let beta = ScriptedProvider::always("beta", "B.");
    let room = room_with(
        TurnPolicy::RoundRobin,
        vec![
            Agent::new("Ada", "alpha", "m1"),
            Agent::new("Ben", "beta", "m2"),
        ],
    );

    let (_, events) = run(&room, registry_with(&[alpha, beta]), "Which store?").await;

    let completed = events
        .iter()
        .find_map(|event| match event {
            SessionEvent::SessionCompleted { usage, messages } => Some((*usage, *messages)),
            _ => None,
        })
        .expect("no session_completed event");
    assert_eq!(completed.1, 3);
    assert_eq!(completed.0.input_tokens, 20);
    assert_eq!(completed.0.output_tokens, 40);
}

#[tokio::test]
async fn the_user_prompt_is_announced_before_any_agent_speaks() {
    let alpha = ScriptedProvider::always("alpha", "A.");
    let room = room_with(
        TurnPolicy::RoundRobin,
        vec![Agent::new("Ada", "alpha", "m1")],
    );

    let (_, events) = run(&room, registry_with(&[alpha]), "Which store?").await;

    let first = events.first().expect("no events");
    match first {
        SessionEvent::UserMessage { message } => assert_eq!(message.content, "Which store?"),
        other => panic!("the first event was {other:?}"),
    }
}

#[tokio::test]
async fn requests_carry_the_agent_model_rather_than_the_provider_default() {
    let alpha = ScriptedProvider::always("alpha", "A.");
    let room = room_with(
        TurnPolicy::RoundRobin,
        vec![Agent::new("Ada", "alpha", "llama3:8b")],
    );

    run(
        &room,
        registry_with(std::slice::from_ref(&alpha)),
        "Which store?",
    )
    .await;

    let seen: &Seen = &alpha.requests()[0];
    assert_eq!(seen.model, "llama3:8b");
}
