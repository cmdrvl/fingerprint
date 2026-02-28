# fingerprint

<div align="center">

[![CI](https://github.com/cmdrvl/fingerprint/actions/workflows/ci.yml/badge.svg)](https://github.com/cmdrvl/fingerprint/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![GitHub release](https://img.shields.io/github/v/release/cmdrvl/fingerprint)](https://github.com/cmdrvl/fingerprint/releases)

**Know what your files are, not just that they exist.**

```bash
brew install cmdrvl/tap/fingerprint
```

</div>

---

You have 100,000 files in a data room. You've hashed them — you know the bytes are identical to yesterday. But *what kind* of file is `model_v3_FINAL.xlsx`? Is it an Argus DCF model, a rent roll, a debt schedule, or a blank template someone forgot to delete? The filename doesn't tell you. The extension doesn't tell you. The hash tells you the bytes haven't changed, but not what those bytes mean.

**fingerprint opens each file, runs versioned assertions against its structure, and tells you exactly what it is — or tells you it doesn't know.**

A fingerprint is a set of deterministic assertions that encode domain knowledge. "This file has a worksheet called Assumptions. Cell A3 says Market Leasing Assumptions. The data range A3:D10 is fully populated. Therefore: this is an Argus model, version 1." That knowledge is versioned, testable, and compiled to Rust. Match or no match — never "87% confident."

---

## What makes this different

### It learns from your documents

Point fingerprint at a folder of example files and it writes the assertions for you.

```bash
fingerprint infer ./cbre-appraisals/ --format markdown \
  --id cbre-appraisal.v1 --out cbre.fp.yaml
```

The infer engine observes structural facts across every document in the corpus — which headings always appear, which tables are always present, which text patterns recur — then emits a `.fp.yaml` definition with confidence annotations. It uses hybrid BM25 + semantic search (384-dim hash embeddings, deterministic, fully local, no GPU) to find invariant patterns and deduplicate boilerplate.

Same inputs, same frankensearch version, same output. Every time.

If you already know what fields you need but have only one example document, use **schema-driven infer** instead:

```bash
fingerprint infer-schema --doc appraisal.md \
  --fields fields.yaml --id cbre-appraisal.v1 --out cbre.fp.yaml
```

Where `fields.yaml` is:
```yaml
- name: as_of_date
  value: "June 15, 2024"
- name: cap_rate
  value: "6.25%"
- name: net_sf
  value: "125,000 SF"
```

Fingerprint locates each field value via hybrid search, finds the nearest stable anchor (heading, label, table header), and generates both the assertion and the extraction rule. One document in, production fingerprint out.

### Chained fingerprints

Real documents have layers. A CBRE appraisal is one document type, but inside it there's a rent roll, an income capitalization section, and a sales comparison grid — each independently versioned, each with its own content hash for change detection.

Chained fingerprints model this directly:

```yaml
# Parent: identifies the document type
fingerprint_id: cbre-appraisal.v1
format: pdf
assertions:
  - page_count: { min: 50, max: 500 }
  - text_contains: "CBRE Valuation & Advisory Services"

---
# Child: extracts the rent roll
fingerprint_id: cbre-appraisal.v1/rent-roll.v1
parent: cbre-appraisal.v1
assertions:
  - table_exists: { heading: "(?i)rent roll" }
extract:
  - name: rent_roll_table
    type: table
    anchor_heading: "(?i)rent roll"
content_hash:
  algorithm: blake3
  over: [rent_roll_table]

---
# Child: extracts the income cap approach
fingerprint_id: cbre-appraisal.v1/income-cap.v1
parent: cbre-appraisal.v1
assertions:
  - heading_regex: { pattern: "(?i)income capitalization" }
  - table_exists: { heading: "(?i)income capitalization" }
```

Parent match triggers child evaluation. Each child produces its own content hash. Add new children without touching the parent. Version each independently. The output includes a `children` array with independent match results — if the rent roll section disappeared in a new version, you'd see it immediately.

### It tells you what went wrong

When assertions fail, most tools say "no match." Fingerprint says *why*, and shows you what the document actually contains:

```bash
vacuum /data | hash | fingerprint --fp cbre-appraisal.v1 --diagnose
```

```json
{
  "name": "heading_regex_rent_roll",
  "passed": false,
  "detail": "No heading matched '(?i)rent roll'",
  "context": {
    "headings_found": ["Property Description", "Income Capitalization Approach", "RENT ROLL SUMMARY"],
    "nearest_match": "RENT ROLL SUMMARY"
  }
}
```

The `context` field shows every heading in the document and the closest match to what you were looking for. Your regex said `rent roll` but the document says `RENT ROLL SUMMARY` — now you know exactly what to fix. With `--diagnose`, all assertions are evaluated even if earlier ones fail, so you see the full diagnostic picture in one pass.

### Write YAML, get compiled Rust

Most fingerprints are authored as declarative YAML and compiled to optimized Rust crates:

```bash
fingerprint compile argus-model.fp.yaml --out fingerprint-argus-model-v1/
cargo install --path fingerprint-argus-model-v1/
fingerprint --list   # Now shows argus-model.v1
```

The compiler is deterministic — same YAML always produces the same Rust source. The compiled crate embeds `source_hash` (BLAKE3 of the canonical YAML) and `compiler_version` for provenance. Domain experts write YAML; the Rust compiler catches structural errors; the runtime gets native performance.

For cases the DSL can't express, write Rust directly against the `Fingerprint` trait. Both modes produce the same runtime artifact.

---

## 26 assertion types across every document structure

Fingerprint doesn't just check filenames and magic bytes. It understands the internal structure of spreadsheets, PDFs, markdown, and plain text.

### Spreadsheet assertions (XLSX, CSV)

| Assertion | What it checks |
|-----------|---------------|
| `sheet_exists` | Worksheet with this name exists |
| `sheet_name_regex` | Any worksheet name matches pattern (with optional `bind` to capture the name for reuse) |
| `cell_eq` | Cell contains exact value |
| `cell_regex` | Cell matches pattern |
| `range_non_null` | All cells in range are populated |
| `range_populated` | At least N% of cells non-empty |
| `sheet_min_rows` | Sheet has minimum data rows |
| `column_search` | Search a column range for a pattern — finds header rows at unknown positions |
| `header_row_match` | Find the row where N cells match column name patterns simultaneously |
| `sum_eq` | Sum of range equals expected value or cell reference |
| `within_tolerance` | Numeric value within declared bounds |

### Content assertions (PDF, Markdown, Text)

| Assertion | What it checks |
|-----------|---------------|
| `heading_exists` | Document contains heading with this text |
| `heading_regex` | Any heading matches pattern |
| `heading_level` | Heading exists at specific level (H1, H2, etc.) |
| `text_contains` | Exact text found anywhere in document |
| `text_regex` | Pattern matches anywhere |
| `text_near` | Pattern matches within N characters of an anchor — bidirectional |
| `section_non_empty` | Section under heading has content |
| `section_min_lines` | Section has minimum line count |
| `table_exists` | Table found under heading |
| `table_columns` | Table has columns matching these patterns |
| `table_shape` | Table has expected column count and inferred types (string, number, currency, date) |
| `table_min_rows` | Table has minimum data rows |
| `page_count` | PDF has expected page range (structural, no OCR) |
| `metadata_regex` | PDF metadata field (author, title, creator) matches pattern |

### Universal

| Assertion | What it checks |
|-----------|---------------|
| `filename_regex` | File basename matches pattern — the cheapest possible pre-filter |

Every assertion is deterministic. Every assertion produces structured context on failure. Every assertion is independently testable.

---

## Real-world: CMBS watchlist files

CMBS Excel workbooks from different servicers have different sheet names (`Watchlist`, `Servicer Watch List`, `Watchlist (2)`), different header row positions (row 4, 9, 11, or 12), and different column naming conventions. Filename conventions break constantly. Manual inspection doesn't scale.

```yaml
fingerprint_id: cmbs-watl.v1
format: xlsx

assertions:
  - name: watl_sheet_present
    sheet_name_regex:
      pattern: "(?i)watch\\s?list|WATL"
      bind: "watl_sheet"           # Capture whichever sheet name matched

  - name: has_header_row
    header_row_match:
      sheet: "{{watl_sheet}}"       # Reuse the bound name
      columns:
        - pattern: "(?i)loan.*id|trans.*id"
        - pattern: "(?i)balance|upb"
        - pattern: "(?i)status|watch.*code"
      min_matches: 2
      search_rows: 1..20            # Header could be anywhere in first 20 rows
```

The `bind` feature captures the actual sheet name that matched, so downstream assertions reference it without hardcoding. `header_row_match` solves the "header row at unknown position" problem — it searches a range of rows for one where multiple cells match column name patterns simultaneously. This handles real-world vendor variability that breaks every other approach.

---

## Pipeline native

Fingerprint is the third tool in the epistemic spine stream pipeline:

```
vacuum  -->  hash  -->  fingerprint  -->  lock  -->  pack
(scan)      (hash)     (recognize)      (pin)      (seal)
```

Each tool reads JSONL from stdin, enriches it, and emits JSONL to stdout.

```bash
# Fingerprint a data room and lock the result
vacuum /data/models/ | hash | fingerprint --fp argus-model.v1 \
  | lock --dataset-id "models-q4" > models.lock.json

# Stack fingerprints: specific first, general fallback
vacuum /data/ | hash | fingerprint --fp argus-model.v1 --fp xlsx.v0 --fp csv.v0

# Full pipeline with evidence sealing
vacuum /data/dec/ | hash | fingerprint --fp csv.v0 \
  | lock --dataset-id "dec-delivery" > dec.lock.json
pack seal dec.lock.json --note "December delivery" --output evidence/dec/
```

Every run is recorded in the ambient witness ledger (`~/.epistemic/witness.jsonl`) — content-addressed, hash-chained, auditable without explicit action.

---

## Working with PDFs

Fingerprint checks PDF structure (page count, metadata) natively via `lopdf`. For content assertions — headings, sections, tables — pair it with a local document extractor like Docling:

```bash
# Step 1: Extract markdown from PDFs (runs fully local, no data leaves machine)
docling --input-dir /data/appraisals/ --to md --output /data/appraisals/

# Step 2: Inject text_path and fingerprint
vacuum /data/appraisals/ --ext pdf \
  | hash \
  | jq -c '. + {"text_path": (.path | sub("\\.pdf$"; ".md"))}' \
  | fingerprint --fp cbre-appraisal.v1

# Step 3: With chained fingerprints for granular extraction
vacuum /data/appraisals/ --ext pdf \
  | hash \
  | jq -c '. + {"text_path": (.path | sub("\\.pdf$"; ".md"))}' \
  | fingerprint \
      --fp cbre-appraisal.v1 \
      --fp cbre-appraisal.v1/rent-roll.v1 \
      --fp cbre-appraisal.v1/income-cap.v1
```

Markdown normalization is built in — Setext-to-ATX heading conversion, bold-as-heading detection (Docling artifact), whitespace normalization, table pipe alignment — so the same logical content produces the same structural parse regardless of which extractor produced it.

| Extractor | Quality | Tables | Local | Notes |
|-----------|---------|--------|-------|-------|
| **Docling** (recommended) | High | Excellent | Yes | 258M VLM, structured markdown/JSON |
| `llm_aided_ocr` | High | Good | Depends on backend | Tesseract + LLM correction |
| `pdftotext` | Low | None | Yes | Plain text only; `format: text` fallback |

---

## Performance

From benchmark suite on the actual codebase:

| Operation | Throughput |
|-----------|-----------|
| Sheet exists assertion | 126-144 us |
| Cell equality assertion | 156-164 us |
| Text contains assertion | 214-262 ns |
| Single record pipeline | 4.5 us |
| Batch pipeline (100 records) | 212K records/sec |
| Registry scaling (50 fingerprints) | 10.3M lookups/sec |
| Markdown normalization | 8.5 MiB/sec |

Parallel processing is bounded by available CPUs (`--jobs <N>` to override). Output order always matches input order regardless of processing order.

---

## Installation

### Homebrew (recommended)

```bash
brew install cmdrvl/tap/fingerprint
```

### Shell script

```bash
curl -fsSL https://raw.githubusercontent.com/cmdrvl/fingerprint/main/scripts/install.sh | bash
```

### From source

```bash
cargo build --release
./target/release/fingerprint --help
```

---

## CLI Reference

### Run mode (default)

Stream enrichment — reads JSONL, tests each record against fingerprints, emits enriched records.

```bash
fingerprint [<INPUT>] [OPTIONS]
```

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--fp <ID>` | string | required | Fingerprint ID to test (repeatable, first match wins) |
| `--list` | flag | | List all available fingerprints and exit |
| `--diagnose` | flag | | Show full diagnostic context on assertion failures |
| `--jobs <N>` | integer | CPU count | Parallel workers |
| `--no-witness` | flag | | Suppress witness ledger recording |
| `--describe` | flag | | Print `operator.json` to stdout |
| `--schema` | flag | | Print JSON Schema to stdout |
| `--progress` | flag | | Emit structured progress JSONL to stderr |
| `--version` | flag | | Print version and exit |

### Compile mode

```bash
fingerprint compile <YAML> --out <DIR> [--check]
```

Compiles a `.fp.yaml` definition to a Rust crate implementing the `Fingerprint` trait. `--check` validates without generating code.

### Infer mode

```bash
# Learn from a corpus of examples
fingerprint infer <DIR> --format <FORMAT> --id <ID> --out <FILE> \
  [--min-confidence <FLOAT>] [--no-extract]

# Learn from one document + known field values
fingerprint infer-schema --doc <FILE> --fields <YAML> --id <ID> --out <FILE>
```

### Exit codes

| Code | Run mode | Compile mode |
|------|----------|--------------|
| `0` | ALL_MATCHED | Compiled successfully |
| `1` | PARTIAL (some unmatched/skipped) | Validation warnings |
| `2` | REFUSAL or CLI error | Malformed YAML or unsupported assertion |

### Refusal codes

| Code | Trigger | Next step |
|------|---------|-----------|
| `E_BAD_INPUT` | Invalid JSONL or missing `bytes_hash` | Run `hash` first |
| `E_UNKNOWN_FP` | Fingerprint ID not found | Check `fingerprint --list` |
| `E_DUPLICATE_FP_ID` | Duplicate ID across providers | Remove duplicate packs |
| `E_UNTRUSTED_FP` | External fingerprint not allowlisted | Add provider to allowlist |
| `E_INVALID_YAML` | YAML parse error (compile mode) | Fix the `.fp.yaml` file |
| `E_UNKNOWN_ASSERTION` | Unrecognized assertion type | Check supported types above |
| `E_MISSING_FIELD` | Required field missing from DSL | Add missing field |

Every refusal includes a concrete `next_command` when mechanical recovery is possible.

---

## Output contract

### Match record

```json
{
  "version": "fingerprint.v0",
  "path": "/data/models/deal-123.xlsx",
  "bytes_hash": "sha256:e3b0c44...",
  "fingerprint": {
    "fingerprint_id": "argus-model.v1",
    "fingerprint_crate": "fingerprint-argus",
    "fingerprint_version": "0.3.2",
    "fingerprint_source": "dsl",
    "matched": true,
    "reason": null,
    "assertions": [
      { "name": "assumptions_sheet_exists", "passed": true, "detail": null },
      { "name": "title_cell_correct", "passed": true, "detail": null }
    ],
    "extracted": {
      "market_leasing_assumptions": { "range": "A3:D10", "row_count": 8 }
    },
    "content_hash": "blake3:9f2a..."
  },
  "tool_versions": { "vacuum": "0.1.0", "hash": "0.1.0", "fingerprint": "0.2.0" }
}
```

### No-match record

```json
{
  "fingerprint": {
    "fingerprint_id": "argus-model.v1",
    "matched": false,
    "reason": "Assertion failed: sheet_exists('Assumptions') - sheet not found",
    "assertions": [
      {
        "name": "assumptions_sheet_exists",
        "passed": false,
        "detail": { "expected": "Assumptions", "found_sheets": ["Sheet1", "Data"] }
      }
    ],
    "content_hash": null
  }
}
```

### Skipped record handling

- **Upstream `_skipped: true`**: Passed through unchanged
- **New skip**: On I/O or parse failure, marked `_skipped: true` with warning appended to `_warnings`

---

## DSL fingerprint authoring

A `.fp.yaml` file is a complete fingerprint definition:

```yaml
fingerprint_id: argus-model.v1
format: xlsx

assertions:
  - sheet_exists: "Assumptions"
  - cell_eq:
      sheet: "Assumptions"
      cell: "A3"
      value: "Market Leasing Assumptions"
  - range_non_null:
      sheet: "Assumptions"
      range: "A3:D10"
  - sheet_min_rows:
      sheet: "Rent Roll"
      min_rows: 10

extract:
  - name: market_leasing_assumptions
    sheet: "Assumptions"
    range: "A3:D10"

content_hash:
  algorithm: blake3
  over: [market_leasing_assumptions]
```

Compile it, install it, use it:

```bash
fingerprint compile argus-model.fp.yaml --out fingerprint-argus-model-v1/
cargo install --path fingerprint-argus-model-v1/
vacuum /data/models | hash | fingerprint --fp argus-model.v1
```

---

## Agent integration

For the full toolchain guide, see the [Agent Operator Guide](https://github.com/cmdrvl/.github/blob/main/profile/AGENT_PROMPT.md).

```bash
# Self-describing contract
fingerprint --describe | jq '.refusals[] | select(.action == "retry_with_flag")'
fingerprint --schema | jq '.properties.fingerprint'

# Programmatic workflow
vacuum /data | hash | fingerprint --fp argus-model.v1 --fp csv.v0 > fp.jsonl

case $? in
  0) echo "all matched" ;;
  1) echo "partial"
     jq -s '[.[] | select(.fingerprint.matched == false)] | length' fp.jsonl ;;
  2) cat fp.jsonl | jq '.refusal.code'; exit 1 ;;
