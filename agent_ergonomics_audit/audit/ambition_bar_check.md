# Ambition Bar Check

Pass 1 meets the requested ergonomics bar for this release candidate:

- First-call discovery is now top-level, structured, and read-only.
- The operator manifest, README, and plan agree on `--json`, `--robot-triage`, `capabilities --json`, and `robot-docs guide`.
- The unavailable repair path is explicit and recoverable instead of a raw parser error.
- Regression tests cover the new routes, the side-effect contract, and the refusal path.
- Release packaging now avoids the Homebrew audit issue found in earlier spine tools.

Deferred: full semantic typo recovery for arbitrary user mistakes outside the new agent entrypoints.
