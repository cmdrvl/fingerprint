#!/usr/bin/env bash
set -euo pipefail

BIN="${FINGERPRINT_BIN:-target/debug/fingerprint}"

set +e
stdout="$("$BIN" doctor --fix 2>/dev/null)"
status=$?
set -e

if [ "$status" -ne 2 ]; then
  echo "expected doctor --fix to exit 2, got $status" >&2
  exit 1
fi
if [ -n "$stdout" ]; then
  echo "expected doctor --fix to leave stdout empty" >&2
  exit 1
fi

set +e
stderr="$("$BIN" doctor --fix 2>&1 >/dev/null)"
set -e

printf '%s\n' "$stderr" | grep -F 'fingerprint doctor --fix is unavailable' >/dev/null
printf '%s\n' "$stderr" | grep -F 'fingerprint --robot-triage' >/dev/null
printf '%s\n' "$stderr" | grep -F 'fingerprint capabilities --json' >/dev/null
