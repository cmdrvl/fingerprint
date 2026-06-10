# Fingerprint Agent Ergonomics Scorecard - Pass 1

## Result

- Surfaces inventoried: 123
- Intent corpus entries: 201
- Recommendations applied: 4 / 4
- Median expected uplift: +16 points on agent discoverability surfaces
- Regression count: 0 after local probes

## Before

- Machine-readable health and capability discovery existed only under `fingerprint doctor ...`.
- `doctor --fix` was a generic clap error with no recovery command.
- `operator.json` advertised `json_flag: null`.
- The release workflow generated a Homebrew formula with a redundant explicit version.

## After

- `fingerprint --robot-triage` returns one read-only JSON report.
- `fingerprint capabilities --json` returns the machine-readable capability contract without reading stdin.
- `fingerprint robot-docs guide` prints concise operating notes for agents.
- `fingerprint --json` is accepted as structured-output intent while run-mode stdout remains JSONL.
- `fingerprint doctor --fix` exits `2` with safe alternatives and no stdout.
- The generated Homebrew formula lets Homebrew infer version from release URLs.

## Residual Risk

- Generic clap typo hints remain the primary behavior for misspelled flags outside the new agent surfaces.
- The doctor surface is still audit-only; any future repair mode needs detector, backup, inverse, fixture, and undo coverage before implementation.
