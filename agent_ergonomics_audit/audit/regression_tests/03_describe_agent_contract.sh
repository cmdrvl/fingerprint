#!/usr/bin/env bash
set -euo pipefail

BIN="${FINGERPRINT_BIN:-target/debug/fingerprint}"

"$BIN" --describe | jq -e '
  .version == "0.9.0"
  and .invocation.json_flag == "--json"
  and (.capabilities.agent_surfaces | index("fingerprint --robot-triage"))
  and (.capabilities.agent_surfaces | index("fingerprint capabilities --json"))
  and (.capabilities.agent_surfaces | index("fingerprint robot-docs guide"))
' >/dev/null
