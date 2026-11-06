# Source-location experiment

Consumer: `bd-lxy` / BDC source-addressable region extraction. This isolated crate
tests an alternative to searching for scraper-reserialized nodes in raw HTML.
It does not change fingerprint, its normalizer, DSL, dependency graph or output.

The probe uses pinned `lol_html` 3.0.2 start-tag source locations, inserts temporary
ordinal attributes, and parses the annotated input with the same `scraper` 0.22.0
used by fingerprint. It requires the entire serialized DOM to equal the original
DOM after removing those attributes. It checks source ranges and counts unique,
duplicated and implicit mappings separately. Input containing the marker is
rejected. Original filing files are never rewritten.

```sh
cargo test --locked --manifest-path experiments/source-location-probe/Cargo.toml
cargo clippy --locked --manifest-path experiments/source-location-probe/Cargo.toml --all-targets -- -D warnings
cargo run --locked --manifest-path experiments/source-location-probe/Cargo.toml -- /path/to/filing.htm
```

Each JSON line reports the input path, elapsed milliseconds and either an error or
`Ok: [unique_source_markers, duplicated_source_markers, implicit_elements]`.
Any input error makes the CLI exit 1. No cell text is emitted.

Seven tests cover case/quote/entity normalization, multibyte prefixes, quoted `>`
inside attributes, exact byte ranges for identical headings, script text containing
fake tags, implicit table bodies, marker collisions and reconstructed formatting
nodes. Duplicated markers are **not** unique source mappings.

This only demonstrates explicit **start-tag** locations and unchanged DOM content.
It does not produce complete element spans, select SOI boundaries, prove footnote
closure, or replace the native six-filer parity test. UTF-8 input is required; legacy
encoding support is not proven. It parses and serializes whole documents, so memory
and throughput need qualification before adoption. Timings are single local debug
runs, not a production benchmark. Independent review and native integration remain
open. Preserve the frozen default locator if an optional source-location mode is
implemented later.

`sources.json` pins the seven retained filing inputs and the probe's source, lockfile
and executable hashes. `results.json` records the final run: all seven full-document
DOM comparisons passed, with no duplicated markers in those particular filings.
These are the six reconstructed legacy filings plus the ICMB transition report.
The tests separately exercise a duplicated-marker adversary. Source files remain
outside Git; SEC URLs or raw S3 provenance and exact byte hashes identify them.
Native `bd-32p` / `bd-3hf` acceptance remains unchanged and unresolved.

Library source-location contract:
https://docs.rs/lol_html/3.0.2/lol_html/html_content/struct.SourceLocation.html
