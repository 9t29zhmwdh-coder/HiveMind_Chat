# Contributing

Thanks for looking. I maintain this project on my own, so the process is short.

## Before you open a pull request

Run what CI runs:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
(cd frontend && npm run build)
```

All four must pass. CI additionally runs `cargo audit` and `npm audit`.

## What I am likely to merge

- Bug fixes, with a test that fails before the fix.
- A new provider implementation, if it fits the existing `ModelProvider` trait
  without widening it.
- Documentation corrections, including to the German README.
- A new turn policy, if you can describe in one sentence what question it
  answers that the existing five do not.

## What I am likely to decline

- Anything on the "deliberately out of scope" list in [ROADMAP.md](ROADMAP.md).
- New dependencies that replace a small amount of code I already understand.
- Reformatting that is not `cargo fmt`.

If you are unsure, open an issue before writing the code. I would rather say
"yes, but differently" before you spend an evening on it.

## Style

- Rust: `cargo fmt` decides layout. Functions stay short. Comments explain
  **why**, never what the line already says.
- TypeScript: strict mode, no `any`.
- Text: no dashes as punctuation. Swiss spelling in German, which means `ss`
  and never `ß`.
- Commits: `type(scope): description`, with `feat`, `fix`, `security`,
  `refactor`, `test`, `docs`, `chore`.

## Reporting a security issue

Not through an issue. See [SECURITY.md](SECURITY.md).
