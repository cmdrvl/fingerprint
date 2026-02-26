# fingerprint — Template Recognition

## One-line promise

**Determine whether an artifact matches a known template — and if so, provide content anchors and hash the matched content.**

If it doesn't match, say so with reasons. If the pipeline can't operate, refuse.
If an individual artifact can't be processed, emit `_skipped` with structured warnings.

Second promise: **Encode domain knowledge as versioned, testable, executable assertions — uniformly across spreadsheets, PDFs, text documents, and CSVs.**

Third promise: **Learn fingerprint definitions from example documents, deterministically.**

---

## Problem (clearly understood)

You have a corpus of files — Excel models, PDF reports, CSV tapes, vendor deliverables, 300-page appraisals. Before you can compare, lock, or reason about them, you need to know *what kind of thing each file is* and *where the important content lives*. Today this means:

- Filename conventions that drift and break
- Manual visual inspection ("is this an Argus model?" / "is this a CBRE appraisal?")
- Fragile regex patterns on filenames
- No structured evidence of template recognition
- No content hashing tied to template semantics
- Reading 300-page PDFs to find the 3 tables you actually need
- Writing extraction code without knowing if the document template has changed

`fingerprint` replaces that with **executable, versioned template assertions** that produce deterministic match/no-match verdicts with content anchors and content hashes. A fingerprint tells downstream extractors *what this document is* and *where to find the content that matters* — whether that's cell A3 in a spreadsheet or the rent roll table under the "Income Capitalization Approach" heading in a 300-page PDF.

---

## Non-goals (explicit)

`fingerprint` is NOT:

