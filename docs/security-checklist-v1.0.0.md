# Security Checklist: v1.0.0

Worked through against `templates/security-checklist.md` from
`engineering-standards`. Items that do not apply are marked N/A with a reason
rather than omitted.

## Threat Modeling

- [x] STRIDE threat model exists and is current: [threat-model.md](threat-model.md)
- [x] Trust boundaries documented: browser to server, server to provider, server
      to disk, and model output to the UI

## Authentication and Authorization

- [x] N/A for MFA: there are no user accounts and no production system. Network
      access is gated by a shared token or by binding to loopback.
- [x] N/A for RBAC/ABAC: a single shared token grants full access by design,
      and that limitation is documented in the threat model and the roadmap.
- [x] N/A for Managed Identity: no Azure resources are involved.
- [x] Token lifetime reviewed: the access token does not expire, which is
      appropriate for a self-hosted instance and is rotated by restarting with
      a new value.

## Input Validation

- [x] Prompts, room and agent names, provider ids, base URLs and environment
      variable names are all validated at the boundary
- [x] Every SQL statement uses bound parameters; no string-concatenated SQL
- [x] Model output is rendered as text only. React escapes it, and there is no
      HTML, URL or shell context that it reaches.

## Secrets Management

- [x] No secret in the diff. The one test fixture that looked like a key was
      changed to `example-credential-...` so scanners do not flag it.
- [x] Secret scanning and push protection are enabled on the repository
- [x] `.env.example` lists every variable the application reads
- [x] Credentials are never persisted: a provider entry stores the *name* of an
      environment variable and the value is read per request

## Personal and Third-Party Information

- [x] No employer, client or colleague name, hostname or IP anywhere in the
      repository. Checked with a grep over the whole tree, including Cargo and
      npm metadata, which carry no `authors` field.
- [x] Screenshots use a synthetic room about database choices, produced against
      local models
- [x] N/A for IP ownership: built outside any employment context

## Encryption

- [x] Outbound calls to hosted providers use HTTPS through rustls
- [x] N/A for encryption at rest: transcripts are stored in the clear, which is
      documented as an accepted limitation in the threat model
- [x] No MD5, SHA-1 or DES anywhere. The only hash in the project is SHA-256
      over release archives.

## Error Handling

- [x] Provider error bodies are never echoed to the client; only the status line
      is surfaced, with the body at debug level. Network errors are reduced to a
      transport-level cause so URLs and query strings cannot leak.
- [x] Detailed errors are logged with the provider id, which is the correlation
      handle that matters here

## Dependency Security

- [x] `cargo audit --deny warnings` and `npm audit --audit-level=high` pass in
      CI on every pull request
- [x] `Cargo.lock` and `frontend/package-lock.json` are committed and match the
      installed tree
- [x] CycloneDX SBOMs for both ecosystems are generated in the release workflow
      and attached to the release archive

## Audit Logging

- [x] Every operation that writes or removes stored data emits an entry under
      the `hivemind::audit` target with the operation, the room and the caller's
      address
- [x] N/A for failed-authentication lockout: there are no accounts to lock. A
      wrong token is rejected in constant time and produces a 401.

## Documentation

- [x] README describes the security posture accurately, including that
      credentials are referenced rather than stored
- [x] Accepted limitations are listed in ROADMAP.md and explained in the threat
      model

## Sign-off

- Prepared by: Claude Opus 5, as AI pair
- Reviewed by: *pending maintainer review*
- Date: 2026-08-14
- Release version: v1.0.0
