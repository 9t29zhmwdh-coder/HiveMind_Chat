# Architecture

## Overview

Three crates and one web UI. The core owns the conversation logic and knows
nothing about HTTP; the server and the CLI are two front ends onto the same
core and the same database.

```
                    ┌──────────────┐        ┌──────────────┐
   browser ────────▶│ hive-server  │        │   hive-cli   │◀──── terminal
   (WebSocket)      │ Axum, REST   │        │ clap         │
                    └──────┬───────┘        └──────┬───────┘
                           │                       │
                           └───────────┬───────────┘
                                       ▼
                            ┌────────────────────┐
                            │     hive-core      │
                            │  Orchestrator      │
                            │  Providers         │
                            │  Store (SQLite)    │
                            └─────────┬──────────┘
                                      │
                 ┌────────────────────┼────────────────────┐
                 ▼                    ▼                    ▼
           Ollama (local)      Anthropic API      OpenAI-compatible
```

## Crates

| Crate | Responsibility |
|---|---|
| `hive-core` | Rooms, agents, providers, turn policies, persistence. No HTTP server, no CLI. |
| `hive-server` | REST API, WebSocket, static web UI, access token, concurrency limits. |
| `hive-cli` | Terminal client. Uses the core directly, so it needs no running server. |
| `frontend` | React and TypeScript. Built to static files that the server ships. |

## How a prompt becomes a conversation

1. The client sends `{"type": "prompt", ...}` over the room's WebSocket.
2. The server claims the room. One turn per room at a time, at most four rooms
   at once, so a second client is told the room is busy instead of interleaving
   its messages into the same transcript.
3. The orchestrator loads the policy and derives the speaking order. The
   starting position rotates per round, because the first speaker measurably
   frames what follows.
4. The transcript is cut to the room's context window: the most recent
   messages, never the oldest. The transcript itself keeps everything; only
   what reaches the model is trimmed.
5. For each speaker, the visible slice is projected into that agent's point of
   view: its own turns become assistant turns, everyone else's become user
   turns prefixed with the speaker name. Chat dialects have no concept of a
   named third participant, so without that prefix a model cannot tell two
   peers apart.
6. The provider streams the answer. Deltas are forwarded as `SessionEvent`s
   and buffered into a complete `Message`.
7. When the turn ends, the produced messages are written to SQLite in one
   transaction.

## Turn policies

| Policy | Speaking order | What each agent sees |
|---|---|---|
| `parallel` | All at once | Only the user prompt. Peer answers are withheld, so the outputs stay comparable. |
| `round_robin` | One after another | Everything said so far. |
| `debate` | One after another | Everything, plus an assigned stance that rotates through favour, against and neutral. |
| `moderated` | One per round | Everything. A moderator agent is asked who should speak; an unparseable answer falls back to rotation rather than stalling the room. |
| `consensus` | Discussion, then a vote | Everything. The closing vote is collected without streaming and reported as a result, not as another chat turn. |

## Providers

`ModelProvider` is the only thing the orchestrator knows about a model:

```rust
async fn list_models(&self) -> Result<Vec<String>>;
async fn chat(&self, request: ChatRequest) -> Result<ChatStream>;
```

Three implementations cover the field. Ollama speaks newline-delimited JSON;
Anthropic and the OpenAI dialect speak server-sent events. Both are
line-oriented, so one framing layer in `provider/sse.rs` serves all three. It
buffers raw bytes rather than decoded text, because a chunk boundary can fall
inside a multi-byte character.

Two provider-specific decisions are worth knowing:

- **The Anthropic provider does not send `temperature`.** Current models reject
  the sampling parameters with a 400, so an agent's temperature is honoured
  only by providers that still accept it.
- **Reasoning is off unless an agent asks for it.** Reasoning tokens come out
  of the same `max_tokens` budget as the answer, and a chat room wants short
  turns. Providers that reason by default are told to switch it off.

## Credentials

A provider entry stores the *name* of an environment variable, never a key:

```toml
api_key_env = "HIVEMIND_KEY_ANTHROPIC"
```

`SecretRef` resolves that name at request time and drops the value immediately.
Nothing sensitive reaches SQLite, `hivemind.toml`, the API responses, or the
logs. The web UI is told only whether a credential currently resolves.

## Storage

One SQLite file with three tables: `rooms`, `agents` and `messages`. Columns
added after a release are applied to an existing database on open, so an older
file keeps working rather than failing to read. WAL is on,
so the UI can read a transcript while a turn is still writing to it. Every
query runs on the blocking pool; the async runtime is never blocked by disk IO.

## Web UI

React and TypeScript, no UI framework and no state library. It holds two lists:
the stored transcript, and the turns still streaming. A partial answer lives
outside the transcript until its `agent_completed` event arrives, so it can
never be mistaken for a stored message.

The room is loaded over REST as well as over the socket. The socket is the live
source once it is ready; until then the REST copy lets the room render, so a
slow or failed socket still leaves a readable transcript rather than an empty
screen.

In a parallel room, consecutive agent turns of the same round are grouped into
one comparison block and laid out in columns. That grouping is a UI concern
only: the transcript itself is a flat list, exactly as it is stored.

## Deliberate limits

- **No user accounts.** A single shared access token, or loopback only. Adding
  real accounts means session management and a permission model, which is a
  bigger commitment than this tool has earned.
- **No retry on a failed agent.** A failing agent is reported and skipped. A
  retry would double the wait for everyone else in the room.
- **No vector store, no RAG.** The transcript is the context.
