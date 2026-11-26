#!/usr/bin/env bash
set -euo pipefail

# This is intentionally outside cargo test: it exercises sibling release binaries.
root_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
fingerprint_bin=${FINGERPRINT_BIN:-"$root_dir/target/debug/fingerprint"}
released_fingerprint_bin=${RELEASED_FINGERPRINT_BIN:?set RELEASED_FINGERPRINT_BIN to fingerprint v0.13.0}
shape_bin=${SHAPE_BIN:?set SHAPE_BIN to released shape v0.10.0}
rvl_bin=${RVL_BIN:?set RVL_BIN to released rvl v0.9.0}
verify_bin=${VERIFY_BIN:?set VERIFY_BIN to released verify v0.7.0}
definitions_dir=${FINGERPRINT_DEFINITIONS:-"$root_dir/tests/fixtures/real"}
work_dir=${E2E_WORK_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/fingerprint-e2e.XXXXXX")}

mkdir -p "$work_dir/released" "$work_dir/current"

materialize() {
  local binary=$1
  local out=$2
  shift 2
  FINGERPRINT_DEFINITIONS="$definitions_dir" "$binary" materialize "$@" --out "$out" --no-witness
}

detail_input="$root_dir/tests/fixtures/real/nucor_2024q2_financial_report.xlsx"
bdc_input="$root_dir/tests/fixtures/real/crescent_2017q1_soi_excerpt.html"

materialize "$released_fingerprint_bin" "$work_dir/released/detail.csv" \
  "$detail_input" --fp nucor-financial.v1 --extract inventory_detail
materialize "$fingerprint_bin" "$work_dir/current/detail.csv" \
  "$detail_input" --fp nucor-financial.v1 --extract inventory_detail
cmp "$work_dir/released/detail.csv" "$work_dir/current/detail.csv"
echo "fingerprint XLSX release/current CSV parity: PASS"

materialize "$released_fingerprint_bin" "$work_dir/released/labels.csv" \
  "$detail_input" --fp nucor-financial.v1 --extract inventory_labels
materialize "$fingerprint_bin" "$work_dir/current/labels.csv" \
  "$detail_input" --fp nucor-financial.v1 --extract inventory_labels
cmp "$work_dir/released/labels.csv" "$work_dir/current/labels.csv"
echo "fingerprint XLSX labels release/current CSV parity: PASS"

materialize "$fingerprint_bin" "$work_dir/current/bdc.csv" \
  "$bdc_input" --fp bdc-soi-real.v1 --extract schedule_region
materialize "$fingerprint_bin" "$work_dir/current/bdc-repeat.csv" \
  "$bdc_input" --fp bdc-soi-real.v1 --extract schedule_region
cmp "$work_dir/current/bdc.csv" "$work_dir/current/bdc-repeat.csv"
echo "fingerprint real BDC HTML repeatability: PASS"

set +e
FINGERPRINT_DEFINITIONS="$definitions_dir" "$released_fingerprint_bin" materialize \
  "$bdc_input" --fp bdc-soi-real.v1 --extract schedule_region \
  --out "$work_dir/released/bdc.csv" --no-witness >"$work_dir/released/bdc.stdout" 2>"$work_dir/released/bdc.stderr"
bdc_oracle_rc=$?
set -e
if [[ "$bdc_oracle_rc" != 2 ]] || ! grep -q '"code":"E_PROJECTION"' "$work_dir/released/bdc.stdout"; then
  echo "released fingerprint BDC oracle changed unexpectedly" >&2
  cat "$work_dir/released/bdc.stdout" >&2
  cat "$work_dir/released/bdc.stderr" >&2
  exit 1
fi
echo "fingerprint v0.13.0 real BDC oracle: expected E_PROJECTION (current code handles SEC layout)"

shape_json=$(
  "$shape_bin" "$work_dir/current/labels.csv" "$work_dir/current/labels.csv" \
    --json --no-witness --delimiter comma
)
grep -q '"outcome":"COMPATIBLE"' <<<"$shape_json"
echo "shape v0.10.0: exit 0 COMPATIBLE on real materialized CSV"

rvl_json=$(
  "$rvl_bin" "$work_dir/current/labels.csv" "$work_dir/current/labels.csv" \
    --json --no-witness --non-numeric --delimiter comma
)
grep -q '"outcome":"NO_REAL_CHANGE"' <<<"$rvl_json"
echo "rvl v0.9.0: exit 0 NO_REAL_CHANGE on real materialized CSV"

verify_json=$(
  "$verify_bin" run "$root_dir/scripts/e2e_real_data.verify.json" \
    --bind "input=$work_dir/current/labels.csv" --json --no-witness
)
grep -q '"outcome": "PASS"' <<<"$verify_json"
echo "verify v0.7.0: exit 0 PASS for not_null + unique"

python3 - "$work_dir/current/detail.csv.provenance.json" "$work_dir/current/bdc.csv.provenance.json" <<'PY'
import json
import sys
for name in sys.argv[1:]:
    with open(name, encoding="utf-8") as handle:
        value = json.load(handle)
    assert value["source"]["bytes_hash"].startswith("blake3:"), name
    assert value["content_hash"].startswith("blake3:"), name
print("provenance source.bytes_hash/content_hash: PASS")
PY
