# Fingerprint DSL authoring reference

This is the author-facing reference for `.fp.yaml` fingerprint definitions. It is
deliberately narrower than `docs/PLAN.md`: use this file when writing a
definition, and use `docs/PLAN.md` when changing tool behavior.

Fingerprint definitions are deterministic. A definition either matches an input
document or it does not; the runtime does not use probabilities, visual guesses,
or LLM classification.

## Definition file anatomy

Every definition is a YAML object:

| Field | Required | Meaning |
|-------|----------|---------|
| `fingerprint_id` | Yes | Stable versioned ID, for example `cbre-appraisal.v1`. |
| `format` | Yes | One of `xlsx`, `csv`, `pdf`, `markdown`, `text`, or `html`. |
| `valid_from` | No | Optional definition validity start date, as a string. |
| `valid_until` | No | Optional definition validity end date, as a string. |
| `parent` | No | Parent fingerprint ID for chained routing. |
| `assertions` | Yes | Ordered deterministic assertions. All must pass for this definition to match. |
| `extract` | No | Bounds/shape extraction rules evaluated only after a match. |
| `content_hash` | No | BLAKE3 hash config over named extract sections. |

Assertions may be explicitly named:

```yaml
assertions:
  - name: title_cell
    cell_eq:
      sheet: "Assumptions"
      cell: "A3"
      value: "Market Leasing Assumptions"
```

If `name` is omitted, fingerprint generates a stable name from the assertion key
and its main target. Names are useful in diagnostics and tests.

## Formats

| Format | Native source | Typical assertions |
|--------|---------------|--------------------|
| `xlsx` | Workbook sheets/cells/ranges. Legacy `.xls` files are also handled through `format: xlsx`. | Spreadsheet assertions and `filename_regex`. |
| `csv` | CSV rows/cells, treated as a virtual sheet. | Spreadsheet assertions and `filename_regex`. |
| `html` | Parsed HTML plus frozen normalized content model. | HTML structural assertions, selector assertions, content assertions, and `filename_regex`. |
| `pdf` | Native PDF metadata/pages from `path`; content assertions from `text_path`. | `page_count`, `metadata_regex`, text/heading/table assertions over `text_path`, and `filename_regex`. |
| `markdown` | Markdown file normalized by the frozen markdown normalizer. | Text/heading/section/table assertions and `filename_regex`. |
| `text` | Plain text file. | Text assertions and `filename_regex`. |

Selector assertions and `region` extracts are `html` only.

## Assertion catalog

The first column below is the exact YAML key. The test suite keeps this catalog
in parity with `fingerprint compile --schema`.

<!-- assertion-catalog:start -->
| Key | Formats | Parameters | Passes when |
|-----|---------|------------|-------------|
| `attr_regex` | `html` only | `selector`, `attr`, `pattern`, optional `min_matches` default `1` | At least `min_matches` selected nodes have attribute text matching `pattern`. |
| `cell_eq` | `xlsx`, `csv` | `sheet`, `cell`, `value` | The addressed cell exactly equals `value`. |
| `cell_regex` | `xlsx`, `csv` | `sheet`, `cell`, `pattern` | The addressed cell matches `pattern`. |
| `column_search` | `xlsx`, `csv` | `sheet`, `column`, `row_range`, `pattern` | A cell in the column/range matches `pattern`. |
| `dominant_column_count` | `html` only | `count`, `tolerance`, optional `sample_pages` | The dominant early HTML table width is within tolerance. |
| `filename_regex` | All formats | `pattern` | The input basename matches `pattern`. |
| `full_width_row` | `html` only | `pattern`, `min_cells` | A full-span/classification row matches `pattern`. |
| `header_row_match` | `xlsx`, `csv` | `sheet`, `row_range`, `min_match`, `columns[].pattern` | A searched row has at least `min_match` header cells matching the column patterns. |
| `header_token_search` | `html` only | `tokens`, `min_matches`, optional `max_matches`, `page`, `index` | Targeted HTML table headers contain enough token regex matches. |
| `heading_exists` | `html`, `markdown`, `pdf` with `text_path` | string | A normalized heading exactly matches the string. |
| `heading_level` | `html`, `markdown`, `pdf` with `text_path` | `level`, `pattern` | A heading at that level matches `pattern`. |
| `heading_regex` | `html`, `markdown`, `pdf` with `text_path` | `pattern` | A normalized heading matches `pattern`. |
| `metadata_regex` | `pdf` only | `key`, `pattern` | A PDF metadata field matches `pattern`. |
| `node_count` | `html` only | `selector`, at least one of `min` or `max` | CSS selector match count is within bounds. |
| `node_exists` | `html` only | `selector` | CSS selector matches at least one DOM node. |
| `node_text_regex` | `html` only | `selector`, `pattern`, optional `min_matches` default `1` | Text of selected DOM nodes matches `pattern` enough times. |
| `page_count` | `pdf` only | at least one of `min` or `max` | PDF page count is within bounds. |
| `page_section_count` | `html` only | at least one of `min` or `max` | Existing HTML page partitions stay within bounds. |
| `range_non_null` | `xlsx`, `csv` | `sheet`, `range` | All cells in the range are populated. |
| `range_populated` | `xlsx`, `csv` | `sheet`, `range`, `min_pct` | Range population ratio is at least `min_pct`. |
| `section_min_lines` | `html`, `markdown`, `pdf` with `text_path` | `heading`, `min_lines` | Section under matching heading has at least `min_lines`. |
| `section_non_empty` | `html`, `markdown`, `pdf` with `text_path` | `heading` | Section under matching heading has content. |
| `sheet_exists` | `xlsx`, `csv` | string | A worksheet or virtual CSV sheet with this name exists. |
| `sheet_min_rows` | `xlsx`, `csv` | `sheet`, `min_rows` | Sheet has at least `min_rows` data rows. |
| `sheet_name_regex` | `xlsx`, `csv` | `pattern`, optional `bind` | A sheet name matches `pattern`; `bind` can be reused in later assertions. |
| `sum_eq` | `xlsx`, `csv` | `range`, `equals_cell`, `tolerance` | Sum of a range equals another cell within tolerance. |
| `table_columns` | `html`, `markdown`, `pdf` with `text_path` | `heading`, `patterns`, optional `index` | Target table columns match the expected header patterns. |
| `table_exists` | `html`, `markdown`, `pdf` with `text_path` | `heading`, optional `index` | A table exists under the matching heading. |
| `table_min_rows` | `html`, `markdown`, `pdf` with `text_path` | `heading`, `min_rows`, optional `index` | Target table has at least `min_rows` data rows. |
| `table_shape` | `html`, `markdown`, `pdf` with `text_path` | `heading`, `min_columns`, `column_types`, optional `index` | Target table has the expected shape and inferred column types. |
| `text_contains` | `html`, `markdown`, `text`, `pdf` with `text_path` | string | Document text contains the string. |
| `text_near` | `html`, `markdown`, `text`, `pdf` with `text_path` | `anchor`, `pattern`, `within_chars` | `pattern` occurs near `anchor`. |
| `text_regex` | `html`, `markdown`, `text`, `pdf` with `text_path` | `pattern` | Document text matches `pattern`. |
| `within_tolerance` | `xlsx`, `csv` | `cell`, `min`, `max` | Numeric cell value is within the inclusive bounds. |
<!-- assertion-catalog:end -->

