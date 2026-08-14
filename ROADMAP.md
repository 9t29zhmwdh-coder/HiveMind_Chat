# Roadmap

Version numbers follow [Semantic Versioning](https://semver.org). Nothing here
is a commitment to a date.

## v1.0.0 (current)

- Five turn policies: parallel, round robin, debate, moderated, consensus
- Ollama, Anthropic Messages API, and OpenAI-compatible endpoints
- Credentials by environment variable reference only
- Live streaming over WebSocket
- Side-by-side comparison view for parallel rooms
- Transcript search and room duplication
- Web UI in English and German
- Terminal client
- SQLite persistence and Markdown export
- Container image with a hardened compose file

## v1.1.0 (current)

- Per-room context limit, so a long-running room keeps producing prompts a
  model can accept

## v1.2.0 (planned)

- **Agent presets.** A named persona plus model that can be dropped into any
  room. Room duplication covers most of this today, which is why it is not
  urgent.
- **Round-level retry.** Re-run one agent's turn without re-running the room.
- **Search across rooms.** Today the search is per room.

## Considered

- **Tool use.** Letting an agent call a tool changes the trust model
  substantially, so it needs a permission design first, not just an
  implementation.
- **Cost accounting.** Token counts are already recorded per message; turning
  them into per-room cost needs a price table that stays current, which is a
  maintenance commitment rather than a feature.
- **Desktop build.** A Tauri wrapper around the same core, for people who do
  not want to run a server at all.

## Known limitations

Documented rather than silently accepted; the reasoning is in
[docs/threat-model.md](docs/threat-model.md).

- The server speaks plain HTTP and expects loopback or a reverse proxy for TLS.
- One shared access token, so the audit log records an address rather than an
  identity.
- Transcripts are stored in the clear; file permissions are the protection.

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

Not applicable. HiveMind Chat is MIT and stays MIT. The problem it solves is
one of curiosity and evaluation rather than a business process, and the honest
answer is that it has no enterprise surface worth licensing separately.
