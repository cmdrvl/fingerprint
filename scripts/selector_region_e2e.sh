#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
ARTIFACT_ROOT="${ARTIFACT_ROOT:-artifacts/html-e2e}"
LABEL="${LABEL:-region}"
DEFINITIONS_DIR="${DEFINITIONS_DIR:-${ARTIFACT_ROOT}/region-definitions}"

mkdir -p "${REPO_ROOT}/${DEFINITIONS_DIR}"
cat > "${REPO_ROOT}/${DEFINITIONS_DIR}/selector-region.fp.yaml" <<'YAML'
fingerprint_id: selector-region.v1
format: html
assertions:
  - node_exists:
      selector: "h1"
  - node_count:
      selector: "table"
      min: 1
extract:
  - name: soi_region
    type: region
    anchor_selector: "h1"
    stop_selector: "h2"
YAML

cat > "${REPO_ROOT}/${DEFINITIONS_DIR}/selector-pagebreaks.fp.yaml" <<'YAML'
fingerprint_id: selector-pagebreaks.v1
format: html
assertions:
  - node_count:
      selector: '[style*="page-break-after"]'
      min: 60
      max: 80
YAML

python3 "${SCRIPT_DIR}/html_e2e.py" selector \
  --artifact-root "${ARTIFACT_ROOT}/region" \
  --label "${LABEL}" \
  --definitions-dir "${DEFINITIONS_DIR}" \
  --fp selector-region.v1 \
  --fp selector-pagebreaks.v1 \
  --fixture-id bdc_soi_ares_like \
  --fixture-id oxsq_pagebreaks \
  "$@"

ARTIFACT_DIR="${REPO_ROOT}/${ARTIFACT_ROOT}/region/selector/${LABEL}"
python3 - <<'PY' "${ARTIFACT_DIR}"
import json
import sys
from pathlib import Path

artifact_dir = Path(sys.argv[1])
records_path = artifact_dir / "stdout.records.json"
summary_path = artifact_dir / "run.summary.json"
records = json.loads(records_path.read_text(encoding="utf-8"))["records"]
summary = json.loads(summary_path.read_text(encoding="utf-8"))

if summary.get("exit_code") != 0:
    raise SystemExit(f"selector region e2e failed: exit_code={summary.get('exit_code')}")

region_records = [
    record
    for record in records
    if record.get("fingerprint", {}).get("fingerprint_id") == "selector-region.v1"
]
if len(region_records) != 1:
    raise SystemExit(f"expected one selector-region match, found {len(region_records)}")

region = (
    region_records[0]
    .get("fingerprint", {})
    .get("extracted", {})
    .get("soi_region")
)
if not isinstance(region, dict):
    raise SystemExit("selector-region match did not emit fingerprint.extracted.soi_region")

required = {"start_line", "end_line", "table_indices", "page_span"}
missing = sorted(required - set(region))
if missing:
    raise SystemExit(f"region missing required keys: {missing}")
if not isinstance(region["start_line"], int) or not isinstance(region["end_line"], int):
    raise SystemExit("region start_line/end_line must be integers")
if region["start_line"] >= region["end_line"]:
    raise SystemExit("region start_line must be less than end_line")
if not all(isinstance(item, int) for item in region["table_indices"]):
    raise SystemExit("region table_indices must contain only integers")
if (
    not isinstance(region["page_span"], list)
    or len(region["page_span"]) != 2
    or not all(isinstance(item, int) for item in region["page_span"])
):
    raise SystemExit("region page_span must be a two-integer array")

def contains_string(value):
    if isinstance(value, str):
        return True
    if isinstance(value, list):
        return any(contains_string(item) for item in value)
    if isinstance(value, dict):
        return any(contains_string(item) for item in value.values())
    return False

if contains_string(region):
    raise SystemExit("region output must not contain string values")

summary["region_table_count"] = len(region["table_indices"])
summary["region_page_span"] = region["page_span"]
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(
    f"[selector] region_table_count={summary['region_table_count']} "
    f"page_span={summary['region_page_span']}"
)
PY
