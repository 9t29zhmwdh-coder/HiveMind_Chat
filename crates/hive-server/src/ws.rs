//! The live conversation socket.
//!
//! One socket serves one room. The client sends prompts, the server streams the
//! orchestrator's events back. Turns are serialised per room, so a second
//! socket on the same room is told the room is busy rather than interleaving
//! its messages into the same transcript.

use std::sync::Arc;

use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::response::Response;
use futures::stream::SplitSink;
use futures::{SinkExt, StreamExt};
use hive_core::{Message, Room, SessionEvent};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::state::AppState;

/// How many events may queue between the orchestrator and a slow client.
const EVENT_BUFFER: usize = 256;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientCommand {
    /// Presents the access token. Required as the first frame when the server
    /// was started with one.
    Auth {
        token: String,
    },
    Prompt {
        text: String,
    },
    Stop,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerFrame {
    Ready {
        room: Box<Room>,
        history: Vec<Message>,
    },
    /// The orchestrator event is nested rather than flattened: both enums tag
    /// themselves with `type`, and flattening would collide on that key.
    Event {
        event: SessionEvent,
    },
    Stopped,
    Error {
        message: String,
    },
}

pub async fn upgrade(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    upgrade: WebSocketUpgrade,
) -> Response {
    upgrade.on_upgrade(move |socket| serve(socket, state, room_id))
}

async fn serve(socket: WebSocket, state: AppState, room_id: String) {
    let (mut sender, mut receiver) = socket.split();

    if !authenticate(&state, &mut sender, &mut receiver).await {
        return;
    }

    let room = match load_room(&state, &room_id, &mut sender).await {
        Some(room) => room,
        None => return,
    };

    while let Some(Ok(frame)) = receiver.next().await {
        let Some(command) = decode(&frame) else {
            continue;
        };
        match command {
            ClientCommand::Prompt { text } => {
                run_turn(&state, &room_id, &room, text, &mut sender, &mut receiver).await;
            }
            ClientCommand::Stop => {
                let _ = send(&mut sender, &ServerFrame::Stopped).await;
            }
            ClientCommand::Auth { .. } => {}
        }
    }
}

type Sink = SplitSink<WebSocket, WsMessage>;

/// Consumes the first frame when a token is configured.
///
/// The token travels in the socket body rather than the query string, which
/// would otherwise end up in proxy and browser history logs.
async fn authenticate(
    state: &AppState,
    sender: &mut Sink,
    receiver: &mut futures::stream::SplitStream<WebSocket>,
) -> bool {
    if state.access_token.is_none() {
        return true;
    }
    let presented = match receiver.next().await {
        Some(Ok(frame)) => match decode(&frame) {
            Some(ClientCommand::Auth { token }) => Some(token),
            _ => None,
        },
        _ => None,
    };
    if state.token_matches(presented.as_deref()) {
        return true;
    }
    let _ = send(
        sender,
        &ServerFrame::Error {
            message: "a valid access token is required".into(),
        },
    )
    .await;
    let _ = sender.close().await;
    false
}

async fn load_room(state: &AppState, room_id: &str, sender: &mut Sink) -> Option<Room> {
    let room = state.store.load_room(room_id).await;
    let history = state.store.load_messages(room_id, 200).await;
    match (room, history) {
        (Ok(room), Ok(history)) => {
            let frame = ServerFrame::Ready {
                room: Box::new(room.clone()),
                history,
            };
            send(sender, &frame).await.ok()?;
            Some(room)
        }
        (Err(error), _) | (_, Err(error)) => {
            let _ = send(
                sender,
                &ServerFrame::Error {
                    message: error.to_string(),
                },
            )
            .await;
            let _ = sender.close().await;
            None
        }
    }
}

/// Runs one prompt and streams its events, persisting the result at the end.
async fn run_turn(
    state: &AppState,
    room_id: &str,
    room: &Room,
    prompt: String,
    sender: &mut Sink,
    receiver: &mut futures::stream::SplitStream<WebSocket>,
) {
    let Some(_guard) = state.claim_room(room_id).await else {
        let _ = send(
            sender,
            &ServerFrame::Error {
                message: "this room is already running a turn".into(),
            },
        )
        .await;
        return;
    };

    let history = match state.store.load_messages(room_id, 200).await {
        Ok(history) => history,
        Err(error) => {
            let _ = send(
                sender,
                &ServerFrame::Error {
                    message: error.to_string(),
                },
            )
            .await;
            return;
        }
    };

    let (tx, mut rx) = mpsc::channel(EVENT_BUFFER);
    let orchestrator = Arc::clone(&state.orchestrator);
    let room_for_turn = room.clone();
    let turn = tokio::spawn(async move {
        orchestrator
            .run(&room_for_turn, &history, &prompt, &tx)
            .await
    });

    let stopped = forward_events(&mut rx, sender, receiver).await;
    if stopped {
        turn.abort();
        let _ = send(sender, &ServerFrame::Stopped).await;
        return;
    }

    match turn.await {
        Ok(Ok(messages)) => persist(state, sender, &messages).await,
        Ok(Err(error)) => {
            let _ = send(
                sender,
                &ServerFrame::Error {
                    message: error.to_string(),
                },
            )
            .await;
        }
        Err(error) => {
            tracing::error!(%error, "the turn task ended unexpectedly");
            let _ = send(
                sender,
                &ServerFrame::Error {
                    message: "the turn ended unexpectedly".into(),
                },
            )
            .await;
        }
    }
}

/// Pumps orchestrator events to the client until the turn ends.
///
/// Returns true when the client asked to stop, so the caller can abort the
/// still-running turn instead of waiting it out.
async fn forward_events(
    rx: &mut mpsc::Receiver<SessionEvent>,
    sender: &mut Sink,
    receiver: &mut futures::stream::SplitStream<WebSocket>,
) -> bool {
    loop {
        tokio::select! {
            event = rx.recv() => match event {
                Some(event) => {
                    if send(sender, &ServerFrame::Event { event }).await.is_err() {
                        return true;
                    }
                }
                None => return false,
            },
            incoming = receiver.next() => match incoming {
                Some(Ok(frame)) => {
                    if matches!(decode(&frame), Some(ClientCommand::Stop)) {
                        return true;
                    }
                }
                // A closed or broken socket ends the turn for the same reason a
                // stop command does: nobody is listening any more.
                _ => return true,
            },
        }
    }
}

async fn persist(state: &AppState, sender: &mut Sink, messages: &[Message]) {
    if let Err(error) = state.store.append_messages(messages).await {
        tracing::error!(%error, "could not persist the transcript");
        let _ = send(
            sender,
            &ServerFrame::Error {
                message: format!("the turn finished but was not saved: {error}"),
            },
        )
        .await;
    }
}

async fn send(sender: &mut Sink, frame: &ServerFrame) -> Result<(), axum::Error> {
    let payload = serde_json::to_string(frame).map_err(axum::Error::new)?;
    sender.send(WsMessage::Text(payload.into())).await
}

fn decode(frame: &WsMessage) -> Option<ClientCommand> {
    match frame {
        WsMessage::Text(text) => serde_json::from_str(text).ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_commands_are_tagged_by_type() {
        let auth: ClientCommand = serde_json::from_str(r#"{"type":"auth","token":"abc"}"#).unwrap();
        assert!(matches!(auth, ClientCommand::Auth { token } if token == "abc"));

        let prompt: ClientCommand =
            serde_json::from_str(r#"{"type":"prompt","text":"Hi"}"#).unwrap();
        assert!(matches!(prompt, ClientCommand::Prompt { text } if text == "Hi"));

        assert!(matches!(
            serde_json::from_str::<ClientCommand>(r#"{"type":"stop"}"#).unwrap(),
            ClientCommand::Stop
        ));
    }

    #[test]
    fn unknown_commands_are_ignored_rather_than_fatal() {
        assert!(serde_json::from_str::<ClientCommand>(r#"{"type":"launch_missiles"}"#).is_err());
        assert!(decode(&WsMessage::Text("not json".into())).is_none());
        assert!(decode(&WsMessage::Binary(vec![1, 2, 3].into())).is_none());
    }

    #[test]
    fn server_frames_carry_their_type_tag() {
        let frame = ServerFrame::Error {
            message: "nope".into(),
        };
        let json = serde_json::to_string(&frame).unwrap();
        assert!(json.contains(r#""type":"error""#));
        assert!(json.contains("nope"));
    }

    #[test]
    fn session_events_are_serialised_inside_the_event_frame() {
        let frame = ServerFrame::Event {
            event: SessionEvent::TurnCompleted { round: 2 },
        };
        let json = serde_json::to_string(&frame).unwrap();
        assert!(json.contains(r#""type":"event""#));
        assert!(json.contains(r#""type":"turn_completed""#));
        assert!(json.contains(r#""round":2"#));
    }
}
