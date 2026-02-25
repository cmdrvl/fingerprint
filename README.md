# fingerprint

<div align="center">

[![CI](https://github.com/cmdrvl/fingerprint/actions/workflows/ci.yml/badge.svg)](https://github.com/cmdrvl/fingerprint/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![GitHub release](https://img.shields.io/github/v/release/cmdrvl/fingerprint)](https://github.com/cmdrvl/fingerprint/releases)

**Template recognition — matches artifacts against versioned template definitions using deterministic assertions, producing content hashes for change detection.**

No AI. No inference. Pure deterministic assertion matching and serialization.

```bash
brew install cmdrvl/tap/fingerprint
```

</div>

---

## TL;DR

**The Problem**: After hashing artifacts, you know *what* bytes exist — but not *what kind* of file it is. Is this Excel file an Argus model, a rent roll, or a random spreadsheet? Teams rely on filename conventions, manual inspection, or brittle regex scripts that break when formats change.

**The Solution**: Fingerprints are versioned template definitions that encode domain knowledge as executable assertions — "this file has an Assumptions sheet, cell A3 says Market Leasing Assumptions, and the data range is populated." Each assertion is deterministic code, not a probability. Match or no match.

### Why Use fingerprint?

| Feature | What It Does |
|---------|--------------|
| **Deterministic** | Assertions are code, not models — `matched: true` or `matched: false`, never "87% confident" |
| **Versioned templates** | Each fingerprint has an ID and version (`argus-model.v1`) — templates evolve without breaking |
| **Content hashing** | Extracts matched sections and computes BLAKE3 content hash for change detection |
| **Multi-fingerprint** | `--fp` is repeatable — first match wins, so you can stack specific → general |
| **DSL authoring** | Write fingerprints in YAML, compile to Rust crates — no hand-coding parsers |
| **Pipeline native** | Reads hash JSONL, emits enriched JSONL for `lock` |
| **Skipped tracking** | Parse failures are captured with warnings, not silently dropped |
| **Audit trail** | Every run recorded in the ambient witness ledger |

---

## Quick Example

```bash
$ vacuum /data/models | hash | fingerprint --fp argus-model.v1
```

```jsonl
{"version":"fingerprint.v0","path":"/data/models/deal-123.xlsx","relative_path":"deal-123.xlsx","root":"/data/models","size":2481920,"mtime":"2025-12-31T12:00:00.000Z","extension":".xlsx","mime_guess":"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet","bytes_hash":"sha256:e3b0c44...","hash_algorithm":"sha256","fingerprint":{"fingerprint_id":"argus-model.v1","fingerprint_crate":"fingerprint-argus","fingerprint_version":"0.3.2","fingerprint_source":"dsl","matched":true,"reason":null,"assertions":[{"name":"assumptions_sheet_exists","passed":true,"detail":null},{"name":"title_cell_correct","passed":true,"detail":null}],"extracted":{"market_leasing_assumptions":{"range":"A3:D10","row_count":8}},"content_hash":"blake3:9f2a..."},"tool_versions":{"vacuum":"0.1.0","hash":"0.1.0","fingerprint":"0.1.0"}}
```

One artifact matched — all assertions passed, content extracted, content hash computed. Ready for `lock`.

```bash
# Stack multiple fingerprints (first match wins):
$ vacuum /data | hash | fingerprint --fp argus-model.v1 --fp csv.v0

# List available fingerprints:
$ fingerprint --list

# Full pipeline into lockfile:
$ vacuum /data/models | hash | fingerprint --fp argus-model.v1 \
    | lock --dataset-id "models" > models.lock.json
```

---

## Where fingerprint Fits

`fingerprint` is the **third tool** in the stream pipeline — it recognizes what kind of artifact each file is.

```
vacuum  →  hash  →  fingerprint  →  lock  →  pack
(scan)    (hash)    (template)     (pin)    (seal)
```

Each tool reads JSONL from stdin and emits enriched JSONL to stdout. `fingerprint` receives hashed records and adds template match results — which fingerprint matched, which assertions passed, what content was extracted, and a content hash for change detection.

---

## What fingerprint Is Not

`fingerprint` does not replace other tools.

| If you need... | Use |
|----------------|-----|
| Enumerate files in a directory | [`vacuum`](https://github.com/cmdrvl/vacuum) |
| Compute SHA256/BLAKE3 hashes | [`hash`](https://github.com/cmdrvl/hash) |
| Pin artifacts into a self-hashed lockfile | [`lock`](https://github.com/cmdrvl/lock) |
| Check structural comparability of CSVs | [`shape`](https://github.com/cmdrvl/shape) |
| Explain numeric changes between CSVs | [`rvl`](https://github.com/cmdrvl/rvl) |
| Bundle into immutable evidence packs | [`pack`](https://github.com/cmdrvl/pack) |

`fingerprint` only answers: **does this artifact match a known template, and what content did it contain?**

---

## Two Modes

### Run Mode (default)

Stream enrichment — reads JSONL, tests each record against fingerprints, emits enriched records.

```bash
$ vacuum /data | hash | fingerprint --fp csv.v0
```

### Compile Mode

DSL compilation — reads a `.fp.yaml` fingerprint definition and generates a Rust crate.

```bash
$ fingerprint compile argus-model.fp.yaml --out fingerprint-argus-model-v1/
```

The compiler is deterministic: same YAML always produces the same Rust source. Generated crates embed `compiler_version`, `source_hash` (BLAKE3 of canonical YAML), and `source: "dsl"`.

---

## The Three Outcomes

### 1. ALL_MATCHED (exit `0`)

Every input record matched a fingerprint. No skipped or unmatched records.

### 2. PARTIAL (exit `1`)

At least one record didn't match any fingerprint or was `_skipped`. The output is valid but incomplete — `exit 1` forces explicit handling.

### 3. REFUSAL (exit `2`)

Input stream is invalid or a fingerprint ID wasn't found.

```json
{
  "code": "E_UNKNOWN_FP",
  "message": "Fingerprint ID not found",
  "detail": { "fingerprint_id": "argus-model.v1", "available": ["csv.v0", "xlsx.v0"] },
  "next_command": "cargo install fingerprint-argus"
}
```

Refusals always include a concrete `next_command` when possible.

---

## DSL Assertions

Fingerprints are defined in `.fp.yaml` files using a declarative assertion DSL:

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

### Available Assertions

| Assertion | Purpose | Example |
|-----------|---------|---------|
| `filename_regex` | File basename matches regex | `pattern: "(?i)financials?"` |
| `sheet_exists` | Worksheet with name exists | `"Assumptions"` |
| `sheet_name_regex` | Any worksheet name matches regex | `pattern: "(?i)FINF"` |
| `cell_eq` | Cell contains exact value | `sheet: "...", cell: "A3", value: "..."` |
| `cell_regex` | Cell matches regex | `sheet: "...", cell: "B1", pattern: "^FY20[0-9]{2}$"` |
| `range_non_null` | All cells in range are non-empty | `sheet: "...", range: "A3:D10"` |
| `range_populated` | Percentage of cells non-empty | `min_pct: 0.8` |
| `sheet_min_rows` | Sheet has minimum data rows | `sheet: "...", min_rows: 10` |
| `sum_eq` | Sum of range equals value/cell | `range: "D3:D10", equals_cell: "D11"` |
| `within_tolerance` | Value within numeric range | `cell: "E5", min: 0, max: 1` |

Every assertion is deterministic — it either passes or fails, with structured detail about why.

---

## How fingerprint Compares

| Capability | fingerprint | `file` / libmagic | Regex scripts | ML classifiers |
|------------|-------------|-------------------|---------------|----------------|
| Domain-specific template matching | Yes | No (magic bytes only) | Partial | Probabilistic |
| Versioned template definitions | Yes | No | No | No |
| Content extraction + hashing | Yes | No | You write it | You write it |
| Assertion-level detail | Yes (each assertion result) | No | No | No |
| DSL → compiled Rust | Yes | N/A | N/A | N/A |
| Deterministic (no probability) | Yes | Yes | Yes | No |
| Pipeline native (JSONL) | Yes | No | You write it | You write it |
| Audit trail (witness ledger) | Yes | No | No | No |

**When to use fingerprint:**
- Template recognition — determine if an Excel/CSV/PDF matches a known format
- Content change detection — extract specific sections and hash them for drift detection
- Pipeline enrichment — add template metadata before locking artifacts

**When fingerprint might not be ideal:**
- You need magic byte detection — use `file` command
- You need fuzzy/probabilistic matching — fingerprint is binary: match or no match
- You need to extract and transform data — fingerprint identifies templates, not transforms data

---

## Installation

### Homebrew (Recommended)

```bash
brew install cmdrvl/tap/fingerprint
```

### Shell Script

```bash
curl -fsSL https://raw.githubusercontent.com/cmdrvl/fingerprint/main/scripts/install.sh | bash
```

### From Source

```bash
cargo build --release
./target/release/fingerprint --help
```

---

## CLI Reference

### Run Mode

```bash
fingerprint [<INPUT>] [OPTIONS]
```

#### Arguments

- `[INPUT]`: JSONL manifest file. Defaults to stdin.

#### Options

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--fp <ID>` | string | required | Fingerprint ID to test (repeatable, first match wins) |
| `--list` | flag | `false` | List all available fingerprints and exit `0` |
| `--jobs <N>` | integer | CPU count | Parallel workers |
| `--no-witness` | flag | `false` | Suppress witness ledger recording |
| `--describe` | flag | `false` | Print compiled `operator.json` to stdout, exit `0` |
| `--schema` | flag | `false` | Print JSON Schema to stdout, exit `0` |
| `--progress` | flag | `false` | Emit structured progress JSONL to stderr |
| `--version` | flag | `false` | Print `fingerprint <semver>` to stdout, exit `0` |

### Compile Mode

```bash
fingerprint compile <YAML> [OPTIONS]
```

#### Arguments

- `<YAML>`: DSL fingerprint file (`.fp.yaml`)

#### Options

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--out <DIR>` | string | required | Output directory for generated Rust crate |
| `--check` | flag | `false` | Validate YAML without generating code |

### Exit Codes

**Run mode:**

| Code | Meaning |
|------|---------|
| `0` | ALL_MATCHED (every record matched a fingerprint) |
| `1` | PARTIAL (some records unmatched or skipped) |
| `2` | REFUSAL or CLI error |

**Compile mode:**

| Code | Meaning |
|------|---------|
| `0` | Compilation succeeded |
| `1` | Validation warnings (crate generated but has issues) |
| `2` | Refusal (malformed YAML, unsupported assertion type) |

### Streams

- `stdout`: enriched JSONL records (run mode) or generated files (compile mode)
- `stderr`: progress diagnostics (with `--progress`) or warnings

---

## Input / Output Contract

### Input

JSONL records with `bytes_hash` (from `hash`). Required fields for non-skipped records:

- `path` — absolute file path
- `bytes_hash` — content hash from `hash`
- `version` — upstream record version

### Output Record

Each input record is enriched with fingerprint results:

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
  "tool_versions": { "vacuum": "0.1.0", "hash": "0.1.0", "fingerprint": "0.1.0" }
}
```

### No-Match Record

When an artifact doesn't match any fingerprint:

```json
{
  "fingerprint": {
    "fingerprint_id": "argus-model.v1",
    "matched": false,
    "reason": "Assertion failed: sheet_exists('Assumptions') — sheet not found",
    "assertions": [
      { "name": "assumptions_sheet_exists", "passed": false, "detail": { "expected": "Assumptions", "found_sheets": ["Sheet1", "Data"] } }
    ],
    "extracted": null,
    "content_hash": null
  }
}
```

When multiple `--fp` are provided, each is tried in order. First match wins. If none match, the record shows the last fingerprint tried.

### Skipped Record Handling

- **Upstream `_skipped`**: Passed through unchanged — only `version` and `tool_versions` updated
- **New skip**: On I/O or parse failure, fingerprint marks `_skipped: true` and appends a warning

---

## Refusal Codes

**Run mode:**

| Code | Trigger | Next Step |
|------|---------|-----------|
| `E_BAD_INPUT` | Invalid JSONL, missing `bytes_hash` on non-skipped rows, or unsupported upstream version | Run `hash` first |
| `E_UNKNOWN_FP` | Fingerprint ID not found | Check `fingerprint --list` for available IDs |
| `E_DUPLICATE_FP_ID` | Duplicate fingerprint ID found across providers | Remove duplicate packs or pin one provider |
| `E_UNTRUSTED_FP` | External fingerprint not allowlisted | Add provider to allowlist or use built-in fingerprints |

**Compile mode:**

| Code | Trigger | Next Step |
|------|---------|-----------|
| `E_INVALID_YAML` | YAML parse error or schema violation | Fix the `.fp.yaml` file |
| `E_UNKNOWN_ASSERTION` | Assertion type not recognized | Check supported assertion types |
| `E_MISSING_FIELD` | Required field missing from DSL | Add missing field to assertion definition |

---

## Troubleshooting

### "E_BAD_INPUT" — missing bytes_hash

You piped vacuum output directly to fingerprint without hashing first:

```bash
# Wrong:
vacuum /data | fingerprint --fp csv.v0

# Right:
vacuum /data | hash | fingerprint --fp csv.v0
```

### "E_UNKNOWN_FP" — fingerprint not found

The fingerprint ID you specified isn't installed. Check what's available:

```bash
fingerprint --list
```

If you need a custom fingerprint, write a `.fp.yaml` and compile it:

```bash
fingerprint compile my-template.fp.yaml --out fingerprint-my-template/
cargo install --path fingerprint-my-template/
```

### All records show `matched: false`

Your fingerprint assertions don't match the actual file contents. Check which assertion failed:

```bash
vacuum /data | hash | fingerprint --fp argus-model.v1 \
  | jq 'select(.fingerprint.matched == false) | .fingerprint.assertions[] | select(.passed == false)'
```

### PARTIAL exit but expected ALL_MATCHED

Some files didn't match any fingerprint or were skipped. Check the output for unmatched and skipped records:

```bash
# Unmatched:
jq 'select(.fingerprint.matched == false) | .path' output.jsonl

# Skipped:
jq 'select(._skipped == true) | .path' output.jsonl
```

### content_hash is null

Content hash is only computed when a fingerprint matches. No match = no extraction = no content hash.

---

## Limitations

| Limitation | Detail |
|------------|--------|
| **No probabilistic matching** | Assertions are binary — match or no match. No confidence scores |
| **Format support** | v0 supports CSV, XLSX, PDF. Other formats require custom crates |
| **No data extraction/transformation** | fingerprint identifies templates — use downstream tools for data work |
| **Plugin discovery** | `FINGERPRINT_PATH` for custom crate discovery is deferred in v0.1 |
| **Compile mode** | Available in v0.1; advanced assertion coverage still evolves over time |
| **Advanced assertions** | `range_populated`, `sum_eq`, `within_tolerance` deferred in v0 |

---

## FAQ

### Why not just use filename conventions?

Filenames lie. A file named `model.xlsx` might be a rent roll, a debt schedule, or a blank template. Fingerprint opens the file and checks structural assertions — sheet names, cell values, data ranges — to determine what it actually is.

### Why compile YAML to Rust?

Performance and correctness. YAML is easy to write; compiled Rust is fast to execute and catches errors at compile time. The compiler is deterministic — same YAML always produces the same Rust source, with a `source_hash` for provenance.

### What happens when multiple `--fp` are specified?

First match wins. Fingerprints are tried in order. If the first `--fp` matches, subsequent ones are skipped. If none match, the record shows the last fingerprint tried with its failure reason.

### Can I write fingerprints in Rust directly?

Yes. Any crate implementing the Fingerprint trait works. DSL-compiled crates have `fingerprint_source: "dsl"`; hand-written crates have `fingerprint_source: "rust"`.

### What is `content_hash`?

A BLAKE3 hash of the extracted content sections. If two files match the same fingerprint but have different data in the extraction ranges, their `content_hash` values will differ. This enables change detection without re-running the full comparison.

### Does fingerprint modify the original files?

No. fingerprint is read-only. It opens files to run assertions and extract content, but never modifies them.

### Why is PARTIAL exit 1 instead of exit 0?

Because unmatched files mean the pipeline didn't fully classify the dataset. `exit 1` forces automation to handle it explicitly — either add more fingerprints or accept partial coverage.

---

## Agent / CI Integration

### Self-describing contract

```bash
$ fingerprint --describe | jq '.exit_codes'
{
  "0": { "meaning": "ALL_MATCHED" },
  "1": { "meaning": "PARTIAL" },
  "2": { "meaning": "REFUSAL" }
}

$ fingerprint --describe | jq '.pipeline'
{
  "upstream": ["hash"],
  "downstream": ["lock"]
}
```

### Agent workflow

```bash
# 1. Fingerprint all artifacts
vacuum /data/models | hash | fingerprint --fp argus-model.v1 --fp csv.v0 > fp.jsonl

case $? in
  0) echo "all matched" ;;
  1) echo "partial — some files unrecognized"
     jq -s '[.[] | select(.fingerprint.matched == false)] | length' fp.jsonl ;;
  2) echo "refusal"
     cat fp.jsonl | jq '.refusal.code'
     exit 1 ;;
esac

# 2. Lock the result
cat fp.jsonl | lock --dataset-id "models" > models.lock.json
```

### What makes this agent-friendly

- **Exit codes** — `0`/`1`/`2` map to complete/partial/error branching
- **Structured JSONL only** — stdout is always machine-readable
- **Assertion-level detail** — agents can inspect exactly which assertion failed and why
- **`--describe`** — prints `operator.json` so an agent discovers the tool without reading docs
- **`--list`** — enumerate available fingerprints programmatically
- **`--schema`** — prints the record JSON schema for programmatic validation
- **content_hash** — enables change detection without full file comparison

---

<details>
<summary><strong>Witness Subcommands</strong></summary>

`fingerprint` records every run to an ambient witness ledger. You can query this ledger:

```bash
# Query by date range or outcome
fingerprint witness query --tool fingerprint --since 2026-01-01 --outcome ALL_MATCHED --json

# Get the most recent run
fingerprint witness last --json

# Count runs matching a filter
fingerprint witness count --since 2026-02-01
```

### Subcommand Reference

```bash
fingerprint witness query [--tool <name>] [--since <iso8601>] [--until <iso8601>] \
  [--outcome <ALL_MATCHED|PARTIAL|REFUSAL>] [--input-hash <substring>] \
  [--limit <n>] [--json]

fingerprint witness last [--json]

fingerprint witness count [--tool <name>] [--since <iso8601>] [--until <iso8601>] \
  [--outcome <ALL_MATCHED|PARTIAL|REFUSAL>] [--input-hash <substring>] [--json]
```

### Exit Codes (witness subcommands)

| Code | Meaning |
|------|---------|
| `0` | One or more matching records returned |
| `1` | No matches (or empty ledger for `last`) |
| `2` | CLI parse error or witness internal error |

### Ledger Location

- Default: `~/.epistemic/witness.jsonl`
- Override: set `EPISTEMIC_WITNESS` environment variable
- Malformed ledger lines are skipped; valid lines continue to be processed.

</details>

---

## Spec and Development

The full specification is [`docs/PLAN.md`](./docs/PLAN.md). This README covers intended v0 behavior; the spec adds implementation details, edge-case definitions, and testing requirements.

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```
