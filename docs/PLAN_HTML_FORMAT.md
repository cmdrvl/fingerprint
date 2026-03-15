# Plan: Native HTML Format Support

## Promise

**Fingerprint an HTML document the same way we fingerprint XLSX, CSV, PDF, and Markdown — with deterministic assertions over headings, tables, text, and structural shape.**

HTML is the fifth input format. Once it works, any `.fp.yaml` with `format: html` evaluates the same assertion vocabulary against HTML documents, produces the same JSONL output, and feeds the same pipeline (`vacuum | hash | fingerprint | lock`).

Second promise: **Replace the tournament's homegrown Python schedule router with compiled fingerprint definitions**, eliminating ~650 lines of custom scoring logic in favor of versioned, testable, inferable `.fp.yaml` artifacts.

---

## Non-goals

- **JavaScript rendering.** SEC filings are static HTML. No headless browser, no DOM execution.
- **CSS-based layout analysis.** Structure comes from elements (`<table>`, `<section>`, `<h1>`), not styling.
- **Visual/pixel fingerprinting.** Not applicable to HTML.
- **Score-based routing.** The Python script uses weighted scoring across families. Fingerprint uses deterministic first-match. The migration replaces scoring with assertion specificity ordering. If two families both match, the fingerprint definitions are insufficiently constrained — fix the definitions, don't add scoring.
- **Canon integration inside fingerprint.** The Python script shells out to a `canon` binary for semantic role resolution. Fingerprint encodes the same knowledge as regex patterns in assertion definitions. If a header says "Business Description" or "Issuer" or "Holding Name", the assertion pattern matches it directly. The vocabulary mapping lives in the `.fp.yaml`, not in a runtime dependency.
- **Replacing the Python script's `family_prior` (ticker-based context).** Prior knowledge about which strategy a BDC uses is external metadata, not document structure. It belongs in the tournament router's dispatch logic, not in fingerprint assertions. Fingerprint answers "what kind of document is this?" — not "what do we already know about this entity?"

---

## What the Python script does that must be accounted for

The plan must account for every capability of `fingerprint_schedule_family.py` — either by implementing it in fingerprint or by explicitly declaring it a non-goal.

| Python Capability | Disposition |
|---|---|
| HTML table parsing with colspan/rowspan | **Implement** — core of `HtmlDocument` |
| `<section data-page-number>` page boundaries | **Implement** — `PageSection` in `HtmlDocument` |
| Column count per page, dominant column count | **Implement** — new `dominant_column_count` assertion |
| Header token detection (known keywords in table headers) | **Implement** — new `header_token_search` assertion |
| Full-width heading detection (rows where all cells match) | **Implement** — new `full_width_row` assertion |
| Heading classification (scope / asset_class / industry / company) | **Non-goal** — fingerprint recognizes structure, not semantics. The tournament router classifies matched full-width rows using its own domain vocabulary. |
| Behavioral trait derivation (has_coupon_column, etc.) | **Replace** — traits become individual assertions; no aggregate trait object needed |
| Parsing shape classification (business_description_split, etc.) | **Replace** — composite shapes become chained fingerprint hierarchies |
| Canon binary integration for role resolution | **Non-goal** — encode as regex patterns in assertions |
| Weighted scoring across families | **Non-goal** — replace with first-match ordering by specificity |
| Ticker-based family priors from master CSV | **Non-goal** — external metadata, not document structure |

---

## Architecture

### Document model — extend, don't abstract

The existing code uses a `Document` enum with per-variant match arms in four dispatch functions:

- `get_content_document()` in `assertions.rs` — returns `&MarkdownDocument`
- `content_source_text()` in `assertions.rs` — returns `Option<&str>`
- `content_document()` in `extract.rs` — returns `Option<&MarkdownDocument>`
- `content_text()` in `extract.rs` — returns `Option<&str>`

Additionally, `evaluate_text_contains` and `evaluate_text_regex` have their own per-variant `match doc {}` blocks that bypass `get_content_document()`.

