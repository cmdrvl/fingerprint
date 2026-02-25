# fingerprint — Template Recognition

## One-line promise

**Determine whether an artifact matches a known template — and if so, hash the matched content.**

If it doesn't match, say so with reasons. If the pipeline can't operate, refuse.
If an individual artifact can't be processed, emit `_skipped` with structured warnings.

Second promise: **Encode domain knowledge as versioned, testable, executable assertions.**

---

## Problem (clearly understood)

You have a corpus of files — Excel models, PDF reports, CSV tapes, vendor deliverables. Before you can compare, lock, or reason about them, you need to know *what kind of thing each file is*. Today this means:

- Filename conventions that drift and break
- Manual visual inspection ("is this an Argus model?")
- Fragile regex patterns on filenames
- No structured evidence of template recognition
- No content hashing tied to template semantics

`fingerprint` replaces that with **executable, versioned template assertions** that produce deterministic match/no-match verdicts with content hashes for change detection.

---

## Non-goals (explicit)

`fingerprint` is NOT:

- An extractor or parser (it does not transform data into a target schema)
- A diff tool (that's `rvl` / `compare`)
- A column scoper (that's `profile`)
- An AI classifier (assertions are deterministic code, not probabilistic)

It does not tell you *what the data means*.
It tells you *whether this file matches a known template*, and if so, *what the content hash is*.

**Clarification: fingerprint content extraction vs data extraction.** Fingerprints include an `extract:` section (DSL) and an `extracted` field (Rust trait) that pull content from matched documents. This is **content identity extraction** — extracting specific cells or ranges to compute a content hash for change detection. It is NOT **data transformation extraction** — parsing document content into structured values in a target schema.

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

---

## Two Modes

### Run mode (stream enrichment)

Tests artifacts against installed fingerprint definitions. This is the pipeline mode.

### Compile mode (crate generation)

Compiles DSL fingerprint definitions (`.fp.yaml`) into Rust crates. This is an authoring/build-time tool.

Both modes share the `fingerprint` binary. Run mode is the default; compile mode is a subcommand.

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
    fn format(&self) -> &str;       // "xlsx"
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

The table below is the full assertion roadmap. The v0.1 ship subset is defined in [Scope: v0.1](#scope-v01-ship-this).

| Assertion | Purpose | Example |
|-----------|---------|---------|
| `filename_regex` | File basename matches regex | `filename_regex: { pattern: "(?i)(?:_FINF\|financials?\|Remit Financial)" }` |
| `sheet_exists` | Worksheet with name exists | `sheet_exists: "Assumptions"` |
| `sheet_name_regex` | Any worksheet name matches regex | `sheet_name_regex: { pattern: "(?i)(FINF\|financial\|Remit\\s*Fin)" }` |
| `cell_eq` | Cell contains exact value | `cell_eq: { sheet: "...", cell: "A3", value: "..." }` |
| `cell_regex` | Cell matches regex | `cell_regex: { sheet: "...", cell: "B1", pattern: "^FY20[0-9]{2}$" }` |
| `range_non_null` | All cells in range are non-empty | `range_non_null: { sheet: "...", range: "A3:D10" }` |
| `range_populated` | ≥X% of cells non-empty | `range_populated: { sheet: "...", range: "...", min_pct: 0.8 }` |
| `sheet_min_rows` | Sheet has ≥N data rows | `sheet_min_rows: { sheet: "...", min_rows: 10 }` |
| `sum_eq` | Sum of range equals value/cell | `sum_eq: { range: "D3:D10", equals_cell: "D11", tolerance: 0.01 }` |
| `within_tolerance` | Value in range | `within_tolerance: { cell: "E5", min: 0, max: 1 }` |

**Why `filename_regex` and `sheet_name_regex`:** Real-world CMBS data uses flat CSV/TXT files where the filename is the primary recognition signal, and Excel workbooks where the relevant sheet name varies across issuers. These assertions handle the pre-screening layer.

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
    f. Try each --fp in order:
       i.   Check fingerprint's declared format vs Document type → skip if mismatch
       ii.  Run assertions in declaration order; short-circuit on first failure
            (remaining assertions are recorded as "Skipped" — some are also
            structurally impossible, e.g., cell check when sheet doesn't exist)
       iii. If all pass: MATCH → extract content, compute content_hash, stop trying
       iv.  If any fail: NO_MATCH → try next --fp
    g. Build fingerprint result (match or last no-match)
    h. Update version, merge tool_versions
    i. Emit to stdout
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
    // Structural access via lopdf (page count, metadata, form fields)
}

pub struct RawDocument {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
}

// === Assertion engine (for DSL fingerprints) ===

pub enum Assertion {
    FilenameRegex { pattern: String },
    SheetExists { sheet: String },
    SheetNameRegex { pattern: String },
    CellEq { sheet: String, cell: String, value: String },
    CellRegex { sheet: String, cell: String, pattern: String },
    RangeNonNull { sheet: String, range: String },
    RangePopulated { sheet: String, range: String, min_pct: f64 },
    SheetMinRows { sheet: String, min_rows: u64 },
    SumEq { range: String, equals_cell: String, tolerance: f64 },
    WithinTolerance { cell: String, min: f64, max: f64 },
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
│   ├── builtin.rs       # Core fingerprints (csv.v0, xlsx.v0, pdf.v0)
│   ├── installed.rs     # Discovery of installed fingerprint crates
│   ├── registry.rs      # FingerprintRegistry: resolution and listing
│   └── mod.rs
├── document/
│   ├── xlsx.rs          # XLSX document access (calamine)
│   ├── csv.rs           # CSV document access
│   ├── pdf.rs           # PDF structural access (lopdf)
│   ├── raw.rs           # Raw byte access
│   ├── dispatch.rs      # Format dispatch from mime_guess/extension
│   └── mod.rs
├── dsl/
│   ├── parser.rs        # Parse .fp.yaml into assertion list
│   ├── assertions.rs    # Assertion enum + evaluation
│   ├── extract.rs       # Content extraction from matched documents
│   ├── content_hash.rs  # Content hash computation
│   └── mod.rs
├── compile/
│   ├── codegen.rs       # Generate Rust source from parsed DSL
│   ├── crate_gen.rs     # Generate Cargo.toml, fixtures/, etc.
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
│   ├── codes.rs         # RefusalCode enum (run + compile)
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
| `lopdf` | PDF structural access |
| `csv` | CSV parsing |
| `blake3` | Content hashing + witness record hashing |
| `chrono` | ISO 8601 timestamp formatting |
| `globset` | Filename pattern matching |
| `regex` | Cell/sheet regex assertions |
| `serde_yaml` | DSL fingerprint parsing |

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
    { "code": "E_UNTRUSTED_FP", "message": "Fingerprint provider not allowlisted", "action": "escalate" }
  ],

  "capabilities": {
    "formats": ["csv", "xlsx", "pdf"],
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
- `fingerprints/` — DSL fingerprint definitions for testing:
  - `test-xlsx.fp.yaml` — simple XLSX fingerprint
  - `test-csv.fp.yaml` — simple CSV fingerprint
- `manifests/` — pre-built JSONL manifests:
  - `hashed_manifest.jsonl` — hash-enriched records pointing to test files
  - `upstream_skipped.jsonl` — manifest with pre-existing `_skipped` records

### Test categories

- **Match tests:** artifact matches fingerprint → `matched: true`, content_hash populated
- **No-match tests:** artifact fails assertions → `matched: false`, reason populated
- **Multiple fingerprint tests:** evaluated in CLI order; first match wins; last no-match reported when none match
- **Assertion tests:** each DSL assertion type works correctly
- **Content hash tests:** same content produces same hash; different content produces different hash
- **Passthrough tests:** upstream fields preserved; `_skipped` records passed through with `fingerprint: null`
- **New _skipped tests:** corrupted files produce `_skipped` records
- **Ordering tests:** output order matches input order
- **Compile tests:** DSL → Rust crate generation is deterministic
- **Compile validation:** malformed YAML produces compile refusal
- **`--list` tests:** lists built-in fingerprints
- **Exit code tests:** 0 all matched, 1 partial, 2 refusal
- **Refusal tests:** E_BAD_INPUT, E_UNKNOWN_FP, E_DUPLICATE_FP_ID, E_UNTRUSTED_FP
- **Witness tests:** witness record appended; witness query/last/count behavior and exit codes
- **Golden file tests:** known XLSX through known fingerprint produces exact expected output

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
- DSL assertion types: `filename_regex`, `sheet_exists`, `sheet_name_regex`, `cell_eq`, `cell_regex`, `range_non_null`, `sheet_min_rows`
- Content hash computation (blake3)
- `_skipped` / `_warnings` for per-file failures
- Passthrough of upstream `_skipped` records
- `tool_versions` accumulation
- Ambient witness recording + `--no-witness`
- `fingerprint witness <query|last|count>` subcommands
- `--version` flag
- `operator.json` + `--describe`
- Exit codes 0/1/2
- Refusal system with `E_BAD_INPUT`, `E_UNKNOWN_FP`, `E_DUPLICATE_FP_ID`, `E_UNTRUSTED_FP`

### Can defer

- DSL assertion types: `range_populated`, `sum_eq`, `within_tolerance`
- MinHash/LSH pre-filtering (Tier 1 optimization)
- MIME-based pre-filtering (Tier 0 optimization)
- `FINGERPRINT_PATH` plugin discovery
- Commercial fingerprint packs

---

## Open Questions

*None currently blocking. Build it.*
