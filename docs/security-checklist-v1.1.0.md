# Security Checklist: v1.1.0

Worked through against `templates/security-checklist.md` from
`engineering-standards`. This release covers everything shipped since v1.0.0,
namely v1.0.1, v1.0.2 and v1.1.0. Sections that are unchanged since
[security-checklist-v1.0.0.md](security-checklist-v1.0.0.md) say so and name
what was re-verified; sections touched by the delta are worked through in full.

## What Changed Since v1.0.0

| Release | Change | Security relevance |
|---------|--------|--------------------|
| v1.0.1 | Strip a self-added speaker label before a message is stored | Text handling only, no new trust boundary |
| v1.0.2 | Strip the same label from the live token stream | Text handling only, no new trust boundary |
| v1.1.0 | Per-room context limit, plus the schema migration for it | New input field and a schema change, worked through below |
| v1.1.0 | Dependency upgrades from Dependabot | Worked through below |

## Threat Modeling

- [x] Unchanged. The context limit adds no actor, no trust boundary and no data
      flow: it only shortens the transcript slice an agent is shown, on the
      server side, after the data has already crossed every existing boundary.
- [x] STRIDE threat model re-read against the delta and still accurate:
      [threat-model.md](threat-model.md)

## Authentication and Authorization

- [x] Unchanged. Verified that the constant-time token comparison in
      `crates/hive-server/src/state.rs` is still the only gate and was not
      touched by the delta.
- [x] No route was added for this feature. The context limit travels on the
      existing `PUT /api/rooms/{room_id}`, which sits in the protected router
      behind the same token as every other write.

## Input Validation

- [x] `context_limit` is validated at the boundary: the type is `u32`, so a
      negative value cannot be represented, and `Room::validate` rejects
      anything above 1000 messages. Zero is an explicit, documented opt-out that
      means no limit.
- [x] The slice itself cannot panic: `context_window` returns the whole
      transcript when the limit is zero or exceeds the length, and only then
      indexes from the tail
- [x] The migration uses a fixed statement with no interpolated value:
      `ALTER TABLE rooms ADD COLUMN context_limit INTEGER NOT NULL DEFAULT 40`.
      It is guarded by a column probe, so it is idempotent and cannot be
      triggered repeatedly to alter a table twice.
- [x] Every other SQL statement still uses bound parameters, including the
      rewritten `write_room`

## Secrets Management

- [x] Unchanged and re-verified: no secret in the tree, secret scanning and push
      protection both `enabled` on the repository, `.env.example` still lists
      every variable the code reads, which is exactly the one lookup in
      `crates/hive-core/src/secrets.rs`
- [x] The context limit is stored per room and holds no credential material

## Personal and Third-Party Information

- [x] Re-verified with a grep over the whole tree for employer, client and
      colleague references, private hostnames and RFC 1918 addresses: no hits.
      Cargo and npm metadata still carry no `authors` or `author` field.
- [x] The new UI strings in `frontend/src/i18n.ts` describe the feature only and
      name no person, employer or host

## Encryption

- [x] Unchanged. Outbound provider calls still go through rustls, and the
      dependency upgrade did not introduce a native TLS backend.
- [x] N/A for encryption at rest: unchanged accepted limitation, and the new
      column stores a count rather than content

## Error Handling

- [x] Unchanged. `error_from_response` still surfaces the status line only and
      logs the provider body at debug level, so a body that quotes request
      headers stays out of the response.
- [x] A rejected context limit produces a validation error naming the bound, not
      the internal state

## Dependency Security

- [x] `cargo audit --deny warnings` and `npm audit --audit-level=high` both pass
      on this tree, and both run in CI on every pull request
- [x] Upgrades in this release: `rusqlite` 0.37 to 0.40, `toml` 0.9 to 1.1,
      `tower-http` 0.6 to 0.7, `vite` 7 to 8, `typescript` 5.9 to 7,
      `@vitejs/plugin-react` 5.1 to 6.0
- [x] `rsqlite-vfs` and `sqlite-wasm-rs` appear in `Cargo.lock` as WebAssembly
      targets of `rusqlite` 0.40. `cargo tree --workspace -i` reports neither in
      the build graph for the targets this project ships, so they are recorded
      but never compiled.
- [x] `Cargo.lock` and `frontend/package-lock.json` are committed and match the
      installed tree
- [x] CycloneDX SBOMs for both ecosystems are generated in the release workflow
      and were confirmed present in the v1.1.0 archive
- [x] Every GitHub Action is pinned to a commit SHA, re-verified across all
      workflow files

## Audit Logging

- [x] The room update path emits an entry under the `hivemind::audit` target
      with the operation, the room and the caller's address, like every other
      write
- [x] N/A for failed-authentication lockout: unchanged, there are no accounts

## Documentation

- [x] Both READMEs and ARCHITECTURE.md describe the context window, including
      that it can be switched off, and the ROADMAP entry moved from planned to
      current
- [x] Accepted limitations in ROADMAP.md re-read and still current

## Sign-off

- Prepared by: Claude Opus 5, as AI pair
- Verified on 2026-08-15 against the repository at commit `5804f7a`, tag
  `v1.1.0`, not against the previous checklist: repository security settings via
  the GitHub API, the full diff from v1.0.0 to v1.1.0, the validation bound and
  the migration statement read in source, `cargo tree` for the new lock file
  entries, and a full run of `cargo audit`, `npm audit` and the test suite
  (136 tests, all passing)
- Signed off by: Rafael Yilmaz, maintainer
- Date: 2026-08-15
- Release version: v1.1.0
