# CMBS-WATL Test Case

Test case derived from CredIQ CMBS pipeline (`lambda-osda-s3-to-db`).

## Problem

The OSDA Lambda parser successfully identifies and extracts Watchlist (WATL) data
from CMBS Excel workbooks with varying sheet names and header positions. The
fingerprint DSL cannot express the same detection because:

1. `cell_regex` / `sheet_min_rows` / `range_non_null` require a hardcoded sheet name
2. `sheet_name_regex` proves a matching sheet exists but doesn't capture the name
3. There is no way to search a column range for a pattern (header row varies by servicer)

## Observed Variants

### Sheet Name Variants
| Servicer | Sheet Name |
|----------|-----------|
| BCMS (Barclays) | `Watchlist` |
| BMO | `Watchlist` |
| CGCMT (supp) | `Watchlist` |
| MSC (KeyBank) | `Servicer Watch List` |
| BMO (duplicate) | `Watchlist (2)` |

### Header Row Position
| Format | Header Row | Field ID Row | CREFC Boilerplate |
|--------|-----------|-------------|-------------------|
| BCMS RSRV .xls | Row 11 | Row 9 | Row 2 |
| CGCMT supp .xlsx | Row 4 | Row 3 | Row 1 (title only) |
| MSC RSRV .xls | Row 12 | Row 11 | Row 3 |

### Lambda Detection Chain
1. S3 key matches filepath_pattern: `(?:.*Watch\s?list.*|_WATL)\.(?:csv|txt)$`
2. Excel sheets converted to CSV; generated filename matched against pattern
3. NAME strategy scans first ~30 rows for header row using column `name_variations` regex
4. Requires 30-50% of 22 columns to match (threshold depends on column count)
5. Row-level metadata filtering skips field number rows, CREFC code rows, etc.

### Key WATL Column Patterns (from watl.py)
```
transaction_id:              Transaction ID | Trans? ID | ^L1, S1, D1$ | ^1(\.0)?$
loan_id:                     ^Loan ID | ^L3, S3, D3$ | ^3(\.0)?$
prospectus_loan_id:          Prospectus Loan ID | ^L4, D4, S4$ | ^4(\.0)?$
property_name:               Property Name | ^S55$ | ^5(\.0)?$
comments_servicer_watchlist: Comments - Servicer Watchlist | Comment/Action to be taken | Watchlist Comments | ^19(\.0)?$
```

## DSL Enhancements Required

1. **Sheet name binding** (`sheet_name_regex` captures → downstream assertions reference)
2. **Column search** (search column A, rows 1-N for a regex pattern)
3. **Row search / header detection** (find a row where N cells match column patterns)
