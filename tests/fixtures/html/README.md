# HTML Fixture Corpus

Shared HTML corpus for the native `format: html` rollout.

This directory is the canonical source for reusable HTML scenarios. If a later
bead needs a new HTML shape, add it here, update [inventory.json](./inventory.json),
and update [../manifests/html_corpus.jsonl](../manifests/html_corpus.jsonl) in the
same change.

Seeded by `bd-1mp`. Follow-on gaps should be filled by the bead that needs them
instead of creating ad hoc temp HTML inside tests.

## Inventory files

- `inventory.json` — machine-readable fixture inventory with expected parser shape
  and canonical content-hash mutation pairs.
- `../manifests/html_corpus.jsonl` — deterministic hash-enriched manifest references
  for every HTML fixture in this corpus.

## Shared fixtures

- `generic_page_sections_schedule.html` — generic multi-page schedule fixture with
  `<section data-page-number>` coverage.
- `span_edge_cases.html` — focused `colspan` and `rowspan` parser edge-case fixture.
- `malformed_static_schedule.html` — malformed-but-static HTML that must remain
  parseable and non-panicking.
- `ambiguity_trap_dual_headers.html` — overlapping table-header tokens to test
  ambiguity handling and future router discrimination.
- `minimal_empty_shell.html` — minimal HTML shell with no headings or tables for
  negative/refusal-path assertions.

## Content-hash mutation set

- `hash_pair_base.html` — baseline extracted schedule fixture.
- `hash_pair_markup_variant.html` — markup-only variant of the baseline; extracted
  content should stay hash-stable.
- `hash_pair_value_change.html` — value-changing variant of the baseline; extracted
  content hash must change.

## BDC family examples

- `bdc_soi_ares_like.html` — wide layout with business-description and coupon-style
  headers plus full-width industry rows.
- `bdc_soi_bxsl_like.html` — investment-type section headings with industry and
  fair-value columns.
- `bdc_soi_pennant_like.html` — explicit `Industry` column with asset-class
  full-width rows and no business-description header.
- `bdc_soi_golub_like.html` — compact layout with industry full-width rows and no
  explicit asset-class column.
- `bdc_soi_blackrock_like.html` — issuer/instrument leading columns with
  coupon-oriented credit layout.
