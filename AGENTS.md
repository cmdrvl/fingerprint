# AGENTS.md — fingerprint

> Repo-specific guidelines. Inherits shared rules from [`../AGENTS.md`](../AGENTS.md).

---

## fingerprint — What This Project Does

`fingerprint` tests artifacts against versioned template definitions using deterministic assertions, producing content hashes for change detection.

Pipeline position:

```
vacuum → hash → fingerprint → lock → pack
```

Three modes:
- **Run mode** (default): stream enrichment — reads hash-enriched JSONL, tests each record against fingerprint definitions, emits enriched JSONL.
- **Compile mode**: DSL compilation — reads `.fp.yaml` and generates a Rust crate implementing the Fingerprint trait.
- **Infer mode**: learn definitions — observes a corpus and generates `.fp.yaml` candidates.
- **Peek mode**: row-shape sensing — reads a CSV-like file and emits metadata only; it must never emit cell values.

### Quick Reference

```bash
# Core pipeline
vacuum /data/models | hash | fingerprint --fp argus-model.v1

# Multiple fingerprints (first match wins)
vacuum /data | hash | fingerprint --fp argus-model.v1 --fp csv.v0

# Compile DSL to Rust crate
fingerprint compile argus-model.fp.yaml --out fingerprint-argus-model-v1/

# Inspect CSV row shape without exposing cell content
fingerprint peek messy-export.csv --json --suggest --no-witness
```

### Source of Truth

- **Spec:** [`docs/PLAN.md`](./docs/PLAN.md) — behavior must follow this document.
- Do not invent behavior not present in the plan.

### Key Files (planned)

| File | Purpose |
|------|---------|
| `src/main.rs` | CLI entry + exit code mapping (≤15 lines) |
| `src/lib.rs` | Orchestration: `pub fn run() → u8` |
| `src/cli/` | Argument parsing (clap derive) + witness subcommands |
| `src/registry/` | Fingerprint resolution: builtin, installed crates, listing |
| `src/document/` | Format-specific document access (xlsx, csv, pdf, markdown, text, raw) |
| `src/dsl/` | DSL parser, assertion engine, content extraction, content hash |
| `src/compile/` | Codegen: DSL → Rust crate |
| `src/pipeline/` | JSONL reader, record enricher, parallel processing |
| `src/output/` | JSONL serialization to stdout |
| `src/peek.rs` | Veil-safe row-shape sensor for CSV pre-parse planning |
| `src/progress/` | Structured progress to stderr |
| `src/refusal/` | Refusal codes and envelope construction |
| `src/witness/` | Witness append/query behavior |
| `src/infer/` | Corpus observation, contrastive analysis, schema-driven inference |
| `operator.json` | Machine-readable operator contract |

---

## Output Contract (Critical)

`fingerprint` is a **template recognizer** that enriches JSONL on stdout:

- Normal path emits enriched JSONL records to stdout (one per input record).
- Refusal path emits one refusal JSON envelope to stdout.
- No human-report mode on stdout — pure JSONL only.
- `--diagnose` may add `fingerprint.diagnostics` with attempted fingerprint IDs, first failed assertion summaries, nearest-match context, and first-match short-circuit metadata. Normal run mode stays schema-stable without it.
- `fingerprint peek` is the exception to stream enrichment: it emits a single `fingerprint.peek.v0` JSON object containing row shape metadata only. It is read-only and must not emit cell content.

| Exit | Meaning |
|------|---------|
| `0` | `ALL_MATCHED` — every record matched a fingerprint |
| `1` | `PARTIAL` — some records unmatched or `_skipped` |
| `2` | `REFUSAL` — pipeline-level failure or CLI error |

---

## Core Invariants (Do Not Break)

### 1. Deterministic assertion evaluation

- Assertions are code, not probabilities — `matched: true` or `matched: false`.
- Same input + same fingerprint + same version always produces identical output.
- Multiple `--fp` evaluated in CLI order; first match wins.

### 2. Record format

- `version` must be `"fingerprint.v0"`.
- All upstream fields preserved verbatim.
- `fingerprint` object always present (set to `null` for `_skipped` records).
- `tool_versions` must include `{ "fingerprint": "<semver>" }` merged with upstream.

### 3. `_skipped` semantics

- **Upstream `_skipped`**: passed through unchanged — only `version`, `tool_versions` updated, `fingerprint: null`.
- **New skip**: IO/parse failure marks `_skipped: true`, `fingerprint: null`, appends `_warnings` entry.
- Skipped records do NOT prevent `ALL_MATCHED` only if they were already `_skipped` upstream. New skips force `PARTIAL` (exit 1).

### 4. Output ordering

- Output order matches input order.
- When `--jobs > 1`, records buffered and emitted in sequence.
- Implementation MUST bound in-flight work and reorder buffers (no unbounded growth).

### 5. Refusal boundary

- `E_BAD_INPUT`: invalid JSONL, missing `bytes_hash` on non-skipped, unrecognized upstream `version`.
- `E_UNKNOWN_FP`: fingerprint ID not found.
- `E_DUPLICATE_FP_ID`: same ID from multiple providers.
- `E_UNTRUSTED_FP`: external provider not allowlisted.
- `E_ORPHAN_CHILD`: child fingerprint references unloaded parent.
- Per-file IO/parse failures are `_skipped` records, NOT refusals.

### 6. Content hash contract