esac

cat fp.jsonl | lock --dataset-id "models" > models.lock.json
```

---

<details>
<summary><strong>Witness subcommands</strong></summary>

Every run is recorded to the ambient witness ledger:

```bash
fingerprint witness query --tool fingerprint --since 2026-01-01 --outcome ALL_MATCHED --json
fingerprint witness last --json
fingerprint witness count --since 2026-02-01
```

| Exit code | Meaning |
|-----------|---------|
| `0` | Matching records found |
| `1` | No matches |
| `2` | CLI error |

Ledger location: `~/.epistemic/witness.jsonl` (override with `EPISTEMIC_WITNESS`).

</details>

---

## Spec and development

The full specification is [`docs/PLAN.md`](./docs/PLAN.md). Benchmarks in [`docs/BENCHMARK_BASELINE.md`](./docs/BENCHMARK_BASELINE.md).

### CI gate

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --lib
cargo test \
  --test chained_fingerprint_scenarios \
  --test chained_fingerprints \
  --test content_assertion_edge_cases \
  --test infer_mode \
  --test infer_schema_mode \
  --test infer_subcommand \
  --test pipeline_integration \
  --test pipeline_parallel_execution \
  --test pipeline_run_mode \
  --test refusal_path_coverage \
  --test run_mode_pipeline
cargo test --test cli_smoke_surfaces
cargo test --test golden_output_determinism
```

---

## Part of the epistemic spine

fingerprint is one of nine shipped tools in [CMD+RVL's](https://github.com/cmdrvl) open-source epistemic toolchain. All tools are MIT licensed, deterministic, and composable via JSONL pipelines.

```
vacuum --> hash --> fingerprint --> lock --> shape --> rvl --> pack
                                              |
                                           profile
                                              |
                                            canon
```
