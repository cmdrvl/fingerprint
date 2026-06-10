# Fingerprint Agent Playbook

Start with:

```bash
fingerprint --robot-triage
fingerprint capabilities --json
fingerprint robot-docs guide
```

Use `fingerprint --json` as a structured-output intent flag. Run mode already emits JSONL, so this flag is intentionally a no-op for normal stream enrichment.

Use `fingerprint doctor health --json` when only a cheap health check is needed. Use `fingerprint doctor --fix` only to confirm repair mode is unavailable; it exits `2` and prints safe read-only alternatives.

Do not expect doctor or top-level agent surfaces to read stdin, evaluate fingerprints, append witness records, mutate definitions, or create `.doctor/` artifacts.
