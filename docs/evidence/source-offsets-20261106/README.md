# Native opt-in source offsets

`tokenized_byte_offsets: true` maps original DOM nodes to explicit source start
tags. The absent/false behavior is unchanged. The complete seven-input source
inventory and hashes are in `experiments/source-location-probe/sources.json`.
Bulk filing bytes remain outside Git.

Validation: 514 tests passed, zero failed/ignored; `cargo fmt --check`,
`cargo clippy --locked --all-targets -- -D warnings`, and
`bash scripts/selector_region_e2e.sh --fingerprint-bin "$PWD/target/debug/fingerprint"`
passed. UBS scanned the seven changed Rust files: zero critical, 760 warnings,
including existing code and tests. New mapping warnings concerned test unwraps,
validated indices, and copying small ranges; this is not a zero-warning result.

`parity.summary.json` and `tokenized-six.stdout.json` retain the native six-filing
run against the unchanged `legacy.jsonl` metrics: six selected, six matched, zero
mismatches, including the row-count check previously unavailable without source
offsets. These legacy metrics were reconstructed from published filings; the
original historical artifact was unavailable. This does not establish holdings
accuracy. The default six-filing stdout was also compared byte-for-byte with the
installed 0.12.0 output and was identical.

The parity run used `scripts/soi_parity.py`, the retained legacy metrics, the
six-filer manifest at `/tmp/bdc-six-filer-parity-20261106/manifest.jsonl`, and this
directory's definitions. Its binary wrapper did exactly:

```sh
exec /Users/zacharyruiz/Source/cmdrvl/fingerprint/target/debug/fingerprint --jobs 1 "$@"
```

The one-worker debug run took 296,701 ms, close to the harness's unchanged 300-second
limit. This is a correctness run, not production throughput qualification.

The ICMB v2 definition additionally retains the earlier `coalesce_by_as_of: false`
configuration. All 12 previously null offsets became present; every other region
field stayed equal. Some regions are empty/nested heading fragments or other
financial evidence, so this does not prove a complete SOI or footnote package.

`producer-hashes.json` identifies exact tested file and binary hashes;
`verification.json` summarizes checks and log hashes. The source-location option
remains unreleased and independently unreviewed. Original default tests and
expectations were not relaxed. Broader `bd-lxy` and `bd-3hf` acceptance remains open.
