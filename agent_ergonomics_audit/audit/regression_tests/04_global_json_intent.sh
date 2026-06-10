#!/usr/bin/env bash
set -euo pipefail

BIN="${FINGERPRINT_BIN:-target/debug/fingerprint}"

"$BIN" --json doctor health | jq -e '.schema_version == "fingerprint.doctor.v1"' >/dev/null
"$BIN" --json --list | grep -F 'csv.v0' >/dev/null
