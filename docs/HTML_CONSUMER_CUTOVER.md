# HTML Consumer Cutover

This document defines the local cutover surface for downstream consumers that
need BDC schedule-family routing from `fingerprint`.

## Current boundary

The historical weighted Python router is not present in this repository. This
repo now ships the authoritative adapter that downstream callers should invoke:

```bash
bash scripts/html_route_consumer.sh \
  --definitions-dir rules \
  --artifact-root artifacts/html-e2e \
  --label fingerprint-only
```

That adapter reads the same manifest/inventory inputs as the shared HTML harness,
routes documents from `fingerprint` child-selection results, and emits one JSONL
row per document on stdout plus artifact files under
`artifacts/html-e2e/consumer/<label>/`.

## Authoritative path

`fingerprint` is the source of truth for routing:

- The adapter derives `authoritative_family` from `resolved_fingerprint_id`
- `route_source: "fingerprint"` means the selected family came from the
  `fingerprint` child route, not from any weighted legacy score
- `fingerprint_route_status` preserves why a route failed (`ambiguous`,
  `no_child_match`, `unmatched`, `skipped`, `refusal`)

Output rows contain both `authoritative_family` and `effective_family` so a
consumer can see whether any fallback changed the final answer.

## Dual-run diff logging

Before a hard cutover, run the adapter in dual-run mode with a legacy route
source. The adapter keeps `fingerprint` authoritative, compares the results
side-by-side, and writes file-level diffs:

```bash
bash scripts/html_route_consumer.sh \
  --definitions-dir rules \
  --legacy-results /tmp/legacy-routes.jsonl \
  --artifact-root artifacts/html-e2e \
  --label dual-run \
  --diagnose-diffs
```

Artifact files:

- `consumer.routes.jsonl` — consumer-facing route rows
- `route.diffs.jsonl` — one row per file where legacy and `fingerprint` differ
- `consumer.summary.json` — authoritative/effective route counts, fallback count,
  unresolved counts, and diff totals
- `legacy.routes.jsonl` — normalized legacy inputs used for the dual run

If diffs exist, the adapter exits `1` by default so rollout tooling can fail
closed while still keeping per-file logs.

## Legacy fallback

The retained legacy path is **diagnostic/fallback only**. It is never the source
of truth unless the adapter is explicitly told to fill unresolved routes from the
legacy source:

```bash
bash scripts/html_route_consumer.sh \
  --definitions-dir rules \
  --legacy-command-template 'python /path/to/fingerprint_schedule_family.py {path}' \
  --legacy-fallback-on-unresolved \
  --allow-diffs \
  --artifact-root artifacts/html-e2e \
  --label rollback-window
```

Fallback behavior is deliberately narrow:

- Only unresolved `fingerprint` routes are filled from legacy output
- Resolved `fingerprint` routes remain authoritative even if legacy disagrees
- `route_source: "legacy_fallback"` and `fallback_applied: true` make the
  fallback visible in the output
- `route.diffs.jsonl` still records the disagreement so the rollback window does
  not hide drift

`--allow-diffs` is intended for temporary rollback windows where the consumer
must keep emitting a route while the mismatch set is being burned down.

## Rollout / rollback expectations

1. Start with `fingerprint`-only mode and confirm the consumer can read the
   emitted JSONL shape.
2. Enable dual-run logging against the legacy route source until
   `route.diffs.jsonl` is empty for the target corpus slice.
3. Cut the downstream caller over to the adapter without legacy flags.
4. If production encounters unresolved routes, use
   `--legacy-fallback-on-unresolved --allow-diffs` only as a temporary rollback
   window, and track every fallback via `consumer.summary.json`.

The external tournament or workflow repo should call the adapter above rather
than reaching directly into the shared harness scripts.
