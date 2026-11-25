# Real SEC XML fixtures

Public SEC EDGAR filings, copied byte for byte (nothing trimmed, nothing edited). Retrieved
2026-11-25 from the CMD+RVL archive `s3://edgar-data-full/extracted/20261124/...`, which holds
the documents exactly as filed on EDGAR.

| File | Filing | Accession | Filed | SEC filing index | S3 source | Bytes | sha256 (prefix) |
|------|--------|-----------|-------|------------------|-----------|-------|------------------|
| `nport-p/primary_doc.xml` | Form N-PORT-P, SEASONS SERIES TRUST (CIK 1003239) | 0001410368-26-122177 | 2026-11-24 | https://www.sec.gov/Archives/edgar/data/1003239/000141036826122177/0001410368-26-122177-index.htm | `.../0001410368-26-122177/NPORT-P/primary_doc.xml` | 46,511 | `49353ebaf459d40e` |
| `abs-ee/exh_102.xml` | Form ABS-EE, EX-102 asset data, CD 2017-CD4 Mortgage Trust (CIK 1702745), CMBS | 0001888524-26-020951 | 2026-11-24 | https://www.sec.gov/Archives/edgar/data/1702745/000188852426020951/0001888524-26-020951-index.htm | `.../0001888524-26-020951/EX-102/exh_102.xml` | 297,654 | `94d1f904990d440b` |
| `abs-ee/exh_103.xml` | Same ABS-EE filing, EX-103 asset-related-document comments (`commentData` footnotes) | 0001888524-26-020951 | 2026-11-24 | (same) | `.../0001888524-26-020951/EX-103/exh_103.xml` | 14,306 | `1beb57ebecc872c8` |

(The EX-103 S3 path above follows the layout of the other two and was not separately confirmed;
the file bytes and hash were supplied with the other fixtures.)

Three different shapes on purpose:

- `primary_doc.xml`: nested and namespaced (`edgarSubmission` in `http://www.sec.gov/edgar/nport`,
  `headerData` then `formData`, repeated `invstOrSec`, attributes such as `regStateConditional`).
- `exh_102.xml`: a wide repeated record table (`assetData` with 47 `assets`, each with many fields and nested
  repeated `property` elements), in `http://www.sec.gov/edgar/document/absee/cmbs/assetdata`.
- `exh_103.xml`: flat prose key/value records (`comments` root, no namespace, 32 `commentData` of
  `itemNumber`, `fieldName`, free-text `comment`). The free text is why the no-value-leak tests
  matter most here.

**These bytes are not strictly well-formed XML.** Each file begins with a carriage return before
the `<?xml ...?>` declaration and uses CR-only line endings, as EDGAR serves them. `xmllint` and
Python's `xml.etree` reject them ("XML declaration allowed only at the start of the document").
fingerprint's reader accepts them as-is; `tests/xml_format.rs` pins that.

Expected values used by the tests (element counts, root, namespaces) were computed independently
with Python's `xml.etree` on a copy with the leading carriage return removed.