- An extractor or parser (it does not transform data into a target schema)
- A diff tool (that's `rvl` / `compare`)
- A column scoper (that's `profile`)
- An AI classifier (assertions are deterministic code, not probabilistic)
- A document converter (that's Docling, `llm_aided_ocr`, or whatever tool the user prefers)

It does not tell you *what the data means*.
It tells you *whether this file matches a known template*, *where the important content lives* (anchors), and *what the content hash is*.

**Clarification: fingerprint as anchor provider.** Fingerprints provide content anchors that enable downstream extraction — but fingerprint does not perform that extraction. A fingerprint says "the rent roll table is under the heading 'Income Capitalization Approach'." What the user does with that anchor — Docling table extraction, regex, Neo4j entity extraction, LLM analysis — is entirely their choice. Fingerprint reduces a 300-page problem to a 2-page problem.

**Clarification: fingerprint content extraction vs data extraction.** Fingerprints include an `extract:` section (DSL) and an `extracted` field (Rust trait) that pull content from matched documents. This is **content identity extraction** — extracting specific cells, ranges, or sections to compute a content hash for change detection. It is NOT **data transformation extraction** — parsing document content into structured values in a target schema.

---

## Relationship to the pipeline

`fingerprint` is the third tool in the stream pipeline. It reads hash-enriched JSONL, tests each artifact against fingerprint definitions, and emits enriched JSONL:

```bash
vacuum /data/models/ | hash | fingerprint --fp argus-model.v1 | lock --dataset-id "models-dec"
```

fingerprint can also test multiple fingerprints (evaluated in CLI order; first match wins):

```bash
vacuum /data/mixed/ | hash | fingerprint --fp argus-model.v1 --fp intex-cdr.v1 --fp xlsx.v0
```

fingerprint has a second mode — **compile** — that generates Rust crates from DSL fingerprint definitions:

```bash
fingerprint compile argus-model.fp.yaml --out fingerprint-argus-model-v1/
```

fingerprint has a third mode — **infer** — that learns fingerprint definitions from example documents:

```bash
# Corpus infer: learn from 10 CBRE appraisals
fingerprint infer ./cbre-examples/ --format markdown --id cbre-appraisal.v1

# Contrastive infer: learn what distinguishes CBRE from others
fingerprint infer ./cbre-examples/ --negative ./non-cbre-examples/ --format markdown

# Schema-driven infer: find where specific fields live
fingerprint infer-schema --doc appraisal.md --fields fields.yaml --id cbre-appraisal.v1
```

---

## Three Modes

### Run mode (stream enrichment)

Tests artifacts against installed fingerprint definitions. This is the pipeline mode.

### Compile mode (crate generation)

Compiles DSL fingerprint definitions (`.fp.yaml`) into Rust crates. This is an authoring/build-time tool.

### Infer mode (definition learning)

Learns fingerprint definitions from example documents. Two sub-modes:

- **Corpus infer** (`fingerprint infer`): Observe N example documents, find structural invariants, emit `.fp.yaml`. Optional `--negative` flag for contrastive learning (what distinguishes this document type from others). Uses frankensearch for hybrid search (BM25 + semantic) and content deduplication to find patterns.
- **Schema-driven infer** (`fingerprint infer-schema`): Given one document and a set of field names + example values, find where each field lives and generate anchor-based assertions. Uses frankensearch hybrid search to locate fields without reading the full document.

Both infer sub-modes are reproducible within a pinned toolchain: same inputs + same frankensearch version + same embedding model → same `.fp.yaml` output. No LLM calls. All local. BM25 lexical search is fully deterministic; semantic search is deterministic given pinned model weights. The generated `.fp.yaml` records the toolchain versions used for reproducibility.

All three modes share the `fingerprint` binary. Run mode is the default; compile and infer are subcommands.

---

## CLI (v0.1 target)

### Run mode

```bash
fingerprint [<INPUT>] [OPTIONS]
fingerprint witness <query|last|count> [OPTIONS]
```

#### Arguments

- `[INPUT]`: JSONL manifest file (default: stdin). Must contain hash-enriched records.

#### Flags

- `--fp <ID>`: Fingerprint ID to test (repeatable). At least one required unless `--list` is specified. Multiple `--fp` flags are evaluated in CLI order; first match wins per artifact.
- `--list`: List all available fingerprints (built-in + installed) and exit 0.
- `--jobs <N>`: Number of parallel workers (default: CPU count). `--jobs 1` for sequential.
- `--no-witness`: Suppress witness ledger recording.
- `--describe`: Print `operator.json` to stdout and exit 0. Checked before input is validated.
- `--schema`: Print JSON Schema for the JSONL record to stdout and exit 0. Like `--describe`, checked before input is validated.
- `--progress`: Emit structured progress JSONL to stderr.
- `--diagnose`: On assertion failure, include `context` in assertion results showing what the document DID contain (found headings, found tables, nearest match). Implies **no short-circuit** — all assertions are evaluated regardless of earlier failures, so the user gets the full picture in one run. Useful for debugging fingerprint definitions against real documents.
- `--version`: Print `fingerprint <semver>` to stdout and exit 0.

#### Exit codes

- `0`: ALL_MATCHED — every record matched a fingerprint.
- `1`: PARTIAL — some records didn't match any fingerprint (or were skipped).
- `2`: REFUSAL — pipeline-level inability to operate / CLI error.

#### Streams

- **stdout (exit 0):** JSONL records, every record has `matched: true`.
- **stdout (exit 1):** JSONL records, mix of matched/no-match/`_skipped`.
- **stdout (exit 2):** Single refusal envelope JSON object (not JSONL).
- **stderr:** Progress JSONL (if `--progress`); warnings for skipped files.

### Compile mode

```bash
fingerprint compile <YAML> [OPTIONS]
```

#### Arguments

- `<YAML>`: DSL fingerprint file (`.fp.yaml`).

#### Flags

- `--out <DIR>`: Output directory for the generated Rust crate.
- `--check`: Validate YAML without generating code.

#### Exit codes

- `0`: Compile/check completed with no validation warnings.
- `1`: Validation warnings (non-refusal). In compile mode a crate may still be generated; in `--check` mode no crate output is written.
- `2`: Refusal (malformed YAML, unsupported assertion type).

### Infer mode

```bash
fingerprint infer <DIR> [OPTIONS]
fingerprint infer-schema --doc <FILE> --fields <YAML> [OPTIONS]
```

#### Corpus infer (`fingerprint infer`)

##### Arguments

- `<DIR>`: Directory of example documents (all same format/type).

##### Flags

- `--format <FMT>`: Expected format (`xlsx`, `csv`, `pdf`, `markdown`, `text`). If omitted, inferred from file extensions.
- `--negative <DIR>`: Directory of negative examples for contrastive inference. Assertions are generated for patterns present in ALL positive examples and ABSENT from ALL negative examples.
- `--id <ID>`: Fingerprint ID for the generated definition (default: derived from directory name).
- `--out <FILE>`: Output `.fp.yaml` path (default: stdout).
- `--min-support <N>`: Minimum number of positive documents a pattern must appear in to become an assertion (default: all). Useful for noisy corpora.

##### Exit codes

- `0`: Infer completed, `.fp.yaml` emitted.
- `1`: Infer completed with warnings (e.g., few assertions found, low-confidence patterns).
- `2`: Refusal (empty directory, no common patterns, format mismatch).

#### Schema-driven infer (`fingerprint infer-schema`)

##### Flags

- `--doc <FILE>`: Single example document (any supported format, including pre-extracted markdown/text).
- `--fields <YAML>`: YAML file mapping field names to example values found in the document.
- `--id <ID>`: Fingerprint ID for the generated definition.
- `--out <FILE>`: Output `.fp.yaml` path (default: stdout).

##### Fields YAML format

```yaml
- name: as_of_date
  value: "June 15, 2024"
- name: cap_rate
  value: "6.25%"
- name: net_sf
  value: "125,000 SF"
- name: property_address
  value: "123 Main Street, New York, NY 10001"
```

For each field, the tool locates the value in the document, identifies the nearest stable anchor (heading, label, or unique surrounding text), and generates an assertion + extraction rule.

##### Exit codes

- `0`: Schema infer completed, `.fp.yaml` emitted.
- `1`: Some fields not located (partial definition emitted with warnings).
- `2`: Refusal (document unreadable, no fields located).

---

## Fingerprint Architecture

### Two authoring modes

| Mode | When to use | Who authors |
|------|-------------|-------------|
| **DSL** (80-90%) | Standard assertions, range checks, simple invariants | Analysts, domain SMEs |
| **Rust** (10-20%) | Complex logic, weird templates, domain-specific math | Engineers |

Both compile to the same runtime artifact — a Rust crate implementing the `Fingerprint` trait. The system treats them uniformly.

### The Fingerprint trait

```rust
pub trait Fingerprint: Send + Sync {
    fn id(&self) -> &str;           // "argus-model.v1"
    fn format(&self) -> &str;       // "xlsx", "pdf", "markdown", "csv", "text"
    fn parent(&self) -> Option<&str> { None }  // "cbre-appraisal.v1" for children
    fn fingerprint(&self, doc: &Document) -> FingerprintResult;
}

pub struct FingerprintResult {
    pub matched: bool,
    pub reason: Option<String>,
    pub assertions: Vec<AssertionResult>,
    pub extracted: Option<HashMap<String, Value>>,  // null when matched=false
    pub content_hash: Option<String>,               // null when matched=false
}

pub struct AssertionResult {
    pub name: String,
    pub passed: bool,
    pub detail: Option<String>,
    pub context: Option<Value>,  // Present when --diagnose and passed=false
}
```

### Fingerprint packs (installable crates)

| Layer | Provider | Installation |
|-------|----------|-------------|
| **Core** | Bundled with `fingerprint` CLI | Ships with the binary |
| **Standard packs** | Open source, separate crates | `cargo install fingerprint-loan-tapes` |
| **Commercial packs** | CMD+RVL | `cargo install fingerprint-cmbs --registry cmdrvl` |

### Fingerprint resolution order

1. Built-in fingerprints bundled with the `fingerprint` CLI
2. Installed fingerprint crates (discovered via cargo install paths)
3. `FINGERPRINT_PATH` env var (colon-separated directories of .so/.dylib plugins, advanced; deferred in v0.1)

Resolution must be deterministic:

- `fingerprint_id` MUST be globally unique after registry load.
- If duplicate IDs are discovered across sources, startup fails with `E_DUPLICATE_FP_ID` (no tie-break fallback).

Trust boundary:

- Built-in fingerprints are trusted by default.
- External crates/plugins require explicit allowlisting in config (`~/.epistemic/config.toml`) before use.
- Unallowlisted external fingerprints fail with `E_UNTRUSTED_FP`.

```bash
# List available fingerprints
fingerprint --list
# csv.v0 (core), xlsx.v0 (core), argus-model.v1 (fingerprint-argus 0.3.2), ...
```

---

## DSL Fingerprint Definitions (`.fp.yaml`)

### Example: Argus model fingerprint

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

### DSL assertion types

The table below is the full assertion roadmap. The v0.1 ship subset is defined in [Scope: v0.1](#scope-v01-ship-this). Assertions are organized by domain: universal (all formats), spreadsheet (xlsx/csv), and content (markdown/text/pdf).

#### Assertion naming

Every assertion in the DSL supports an optional `name` field for human-readable identification in results:

```yaml
assertions:
  - name: has_income_cap_heading
    heading_regex: { pattern: "(?i)income capitali[sz]ation" }
  - name: has_rent_roll_table
    table_exists: { heading: "(?i)rent roll", index: 0 }
  - heading_regex: { pattern: "(?i)property description" }  # auto-named
```

When `name` is omitted, it is auto-generated from the assertion type + distinguishing field:
- `heading_regex` → `heading_regex__income_capitali` (first 20 chars of pattern)
- `table_exists` → `table_exists__rent_roll__0` (heading excerpt + index)
- `cell_eq` → `cell_eq__Assumptions__A3` (sheet + cell)
- `text_near` → `text_near__capitali_ation_rate` (anchor excerpt)

Named assertions make `--diagnose` output and failure aggregation immediately actionable. With 15 assertions across parent + children, auto-generated names are unreadable.

#### Universal assertions

| Assertion | Purpose | Example |
|-----------|---------|---------|
| `filename_regex` | File basename matches regex | `filename_regex: { pattern: "(?i)(?:_FINF\|financials?\|Remit Financial)" }` |

#### Spreadsheet assertions (xlsx, csv)

| Assertion | Purpose | Example |
|-----------|---------|---------|
| `sheet_exists` | Worksheet with name exists | `sheet_exists: "Assumptions"` |
| `sheet_name_regex` | Any worksheet name matches regex | `sheet_name_regex: { pattern: "(?i)(FINF\|financial\|Remit\\s*Fin)" }` |
| `cell_eq` | Cell contains exact value | `cell_eq: { sheet: "...", cell: "A3", value: "..." }` |
| `cell_regex` | Cell matches regex | `cell_regex: { sheet: "...", cell: "B1", pattern: "^FY20[0-9]{2}$" }` |
| `range_non_null` | All cells in range are non-empty | `range_non_null: { sheet: "...", range: "A3:D10" }` |
| `range_populated` | ≥X% of cells non-empty | `range_populated: { sheet: "...", range: "...", min_pct: 0.8 }` |
| `sheet_min_rows` | Sheet has ≥N data rows | `sheet_min_rows: { sheet: "...", min_rows: 10 }` |
| `sum_eq` | Sum of range equals value/cell | `sum_eq: { range: "D3:D10", equals_cell: "D11", tolerance: 0.01 }` |
| `within_tolerance` | Value in range | `within_tolerance: { cell: "E5", min: 0, max: 1 }` |

#### Content assertions (markdown, text, pdf)

Content assertions operate on structured text — typically the output of a document conversion tool (Docling, `llm_aided_ocr`, `pdftotext`, etc.) provided via the `text_path` field in the JSONL record. Fingerprint does not perform text extraction; it consumes pre-extracted text.

| Assertion | Purpose | Example |
|-----------|---------|---------|
| `heading_exists` | Heading with text exists at any level | `heading_exists: "Income Capitalization Approach"` |
| `heading_regex` | Any heading matches regex | `heading_regex: { pattern: "(?i)income capitali[sz]ation" }` |
| `heading_level` | Heading exists at specific level | `heading_level: { level: 2, pattern: "(?i)rent roll" }` |
| `text_contains` | Exact text found anywhere in document | `text_contains: "CBRE Valuation & Advisory Services"` |
| `text_regex` | Regex matches anywhere in document | `text_regex: { pattern: "(?i)as of \\w+ \\d{1,2},? \\d{4}" }` |
| `text_near` | Regex matches near an anchor | `text_near: { anchor: "(?i)capitalization rate", pattern: "\\d+\\.\\d+%", within_chars: 200 }` |
| `section_non_empty` | Section under heading has content | `section_non_empty: { heading: "(?i)property description" }` |
| `section_min_lines` | Section has minimum line count | `section_min_lines: { heading: "(?i)rent roll", min_lines: 10 }` |
| `table_exists` | Markdown table exists under heading | `table_exists: { heading: "(?i)rent roll", index: 0 }` |
| `table_columns` | Table has columns matching patterns | `table_columns: { heading: "(?i)rent roll", index: 0, patterns: ["(?i)tenant", "(?i)suite\|unit", "(?i)sf\|sq.*ft", "(?i)rent"] }` |
| `table_shape` | Table has expected column count and types | `table_shape: { heading: "(?i)rent roll", index: 0, min_columns: 4, column_types: [string, number, string, number] }` |
| `table_min_rows` | Table has minimum data rows | `table_min_rows: { heading: "(?i)rent roll", index: 0, min_rows: 5 }` |
| `page_count` | PDF has N pages (structural, no text needed) | `page_count: { min: 100, max: 500 }` |
| `metadata_regex` | PDF metadata field matches regex | `metadata_regex: { key: "Creator", pattern: "(?i)cbre" }` |

### Diagnostic context (`--diagnose`)

When `--diagnose` is set and an assertion fails, the assertion result includes a `context` field showing what the document actually contains. This turns "your regex didn't match" into "here's what you should have written."

| Assertion type | Context includes |
|---------------|-----------------|
| `heading_exists` / `heading_regex` | `headings_found`: all headings in the document; `nearest_match`: closest heading by edit distance |
| `text_contains` / `text_regex` | `partial_matches`: up to 5 near-misses (if any substrings are close) |
| `text_near` | `anchor_found`: whether the anchor was found; `matches_outside_range`: matches that exist but were beyond `within_chars` |
| `table_exists` / `table_columns` / `table_shape` | `tables_found`: tables under the heading (with columns and row counts); `heading_found`: whether the heading itself was found |
| `section_non_empty` / `section_min_lines` | `section_lines`: actual line count; `heading_found`: whether the heading was found |

Example diagnostic output:
```json
{
  "name": "heading_regex_rent_roll",
  "passed": false,
  "detail": "No heading matched '(?i)rent roll'",
  "context": {
    "headings_found": ["Property Description", "Income Capitalization Approach", "Sales Comparison Approach", "RENT ROLL SUMMARY"],
    "nearest_match": "RENT ROLL SUMMARY"
  }
}
```

**Why separate spreadsheet and content assertions:** A spreadsheet user thinks in sheets and cells. A PDF/text user thinks in headings and sections. The vocabulary matches how practitioners already think about their documents. The assertion engine dispatches to the right implementation based on the fingerprint's `format` field.

**Why `text_near`:** In dense text, values like percentages and dollar amounts appear everywhere. `text_near` requires a match within N characters of an anchor phrase, dramatically reducing false positives. "6.25%" means nothing; "6.25% within 200 chars of 'Capitalization Rate'" is a precise content marker. Character distance is more stable than line distance across extraction tools (which wrap text differently and may insert blank lines or page break markers).

**`text_near` search semantics:**
- **Bidirectional:** `within_chars` measures distance in both directions from the anchor. `"Capitalization Rate: 6.25%"` (value after) and `"6.25% (Overall Capitalization Rate)"` (value before) both match.
- **Multi-match:** The anchor may appear multiple times in the document. The assertion passes if ANY occurrence of the anchor has the pattern within range. The first match found is reported in `extracted`.
- **Distance measurement:** Character distance is measured from the end of the anchor match to the start of the pattern match (or vice versa), ignoring whitespace-only gaps of < 10 chars.

**Why `table_columns` uses regex patterns:** Column headers are the most variable part of an extracted table. Docling might produce "Tenant Name" instead of "Tenant", "Sq. Ft." instead of "SF". Using regex patterns per column (`["(?i)tenant", "(?i)sf|sq.*ft"]`) absorbs this variation. When headers are truly stable, exact-match patterns work fine.

**Why `table_shape`:** When column headers aren't reliable at all (different extraction tools, language variations), the structural shape of a table — column count and inferred column types — is more stable. A rent roll always has 4+ columns in the pattern string/number/string/number regardless of header text.

**`table_shape` column type inference rules:**

Before type inference, markdown formatting is stripped from cell values (`**bold**` → `bold`, `*italic*` → `italic`).

| Type | Pattern | Examples |
|------|---------|---------|
| `number` | `^\$?-?[\d,]+\.?\d*$` | `12500`, `12,500`, `$312,500`, `-1.5` |
| `currency` | `^\$[\d,]+\.?\d*$` | `$312,500`, `$25.00` |
| `percentage` | `^-?[\d.]+%$` | `6.25%`, `95.5%`, `-2.1%` |
| `date` | Common date patterns (ISO, US, written) | `2024-06-15`, `06/15/2024`, `June 15, 2024` |
| `empty` | Blank or whitespace-only | ` `, `` |
| `string` | Anything else | `Acme Corp`, `Suite 100`, `N/A` |

Type is determined per column by majority vote (> 50% of non-empty cells). Blank cells are excluded from voting. If no type reaches majority, the column is classified as `string`. `currency` is a subtype of `number` — a `number` column type matches cells that are `currency` and vice versa.

**Why `index`:** A heading may have multiple tables beneath it. The `index` parameter (0-based, default 0) specifies which table to target. When omitted, the first table under the heading is used.

### Example: CBRE appraisal fingerprint (content assertions)

```yaml
fingerprint_id: cbre-appraisal.v1
format: pdf
valid_from: "2021-01-01"

assertions:
  # Structural (from PDF directly)
  - name: page_range
    page_count: { min: 50, max: 500 }
  - name: cbre_creator
    metadata_regex: { key: "Creator", pattern: "(?i)cbre" }

  # Content (from text_path — requires pre-extracted markdown)
  - name: cbre_branding
    text_contains: "CBRE Valuation & Advisory Services"
  - name: has_income_cap
    heading_regex: { pattern: "(?i)income capitali[sz]ation approach" }
  - name: has_sales_comp
    heading_regex: { pattern: "(?i)sales comparison approach" }
  - name: has_property_desc
    heading_regex: { pattern: "(?i)property description" }
  - name: has_rent_roll_table
    table_exists: { heading: "(?i)rent roll", index: 0 }
  - name: rent_roll_shape
    table_shape:
      heading: "(?i)rent roll"
      index: 0
      min_columns: 4
      column_types: [string, string, number, number]
  - name: cap_rate_present
    text_near:
      anchor: "(?i)capitali[sz]ation rate"
      pattern: "\\d+\\.\\d+%"
      within_chars: 200
  - name: property_desc_has_content
    section_min_lines:
      heading: "(?i)property description"
      min_lines: 10

extract:
  - name: rent_roll_table
    type: table
    anchor_heading: "(?i)rent roll"
    index: 0
  - name: income_cap_section
    type: section
    anchor_heading: "(?i)income capitali[sz]ation"
  - name: as_of_date
    type: text_match
    anchor: "(?i)as of"
    pattern: "\\w+ \\d{1,2},? \\d{4}"
    within_chars: 100

content_hash:
  algorithm: blake3
  over: [rent_roll_table, income_cap_section]
```

This fingerprint targets PDF appraisals. Structural assertions (`page_count`, `metadata_regex`) read the PDF directly. Content assertions (`heading_regex`, `text_near`, `table_shape`, etc.) read from the `text_path` field — pre-extracted markdown provided by an upstream tool like Docling. The extract section provides anchors for downstream tools.

### Content extract types

Content extract rules specify how to locate and delimit content regions for hashing and downstream anchor reporting. Each rule has a `type` and type-specific parameters:

| Type | Parameters | What it captures | Example output |
|------|-----------|-----------------|----------------|
| `table` | `anchor_heading`, `index` (default 0) | Full markdown table (header + all rows) | `{ "start_line": 45, "end_line": 62, "columns": [...], "row_count": 15 }` |
| `section` | `anchor_heading` | All content from heading to next heading at equal/lesser depth | `{ "start_line": 30, "end_line": 90, "heading": "Income Capitalization Approach" }` |
| `text_match` | `anchor`, `pattern`, `within_chars` | The matched text and its location | `{ "line": 12, "char_offset": 45, "matched": "June 15, 2024" }` |
| `range` | `sheet`, `range` | Spreadsheet cell range (xlsx/csv only) | `{ "range": "A3:D10", "row_count": 8 }` |

**Invariants:**
- Extract rules run only when all assertions pass (`matched: true`).
- The `extracted` field in the output record reports the anchor location, not the content itself (zero-retention). Downstream tools use the anchor to perform their own extraction.
- Content hashes are computed over the raw bytes of the matched content region (the actual text, not the anchor metadata).
- If an extract rule cannot locate its target (e.g., table not found at the expected index), the rule is omitted from `extracted` with a warning, but the match still holds (extract failure is non-fatal).

### Compiling DSL to Rust

```bash
fingerprint compile argus-model.fp.yaml --out fingerprint-argus-model-v1/
# Generates:
#   fingerprint-argus-model-v1/
#   ├── Cargo.toml
#   ├── src/lib.rs           # Generated Rust implementing Fingerprint trait
#   └── fixtures/            # Golden test stubs
```

The compiler is deterministic: **same YAML → same Rust source**. Binary reproducibility depends on the Rust toolchain version; the compiler guarantees source-level determinism.

The generated crate embeds:
- `compiler_version`: semver of `fingerprint compile`
- `source_hash`: blake3 of the canonicalized YAML
- `source`: `"dsl"` (vs `"rust"` for hand-written)

### Compile refusal codes

| Code | Trigger | Next step |
|------|---------|-----------|
| `E_INVALID_YAML` | YAML parse error or schema violation | Fix the `.fp.yaml` file |
| `E_UNKNOWN_ASSERTION` | Assertion type not recognized | Check supported assertion types |
| `E_MISSING_FIELD` | Required field missing from DSL | Add the required field |

---

## Run Mode: Output Record Schema (`fingerprint.v0`)

Each record passes through all upstream fields and adds fingerprint results:

```json
{
  "version": "fingerprint.v0",
  "path": "/data/models/deal-123.xlsx",
  "relative_path": "deal-123.xlsx",
  "root": "/data/models",
  "size": 2481920,
  "mtime": "2025-12-31T12:00:00.000Z",
  "extension": ".xlsx",
  "mime_guess": "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
  "bytes_hash": "sha256:e3b0c44...",
  "hash_algorithm": "sha256",
  "tool_versions": { "vacuum": "0.1.0", "hash": "0.1.0", "fingerprint": "0.1.0" },
  "fingerprint": {
    "fingerprint_id": "argus-model.v1",
    "fingerprint_crate": "fingerprint-argus",
    "fingerprint_version": "0.3.2",
    "fingerprint_source": "dsl",
    "matched": true,
    "reason": null,
    "assertions": [
      { "name": "assumptions_sheet_exists", "passed": true, "detail": null },
      { "name": "title_cell_correct", "passed": true, "detail": null },
      { "name": "range_non_null", "passed": true, "detail": null }
    ],
    "extracted": {
      "market_leasing_assumptions": { "range": "A3:D10", "row_count": 8 }
    },
    "content_hash": "blake3:9f2a..."
  }
}
```

### Fingerprint result fields

| Field | Type | Nullable | Notes |
|-------|------|----------|-------|
| `fingerprint_id` | string | no | ID of the fingerprint tested (or matched) |
| `fingerprint_crate` | string | no | Crate name that provides this fingerprint |
| `fingerprint_version` | string | no | Semver of the fingerprint crate |
| `fingerprint_source` | string | no | `"dsl"` (compiled from YAML) or `"rust"` (hand-written) |
| `matched` | bool | no | Whether all assertions passed |
| `reason` | string | yes | Why it didn't match (`null` if matched) |
| `assertions` | array | no | Individual assertion results |
| `extracted` | object | yes | Content extracted by the fingerprint (for inspection); `null` if no match |
| `content_hash` | string | yes | Hash of extracted content; `null` if no match |

### No-match records

When an artifact doesn't match any provided fingerprint:

```json
{
  "version": "fingerprint.v0",
  "path": "/data/models/unknown.xlsx",
  "fingerprint": {
    "fingerprint_id": "argus-model.v1",
    "fingerprint_crate": "fingerprint-argus",
    "fingerprint_version": "0.3.2",
    "fingerprint_source": "dsl",
    "matched": false,
    "reason": "Assertion failed: sheet 'Assumptions' not found",
    "assertions": [
      { "name": "assumptions_sheet_exists", "passed": false, "detail": "Sheet not found" },
      { "name": "title_cell_correct", "passed": false, "detail": "Skipped (prior assertion failed)" }
    ],
    "extracted": null,
    "content_hash": null
  }
}
```

When multiple `--fp` are provided: each fingerprint is tried in the exact CLI order. The first match wins. If none match, the record shows the LAST fingerprint tried (with `matched: false`).
Recommended ordering is most specific → most general so confidence-tier fallback is explicit and deterministic.

### Key invariant

Fingerprint results are only comparable if `fingerprint_id` + `fingerprint_version` match exactly. If the fingerprint logic changes, the version changes.

### Passthrough of upstream `_skipped` records

If an input record has `_skipped: true`, fingerprint passes it through without attempting fingerprinting. This path bypasses the `bytes_hash` requirement. Fingerprint updates `version`, `tool_versions`, and sets `fingerprint: null` (so the key is always present for uniform downstream schema).

### New `_skipped` records

When fingerprint encounters an IO/parse failure for a record (e.g., corrupted XLSX), it marks `_skipped: true`, sets `fingerprint: null`, and appends a warning. All upstream fields are preserved:

```json
{
  "version": "fingerprint.v0",
  "path": "/data/models/corrupt.xlsx",
  "bytes_hash": "sha256:abc1...",
  "hash_algorithm": "sha256",
  "tool_versions": { "vacuum": "0.1.0", "hash": "0.1.0", "fingerprint": "0.1.0" },
  "fingerprint": null,
  "_skipped": true,
  "_warnings": [
    { "tool": "fingerprint", "code": "E_PARSE", "message": "Cannot parse XLSX", "detail": { "path": "corrupt.xlsx", "error": "Invalid ZIP" } }
  ]
}
```

### Ordering

Output order matches input order. When processing in parallel (`--jobs > 1`), records are buffered and emitted in sequence.
Implementation MUST bound in-flight work and reorder buffers (no unbounded growth). When the reorder buffer hits its configured limit, input reading pauses until earlier sequence slots are emitted.

### Upstream version compatibility

- fingerprint accepts records from the immediate upstream tool (`hash`) for the current schema version and explicitly supported prior versions.
- fingerprint refuses unknown future upstream versions with `E_BAD_INPUT`.

---

## Refusal Codes (run mode)

Per-file IO/parse failures are NOT refusals. They are recorded as `_skipped: true` records with `_warnings` and cause exit code `1` (partial). Refusals are reserved for pipeline-level inability to operate.

| Code | Trigger | Next step |
|------|---------|-----------|
| `E_BAD_INPUT` | Invalid JSONL, missing `bytes_hash` on non-skipped records, or unrecognized upstream `version` | Run hash first |
| `E_UNKNOWN_FP` | Fingerprint ID not found in any installed crate | Check installed fingerprint crates (`fingerprint --list`) |
| `E_DUPLICATE_FP_ID` | Same `fingerprint_id` discovered from multiple sources during registry load | Remove duplicate packs or pin to one source |
| `E_UNTRUSTED_FP` | External fingerprint crate/plugin is not allowlisted | Add to allowlist or use built-in fingerprints |
| `E_ORPHAN_CHILD` | Child fingerprint references a parent not loaded in `--fp` | Add the parent fingerprint to `--fp` |

Per-code `detail` schemas:

```
E_BAD_INPUT:
  { "line": 42, "error": "..." }        // parse failure
  or
  { "line": 1, "missing_field": "bytes_hash" }  // missing required field
  or
  { "line": 1, "version": "unknown.v3" }        // unrecognized version

E_UNKNOWN_FP:
  { "fingerprint_id": "argus-model.v1", "available": ["csv.v0", "xlsx.v0"] }

E_DUPLICATE_FP_ID:
  { "fingerprint_id": "argus-model.v1", "providers": ["builtin:argus", "crate:fingerprint-argus"] }

E_UNTRUSTED_FP:
  { "fingerprint_id": "argus-model.v1", "provider": "crate:fingerprint-argus", "policy": "allowlist_required" }

E_ORPHAN_CHILD:
  { "child_id": "cbre-appraisal.v1/rent-roll.v1", "parent_id": "cbre-appraisal.v1", "loaded": ["csv.v0", "xlsx.v0"] }
```

Refusal envelope (emitted to stdout):

```json
{
  "version": "fingerprint.v0",
  "outcome": "REFUSAL",
  "refusal": {
    "code": "E_UNKNOWN_FP",
    "message": "Fingerprint ID not found",
    "detail": { "fingerprint_id": "argus-model.v1", "available": ["csv.v0", "xlsx.v0"] },
    "next_command": "cargo install fingerprint-argus"
  }
}
```

---

## Content Document Model

### How pre-extracted text enters the pipeline

Fingerprint does not extract text from PDFs. That's the job of an upstream tool — Docling, `llm_aided_ocr`, `pdftotext`, or any tool the user chooses. The extracted text reaches fingerprint via the JSONL record's `text_path` field:

```json
{
  "version": "hash.v0",
  "path": "/data/appraisals/cbre-deal-123.pdf",
  "text_path": "/data/appraisals/cbre-deal-123.md",
  "bytes_hash": "sha256:e3b0c44...",
  "hash_algorithm": "sha256",
  "tool_versions": { "vacuum": "0.1.0", "hash": "0.1.0" }
}
```

### Assertion dispatch by format

| Format | Structural assertions | Content assertions | Source |
|--------|----------------------|-------------------|--------|
| `pdf` | `page_count`, `metadata_regex` read from `path` (the PDF) | `heading_*`, `text_*`, `section_*`, `table_*` read from `text_path` | PDF for structure, pre-extracted markdown for content |
| `markdown` | N/A | All content assertions read from `path` directly | Standalone markdown file |
| `text` | N/A | `text_contains`, `text_regex`, `text_near` read from `path` directly | Standalone text file |
| `xlsx` / `csv` | All spreadsheet assertions read from `path` | N/A | Spreadsheet file |

When `format: pdf` and `text_path` is present: structural assertions read from `path`, content assertions read from `text_path`. Both in the same fingerprint, same assertion list.

When `format: pdf` and `text_path` is absent: structural assertions (`page_count`, `metadata_regex`, `filename_regex`) are evaluated normally. Content assertions **fail** with detail `"No text_path provided (E_NO_TEXT)"`. This means a fingerprint with content assertions will not match without `text_path` — the behavior is honest and predictable.

If you want structural-only matching as a fallback (e.g., "probably a CBRE appraisal based on page count + PDF metadata"), write a structural-only parent fingerprint with no content assertions. Chained fingerprints make this natural:

```yaml
# Structural-only parent (matches without text_path)
fingerprint_id: cbre-appraisal.v1
format: pdf
assertions:
  - page_count: { min: 50, max: 500 }
  - metadata_regex: { key: "Creator", pattern: "(?i)cbre" }

# Content children (require text_path)
fingerprint_id: cbre-appraisal.v1/rent-roll.v1
parent: cbre-appraisal.v1
format: pdf
assertions:
  - table_exists: { heading: "(?i)rent roll", index: 0 }
```

This way, the parent matches on structure alone. Children are only evaluated when text_path is present. When text_path is absent, the parent still matches but children fail with `E_NO_TEXT`, producing exit 1 (PARTIAL) — signaling that extraction anchors are missing.

When `format: markdown` or `text`: `text_path` is ignored. The `path` field IS the text document.

### Supported content formats

| Format | Document type | Source | Structural features |
|--------|--------------|--------|-------------------|
| `markdown` | `MarkdownDocument` | Docling, `llm_aided_ocr`, any md producer | Heading hierarchy, sections, tables, emphasis |
| `text` | `TextDocument` | `pdftotext`, `cat`, any plain text | Lines only (no heading/table parsing) |

The `markdown` format is the recommended path for content fingerprinting because it preserves document structure. The `text` format is a fallback for when structured extraction isn't available — content assertions still work but heading/table assertions do not.

### Markdown normalization

`document/markdown.rs` applies a normalization pass before parsing to absorb common extraction tool variations:

- **Setext headings → ATX:** `Heading\n======` normalized to `# Heading`
- **Bold-as-heading detection:** Lines that are solely `**Bold Text**` with blank lines above and below are treated as headings (common Docling artifact for headings it can't classify)
- **Whitespace normalization:** Consecutive blank lines collapsed to one; trailing whitespace stripped
- **Table pipe alignment:** Inconsistent pipe spacing in markdown tables normalized for reliable column parsing

Normalization is applied before heading/section/table parsing and before content hash computation, so the same logical content produces the same structural parse regardless of minor extraction tool formatting differences.

### Section boundary robustness

Section boundaries are computed by heading level hierarchy: a section extends from its heading until the next heading at **equal or lesser depth** (not just the same level). This means:

- `## Rent Roll` captures content until the next `##` or `#` heading
- Misclassified heading levels (H2 rendered as H3) don't cause silent data loss
- Content before the first heading is captured as a preamble section (`heading: None`)

### Content hash and extraction tool versioning

Content hashes are computed over extracted text, not over the original PDF bytes. This means content hashes are only comparable within the same extraction tool and version — upgrading Docling or switching to `llm_aided_ocr` will produce different markdown and therefore different content hashes, even for an unchanged PDF.

This is by design: the upstream `bytes_hash` (from the `hash` tool) already covers original file identity. The content hash captures *semantic content identity* — whether the meaningful extracted content has changed. When the extraction tool changes, the content hash correctly reflects that the extracted representation has changed.

The JSONL record's `tool_versions` field should include the extraction tool version (e.g., `"docling": "2.3.1"`) so downstream consumers can determine content hash comparability.

### Zero-retention compatibility

The content document model is designed for zero-retention environments:

- Fingerprint reads the pre-extracted text file, evaluates assertions, and discards it. No document content is stored, cached, or transmitted.
- The `text_path` can point to a temporary file that the user deletes after the pipeline completes.
- Content hashes computed from extracted text enable change detection without retaining the content.
- The `infer` and `infer-schema` subcommands observe structural facts (headings, table columns, text patterns) without storing document content. Only the `.fp.yaml` definition is persisted.

---

## Chained Fingerprints (Content-Level)

### The problem

A single document often has multiple extraction targets that evolve independently. A CBRE appraisal might initially need just the rent roll, then later someone needs the income capitalization section, then the sales comparison section, then the property description. With monolithic fingerprints, every new extraction target means modifying the same `.fp.yaml` — version churn, regression risk, and team coordination overhead.

### The solution: parent-child fingerprint chaining

A fingerprint can declare a `parent` field referencing another fingerprint. When present:

1. The parent fingerprint must match first (document-level identification).
2. Child fingerprints are evaluated only on documents where the parent matched.
3. Multiple children can chain to the same parent — all matching children are included in the output (not first-match-wins).
4. Children can be added, removed, or versioned independently of the parent.

```yaml
# Parent: identifies the document type
fingerprint_id: cbre-appraisal.v1
format: pdf
valid_from: "2021-01-01"

assertions:
  - page_count: { min: 50, max: 500 }
  - text_contains: "CBRE Valuation & Advisory Services"
  - heading_regex: { pattern: "(?i)income capitali[sz]ation approach" }
  - heading_regex: { pattern: "(?i)property description" }

extract:
  - name: document_type
    type: text_match
    anchor: "CBRE"
    pattern: "Valuation & Advisory Services"
    within_chars: 100
```

```yaml
# Child: targets the rent roll section
fingerprint_id: cbre-appraisal.v1/rent-roll.v1
parent: cbre-appraisal.v1
format: pdf

assertions:
  - table_exists: { heading: "(?i)rent roll", index: 0 }
  - table_shape:
      heading: "(?i)rent roll"
      index: 0
      min_columns: 4
      column_types: [string, string, number, number]

extract:
  - name: rent_roll_table
    type: table
    anchor_heading: "(?i)rent roll"
    index: 0

content_hash:
  algorithm: blake3
  over: [rent_roll_table]
```

```yaml
# Another child: targets income capitalization (added 3 months later)
fingerprint_id: cbre-appraisal.v1/income-cap.v1
parent: cbre-appraisal.v1
format: pdf

assertions:
  - heading_regex: { pattern: "(?i)income capitali[sz]ation approach" }
  - text_near:
      anchor: "(?i)capitali[sz]ation rate"
      pattern: "\\d+\\.\\d+%"
      within_chars: 200

extract:
  - name: income_cap_section
    type: section
    anchor_heading: "(?i)income capitali[sz]ation"

content_hash:
  algorithm: blake3
  over: [income_cap_section]
```

### CLI usage with chained fingerprints

```bash
# Parent + children evaluated together
vacuum /data/appraisals | hash | fingerprint \
  --fp cbre-appraisal.v1 \
  --fp cbre-appraisal.v1/rent-roll.v1 \
  --fp cbre-appraisal.v1/income-cap.v1
```

Evaluation order:
1. All fingerprints without `parent` are evaluated first (document-level), in CLI order, first match wins.
2. All fingerprints whose `parent` matches the winning document-level fingerprint are evaluated (content-level), independently — each produces its own match/no-match result.

**Startup validation:** If a child fingerprint references a `parent` that is not loaded (not in any `--fp` argument), fingerprint refuses at startup with `E_ORPHAN_CHILD`:

```
E_ORPHAN_CHILD: Child fingerprint 'cbre-appraisal.v1/rent-roll.v1' references parent
'cbre-appraisal.v1' which is not loaded. Add --fp cbre-appraisal.v1.
```

This prevents silent failures where children are specified but can never trigger.

### Output record with chained fingerprints

```json
{
  "version": "fingerprint.v0",
  "path": "/data/appraisals/cbre-deal-123.pdf",
  "text_path": "/data/appraisals/cbre-deal-123.md",
  "fingerprint": {
    "fingerprint_id": "cbre-appraisal.v1",
    "matched": true,
    "assertions": [...],
    "extracted": { "document_type": { "line": 1, "matched": "Valuation & Advisory Services" } },
    "content_hash": null,
    "children": [
      {
        "fingerprint_id": "cbre-appraisal.v1/rent-roll.v1",
        "matched": true,
        "assertions": [...],
        "extracted": { "rent_roll_table": { "start_line": 45, "end_line": 62, "columns": [...], "row_count": 15 } },
        "content_hash": "blake3:9f2a..."
      },
      {
        "fingerprint_id": "cbre-appraisal.v1/income-cap.v1",
        "matched": true,
        "assertions": [...],
        "extracted": { "income_cap_section": { "start_line": 30, "end_line": 90, "heading": "Income Capitalization Approach" } },
        "content_hash": "blake3:d4e1..."
      }
    ]
  }
}
```

### Exit code semantics with chained fingerprints

Child fingerprint failures affect the exit code: if the parent matches but any child does not, the record is treated as **PARTIAL** (exit 1). Rationale: a child no-match means a downstream extraction anchor is missing, and the user should know.

| Parent | Children | Exit code |
|--------|----------|-----------|
| Match | All match | `0` (ALL_MATCHED) |
| Match | Some fail | `1` (PARTIAL) |
| No match | Not evaluated | `1` (PARTIAL) |

When no children are provided in `--fp`, only the parent matters — chained semantics only activate when children are explicitly requested.

### Why chained fingerprints matter

- **Evolutionary extraction:** Start with 2 content fingerprints, add 10 more over 6 months. The parent never changes.
- **Team independence:** The rent roll team and the income cap team can version their fingerprints independently.
- **Granular content hashes:** Each child has its own content hash. When the rent roll changes but the income cap section doesn't, only one content hash changes.
- **Composable:** Different clients can use different subsets of children for the same parent document type.
- **Convention:** Child fingerprint IDs use `/` as a separator: `{parent_id}/{child_name}.{version}`. This is a naming convention, not enforced structurally — the `parent` field is what creates the relationship.

---

## PDF Content Pipeline (Docling Integration)

Fingerprint does not extract text from PDFs — but the most common content fingerprinting workflow pairs fingerprint with [Docling](https://github.com/DS4SD/docling) for text extraction. This section documents the full pipeline.

### Prerequisites

```bash
# Install Docling (Python, runs locally, 258M VLM model)
pip install docling

# Verify
docling --version
```

Docling runs fully local. No data leaves the machine. The 258M parameter VLM runs on CPU (no GPU required). Air-gap compatible.

### Pipeline: PDF → Docling → fingerprint

```bash
# Step 1: Extract text from PDFs to markdown (one-time, per corpus)
# Batch mode — Docling parallelizes internally
docling --input-dir /data/appraisals/ --to md --output /data/appraisals/
# Produces: /data/appraisals/cbre-deal-123.md alongside cbre-deal-123.pdf

# Step 2: Run the epistemic pipeline with text_path injection
vacuum /data/appraisals/ --ext pdf \
  | hash \
  | jq -c '. + {"text_path": (.path | sub("\\.pdf$"; ".md"))}' \
  | fingerprint --fp cbre-appraisal.v1

# Or with chained fingerprints:
vacuum /data/appraisals/ --ext pdf \
  | hash \
  | jq -c '. + {"text_path": (.path | sub("\\.pdf$"; ".md"))}' \
  | fingerprint \
      --fp cbre-appraisal.v1 \
      --fp cbre-appraisal.v1/rent-roll.v1 \
      --fp cbre-appraisal.v1/income-cap.v1
```

The `jq` step injects `text_path` by replacing `.pdf` → `.md` in the path. This assumes Docling output lives alongside the PDFs. Adjust the path pattern for your directory structure.

### Alternative extraction tools

| Tool | Quality | Tables | Local | Notes |
|------|---------|--------|-------|-------|
| **Docling** (recommended) | High (97.9% table accuracy) | Excellent | Yes | 258M VLM, structured markdown/JSON |
| `llm_aided_ocr` | High | Good | Depends on LLM | Tesseract + LLM correction, produces markdown |
| `pdftotext` | Low | None | Yes | No table extraction, no heading detection. Works as `format: text` fallback |

### Scanned PDF detection

When `format: pdf` and `text_path` is present but suspiciously short (< 100 chars for a document with `page_count` > 10), fingerprint emits warning `W_SPARSE_TEXT`:

```json
{ "tool": "fingerprint", "code": "W_SPARSE_TEXT", "message": "text_path has 47 chars but PDF has 287 pages — possible scanned PDF or extraction failure" }
```

This helps distinguish "this isn't a CBRE appraisal" from "your extraction tool didn't handle this PDF."

### Corpus-level failure analysis

After running fingerprint with `--diagnose`, use `jq` to aggregate failure reasons across the corpus:

```bash
# Count failures by assertion name
jq -r 'select(.fingerprint.matched == false)
  | .fingerprint.assertions[]
  | select(.passed == false)
  | .name' output.jsonl \
  | sort | uniq -c | sort -rn

# Example output:
#   15 has_rent_roll_table
#   10 rent_roll_shape
#    5 cbre_branding

# Show diagnostic context for a specific assertion across all failures
jq 'select(.fingerprint.matched == false)
  | { path: .path, context: (.fingerprint.assertions[]
      | select(.name == "has_rent_roll_table" and .passed == false)
      | .context) }' output.jsonl

# Show nearest heading matches for heading_regex failures
jq -r 'select(.fingerprint.matched == false)
  | .fingerprint.assertions[]
  | select(.passed == false and .context.nearest_match != null)
  | "\(.name): \(.context.nearest_match)"' output.jsonl \
  | sort | uniq -c | sort -rn

# Example output:
#   10 has_rent_roll_table: RENT ROLL SUMMARY
#    5 has_rent_roll_table: Rent Roll - Detail
```

This workflow — run with `--diagnose`, aggregate with `jq`, adjust `.fp.yaml`, re-run — is the calibration loop for tuning fingerprints against a real corpus.

---

## Source Derivation Metadata

Fingerprint templates carry an implicit relationship: some document types are **independent sources**, while others are **derived from** other document types. This matters for downstream multi-source confirmation scoring.

Example from CMBS:
- `trustee_report` — independent: aggregated from servicer data
- `argus_model` — independent: different methodology, different timing
- `quarterly_summary` — **derived from** monthly trustee reports (rollup)

This metadata is **not part of the fingerprint assertion logic**. Derivation relationships are declared alongside fingerprint packs as a separate versioned artifact (e.g., `derivation.cmbs.v1.yaml`) and consumed by downstream policy engines.

---

## Progress Reporting (`--progress`)

```jsonl
{"type": "progress", "tool": "fingerprint", "processed": 500, "total": 10000, "percent": 5.0, "elapsed_ms": 3200}
{"type": "warning", "tool": "fingerprint", "path": "/data/corrupt.xlsx", "message": "skipped: Invalid ZIP"}
```

---

## Witness Record

fingerprint appends a witness record for every **run mode** invocation (success or refusal). Compile mode does not produce witness records — it is a build tool, not a pipeline run. The record follows the standard `witness.v0` schema:

```json
{
  "id": "blake3:...",
  "tool": "fingerprint",
  "version": "0.1.0",
  "binary_hash": "blake3:...",
  "inputs": [
    { "path": "stdin", "hash": null, "bytes": null }
  ],
  "params": { "fingerprints": ["argus-model.v1"], "jobs": 4 },
  "outcome": "ALL_MATCHED",
  "exit_code": 0,
  "output_hash": "blake3:...",
  "prev": "blake3:...",
  "ts": "2026-02-24T10:00:00Z"
}
```

Possible outcomes: `ALL_MATCHED` (exit 0), `PARTIAL` (exit 1), `REFUSAL` (exit 2).

For fingerprint, `inputs` describes the JSONL source: `"stdin"` when piped, or the file path when a positional argument is given. `inputs[].hash` and `inputs[].bytes` are `null` for stdin; when a file argument is provided, they can be populated after reading. The `output_hash` is BLAKE3 of the full JSONL output (per spine witness protocol).

---

## Implementation Notes

### Execution flow (run mode)

```
 1. Parse CLI args (clap)                → exit 2 on bad args; --version handled by clap
 2. If witness subcommand: dispatch to witness query/last/count, exit
 3. If --describe: print operator.json, exit 0
 4. If --schema: print JSON Schema, exit 0
 5. If --list: enumerate available fingerprints, print, exit 0
 6. Validate at least one --fp provided  → exit 2 if none (CLI usage error)
 7. Resolve --fp IDs to fingerprint implementations
    → E_UNKNOWN_FP if any ID not found
    → E_DUPLICATE_FP_ID if duplicate IDs exist across providers
    → E_UNTRUSTED_FP if provider is not allowlisted
 8. Open input (file or stdin)
 9. For each JSONL line:
    a. Parse as JSON                     → E_BAD_INPUT if invalid
    b. Check version field               → E_BAD_INPUT if unrecognized
    c. If _skipped: true, pass through   → update version, tool_versions, set fingerprint: null
    d. Validate has bytes_hash           → E_BAD_INPUT if missing (non-skipped only)
    → On refusal (steps 7/9a/9b/9d): emit refusal envelope to stdout, append
      witness record with outcome "REFUSAL" (if not --no-witness), exit 2
    e. Open/parse the file once (using mime_guess/extension for format dispatch)
       → On IO/parse error: mark _skipped, set fingerprint: null, append _warning, continue
    f. Partition --fp into document-level (no parent) and content-level (has parent)
    g. Try each document-level --fp in order:
       i.   Check fingerprint's declared format vs Document type → skip if mismatch
       ii.  Run assertions in declaration order; short-circuit on first failure
            (remaining assertions are recorded as "Skipped" — some are also
            structurally impossible, e.g., cell check when sheet doesn't exist)
            Exception: when --diagnose is set, ALL assertions are evaluated
            regardless of earlier failures (no short-circuit)
       iii. If all pass: MATCH → extract content, compute content_hash, stop trying
       iv.  If any fail: NO_MATCH → try next --fp
    h. If document-level match found, evaluate all content-level --fps whose parent
       matches the winning document-level fingerprint ID (independently, not first-match-wins)
    i. Build fingerprint result (match or last no-match; include children array if chained)
    j. Update version, merge tool_versions
    k. Emit to stdout
10. Track: any skipped or unmatched? → exit 1 if yes, exit 0 if all matched
11. Append witness record (if not --no-witness)
12. Exit
```

### Core data structures

```rust
// === Fingerprint registry ===

pub struct FingerprintRegistry {
    fingerprints: Vec<Box<dyn Fingerprint>>,
}

impl FingerprintRegistry {
    /// Resolve a fingerprint ID to an implementation
    pub fn get(&self, id: &str) -> Option<&dyn Fingerprint>;

    /// List all available fingerprints
    pub fn list(&self) -> Vec<FingerprintInfo>;
}

pub struct FingerprintInfo {
    pub id: String,
    pub crate_name: String,
    pub version: String,
    pub source: String,  // "dsl" or "rust"
    pub format: String,
    pub parent: Option<String>,  // Parent fingerprint ID for chained fingerprints
}

// === Document abstraction ===

/// Format-specific document access.
/// All variants carry the original file path (from the JSONL record's `path` field)
/// so that metadata assertions like `filename_regex` can operate without
/// needing a separate context parameter on the Fingerprint trait.
pub enum Document {
    Xlsx(XlsxDocument),
    Csv(CsvDocument),
    Pdf(PdfDocument),
    Markdown(MarkdownDocument),
    Text(TextDocument),
    Unknown(RawDocument),
}

impl Document {
    pub fn path(&self) -> &Path; // delegates to inner variant
}

pub struct XlsxDocument {
    pub path: PathBuf,
    // Lazy sheet access via calamine
}

pub struct CsvDocument {
    pub path: PathBuf,
    // Header + streaming record access
}

pub struct PdfDocument {
    pub path: PathBuf,
    pub text: Option<MarkdownDocument>,  // Pre-extracted content from text_path (if present)
    // Structural access via lopdf (page count, metadata, form fields)
    // Content assertions dispatch to self.text when available
}

/// Structured text with heading hierarchy, sections, and tables.
/// Typically the output of a document conversion tool (Docling, llm_aided_ocr, etc.).
/// Loaded from the `text_path` field in the JSONL record.
pub struct MarkdownDocument {
    pub path: PathBuf,
    pub headings: Vec<Heading>,       // Parsed heading hierarchy
    pub sections: Vec<Section>,       // Content under each heading
    pub tables: Vec<TableRef>,        // Tables with their parent heading
}

pub struct Heading {
    pub level: u8,                    // 1-6 (markdown heading level)
    pub text: String,                 // Heading text content
    pub line: usize,                  // Line number in source
}

pub struct Section {
    pub heading: Option<Heading>,     // None for content before first heading
    pub content: String,              // Raw text of the section
    pub start_line: usize,
    pub end_line: usize,
}

pub struct TableRef {
    pub heading: Option<Heading>,     // Parent heading (nearest above)
    pub columns: Vec<String>,         // Column headers
    pub row_count: usize,
    pub start_line: usize,
}

/// Unstructured plain text (no heading parsing).
pub struct TextDocument {
    pub path: PathBuf,
    pub content: String,
    pub line_count: usize,
}

pub struct RawDocument {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
}

// === Assertion engine (for DSL fingerprints) ===

pub enum Assertion {
    // Universal
    FilenameRegex { pattern: String },

    // Spreadsheet (xlsx, csv)
    SheetExists { sheet: String },
    SheetNameRegex { pattern: String },
    CellEq { sheet: String, cell: String, value: String },
    CellRegex { sheet: String, cell: String, pattern: String },
    RangeNonNull { sheet: String, range: String },
    RangePopulated { sheet: String, range: String, min_pct: f64 },
    SheetMinRows { sheet: String, min_rows: u64 },
    SumEq { range: String, equals_cell: String, tolerance: f64 },
    WithinTolerance { cell: String, min: f64, max: f64 },

    // Content (markdown, text, pdf)
    HeadingExists { text: String },
    HeadingRegex { pattern: String },
    HeadingLevel { level: u8, pattern: String },
    TextContains { text: String },
    TextRegex { pattern: String },
    TextNear { anchor: String, pattern: String, within_chars: u32 },
    SectionNonEmpty { heading: String },
    SectionMinLines { heading: String, min_lines: u64 },
    TableExists { heading: String, index: Option<usize> },
    TableColumns { heading: String, index: Option<usize>, patterns: Vec<String> },
    TableShape { heading: String, index: Option<usize>, min_columns: Option<u32>, column_types: Option<Vec<String>> },
    TableMinRows { heading: String, index: Option<usize>, min_rows: u64 },
    PageCount { min: Option<u64>, max: Option<u64> },
    MetadataRegex { key: String, pattern: String },
}
```

### Cli struct

```rust
#[derive(Parser)]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// JSONL manifest file (default: stdin)
    #[arg(value_name = "INPUT")]
    pub input: Option<PathBuf>,

    /// Fingerprint ID to test (repeatable; evaluated in CLI order, first match wins)
    #[arg(long = "fp", value_name = "ID")]
    pub fingerprints: Vec<String>,

    /// List available fingerprints and exit
    #[arg(long)]
    pub list: bool,

    /// Number of parallel workers (default: CPU count)
    #[arg(long)]
    pub jobs: Option<usize>,

    /// Suppress witness ledger recording
    #[arg(long)]
    pub no_witness: bool,

    /// Emit progress to stderr
    #[arg(long)]
    pub progress: bool,

    /// Print operator.json and exit
    #[arg(long)]
    pub describe: bool,

    /// Print JSON Schema and exit
    #[arg(long)]
    pub schema: bool,
}

#[derive(Subcommand)]
pub enum Command {
    /// Compile DSL fingerprint to Rust crate
    Compile {
        /// DSL fingerprint file (.fp.yaml)
        yaml: PathBuf,

        /// Output directory for generated crate
        #[arg(long)]
        out: Option<PathBuf>,

        /// Validate only, don't generate code
        #[arg(long)]
        check: bool,
    },
    /// Query the witness ledger
    Witness {
        #[command(subcommand)]
        action: WitnessAction,
    },
    /// Infer fingerprint definition from example documents
    Infer {
        /// Directory of example documents
        dir: PathBuf,
        /// Negative examples for contrastive inference
        #[arg(long)]
        negative: Option<PathBuf>,
        /// Expected format
        #[arg(long)]
        format: Option<String>,
        /// Fingerprint ID
        #[arg(long)]
        id: Option<String>,
        /// Output .fp.yaml path (default: stdout)
        #[arg(long)]
        out: Option<PathBuf>,
        /// Minimum support count
        #[arg(long)]
        min_support: Option<usize>,
    },
    /// Infer fingerprint from a document + field values
    InferSchema {
        /// Example document
        #[arg(long)]
        doc: PathBuf,
        /// Field definitions YAML
        #[arg(long)]
        fields: PathBuf,
        /// Fingerprint ID
        #[arg(long)]
        id: Option<String>,
        /// Output .fp.yaml path (default: stdout)
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
pub enum WitnessAction {
    Query { /* filter flags */ },
    Last,
    Count { /* filter flags */ },
}
```

### Module structure

```
src/
├── cli/
│   ├── args.rs          # clap derive Cli / Command / WitnessAction
│   ├── exit.rs          # Outcome, exit_code()
│   └── mod.rs
├── registry/
│   ├── builtin.rs       # Core fingerprints (csv.v0, xlsx.v0, pdf.v0, markdown.v0)
│   ├── installed.rs     # Discovery of installed fingerprint crates
│   ├── core.rs          # FingerprintRegistry: resolution and listing
│   └── mod.rs
├── document/
│   ├── xlsx.rs          # XLSX document access (calamine)
│   ├── csv.rs           # CSV document access
│   ├── pdf.rs           # PDF structural access (lopdf)
│   ├── markdown.rs      # Markdown parsing: headings, sections, tables
│   ├── text.rs          # Plain text document access
│   ├── raw.rs           # Raw byte access
│   ├── dispatch.rs      # Format dispatch from mime_guess/extension/text_path
│   └── mod.rs
├── dsl/
│   ├── parser.rs        # Parse .fp.yaml into assertion list
│   ├── assertions.rs    # Assertion enum + evaluation (spreadsheet + content)
│   ├── extract.rs       # Content extraction from matched documents
│   ├── content_hash.rs  # Content hash computation
│   └── mod.rs
├── compile/
│   ├── codegen.rs       # Generate Rust source from parsed DSL
│   ├── crate_gen.rs     # Generate Cargo.toml, fixtures/, etc.
│   ├── schema.rs        # JSON Schema for DSL format
│   └── mod.rs
├── infer/
│   ├── observer.rs      # Observe structural facts from documents (format-aware)
│   ├── aggregator.rs    # Aggregate observations into assertion candidates
│   ├── contrastive.rs   # Set subtraction for contrastive inference
│   ├── schema_infer.rs  # Schema-driven: locate fields via frankensearch
│   ├── emitter.rs       # Emit .fp.yaml from aggregated profile
│   └── mod.rs
├── pipeline/
│   ├── reader.rs        # JSONL input reading + validation
│   ├── enricher.rs      # Record enrichment with fingerprint results
│   ├── parallel.rs      # Parallel processing with ordered output
│   └── mod.rs
├── output/
│   ├── jsonl.rs         # JSONL serialization to stdout
│   └── mod.rs
├── progress/
│   ├── reporter.rs      # Structured progress to stderr
│   └── mod.rs
├── refusal/
│   ├── codes.rs         # RefusalCode enum (run + compile + infer)
│   ├── payload.rs       # RefusalPayload construction
│   └── mod.rs
├── witness/
│   ├── record.rs
│   ├── ledger.rs
│   ├── query.rs
│   └── mod.rs
├── lib.rs               # pub fn run() → u8 (handles errors internally, returns exit code)
└── main.rs              # Minimal: calls fingerprint::run(), maps to ExitCode
```

### `main.rs` (≤15 lines)

```rust
#![forbid(unsafe_code)]

fn main() -> std::process::ExitCode {
    let code = fingerprint::run();
    std::process::ExitCode::from(code)
}
```

### Key dependencies

| Crate | Purpose |
|-------|---------|
| `clap` | CLI argument parsing (derive API) |
| `serde` + `serde_json` | JSONL serialization/deserialization |
| `calamine` | Excel parsing (lazy sheet enumeration) |
| `lopdf` | PDF structural access (page count, metadata) |
| `csv` | CSV parsing |
| `blake3` | Content hashing + witness record hashing |
| `chrono` | ISO 8601 timestamp formatting |
| `globset` | Filename pattern matching |
| `regex` | Cell/sheet/content regex assertions |
| `serde_yaml` | DSL fingerprint parsing |
| `frankensearch` | Hybrid search (BM25 + semantic) for infer mode |

---

## Infer Architecture

### How infer mode works

Infer mode uses [frankensearch](https://github.com/Dicklesworthstone/frankensearch) for hybrid search (BM25 lexical + semantic vector + cross-encoder reranking). All processing is local, reproducible within a pinned toolchain, and zero-retention (no document content stored — only structural facts). BM25 search is fully deterministic. Semantic search and reranking are deterministic given pinned model weights; different model versions may produce different rankings.

### Corpus infer (`fingerprint infer`)

```
Input: N example documents (same format/type)
  ↓
Observer (per-document, format-aware):
  ├── XLSX: sheet names, non-null cells, row counts, cell values at fixed addresses
  ├── Markdown: heading hierarchy, section content hashes, table columns, table row counts
  ├── CSV: column headers, row counts, value patterns in first/last rows
  └── Text: line count, recurring phrases (via content dedup)
  ↓
frankensearch index:
  ├── BM25 index of all sections/sheets across all documents
  ├── SHA-256 content dedup: byte-identical sections across corpus → boilerplate (instant assertions)
  └── Semantic embeddings for cross-document section matching (heading variation tolerance)
  ↓
Aggregator:
  ├── Intersection: patterns present in ALL positive documents → candidate assertions
  ├── Contrastive (if --negative): subtract patterns present in negative documents
  └── Rank by discriminating power (patterns unique to positive set ranked highest)
  ↓
Emitter: candidate assertions → .fp.yaml
```

### Schema-driven infer (`fingerprint infer-schema`)

```
Input: 1 document + fields.yaml (field names + example values)
  ↓
frankensearch index of document sections
  ↓
For each field in fields.yaml:
  ├── BM25 search for exact value string → candidate sections
  ├── Semantic search for field description → candidate sections
  ├── RRF fusion + cross-encoder reranking → best section
  └── Anchor detection: nearest heading above the value → assertion
  ↓
Emitter: field anchors → .fp.yaml with assertions + extract rules
```

### Infer mode is authoring, not runtime

Infer mode generates a `.fp.yaml` file. It does NOT produce fingerprint results. The generated definition is then used with `fingerprint compile` or directly in run mode. This separation ensures:

- Infer can be slow (indexing, embedding) — it runs once per template type
- Run mode stays fast — it evaluates pre-defined assertions
- The `.fp.yaml` is human-reviewable and editable before deployment

### Authoring paths summary

| Path | Command | Input | Deterministic | Zero-retention |
|------|---------|-------|---------------|----------------|
| Manual | Human writes `.fp.yaml` | Domain expertise | Yes | Yes |
| Corpus infer | `fingerprint infer` | N example docs | Pinned toolchain | Yes |
| Contrastive infer | `fingerprint infer --negative` | N positive + M negative docs | Pinned toolchain | Yes |
| Schema-driven infer | `fingerprint infer-schema` | 1 doc + field values | Pinned toolchain | Yes |
| Code inference | `fp infer-code` (cmdrvl-cli) | Existing parser code | No (LLM) | Yes |

---

## Operator Manifest (`operator.json`)

```json
{
  "schema_version": "operator.v0",
  "name": "fingerprint",
  "version": "0.1.0",
  "description": "Tests artifacts against fingerprint definitions and produces content hashes",
  "repository": "https://github.com/cmdrvl/fingerprint",
  "license": "MIT",

  "invocation": {
    "binary": "fingerprint",
    "output_mode": "stream",
    "output_schema": "fingerprint.v0",
    "json_flag": null
  },

  "arguments": [
    { "name": "input", "type": "file_path", "required": false, "position": 0, "description": "JSONL manifest file (default: stdin)" }
  ],

  "options": [
    { "name": "fp", "flag": "--fp", "type": "string", "repeatable": true, "description": "Fingerprint ID (evaluated in CLI order; first match wins)" },
    { "name": "list", "flag": "--list", "type": "boolean", "description": "List available fingerprints" },
    { "name": "jobs", "flag": "--jobs", "type": "integer", "description": "Number of parallel workers" },
    { "name": "no_witness", "flag": "--no-witness", "type": "boolean", "description": "Suppress witness ledger recording" },
    { "name": "progress", "flag": "--progress", "type": "boolean", "description": "Emit structured progress on stderr" },
    { "name": "describe", "flag": "--describe", "type": "boolean", "description": "Print operator manifest and exit" },
    { "name": "schema", "flag": "--schema", "type": "boolean", "description": "Print output schema and exit" }
  ],

  "exit_codes": {
    "0": { "meaning": "ALL_MATCHED", "domain": "positive" },
    "1": { "meaning": "PARTIAL", "domain": "negative" },
    "2": { "meaning": "REFUSAL", "domain": "error" }
  },

  "refusals": [
    { "code": "E_BAD_INPUT", "message": "Invalid JSONL or missing hash fields", "action": "run_upstream", "tool": "hash" },
    { "code": "E_UNKNOWN_FP", "message": "Fingerprint ID not found", "action": "escalate" },
    { "code": "E_DUPLICATE_FP_ID", "message": "Duplicate fingerprint ID across providers", "action": "escalate" },
    { "code": "E_UNTRUSTED_FP", "message": "Fingerprint provider not allowlisted", "action": "escalate" },
    { "code": "E_ORPHAN_CHILD", "message": "Child fingerprint references unloaded parent", "action": "escalate" }
  ],

  "capabilities": {
    "formats": ["csv", "xlsx", "pdf", "markdown", "text"],
    "profile_aware": false,
    "streaming": true
  },

  "pipeline": {
    "upstream": ["hash"],
    "downstream": ["lock"]
  }
}
```

---

## Testing Requirements

### Fixtures

- `test_files/` — small test files of each supported format:
  - `argus_model.xlsx` — matches the argus-model example assertions
  - `not_argus.xlsx` — XLSX that fails argus assertions
  - `simple.csv` — basic CSV
  - `report.pdf` — basic PDF
  - `cbre_appraisal.md` — markdown with headings, sections, tables matching CBRE assertions
  - `not_cbre.md` — markdown that fails CBRE content assertions
  - `plain_report.txt` — plain text document for text format tests
- `fingerprints/` — DSL fingerprint definitions for testing:
  - `test-xlsx.fp.yaml` — simple XLSX fingerprint
  - `test-csv.fp.yaml` — simple CSV fingerprint
  - `test-markdown.fp.yaml` — content fingerprint with heading/table/text assertions
  - `test-text.fp.yaml` — plain text fingerprint with text_contains/text_regex
- `manifests/` — pre-built JSONL manifests:
  - `hashed_manifest.jsonl` — hash-enriched records pointing to test files
  - `hashed_manifest_with_text.jsonl` — hash-enriched records with `text_path` fields
  - `upstream_skipped.jsonl` — manifest with pre-existing `_skipped` records
- `infer_corpus/` — example documents for infer mode testing:
  - `positive/` — 3+ documents of the same type
  - `negative/` — 3+ documents of a different type
  - `expected.fp.yaml` — golden output for corpus infer
- `infer_schema/` — example for schema-driven infer testing:
  - `example.md` — single document
  - `fields.yaml` — field names + values
  - `expected.fp.yaml` — golden output for schema infer

### Test categories

- **Match tests:** artifact matches fingerprint → `matched: true`, content_hash populated
- **No-match tests:** artifact fails assertions → `matched: false`, reason populated
- **Multiple fingerprint tests:** evaluated in CLI order; first match wins; last no-match reported when none match
- **Spreadsheet assertion tests:** each spreadsheet assertion type works correctly
- **Content assertion tests:** each content assertion type works correctly (heading_exists, heading_regex, text_contains, text_regex, text_near with within_chars, section_non_empty, section_min_lines, table_exists with index, table_columns with regex patterns, table_shape with column_types, table_min_rows, page_count, metadata_regex)
- **Markdown normalization tests:** setext→ATX conversion, bold-as-heading detection, whitespace normalization, table pipe alignment
- **Chained fingerprint tests:** parent match triggers child evaluation, children array in output, independent versioning, unmatched children reported correctly
- **Content hash tests:** same content produces same hash; different content produces different hash
- **Passthrough tests:** upstream fields preserved; `_skipped` records passed through with `fingerprint: null`
- **New _skipped tests:** corrupted files produce `_skipped` records; missing text_path produces `_skipped` with `E_NO_TEXT`
- **Ordering tests:** output order matches input order
- **Compile tests:** DSL → Rust crate generation is deterministic
- **Compile validation:** malformed YAML produces compile refusal
- **`--list` tests:** lists built-in fingerprints
- **Exit code tests:** 0 all matched, 1 partial, 2 refusal
- **Refusal tests:** E_BAD_INPUT, E_UNKNOWN_FP, E_DUPLICATE_FP_ID, E_UNTRUSTED_FP
- **Witness tests:** witness record appended; witness query/last/count behavior and exit codes
- **Golden file tests:** known XLSX through known fingerprint produces exact expected output
- **Golden file tests (content):** known markdown through known content fingerprint produces exact expected output
- **Infer corpus tests:** N example documents → deterministic `.fp.yaml` (same inputs = same output)
- **Infer contrastive tests:** positive + negative examples → assertions are discriminating (pass positives, fail negatives)
- **Infer-schema tests:** document + field values → `.fp.yaml` with correct anchors
- **Cross-format parity tests:** same assertion semantics produce equivalent results across formats where applicable

---

## Scope: v0.1 (ship this)

### Must have

- Run mode: stream enrichment with `--fp` (repeatable)
- Compile mode (`fingerprint compile`)
- `--list` flag
- `--jobs` for parallelism
- `--schema` flag
- `--progress` flag
- Core fingerprints: `csv.v0`, `xlsx.v0`, `pdf.v0`
- DSL spreadsheet assertion types: `filename_regex`, `sheet_exists`, `sheet_name_regex`, `cell_eq`, `cell_regex`, `range_non_null`, `sheet_min_rows`
- Content hash computation (blake3)
- `_skipped` / `_warnings` for per-file failures
- Passthrough of upstream `_skipped` records
- `tool_versions` accumulation
- Ambient witness recording + `--no-witness`
- `fingerprint witness <query|last|count>` subcommands
- `--version` flag
- `operator.json` + `--describe`
- Exit codes 0/1/2
- Refusal system with `E_BAD_INPUT`, `E_UNKNOWN_FP`, `E_DUPLICATE_FP_ID`, `E_UNTRUSTED_FP`, `E_ORPHAN_CHILD`

### v0.2: Content assertions + Infer mode

- `MarkdownDocument` and `TextDocument` types with `text_path` JSONL field support
- `PdfDocument.text` field: optional pre-extracted markdown loaded from `text_path`
- Format-aware assertion dispatch: structural assertions read PDF, content assertions read text_path
- Markdown normalization pass (setext→ATX, bold-as-heading, whitespace, table pipe alignment)
- Robust section boundary computation (equal-or-lesser-depth heading termination)
- Core content assertions: `heading_exists`, `heading_regex`, `text_contains`, `text_regex`, `text_near` (within_chars), `section_non_empty`, `section_min_lines`
- Table assertions: `table_exists`, `table_columns` (regex patterns), `table_shape` (column count + types with inference rules), `table_min_rows` — all with `index` parameter
- PDF structural assertions: `page_count`, `metadata_regex`
- Content extract types: `table`, `section`, `text_match` with formal spec
- Content hash extraction-tool-version documentation
- Core fingerprint: `markdown.v0`
- `valid_from` / `valid_until` temporal metadata fields
- Chained fingerprints: `parent` field on trait + FingerprintInfo, child evaluation after parent match, `children` array in output, strict exit code semantics
- `--diagnose` flag: context-rich assertion failure output (headings found, nearest match, tables found), no short-circuit (all assertions evaluated)
- Optional `name` field on DSL assertions, with auto-generated names as fallback
- `text_near` bidirectional search + multi-match semantics (pass if ANY anchor occurrence matches)
- `E_NO_TEXT` = fail for content assertions (structural-only parent pattern documented)
- `E_ORPHAN_CHILD` refusal for child fingerprints without loaded parent
- `W_SPARSE_TEXT` warning for scanned PDF detection
- Docling integration documentation (batch mode, pipeline examples, `jq` text_path injection, corpus failure analysis patterns)
- Infer mode: `fingerprint infer` (corpus observation, with `--negative` contrastive flag)
- Infer mode: `fingerprint infer-schema` (schema-driven field location)
- frankensearch integration for hybrid search in infer mode (reproducible within pinned toolchain)
- `compile --schema` for DSL JSON Schema output

### Can defer

- DSL spreadsheet assertion types: `range_populated`, `sum_eq`, `within_tolerance`
- `heading_level` assertion (heading at specific level)
- Temporal assertion type: `date_in_range` — first-class temporal gating in assertions (e.g., `date_in_range: { sheet: "Cover", cell: "B2", format: "%Y-%m", after: "2021-01" }`). Deferred because `valid_from`/`valid_until` metadata fields + fingerprint version proliferation handle most temporal cases without engine changes. Revisit if version proliferation becomes unmanageable.
- MinHash/LSH pre-filtering (Tier 1 optimization)
- MIME-based pre-filtering (Tier 0 optimization)
- `FINGERPRINT_PATH` plugin discovery
- Commercial fingerprint packs

---

## Open Questions

*None currently blocking. Build it.*
