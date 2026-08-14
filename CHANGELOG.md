# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-08-14

First release.

### Added

- Orchestration core (`hive-core`) with rooms, agents and five turn policies:
  parallel, round robin, debate, moderated and consensus.
- Provider layer covering Ollama, the Anthropic Messages API and any
  OpenAI-compatible endpoint, behind one streaming trait.
- Credential handling by environment variable reference: a provider entry
  stores the variable name, never the key.
- HTTP and WebSocket server (`hive-server`) serving the JSON API and the web UI
  from one port, with an optional shared access token, per-room turn locking
  and a concurrency cap.
- Terminal client (`hive`) for rooms, agents, live turns and Markdown export,
  working against the same database without a running server.
- Web UI in React and TypeScript with live token streaming, per-agent colours,
  vote results and English and German interface language.
- SQLite persistence with WAL, and transcript export as Markdown.
- Container image and a compose file with a read-only root filesystem, dropped
  capabilities and an unprivileged user.
- CI with formatting, Clippy, tests on Linux and macOS, a web build, dependency
  audits for both ecosystems, a container smoke test, CodeQL and OpenSSF
  Scorecard. All actions pinned to commit SHAs.

[0.1.0]: https://github.com/9t29zhmwdh-coder/HiveMind_Chat/releases/tag/v0.1.0