**Approach: add `Document::Html` arms to all six dispatch points.** No trait. The `Document` enum already has six variants — a seventh does not change the pattern. A trait with exactly two implementors adds indirection without value. If a third content format appears later, reconsider.

`HtmlDocument` stores parsed content in the same `Heading`, `Section`, and `Table` types that `MarkdownDocument` uses. The `Section` type gets one new optional field (`pub page: Option<u32>`) for page-boundary context from `<section data-page-number>`. No `HtmlSection` or `PageSection` types — use the existing `Section` with an optional page annotation.

```rust
pub struct HtmlDocument {
    pub path: PathBuf,
    pub raw: String,
    pub normalized: String,        // Full text, whitespace-normalized
    pub headings: Vec<Heading>,    // <h1> through <h6>
    pub sections: Vec<Section>,    // Grouped by heading hierarchy, Section.page set from <section>
    pub tables: Vec<Table>,        // All <table> elements, Table.heading_ref set from preceding heading
}
```

### HTML table parsing — the hard part

SEC filing HTML tables use `colspan`, `rowspan`, nested tables, `<colgroup>`, and structurally ambiguous headers. This is where `pandas.read_html()` spends thousands of lines. The Rust parser must handle:

**Colspan expansion.** `<td colspan="3">Value</td>` produces 3 cells: `["Value", "Value", "Value"]`. If colspan plus existing cells exceeds the row width established by earlier rows, the extra cells are appended (widening the row).

**Rowspan carry-forward.** `<td rowspan="2">Value</td>` produces `"Value"` in the current row and the next row at the same column index. Carry-forward state is tracked per-column and decremented each row.

**Nested tables.** A `<table>` inside a `<td>` is ignored — only top-level tables within each `<section>` (or document root if no sections) are extracted. Nested tables are flattened to their text content.

**Header detection.** If the table contains `<th>` elements, those are the headers. If not, the first `<tr>` is treated as headers. If the first row is a separator or empty, scan forward up to 3 rows.

**Empty cells.** An empty `<td></td>` or `<td>&nbsp;</td>` becomes an empty string `""` in the row, never omitted.

**Whitespace normalization.** `&nbsp;` (`\u00a0`), `&ndash;`, `&mdash;`, and multi-space runs are collapsed to single spaces. Leading/trailing whitespace trimmed per cell.

**Crate: `scraper` 0.22.** CSS selector-based, no JS, handles malformed HTML gracefully.

---

## New assertions

### `header_token_search`

The core Python fingerprinting question: "does this table have a Business Description column anywhere in its headers?"

The existing `table_columns` assertion requires positional matching (pattern[0] must match header[0]). That is wrong for this use case — we need "does any header cell match any of these patterns?"

```yaml
header_token_search:
  page: 1                          # Optional: restrict to tables on this page
  index: 0                         # Optional: which table (default: any)
  tokens:
    - "(?i)business\\s+description"
    - "(?i)coupon"
    - "(?i)fair\\s+value"
  min_matches: 2                   # At least 2 of the tokens must match some header cell
```

**Evaluation:** For each table matching the page/index filters, scan all header cells against all token patterns. Count distinct pattern matches. Pass if `count >= min_matches`.

This replaces the Python script's `HEADER_TOKENS` dict and the `has_business_description_column`, `has_coupon_column`, etc. behavior traits.

### `dominant_column_count`

Most common table width across the first N pages.

```yaml
dominant_column_count:
  count: 66                        # Expected dominant width
  tolerance: 3                     # Absolute: pass if dominant is within [count - tolerance, count + tolerance]
  sample_pages: 4                  # Only consider tables from first N pages (default: 4)
```

**Evaluation:** Collect column counts from all tables on the first `sample_pages` pages. Find the mode (most common count). Pass if `|mode - count| <= tolerance`.

### `full_width_row`

Row where all cells contain identical text — used as section/industry/asset-class headings in SOI tables.

```yaml
full_width_row:
  pattern: "(?i)^(software|first lien|equity)"
  min_cells: 10                    # Minimum cell count to qualify (prevents false positives on short rows)
```

