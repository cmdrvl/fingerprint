# CLAUDE.md — fingerprint

Follow [`AGENTS.md`](./AGENTS.md) as the source of truth.

Claude-specific reminders:

- Do not rewrite unrelated modules or normalize formatting outside touched files.
- Use exact file reservations if other agents are active.
- Keep `fingerprint peek` Veil-safe: no raw cell content in stdout, stderr, refusal details, or witness params.
- Use focused cargo tests first, then `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test`.
- Do not stop with local-only work. Sync Beads, commit, push `main`, and push `origin main:master`.