### Selector assertions

Selector assertions target the real parsed HTML DOM with CSS selectors, in
document order. They do not change the normalized heading/section/table model.
Malformed selectors are definition errors surfaced as `E_INVALID_SELECTOR`.

Use selectors when the input actually contains structural hints such as classes,
attributes, centered/bold title blocks, or page-break markers. Do not ask the
normalizer to infer those cues.

## Extract block

Extract rules run after all assertions pass. They report bounds, shape, or
location metadata for downstream tools. They do not turn a matched document into
a domain schema.

<!-- extract-catalog:start -->
| Type | Formats | Required fields | Output shape |
|------|---------|-----------------|--------------|
| `range` | `xlsx`, `csv` | `name`, `type`, `sheet`, `range` | `{ "range": "A3:D10", "row_count": 8 }` |
| `table` | `html`, `markdown`, `pdf` with `text_path` | `name`, `type`, `anchor_heading` | `{ "start_line": 45, "end_line": 62, "columns": [...], "row_count": 15 }` |
| `section` | `html`, `markdown`, `pdf` with `text_path` | `name`, `type`, `anchor_heading` | `{ "start_line": 30, "end_line": 90, "heading": "Income Capitalization Approach" }` |
| `text_match` | `html`, `markdown`, `text`, `pdf` with `text_path` | `name`, `type`, `anchor`, `pattern`, `within_chars` | `{ "line": 12, "char_offset": 45, "matched": "June 15, 2024" }` |
| `region` | `html` only | `name`, `type`, `anchor_selector`, `stop_selector` | Bounds-only region metadata: `anchor_selector`, `stop_selector`, `start_line`, `end_line`, DOM-order `table_indices`, `page_span`, `byte_offsets`, and `as_of`. Multiple anchors emit `{ "regions": [...] }`. |
<!-- extract-catalog:end -->

Optional fields:

- `index` selects the Nth table under a heading, defaulting to `0`.
- `anchor_text_regex` on `region` optionally filters anchor-selector matches by
  visible node text. This is deterministic and does not add normalizer
  heuristics.
- `stop_text_regex` on `region` optionally filters stop-selector matches by
  visible node text.
- `continue_past` on `region` is a list of regexes for stop candidates such as
  repeated `(continued)` headers that should not end the region.

Region output is intentionally bounds-only. It must not include copied HTML,
table text, holding rows, or other carved document content. Downstream tools own
period selection and slicing.

## Content hashes

`content_hash` currently supports only BLAKE3:

```yaml
content_hash:
  algorithm: blake3
  over: [income_section, rent_roll_table]
```

Each `over` entry must name an `extract` section. Content hashes are computed
only on match. If a document does not match, `content_hash` is `null`.

For PDFs, content hashes over text/heading/table/section extracts are hashes of
the extracted text representation. Changing the upstream extractor or extractor
version can legitimately change the hash even when the original PDF bytes are
unchanged.