**Evaluation:** Scan all table rows. For rows where (a) all non-empty cells have identical text, (b) the cell count >= `min_cells`, and (c) the text matches `pattern` — pass. Fingerprint reports that the structural pattern exists; what those rows *mean* (industry vs asset class vs scope) is the downstream consumer's job.

### `page_section_count`

Number of `<section data-page-number>` blocks.

```yaml
page_section_count:
  min: 5
  max: 150
```

**Evaluation:** Count `Section` entries that have `page` set. Pass if count is within `[min, max]`.

### Summary

| Assertion | Purpose | Replaces from Python |
|---|---|---|
| `header_token_search` | "Does any table header match these keywords?" | `HEADER_TOKENS`, behavior traits |
| `dominant_column_count` | "What is the most common table width?" | `dominant_column_count` derivation |
| `full_width_row` | "Are there full-width heading rows in the tables?" | `distinct_full_width_headings()`, heading classification |
| `page_section_count` | "How many pages does this schedule span?" | `page_count` in fingerprint dict |

All existing content assertions (`heading_exists`, `text_contains`, `table_shape`, `table_min_rows`, etc.) work on HTML by adding `Document::Html` arms to the six dispatch functions. No trait needed.

---

## BDC schedule fingerprint definitions

### Chained fingerprint hierarchy

The Python script's `family_match()` function tests every family against a shared set of base conditions, then family-specific conditions. This maps to chained fingerprints:

```
bdc-soi.v1                  (parent: verifies this is a Schedule of Investments)
├── bdc-soi-ares.v1         (child: wide layout, business description, industry headings)
├── bdc-soi-bxsl.v1         (child: investment type from section headings, industry headings)
├── bdc-soi-pennant.v1      (child: explicit industry column, asset class headings)
├── bdc-soi-golub.v1        (child: compact layout, industry headings, no explicit type column)
└── bdc-soi-blackrock.v1    (child: issuer/instrument leading columns, coupon-oriented)
```

Parent match is a prerequisite for child evaluation. Each child produces its own content hash. Unmatched children → `matched: false` at the child level, parent still matched.

### `bdc-soi.v1.fp.yaml` (parent)

```yaml
fingerprint_id: bdc-soi.v1
format: html

assertions:
  - name: has_schedule_content
    text_regex:
      pattern: "(?i)schedule\\s+of\\s+investments"
  - name: has_tabular_data
    page_section_count:
      min: 3
  - name: has_fair_value_column
    header_token_search:
      tokens:
        - "(?i)fair\\s+value"
      min_matches: 1
```

### `bdc-soi-ares.v1.fp.yaml` (child)

```yaml
fingerprint_id: bdc-soi-ares.v1
parent: bdc-soi.v1
format: html

assertions:
  - name: wide_table_layout
    dominant_column_count:
      count: 66
      tolerance: 3
      sample_pages: 4
  - name: has_business_description
    header_token_search:
      tokens:
        - "(?i)business\\s+description"
      min_matches: 1
  - name: has_industry_section_headings
    full_width_row:
      pattern: "(?i)^(software|aerospace|automotive|banking|beverage|building|capital|chemical|consumer|diversified|education|electric|energy|environmental|financial|food|healthcare|hotel|human|insurance|internet|leisure|machinery|media|metals|oil|paper|personal|pharmaceuticals|real|retail|semiconductor|technology|telecommunications|textiles|transportation|utilities)"
      min_cells: 10
  - name: has_coupon_column
    header_token_search:
      tokens:
        - "(?i)coupon"
      min_matches: 1

extract:
  - name: schedule_tables
    type: table
    anchor_heading: null
    all: true

content_hash:
  algorithm: blake3
  over: [schedule_tables]
```

### `bdc-soi-pennant.v1.fp.yaml` (child)

