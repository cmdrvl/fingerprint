# Fixture Inventory

Core fixture corpus for `fingerprint` test bring-up.

## `files/` (legacy-compatible core corpus)

- `sample.csv` — happy-path CSV fixture for dispatch, spreadsheet assertions, and pipeline enrichment.
- `sample.xlsx` — happy-path XLSX fixture used by manifest-driven pipeline tests.
- `sample.xls` — happy-path legacy Excel fixture used to verify official `.xls` workbook support.
- `sample.pdf` — happy-path PDF fixture for structural assertion and dispatch tests.
- `sample.md` — markdown content paired with `sample.pdf` via `text_path` manifests.
- `cbre_appraisal_sample.md` — CBRE-style commercial real estate appraisal sample for markdown structure and content assertions.
- `financial_summary.md` — financial-summary markdown fixture with tables, business metrics, and formatting edge cases.
- `corrupt.xlsx` — intentionally invalid XLSX bytes for parse-failure / `_skipped` path testing.
- `unsupported.docx` — unsupported format fixture for testing `E_UNSUPPORTED_FORMAT` error paths.
- `image.png` — another unsupported format fixture for comprehensive format rejection testing.

## `test_files/` (expanded format corpus)

- `simple.csv` — additional CSV happy-path fixture.
- `sample.xlsx` — additional XLSX happy-path fixture.
- `report.pdf` — additional PDF happy-path fixture.
- `cbre_appraisal.md` — markdown content fixture paired with `report.pdf`.
- `plain_report.txt` — plain text fixture for `text_contains`, `text_regex`, and `text_near`.
- `corrupt.xlsx` — intentionally invalid XLSX bytes for parse-failure testing.

## `html/` (shared native HTML corpus)

- `README.md` — HTML-specific scenario guide and maintenance instructions.
- `inventory.json` — machine-readable inventory with expected heading/table/page counts and canonical hash-pair metadata.
- `generic_page_sections_schedule.html` — generic multi-page schedule using `<section data-page-number>`.
- `span_edge_cases.html` — focused `colspan`/`rowspan` table-expansion fixture.
- `malformed_static_schedule.html` — malformed-but-static HTML that must remain parseable.
- `ambiguity_trap_dual_headers.html` — overlapping header-token fixture for ambiguity and routing tests.
- `minimal_empty_shell.html` — negative-path HTML shell with no headings or tables.
- `hash_pair_base.html` — baseline HTML extract fixture for content-hash comparisons.
- `hash_pair_markup_variant.html` — markup-only variant of the baseline; extracted content should remain stable.
- `hash_pair_value_change.html` — value-changing variant of the baseline; extracted content hash should change.
- `bdc_soi_ares_like.html` — Ares-like BDC schedule example with business-description and coupon columns.
- `bdc_soi_bxsl_like.html` — BXSL-like BDC schedule example with investment-type section headings.
- `bdc_soi_pennant_like.html` — Pennant-like BDC schedule example with explicit `Industry` column and asset-class headings.
- `bdc_soi_golub_like.html` — Golub-like BDC schedule example with compact layout and industry section rows.
- `bdc_soi_blackrock_like.html` — BlackRock-like BDC schedule example with issuer/instrument leading columns.

## `manifests/` (hash-enriched and failure fixtures)

- `happy.jsonl` — baseline hash-enriched happy-path records referencing `files/`.
- `parse_fail.jsonl` — hash-enriched record referencing `files/corrupt.xlsx` for parse-failure path.
- `hashed_manifest.jsonl` — core hash-enriched records for CSV/XLSX/PDF happy-path pipeline tests.
- `hashed_manifest_with_text.jsonl` — hash-enriched PDF record with `text_path` for content assertion dispatch tests.
- `malformed_input.jsonl` — malformed JSONL fixture for refusal path tests (`E_BAD_INPUT`).
- `version_mismatch.jsonl` — manifest records with invalid version strings (`hash.v99`, `unknown.v1`) for version validation testing.
- `invalid_duplicates.jsonl` — manifest with duplicate records and missing required fields for validation and deduplication testing.
- `unsupported_formats.jsonl` — manifest entries pointing to unsupported file types (`.docx`, `.png`, `.json`) for format dispatch rejection testing.
- `html_corpus.jsonl` — deterministic hash-enriched references for every committed HTML fixture in `html/`.

## `witness/` (witness ledger and audit trail fixtures)

- `mixed_outcomes_witness.jsonl` — witness records with various outcomes (`ALL_MATCHED`, `PARTIAL`, `REFUSAL`) for outcome filtering and witness querying tests.
- `large_payload_witness.jsonl` — witness record with substantial payload content for stress testing serialization and storage limits.
- `malformed_witness.jsonl` — witness ledger with invalid JSON, incomplete records, and malformed entries for parsing robustness testing.
- `empty_ledger.jsonl` — empty witness ledger file for edge case testing of witness query operations on empty datasets.

## Intended usage

- Unit tests: document loaders and assertion engine format coverage.
- Integration/smoke tests: run-mode JSONL enrichment across core formats.
- Refusal tests: malformed input, unsupported formats, version mismatches, and parse-failure handling.
- Witness tests: audit trail generation, outcome filtering, and witness ledger query robustness.
- Edge case coverage: duplicate detection, validation logic, storage limits, and error path testing.
- Golden tests: deterministic output against a stable fixture corpus.
- Content assertion tests: text_near bidirectional search, table_shape type inference, markdown normalization, regex boundary conditions.
- HTML corpus tests: parser shape checks, known-family counts, ambiguity traps, and content-hash mutation regression.
- Chained fingerprint tests: parent-child inheritance, orphan scenarios, circular reference detection, E_NO_TEXT fallback paths.
- Parallel processing tests: deterministic pipeline ordering, resource contention handling, consistent execution under concurrency.
- Real-world document tests: CBRE appraisal reports, financial summaries, complex markdown structures, and representative BDC-style HTML schedules.
