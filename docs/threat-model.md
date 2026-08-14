# Threat Model

STRIDE analysis for HiveMind Chat v1.0.0. The scope is one instance run by one
person or a small team, either on loopback or on a private network.

## Assets

| Asset | Why it matters |
|---|---|
| Provider credentials | Direct financial cost and access to the account if leaked |
| Transcripts | Whatever the user typed into a room, which can be anything |
| The instance itself | Can spend the user's money by making paid model calls |

## Trust boundaries

```
[browser] ──HTTP/WS──▶ [hivemind-server] ──HTTPS──▶ [hosted provider]
                              │
                              ├──HTTP──▶ [local provider on the same host]
                              │
                              └──file──▶ [SQLite, hivemind.toml]

  boundary 1: browser to server        (access token, or loopback only)
  boundary 2: server to provider       (credential from the environment)
  boundary 3: server to disk           (file permissions of the host)
  boundary 4: model output to the UI   (untrusted text, rendered as text)
```

## STRIDE

### Spoofing

| Threat | Mitigation |
|---|---|
| Someone on the LAN calls the API as if they were the user | `HIVEMIND_ACCESS_TOKEN` is required on every API call and in the first socket frame. The comparison is constant-time. Without a token the server binds to loopback and warns loudly if told to bind elsewhere. |
| An agent claims to be another agent in the transcript | Speaker labels are added by the orchestrator, not by the model. Agent names reject control characters, so a name cannot forge a second label inside its own text. |

### Tampering

| Threat | Mitigation |
|---|---|
| A crafted configuration file reaches shell or filesystem access | Environment variable names are restricted to `A-Z0-9_`; provider base URLs must be `http://` or `https://`. |
| A malicious room definition exhausts memory | Request bodies are capped at 256 KB, prompts at 32,000 characters, agents at 16 per room, rounds at 20. |
| A dependency is swapped underneath the build | Lock files are committed, actions are pinned to commit SHAs, releases carry a provenance attestation and an SBOM. |

### Repudiation

| Threat | Mitigation |
|---|---|
| A transcript is deleted and nobody can tell when or by whom | Every operation that writes or removes stored data emits an audit entry with the operation, the room and the caller's address, under the `hivemind::audit` target. There are no user accounts, so the address is the strongest available identity. |

### Information disclosure

| Threat | Mitigation |
|---|---|
| A credential leaks through the API, the database or a backup | Credentials are never stored. A provider entry holds the *name* of an environment variable; the value is read per request and dropped. |
| A credential leaks through an error message | Provider error bodies are never echoed to the client; only the status line is surfaced, and the body goes to the debug log. Network errors are reduced to a transport-level cause. |
| A prompt is sent to a provider the user did not intend | The room's agent list determines exactly which endpoints are contacted. The UI shows each agent's provider and model, and a room with only local agents makes no outbound connection. |
| The browser leaks the token through a URL | The token travels in the `Authorization` header, and in the socket body rather than the query string, so it does not reach proxy or history logs. |

### Denial of service

| Threat | Mitigation |
|---|---|
| Many concurrent turns exhaust the host or the provider quota | One turn per room at a time, at most four rooms concurrently. Each provider has a connect and read timeout. |
| A hung provider blocks a room forever | The read timeout ends the turn; the client can also stop a turn, which aborts it server-side. |
| A large model answer fills the disk | `max_tokens` is capped at 32,000 per agent. |

### Elevation of privilege

| Threat | Mitigation |
|---|---|
| Model output is executed rather than displayed | Model output is only ever rendered as text. There is no tool use, no code execution, and no HTML interpretation of a message. |
| The container process escapes or persists | The image runs as an unprivileged user with a read-only root filesystem, all capabilities dropped and `no-new-privileges`. Only `/data` is writable. |

## Accepted limitations

These are known and deliberate for v1.0.0. Each is documented rather than
silently accepted.

- **No TLS in the server itself.** The server speaks plain HTTP and expects
  either loopback or a reverse proxy that terminates TLS. Adding certificate
  handling to the binary would duplicate what every deployment already has.
- **One shared token, no accounts.** Anyone with the token can do anything. The
  audit log records the caller's address, not an identity.
- **Prompt injection is not defended against.** A model can be talked into
  saying anything by another participant, because that is what a room of models
  is for. The mitigation is that output has no capability: it is text, rendered
  as text.
- **The transcript is stored in the clear.** SQLite file permissions are the
  protection. Anyone who can read the file can read the conversations.