```yaml
fingerprint_id: bdc-soi-pennant.v1
parent: bdc-soi.v1
format: html

assertions:
  - name: has_explicit_industry_column
    header_token_search:
      tokens:
        - "(?i)^industry$"
      min_matches: 1
  - name: has_asset_class_headings
    full_width_row:
      pattern: "(?i)^(first lien|second lien|subordinated|unsecured|equity|preferred|common|warrant)"
      min_cells: 10
  - name: no_business_description
    header_token_search:
      tokens:
        - "(?i)business\\s+description"
      min_matches: 0
      max_matches: 0
```

### Routing model

Children are evaluated in CLI order. First child match wins:

```bash
fingerprint \
  --fp bdc-soi.v1 \
  --fp bdc-soi-ares.v1 \
  --fp bdc-soi-pennant.v1 \
  --fp bdc-soi-golub.v1 \
  --fp bdc-soi-bxsl.v1 \
  --fp bdc-soi-blackrock.v1
```

Order by specificity: most constrained first. If two children match the same document, the definitions are insufficiently discriminating — tighten the losing child's assertions until it correctly rejects. `--diagnose` shows exactly which assertions each child passed/failed, making this debuggable.

**Why first-match replaces scoring.** The Python script's scoring exists because its traits are soft signals. Fingerprint assertions are hard: pass or fail. If a document matches pennant's assertions, it IS pennant-shaped. If both pennant and ares match, one of the definitions is wrong — it accepted a document it shouldn't have. Fix the definition, don't add a tiebreaker.

---

## Learning from existing filings

### Corpus infer

The infer engine must be extended for HTML. Three specific changes:

1. **`observer.rs`**: Add `observe_html()` function. The `Observation` struct needs new fields for content-document observations (headings found, table shapes, text patterns). Currently only has spreadsheet/PDF fields. Either extend `Observation` or introduce a `ContentObservation` sub-struct.

2. **`aggregator.rs`**: Add `aggregate_html()` function in the format match block (currently rejects anything except xlsx/csv/pdf). HTML aggregation should produce heading frequency maps, table shape distributions, and full-width row pattern candidates — directly analogous to what the Python script's `fingerprint_section()` computes.

3. **`--format html` flag**: Accept in CLI argument validation.

### Contrastive infer for family discrimination

```bash
# Given: directories of known-family schedule sections
fingerprint infer ./ares-sections/ \
  --negative ./pennant-sections/ ./golub-sections/ ./bxsl-sections/ \
  --format html \
  --id bdc-soi-ares.v1 \
  --out bdc-soi-ares.v1.fp.yaml
```

This auto-discovers the minimal assertion set that matches all Ares documents and rejects all non-Ares documents. The hybrid search engine (BM25 + semantic) identifies which structural features are invariant within Ares and absent outside it.

**This is the primary authoring workflow for BDC fingerprints.** Manual YAML writing is the bootstrap. Contrastive infer on the existing 40+ parsed sections is how the definitions get refined.

### Schema-driven infer

```bash
# Given: one Ares filing + known field locations
fingerprint infer-schema \
  --doc ares-capital_2025-09-30.html \
  --fields bdc-soi-fields.yaml \
  --id bdc-soi-ares.v1 \
  --out bdc-soi-ares.v1.fp.yaml
```

Where `bdc-soi-fields.yaml` declares the fields we care about:

```yaml
fields:
  - name: fair_value_column
    expected: "Fair Value"
    type: header_token
  - name: industry_heading
    expected: "Software and Services"
    type: full_width_row
```

---

## Implementation sequence