- Content hash computed only on match (`null` when `matched: false`).
- BLAKE3 over extracted content sections.
- Same document + same fingerprint + same extraction = same content hash.

### 7. Witness parity

Ambient witness semantics must match spine conventions:
- Append by default to `$EPISTEMIC_WITNESS` or `~/.cmdrvl/state/witness/witness.jsonl`.
- Trust config defaults to `~/.cmdrvl/config/fingerprint/trust.yaml`.
- Installed definitions default to `~/.cmdrvl/config/fingerprint/definitions/`.
- `--no-witness` opt-out.
- Witness failures do not mutate domain outcome semantics (non-fatal).
- Witness query subcommands supported (`query`, `last`, `count`).
- Compile mode does NOT produce witness records.
- Peek witness records may include input path, file hash, row count, and shape summary parameters; they must not include raw cell content.

### 8. Peek content safety

- `fingerprint peek` may inspect cell values internally to classify type and length.
- It must emit only counts, row indexes, length buckets, type classes, delimiter metadata, and pre-parse suggestions.
- Do not put raw cell values into JSON output, refusal details, stderr warnings, test names, or witness params.
- If a parser error happens, return a generic parse refusal with path and error class only.

### 9. Normalizer freeze

- The HTML/Markdown/PDF content normalizers are frozen for compatibility. Do not add new visual-heading, page-break, or “looks like structure” heuristics.
- Route new HTML structural cues through selector assertions (`node_exists`, `node_count`, `node_text_regex`, `attr_regex`) and selector-anchored extract definitions.
- Existing markdown passes, including bold-as-heading promotion for Docling artifacts, are grandfathered but must not expand without a superseding SCP.

## Repo Ownership Boundaries

This repo owns template recognition, fingerprint DSL compilation, deterministic inference, row-shape sensing, and witness records for those operations. It does not own profile slicing, row-level reconciliation, lockfile semantics, packaging, or Homebrew tap policy beyond the release workflow files in this repo.

Keep changes inside this repo's behavioral surface. If a fix requires a downstream contract change, file a bead in the downstream repo instead of smuggling a compatibility shim into `fingerprint`.

## Build, Test, And Lint Commands

Run these after substantive Rust changes:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Before committing, run UBS on changed files:

```bash
ubs $(git diff --name-only --cached)
```

For focused work, use the narrow tests first, then the full gate:

```bash
cargo test --test peek
cargo test doctor
cargo test --test selector_assertions
cargo test --test normalizer_freeze
cargo test --test selector_region
```

## Beads And Agent Mail

- Use `br ready --json`, `br show <id> --json`, `br update <id> --status in_progress`, `br close <id> --reason "..."`, and `br sync --flush-only`.
- Never run bare `bv`; use only `bv --robot-*` flags if graph triage is needed.
- If multiple agents are editing this repo, reserve only exact files through Agent Mail. Use the bead ID as the shared thread ID.
- Treat unexpected working-tree edits as concurrent work. Do not revert, stash, or overwrite them.

## Tool Guidance

- Use `rg` for text/file search and `ast-grep` when Rust syntax or structural matching matters.
- Prefer existing modules and output helpers over new abstractions.
- Keep CLI output machine-readable. Do not add casual stdout text to run mode or `peek`.
- Avoid network-dependent tests unless the existing suite already gates that behavior.

## Landing The Plane

Before ending a session with repo changes:

1. Run the relevant quality gates.
2. Close or update the worked Beads and run `br sync --flush-only`.
3. Commit all repo-owned changes.
4. Push `main`, then `git push origin main:master`.
5. Verify `git status -sb` is clean and up to date.

---

## Key Dependencies

| Crate | Purpose |
|-------|---------|
| `clap` | CLI argument parsing (derive API) |
| `serde` + `serde_json` | JSONL serialization |
| `calamine` | Excel parsing (lazy sheet enumeration) |
| `lopdf` | PDF structural access |
| `csv` | CSV parsing |
| `blake3` | Content hashing + witness hashing |
| `chrono` | ISO 8601 timestamps |
| `regex` | Cell/sheet regex assertions |
| `serde_yaml` | DSL fingerprint parsing |

---

## Minimum Coverage Areas

- Assertion engine: each DSL assertion type (sheet_exists, cell_eq, cell_regex, range_non_null, sheet_min_rows, filename_regex, sheet_name_regex, heading_exists, heading_regex, text_contains, text_regex, text_near, section_non_empty, section_min_lines, table_exists, table_columns, table_shape, table_min_rows, page_count, metadata_regex)
- HTML selector assertions: node_exists, node_count, node_text_regex, attr_regex
- Normalizer freeze guards: no styled-heading promotion, no synthesized page model, and golden stability for existing HTML normalization
- Match/no-match/partial outcome routing and exit codes
- Multiple `--fp` evaluation order (first match wins)
- Content hash determinism (same content → same hash)
- `_skipped` passthrough (upstream) and new skip creation (IO/parse failure)
- Refusal paths (E_BAD_INPUT, E_UNKNOWN_FP, E_DUPLICATE_FP_ID, E_UNTRUSTED_FP, E_ORPHAN_CHILD)
- Output ordering under parallel execution
- Compile mode: DSL → Rust determinism
- Witness append/query behavior
- Chained fingerprints (parent-child evaluation, exit code semantics)
- Content assertions on format:pdf with text_path dispatch
- E2E spine compatibility (`vacuum → hash → fingerprint → lock`)
