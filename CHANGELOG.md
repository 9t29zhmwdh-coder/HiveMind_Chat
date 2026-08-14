# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.2] - 2026-08-14

### Fixed

- The speaker label a model added to its own answer was only stripped from the
  stored message, so a client rendering the live stream still showed it until
  the finished message replaced it. The opening of an answer is now held back
  just long enough to decide, and the label never reaches the stream.

### Changed

- Dependency updates: rusqlite 0.37 to 0.40, tower-http 0.6 to 0.7, vite 7 to 8,
  and the container build image from Node 22 to Node 26.

## [1.0.1] - 2026-08-14

### Fixed

- An agent that put its own name in front of its answer produced a doubled
  speaker label in the transcript. Peers appear as `Name: text` in the prompt,
  so smaller models copy that shape for themselves despite the house rule
  against it. The label is now stripped when it matches the speaker, while a
  quoted peer label and text that merely starts with the name are left alone.

## [1.0.0] - 2026-08-14

First release.

### Added

- Orchestration core (`hive-core`) with rooms, agents and five turn policies:
  parallel, round robin, debate, moderated and consensus.
- Provider layer covering Ollama, the Anthropic Messages API and any
  OpenAI-compatible endpoint, behind one streaming trait. `ProviderRegistry`
  accepts external implementations, so an embedder can add a dialect the crate
  does not ship.
- Credential handling by environment variable reference: a provider entry
  stores the variable name, never the key.
- HTTP and WebSocket server (`hive-server`) serving the JSON API and the web UI
  from one port, with an optional shared access token, per-room turn locking
  and a concurrency cap.
- Terminal client (`hive`) for rooms, agents, live turns and Markdown export,
  working against the same database without a running server.
- Web UI in React and TypeScript with live token streaming, per-agent colours,
  vote results, transcript search, and English and German interface language
  chosen from the browser rather than hard-coded.
- Side-by-side comparison view for the parallel policy, where independent
  answers to the same prompt are laid out in columns.
- Room duplication, which copies a line-up onto a new question without its
  transcript.
- SQLite persistence with WAL, and transcript export as Markdown.
- Container image and a compose file with a read-only root filesystem, dropped
  capabilities and an unprivileged user.
- CI with formatting, Clippy, tests on Linux and macOS, a web build, dependency
  audits for both ecosystems, a container smoke test, CodeQL and OpenSSF
  Scorecard. All actions pinned to commit SHAs.
- 120 tests, including end-to-end coverage of every turn policy against
  scripted providers.

### Notes

- The Anthropic provider deliberately omits `temperature`: current models
  reject the sampling parameters. An agent's temperature is honoured by the
  providers that still accept it.
- Reasoning is off unless an agent opts in, because reasoning tokens are drawn
  from the same `max_tokens` budget as the answer.

[1.0.2]: https://github.com/9t29zhmwdh-coder/HiveMind_Chat/releases/tag/v1.0.2
[1.0.1]: https://github.com/9t29zhmwdh-coder/HiveMind_Chat/releases/tag/v1.0.1
[1.0.0]: https://github.com/9t29zhmwdh-coder/HiveMind_Chat/releases/tag/v1.0.0
