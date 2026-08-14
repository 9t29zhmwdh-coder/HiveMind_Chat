# Security Policy

## Reporting a Vulnerability

**Do NOT open a public GitHub issue for security vulnerabilities.**

Instead, report it via [GitHub Security Advisory](https://github.com/9t29zhmwdh-coder/HiveMind_Chat/security/advisories/new) or contact the maintainer via the GitHub profile.

Include:

- Description of the vulnerability
- Steps to reproduce
- Potential impact
- Suggested fix (if any)

A response within **48 hours** is the target, and the issue will be worked on promptly.

## Credential Handling

This is the part of the design most worth reviewing.

- A provider entry stores the **name** of an environment variable (`api_key_env`), never a key. `SecretRef` resolves it per request and drops the value immediately.
- No credential is written to SQLite, to `hivemind.toml`, or to any API response. The web UI is told only whether a credential currently resolves.
- Environment variable names are restricted to `A-Z`, `0-9` and underscore, so a configuration file cannot smuggle shell syntax into a variable lookup.
- Provider error bodies are not echoed to the client: they have been observed to quote request headers. Only the status line is surfaced; the body goes to the trace log at debug level.
- `redact()` exists for the rare status output that needs to distinguish two keys, and keeps only the last four characters.

## Network Exposure

- The server binds to `127.0.0.1:8750` by default and the compose file publishes on loopback only.
- Set `HIVEMIND_ACCESS_TOKEN` before binding to any other address. Every API call must then present it as a bearer token, and every WebSocket must present it in its first frame. The comparison is constant-time, so a wrong token cannot be discovered one byte at a time.
- The server logs a warning when it binds to a non-loopback address without a token.
- There are no user accounts. The access token is a single shared secret, appropriate for a personal or small-team instance and not for a multi-tenant deployment.
- CORS is same-origin unless `allowed_origins` names other origins, because the server ships the UI it serves.

## Input Handling

- Prompts are capped at 32,000 characters and request bodies at 256 KB.
- Agent and room names reject control characters, which would otherwise let a name forge a speaker label in another agent's prompt.
- Provider base URLs must be `http://` or `https://`, keeping other schemes out of a URL used to build outbound requests.
- One turn per room at a time, at most four rooms concurrently.

## Supply Chain Security

- All GitHub Actions used in the CI pipeline are pinned to a specific commit SHA, not a mutable tag or branch.
- Dependencies are managed via `Cargo.lock` and `frontend/package-lock.json`, both committed for reproducible builds.
- `cargo audit --deny warnings` and `npm audit --audit-level=high` run in CI on every pull request.
- Dependabot watches the Cargo, npm, GitHub Actions and Docker ecosystems, which keeps the action pins from rotting.
- CodeQL runs as a workflow rather than through the repository's default setup, because the default setup never runs on Dependabot pull requests.
- Release archives carry a build provenance attestation and a SHA-256 checksum.

## Container Hardening

The compose file runs the image with a read-only root filesystem, `no-new-privileges`, all capabilities dropped, and as an unprivileged user (uid 10001). Only `/data` is writable.

## Supported Versions

| Version | Supported |
|---------|-----------|
| Latest  | ✅ Yes    |
| Older   | ❌ No     |

Security fixes are only applied to the latest release.
