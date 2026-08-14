# Roadmap

Version numbers follow [Semantic Versioning](https://semver.org). Nothing here
is a commitment to a date.

## v0.1.0 (current)

- Five turn policies: parallel, round robin, debate, moderated, consensus
- Ollama, Anthropic Messages API, and OpenAI-compatible endpoints
- Credentials by environment variable reference only
- Live streaming over WebSocket
- Web UI in English and German
- Terminal client
- SQLite persistence and Markdown export
- Container image with a hardened compose file

## v0.2.0 (planned)

- **Per-room model comparison view.** The parallel policy already produces the
  data; the UI should lay the answers out side by side rather than as a list.
- **Transcript search.** Across rooms, from the sidebar.
- **Agent presets.** A named persona plus model that can be dropped into any
  room, instead of retyping it.
- **Round-level retry.** Re-run one agent's turn without re-running the room.

## v0.3.0 (considered)

- **Tool use.** Letting an agent call a tool changes the trust model
  substantially, so it needs a permission design first, not just an
  implementation.
- **Cost accounting.** Token counts are already recorded per message; turning
  them into per-room cost needs a price table that stays current, which is a
  maintenance commitment rather than a feature.
- **Desktop build.** A Tauri wrapper around the same core, for people who do
  not want to run a server at all.

## Deliberately out of scope

- **User accounts and roles.** A shared access token fits a personal or
  small-team instance. Real accounts mean session management, a permission
  model and a migration path, which is a different product.
- **Hosting other people's conversations.** This is a tool you run for
  yourself, not a service.
- **A vector store or RAG pipeline.** The transcript is the context. Anything
  more belongs in a dedicated tool.
- **Prompt marketplaces or persona sharing.** Personas are two sentences; a
  sharing mechanism would be more code than the thing it shares.

## Dual-licensing readiness

Not applicable for now. HiveMind Chat is MIT and stays MIT. The problem it
solves is one of curiosity and evaluation rather than a business process, and
the honest answer is that it has no enterprise surface worth licensing
separately.
