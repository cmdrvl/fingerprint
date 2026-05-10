# GEMINI.md — fingerprint

This harness follows [`AGENTS.md`](./AGENTS.md).

Gemini agents should keep analysis grounded in the current repo files, not inferred product intent. If a behavior is not in `docs/PLAN.md`, update the plan or file a bead before implementing it.

Useful commands:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
br ready --json
br sync --flush-only
```

When reviewing CLI output, verify exit codes and stdout/stderr contracts. For `peek`, also verify that adversarial cell values do not appear in JSON output or witness records.
