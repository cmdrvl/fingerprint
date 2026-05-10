# CODEX.md — fingerprint

Codex agents inherit [`AGENTS.md`](./AGENTS.md). Read that file first.

Use narrow commands while editing, then run the full Rust gate before commit:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

For CLI surface work, start with focused tests such as `cargo test --test peek`, `cargo test doctor`, or the relevant integration test. Keep stdout JSON-only for run mode and `peek`; put diagnostics on stderr only when the existing contract permits it.

Before landing, update Beads with `br sync --flush-only`, run UBS on staged changed files, commit, push `main`, and push `origin main:master`.