## Parent/child chaining

Use `parent` when a broad recognizer should route into mutually exclusive child
families. Run mode evaluates requested fingerprints in CLI order, but child
siblings under a matched parent are treated as alternative routes:

- exactly one matching child: `child_routing.status = "selected"`;
- zero matching children: partial outcome with `no_child_match`;
- multiple matching children: partial outcome with `ambiguous`.

Every evaluated child still appears in the parent payload's `children` array, so
diagnostics remain inspectable.

## PDF authoring under the normalizer freeze

Decision: PDFs use **Option B** from `docs/PLAN.md`.

PDF fingerprints may use native PDF structural assertions:

- `page_count`
- `metadata_regex`

They may also use content assertions and extracts through a caller-supplied
`text_path`, usually markdown emitted by Docling or another upstream extractor:

- `text_contains`, `text_regex`, `text_near`
- `heading_exists`, `heading_regex`, `heading_level`
- `section_non_empty`, `section_min_lines`
- `table_exists`, `table_columns`, `table_shape`, `table_min_rows`
- `section`, `table`, and `text_match` extracts

PDFs do not have an HTML DOM. `node_exists`, `node_count`, `node_text_regex`,
`attr_regex`, and `region` are rejected under `format: pdf` with an html-only
validation message. If a PDF workflow needs Docling JSON reading order, table
geometry, or another richer structure, that should become a new
extractor-targeting surface over the upstream artifact rather than a new
fingerprint normalizer heuristic.

## Refusal and validation reference

<!-- refusal-catalog:start -->
| Code | Mode | Trigger | Fix |
|------|------|---------|-----|
| `E_BAD_INPUT` | Run | Invalid JSONL, missing `bytes_hash`, or unrecognized upstream version. | Run `hash` first and verify JSONL. |
| `E_UNKNOWN_FP` | Run | Requested fingerprint ID is not loaded. | Check `fingerprint --list` and installed definitions. |
| `E_DUPLICATE_FP_ID` | Run | Same fingerprint ID appears from multiple providers. | Remove or rename the duplicate provider. |
| `E_UNTRUSTED_FP` | Run | External provider is not allowlisted. | Add the provider to the trust config or remove it. |
| `E_ORPHAN_CHILD` | Run | Child fingerprint references an unloaded parent. | Request the parent fingerprint before the child. |
| `E_INVALID_SELECTOR` | Run/compile | CSS selector in a definition is malformed. | Fix the selector string. |
| `E_INVALID_YAML` | Compile | YAML parse error or schema/validation failure. | Fix YAML syntax and field shapes. |
| `E_UNKNOWN_ASSERTION` | Compile | Assertion key is not recognized. | Use a supported assertion key from this reference. |
| `E_MISSING_FIELD` | Compile | Required top-level field is absent. | Add the missing field. |
<!-- refusal-catalog:end -->

## Runnable examples

### Spreadsheet example

```yaml fingerprint-example:xlsx
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
extract:
  - name: market_leasing_assumptions
    type: range
    sheet: "Assumptions"
    range: "A3:D10"
content_hash:
  algorithm: blake3
  over: [market_leasing_assumptions]
```

### HTML selector and region example

```yaml fingerprint-example:html
fingerprint_id: soi-schedule.v1
format: html
assertions:
  - node_text_regex:
      selector: "h1, div[style*='text-align:center'], p[style*='text-align:center']"
      pattern: "(?i)schedu\\s*les?\\s+of\\s+investments"
      min_matches: 1
  - node_count:
      selector: "table"
      min: 1
extract:
  - name: schedule_region
    type: region
    anchor_selector: "h1.soi, div[id] + hr + div[style*='min-height']"
    anchor_text_regex: "(?i)schedu\\s*les?\\s+of\\s+investments"
    stop_selector: "h1, h2.notes, div[id] + hr + div[style*='min-height'], hr + div[style*='min-height'] + div"
    stop_text_regex: "(?i)(schedu\\s*les?\\s+of\\s+investments|notes|derivative\\s+instruments|item\\s+[0-9])"
    continue_past:
      - "(?i)(\\(continued\\)|\\bcontinued\\b)"
content_hash:
  algorithm: blake3
  over: [schedule_region]
```

### PDF frozen-path example

```yaml fingerprint-example:pdf
fingerprint_id: cbre-appraisal.v1
format: pdf
assertions:
  - page_count:
      min: 10
      max: 500
  - metadata_regex:
      key: "Creator"
      pattern: "(?i)(pdf|docling|word)"
  - heading_regex:
      pattern: "(?i)income capitali[sz]ation approach"
  - text_near:
      anchor: "(?i)as of"
      pattern: "\\w+ \\d{1,2},? \\d{4}"
      within_chars: 120
extract:
  - name: income_section
    type: section
    anchor_heading: "(?i)income capitali[sz]ation approach"
  - name: as_of_date
    type: text_match
    anchor: "(?i)as of"
    pattern: "\\w+ \\d{1,2},? \\d{4}"
    within_chars: 120
content_hash:
  algorithm: blake3
  over: [income_section]
```
