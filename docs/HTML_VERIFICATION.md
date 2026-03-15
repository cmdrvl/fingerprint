# HTML Verification Commands

Use these commands when validating the HTML rollout end to end.

## Full local verification

Run the full repository verification surface, including tests and clippy:

```bash
bash scripts/html_verify.sh
```

This is the non-interactive command intended for CI or local pre-merge checks.

## Legacy parity audit

Compare `fingerprint` family routing to a legacy route source and emit mismatch
artifacts under `artifacts/html-e2e/parity/<label>/`.

### With precomputed legacy routes

Prepare a JSONL file with one row per document:

```json
{"path":"/abs/path/to/filing.html","legacy_family":"ares"}
```

Then run:

```bash
bash scripts/html_parity_audit.sh \
  --definitions-dir rules \
  --legacy-results /tmp/legacy-routes.jsonl \
  --artifact-root artifacts/html-e2e \
  --label committed-fixtures
```

### Against an external legacy router

If the full 40+ filing corpus lives outside the repository, point the harness at
an external manifest and invoke the existing Python router with a command
template. `{path}` is replaced with the absolute document path for each file.

```bash
bash scripts/html_parity_audit.sh \
  --definitions-dir rules \
  --manifest /data/bdc/html_corpus.jsonl \
  --inventory tests/fixtures/html/inventory.json \
  --legacy-command-template 'python /path/to/fingerprint_schedule_family.py {path}' \
  --artifact-root artifacts/html-e2e \
  --label external-corpus \
  --diagnose-mismatches
```

## Key artifacts

- `matrix/<label>/` — raw fingerprint matrix run produced by the shared HTML e2e harness
- `parity/<label>/parity.summary.json` — overall parity counts and artifact pointers
- `parity/<label>/parity.mismatches.jsonl` — file-level mismatches with observed family, legacy family, child-routing status, and diagnose artifact paths
- `parity/<label>/legacy.routes.jsonl` — normalized legacy routing records used for the comparison

## Reading progress and diagnose artifacts

- `stderr.events.json` — parsed `--progress` stream from `fingerprint`; inspect this when a run appears to stall or when warning counts increase unexpectedly.
- `diagnostics.json` — normalized `fingerprint.diagnostics` payloads for each stdout record, including attempted fingerprints, first failed assertions, near misses, and short-circuit context.
- `fixture.summary.jsonl` — one row per document with `route_resolved`, `child_routing_status`, `selected_child_fingerprint_id`, `matched_child_fingerprint_ids`, refusal codes, and skip status.
- `family.summary.json` — aggregated routed-family counts keyed by expected family and selected fingerprint ID.
- `run.summary.json` — top-level counters such as `ambiguous_route_count`, `selected_child_count`, `progress_event_count`, `warning_event_count`, and the artifact file manifest for the run.

For mismatch triage, start with `parity.summary.json`, open the corresponding rows in
`parity.mismatches.jsonl`, then follow any `diagnose_artifact_dir` pointer into the
shared harness artifacts. The quickest signal is usually:

1. `fixture.summary.jsonl` to see whether the route failed, skipped, or became ambiguous.
2. `diagnostics.json` to identify which child assertion lost the route.
3. `stderr.events.json` to confirm whether parser warnings or progress anomalies happened during the run.
