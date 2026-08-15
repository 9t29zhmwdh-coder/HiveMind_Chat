# API Reference

The server exposes one HTTP API and one WebSocket. The web UI in `frontend/` is
an ordinary client of both, so anything the UI does can be done from a script.

Base URL in the default configuration: `http://127.0.0.1:8750`.

## Authentication

If the server was started with `HIVEMIND_ACCESS_TOKEN`, every route below except
`GET /api/health` and the WebSocket upgrade requires a bearer token:

```
Authorization: Bearer <token>
```

The WebSocket cannot carry a header through the browser API, so it authenticates
itself in its first frame instead. See [The socket](#the-socket).

Without a token the server binds to loopback only and refuses nothing. It logs a
warning if it binds to any other address without one.

## Limits

| Limit | Value |
|-------|-------|
| Request body | 256 KB |
| Prompt length | 32,000 characters |
| Agents per room | 16 |
| Rounds per turn | 1 to 20 |
| Context limit per room | 0 to 1000 messages, 0 meaning no limit |
| Concurrent turns | one per room, four rooms at a time |

## Errors

Errors come back as JSON with the shape below. A provider failure names the
provider and the HTTP status it returned, never the body it sent.

```json
{ "error": "a room holds at most 16 agents" }
```

| Status | Meaning |
|--------|---------|
| 400 | Validation failed, or the configuration rejects the request |
| 401 | Missing or wrong access token |
| 404 | No room, agent or provider by that id |
| 413 | Body over the size limit |
| 424 | The provider's credential does not resolve, so the environment variable it names is unset |
| 500 | The database could not be read or written |
| 502 | A provider returned an error or could not be reached |

A room that is already running a turn is not an HTTP error: turns start over the
socket, and the second socket receives an `error` frame.

## Routes

### `GET /api/health`

Open, no token. Returns `{"status":"ok","version":"1.1.1"}`. Used by the
container healthcheck.

### `GET /api/providers`

Lists the configured providers and whether each one's credential currently
resolves. The credential value itself is never returned.

`credential_env` names the environment variable a provider reads, and
`credential_available` says whether it currently resolves. A local provider
needs none, so it reports `null` and `true`.

```json
[
  {
    "id": "local",
    "label": "Ollama (local)",
    "kind": "ollama",
    "base_url": "http://127.0.0.1:11434",
    "local": true,
    "credential_env": null,
    "credential_available": true
  },
  {
    "id": "anthropic-main",
    "label": "Anthropic",
    "kind": "anthropic",
    "base_url": "https://api.anthropic.com",
    "local": false,
    "credential_env": "HIVEMIND_KEY_ANTHROPIC",
    "credential_available": true
  }
]
```

### `GET /api/providers/{provider_id}/models`

Asks the provider which models it currently serves. This is a live call, so it
fails with 502 if the provider is unreachable.

### `GET /api/policies`

Lists the five turn policies as `{"id": ..., "summary": ...}`: `parallel`,
`round_robin`, `debate`, `moderated` and `consensus`.

### `GET /api/rooms`

Lists all rooms, newest first, without their transcripts.

### `POST /api/rooms`

Creates a room. `agents` needs at least one entry; ids are assigned by the
server. `moderator_id` is required for the `moderated` policy and rejected for
every other one.

```json
{
  "name": "Storage engines",
  "topic": "Which database fits an append-heavy workload",
  "policy": "debate",
  "rounds": 3,
  "context_limit": 40,
  "agents": [
    {
      "name": "Ada",
      "provider_id": "local",
      "model": "llama3:8b",
      "persona": "Answers briefly and names trade-offs.",
      "colour": "#7c9cff",
      "max_tokens": 1024,
      "temperature": 0.7,
      "reasoning": false,
      "enabled": true
    }
  ]
}
```

Returns the created room with the generated ids and a `created_at` timestamp.
Every agent field except `name`, `provider_id` and `model` may be left out and
takes its default: `temperature` 0.7, `reasoning` false, `enabled` true.

`temperature` is passed to providers that accept it. It is deliberately not sent
to the Anthropic Messages API, which rejects the sampling parameters on current
models. `reasoning` switches extended thinking on for providers that offer it.

### `GET /api/rooms/{room_id}`

Returns one room with its full transcript.

### `PUT /api/rooms/{room_id}`

Replaces the room's configuration. The transcript is untouched. This is the
route that changes `context_limit`, the line-up, the policy or the topic.

### `DELETE /api/rooms/{room_id}`

Deletes the room and its transcript.

### `POST /api/rooms/{room_id}/duplicate`

Copies the room's line-up into a new room with a fresh id and an empty
transcript. Agent ids are regenerated and the moderator is remapped to the copy.

### `GET /api/rooms/{room_id}/transcript`

Exports the transcript as Markdown. The response is served as
`text/plain; charset=utf-8`.

### `DELETE /api/rooms/{room_id}/transcript`

Clears the transcript and keeps the room.

## The socket

`GET /api/rooms/{room_id}/ws` upgrades to a WebSocket. One socket serves one
room. A second socket on a room that is already running a turn is told the room
is busy rather than interleaving into the same transcript.

Both directions are JSON objects tagged with `type`.

### Client frames

| `type` | Fields | Meaning |
|--------|--------|---------|
| `auth` | `token` | Required as the first frame when the server has a token |
| `prompt` | `text` | Starts a turn with this prompt |
| `stop` | none | Asks the running turn to stop after the current agent |

### Server frames

| `type` | Fields | Meaning |
|--------|--------|---------|
| `ready` | `room`, `history` | Sent once on connect |
| `event` | `event` | One orchestrator event, see below |
| `stopped` | none | The turn ended because of a `stop` |
| `error` | `message` | The socket could not do what was asked |

The orchestrator event is nested under `event` rather than flattened, because
both enums tag themselves with `type` and flattening would collide.

### Orchestrator events

| `type` | Fields |
|--------|--------|
| `user_message` | `message` |
| `turn_started` | `round`, `rounds`, `policy`, `speakers` |
| `agent_started` | `agent_id`, `agent_name`, `colour`, `round` |
| `agent_delta` | `agent_id`, `text` |
| `agent_completed` | `message` |
| `agent_failed` | `agent_id`, `agent_name`, `error` |
| `vote_cast` | `agent_id`, `agent_name`, `choice`, `rationale` |
| `turn_completed` | `round` |
| `session_completed` | `messages`, `usage` |

`agent_delta` carries the token deltas as they arrive. An agent that prefixes
its own name has that prefix withheld, so the delta stream never shows a
duplicated speaker label.

`agent_failed` ends that agent's contribution but not the turn: the remaining
speakers still run.

### A minimal session

```
> {"type":"auth","token":"..."}
< {"type":"ready","room":{...},"history":[]}
> {"type":"prompt","text":"Which storage engine fits an append-heavy workload?"}
< {"type":"event","event":{"type":"user_message","message":{...}}}
< {"type":"event","event":{"type":"turn_started","round":1,"rounds":3,"policy":"debate","speakers":["Ada","Grace"]}}
< {"type":"event","event":{"type":"agent_started","agent_id":"...","agent_name":"Ada","colour":"#7c9cff","round":1}}
< {"type":"event","event":{"type":"agent_delta","agent_id":"...","text":"An append"}}
...
< {"type":"event","event":{"type":"session_completed","messages":6,"usage":{...}}}
```

## The terminal client

`hive` covers the same ground without a server: `providers`, `models`, `rooms`,
`new-room`, `add-agent`, `show`, `chat`, `export`, `duplicate-room` and
`delete-room`. Run `hive --help` for the current list. It talks to the SQLite
file directly, so it works when no server is up.
