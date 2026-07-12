#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
ARTIFACT_ROOT="${ARTIFACT_ROOT:-artifacts/html-e2e}"
LABEL="${LABEL:-page-breaks}"
DEFINITIONS_DIR="${DEFINITIONS_DIR:-${ARTIFACT_ROOT}/region-definitions}"

mkdir -p "${REPO_ROOT}/${DEFINITIONS_DIR}"
cat > "${REPO_ROOT}/${DEFINITIONS_DIR}/selector-pagebreaks.fp.yaml" <<'YAML'
fingerprint_id: selector-pagebreaks.v1
format: html
assertions:
  - node_count:
      selector: '[style*="page-break-after"]'
      min: 60
      max: 80
YAML

exec python3 "${SCRIPT_DIR}/html_e2e.py" selector \
  --artifact-root "${ARTIFACT_ROOT}/region" \
  --label "${LABEL}" \
  --definitions-dir "${DEFINITIONS_DIR}" \
  --fp selector-pagebreaks.v1 \
  --fixture-id oxsq_pagebreaks \
  "$@"
