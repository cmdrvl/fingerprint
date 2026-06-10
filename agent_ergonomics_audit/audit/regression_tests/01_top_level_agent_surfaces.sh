#!/usr/bin/env bash
set -euo pipefail

BIN="${FINGERPRINT_BIN:-target/debug/fingerprint}"

"$BIN" --robot-triage | jq -e '.schema_version == "fingerprint.doctor.v1" and .read_only == true' >/dev/null
"$BIN" capabilities --json | jq -e '.schema_version == "fingerprint.doctor.capabilities.v1" and .side_effects.writes_witness_ledger == false' >/dev/null
"$BIN" robot-docs guide | grep -F 'fingerprint capabilities --json' >/dev/null
