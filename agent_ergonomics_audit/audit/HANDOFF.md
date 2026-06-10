# Fingerprint Ergonomics Handoff

This pass added the standard spine agent surfaces:

- `fingerprint --robot-triage`
- `fingerprint capabilities --json`
- `fingerprint robot-docs guide`
- `fingerprint --json`
- hidden `fingerprint doctor --fix` refusal

Key implementation files:

- `src/cli/args.rs` owns parsing.
- `src/lib.rs` short-circuits read-only agent routes before stream processing.
- `src/doctor.rs` owns shared health, triage, capabilities, docs, and fix-unavailable output.
- `tests/doctor.rs` locks in read-only behavior and machine-readable payloads.

Release note: version was bumped to `0.9.0`; after quality gates, tag `v0.9.0` and let the release workflow update `cmdrvl/tap/fingerprint`.
