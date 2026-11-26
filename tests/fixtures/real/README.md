# Real-data materialize fixtures (bd-uai)

These are real SEC bytes, not synthetic stand-ins.

- `nucor_2024q2_financial_report.xlsx` is the SEC-generated `Financial_Report.xlsx` for Nucor Corporation (CIK `000073309`), Form 10-Q accession `0000950170-24-092479`, filed 2024-08-07: [SEC directory](https://www.sec.gov/Archives/edgar/data/73309/000095017024092479/). It is a real 60-sheet, multi-sheet financial workbook with merged headers and disclosure/footnote sheets, but it is **not a CMBS servicer watchlist**; it is the operator-approved fallback after no public CREFC watchlist workbook was found. The committed file is the exact downloaded `Financial_Report.xlsx`, SHA-256 `0a04a8452a058e6fedfd773bec207bf318d3ca8930b3f645e4e72932de641bd9` (85,173 bytes). `nucor_financial.v1.fp.yaml` binds the real `Inventories - Additional Information` sheet and projects `A1:C4`, retaining the filing's title/date columns, abstract row, and two reported percentages.
- `crescent_2017q1_soi_excerpt.html` is a byte excerpt from Crescent Capital BDC, Inc.'s real Form 10-Q (CIK `0001633336`), accession `0001193125-17-168696`, filed 2017-05-11: [SEC HTML](https://www.sec.gov/Archives/edgar/data/1633336/000119312517168696/d396103d10q.htm). The full downloaded source is 1,413,427 bytes with SHA-256 `f7fb8e34642137a7a49eb04522bc79a02d47456ccbf496e996b6a7f3ac04c762`; the committed excerpt is SHA-256 `05ff7083903dfd78de681aabd1dc4f356ee3fc86fe395ce46f3f31e46559724c` (7,131 bytes). Exact excerpt method: preserve the source bytes from the opening `<P>` containing `Investments at fair value consisted of the following at March 31, 2017` through the end of the first following `</TABLE>`, then wrap those exact bytes in literal `<html><body>` and `</body></html>` bytes; no HTML normalization or rewriting was applied. This real BDC table is a summary of investments at fair value by type, not a loan-level Schedule of Investments. `bdc_soi_real.v1.fp.yaml` uses the existing region extractor over that real table page.

The full SEC source hashes are recorded for provenance; only the bounded HTML table page is committed to keep the fixture small. The XLSX is small enough to retain byte-for-byte because its source is only 85,173 bytes.

## Sibling-binary E2E

`../../scripts/e2e_real_data.sh` is intentionally not a cargo test. It requires the released
Intel macOS binaries for fingerprint 0.13.0, shape 0.10.0, rvl 0.9.0, and verify 0.7.0 through
`RELEASED_FINGERPRINT_BIN`, `SHAPE_BIN`, `RVL_BIN`, and `VERIFY_BIN`; `FINGERPRINT_BIN` defaults
to this checkout's `target/debug/fingerprint`. It materializes the real inputs, compares the
release/current XLSX CSV bytes, repeats the real HTML materialization, then runs shape, rvl, and
verify over the materialized CSV. Set `E2E_WORK_DIR` to a fresh directory for each run.
