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
      selector: "h1.soi"
  - node_count:
      selector: "table"
      min: 2
extract:
  - name: soi_region
    type: region
    anchor_selector: "h1.soi"
    stop_selector: "h2.notes"
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
  --fixture-id ares_multi_soi \
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

region_payload = (
    region_records[0]
    .get("fingerprint", {})
    .get("extracted", {})
    .get("soi_region")
)
if not isinstance(region_payload, dict):
    raise SystemExit("selector-region match did not emit fingerprint.extracted.soi_region")

regions = region_payload.get("regions")
if not isinstance(regions, list):
    regions = [region_payload]
if len(regions) != 2:
    raise SystemExit(f"expected two selector regions, found {len(regions)}")

required = {
    "anchor_selector",
    "stop_selector",
    "start_line",
    "end_line",
    "table_indices",
    "page_span",
    "byte_offsets",
}
dates = []
byte_offset_presence = []
for index, region in enumerate(regions):
    missing = sorted(required - set(region))
    if missing:
        raise SystemExit(f"region {index} missing required keys: {missing}")
    if region["anchor_selector"] != "h1.soi":
        raise SystemExit(f"region {index} anchor_selector mismatch")
    if region["stop_selector"] != "h2.notes":
        raise SystemExit(f"region {index} stop_selector mismatch")
    if not isinstance(region["start_line"], int) or not isinstance(region["end_line"], int):
        raise SystemExit(f"region {index} start_line/end_line must be integers")
    if region["start_line"] >= region["end_line"]:
        raise SystemExit(f"region {index} start_line must be less than end_line")
    if not all(isinstance(item, int) for item in region["table_indices"]):
        raise SystemExit(f"region {index} table_indices must contain only integers")
    if (
        not isinstance(region["page_span"], list)
        or len(region["page_span"]) != 2
        or not all(isinstance(item, int) for item in region["page_span"])
    ):
        raise SystemExit(f"region {index} page_span must be a two-integer array")
    as_of = region.get("as_of")
    if not isinstance(as_of, str) or len(as_of) != 10:
        raise SystemExit(f"region {index} as_of must be an ISO date string")
    dates.append(as_of)
    byte_offsets = region.get("byte_offsets")
    if byte_offsets is None:
        byte_offset_presence.append(False)
    else:
        if not isinstance(byte_offsets, dict):
            raise SystemExit(f"region {index} byte_offsets must be null or an object")
        start = byte_offsets.get("start")
        end = byte_offsets.get("end")
        if not isinstance(start, int) or not isinstance(end, int) or start > end:
            raise SystemExit(f"region {index} byte_offsets must carry integer start/end")
        byte_offset_presence.append(True)

if sorted(dates) != ["2024-12-31", "2025-09-30"]:
    raise SystemExit(f"unexpected region as_of dates: {dates}")

def is_iso_date(value):
    return (
        isinstance(value, str)
        and len(value) == 10
        and value[4] == "-"
        and value[7] == "-"
        and value[:4].isdigit()
        and value[5:7].isdigit()
        and value[8:].isdigit()
    )

def contains_string(value):
    if isinstance(value, str):
        return value not in {"h1.soi", "h2.notes"} and not is_iso_date(value)
    if isinstance(value, list):
        return any(contains_string(item) for item in value)
    if isinstance(value, dict):
        return any(contains_string(item) for item in value.values())
    return False

for index, region in enumerate(regions):
    if contains_string(region):
        raise SystemExit(f"region {index} output must not contain document-text string values")

summary["region_table_count"] = sum(len(region["table_indices"]) for region in regions)
summary["region_page_spans"] = [region["page_span"] for region in regions]
summary["region_as_of_dates"] = dates
summary["region_byte_offsets_present"] = byte_offset_presence
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(
    f"[selector] region_table_count={summary['region_table_count']} "
    f"as_of_dates={summary['region_as_of_dates']} "
    f"byte_offsets_present={summary['region_byte_offsets_present']}"
)
PY