| Step | Files | Change | Scope |
|---|---|---|---|
| 1 | `Cargo.toml` | Add `scraper = "0.22"` | 1 line |
| 2 | `src/document/html.rs` | `HtmlDocument` struct, HTML→headings/sections/tables parser, colspan/rowspan expansion | ~500 lines |
| 3 | `src/document/mod.rs` | Add `Document::Html(HtmlDocument)` variant, `pub mod html` | ~10 lines |
| 4 | `src/document/dispatch.rs` | Add `"html" \| "htm"` extension case | ~5 lines |
| 5 | `src/document/markdown.rs` | Add `pub page: Option<u32>` to `Section` struct | ~3 lines |
| 6 | `src/dsl/assertions.rs` | Add `Document::Html` arms to `get_content_document`, `content_source_text`, `evaluate_text_contains`, `evaluate_text_regex`, and all other per-variant match blocks (audit every `match doc` in the file) | ~60 lines |
| 7 | `src/dsl/extract.rs` | Add `Document::Html` arms to `content_document` and `content_text` | ~10 lines |
| 8 | `src/dsl/assertions.rs` | New assertion types: `HeaderTokenSearch`, `DominantColumnCount`, `FullWidthRow`, `PageSectionCount` | ~200 lines |
| 9 | `src/dsl/parser.rs` | Parse new assertion YAML keys | ~40 lines |
| 10 | `src/infer/observer.rs` | `observe_html()`, extend `Observation` struct for content fields | ~150 lines |
| 11 | `src/infer/aggregator.rs` | `aggregate_html()` in format match block | ~100 lines |
| 12 | `operator.json` | Add `html` to supported formats list | 1 line |
| 13 | `tests/` | HTML parser unit tests, assertion golden tests, chained fingerprint test, infer test | ~300 lines |
| 14 | `rules/bdc-soi-*.fp.yaml` | Parent + 5 child fingerprint definitions for BDC families | ~150 lines YAML |

**Total estimated: ~1,500 lines.** Step 2 (HTML parser) is the largest and riskiest — colspan/rowspan handling is the complexity center.

---

## Versioning

- `format: html` is a **v0.5.0 DSL extension**. Older `fingerprint` binaries that encounter `format: html` in a `.fp.yaml` must emit `E_UNKNOWN_FORMAT` refusal (not crash).
- Output schema remains `fingerprint.v0` — the JSONL output format is unchanged.
- New assertions (`header_token_search`, `dominant_column_count`, `full_width_row`, `page_section_count`) are v0.5.0 additions. Older binaries encountering these in a `.fp.yaml` must emit `E_UNKNOWN_ASSERTION` refusal.
- `operator.json` version bumps to 0.5.0.

---

## Verification

1. **HTML parser unit tests**: Parse known Ares schedule HTML → verify table count, column count per table, heading extraction, full-width row detection, colspan expansion, rowspan carry-forward.
2. **Parent fingerprint test**: `bdc-soi.v1` matches all 40+ known schedule sections → `matched: true` for every one.
3. **Child fingerprint tests**: Each family fingerprint correctly matches its own known sections and rejects all other families' sections. Matrix: 5 families × 40+ filings.
4. **Diagnose test**: Run `--diagnose` on an unmatched filing → output shows exactly which assertions failed per fingerprint, with enough context to author a new family definition.
5. **Infer test**: `fingerprint infer` on 5 Ares sections → generates `.fp.yaml` that matches all 5 and rejects 5 Pennant sections.
6. **Contrastive infer test**: Contrastive infer with negative examples → produces assertions that discriminate between families.
7. **Content hash stability**: Same HTML → same BLAKE3 hash. Edit one table cell → different hash.
8. **Parity test**: Run both Python script and Rust fingerprint on all 40+ filings → same family routing decisions for every filing.
9. **Compile test**: `fingerprint compile bdc-soi-ares.v1.fp.yaml` → produces valid Rust crate that can be `cargo build`-ed and produces identical results to DSL evaluation.
10. **Pipeline integration test**: `echo '{"path":"ares.html","extension":"html","bytes_hash":"sha256:abc"}' | fingerprint --fp bdc-soi.v1 --fp bdc-soi-ares.v1` → valid JSONL with `matched: true`, content hash, and assertion results.

---

## Migration path

1. Build HTML format support and new assertions (steps 1-9)
2. Build infer support (steps 10-11)
3. Use contrastive infer on existing 40+ parsed sections to bootstrap `.fp.yaml` definitions for all 5 families
4. Manually review and tighten generated definitions against known edge cases
5. Run parity test: Python script and fingerprint tool in parallel on all filings
6. Once parity is confirmed, switch tournament router to use `fingerprint` JSONL output
7. Deprecate `fingerprint_schedule_family.py` → move to `legacy/`
