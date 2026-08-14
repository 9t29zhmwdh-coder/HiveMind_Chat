# Privacy Policy: HiveMind Chat

## What I Collect

Nothing. HiveMind Chat has no telemetry, no analytics, no crash reporting and no update check. I receive no data from your instance, and there is no server of mine for it to reach.

## What the Application Stores

Everything stays on the machine you run it on, in two files:

| File | Contents |
|---|---|
| `hivemind.db` (SQLite) | Rooms, agents, and the full transcript of every conversation. |
| `hivemind.toml` | Server settings and provider entries, including the *names* of the environment variables that hold your credentials. |

Neither file ever contains a credential. In the container both live in the `/data` volume.

The browser stores two things in `localStorage`: your language preference, and the access token if the instance uses one.

## What Leaves Your Machine

Only what you deliberately send, and only to the endpoints you configured.

- **Local providers.** With Ollama or another local endpoint, prompts and transcripts never leave the machine.
- **Hosted providers.** When a room contains an agent bound to a hosted endpoint, that turn's prompt is sent there. The prompt contains the room's system prompt, the conversation so far as that agent sees it, and your question. Their handling of that data is governed by their own terms, not by this policy.

Which agents are in a room determines exactly what is sent and where. A room with only local agents makes no outbound connection beyond your own network.

## Logs

The server logs requests, provider identifiers, and errors at the level set by `RUST_LOG`. Provider response bodies are logged only at `debug`. Credentials are never logged. Logs go to standard output and are not written to disk by the application itself.

## Deleting Your Data

Delete a room in the UI, or run `hive delete-room <id>`. To remove everything, delete `hivemind.db`, or run `docker compose down -v` for the container. There is no copy anywhere else.
