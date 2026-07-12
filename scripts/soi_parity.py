#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import re
import shlex
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_MANIFEST = REPO_ROOT / "tests" / "fixtures" / "manifests" / "html_corpus.jsonl"
DEFAULT_INVENTORY = REPO_ROOT / "tests" / "fixtures" / "html" / "inventory.json"
DEFAULT_DEFINITIONS = REPO_ROOT / "rules"
DEFAULT_ARTIFACT_ROOT = REPO_ROOT / "artifacts" / "html-e2e"
DEFAULT_FP = "soi-schedule.v1"
COMMAND_TIMEOUT_SECONDS = 300

PARITY_SUMMARY_SCHEMA = "fingerprint.soi-parity.summary.v1"
PARITY_MISMATCH_SCHEMA = "fingerprint.soi-parity.mismatch.v1"
RUN_SUMMARY_SCHEMA = "fingerprint.soi-parity.run-summary.v1"
STDOUT_RECORDS_SCHEMA = "fingerprint.soi-parity.stdout-records.v1"
STDERR_EVENTS_SCHEMA = "fingerprint.soi-parity.stderr-events.v1"
LEGACY_ROUTES_SCHEMA = "fingerprint.soi-parity.legacy-routes.v1"


class HarnessError(RuntimeError):
    pass


def repo_resolve(pathlike: str | Path) -> Path:
    path = Path(pathlike)
    if path.is_absolute():
        return path.resolve()
    return (REPO_ROOT / path).resolve()


def write_json(path: Path, payload: Any) -> None:
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def write_jsonl(path: Path, rows: list[dict[str, Any]]) -> None:
    with path.open("w", encoding="utf-8") as handle:
        for row in rows:
            handle.write(json.dumps(row, sort_keys=True))
            handle.write("\n")


def read_jsonl(path: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    with path.open("r", encoding="utf-8") as handle:
        for line_number, line in enumerate(handle, start=1):
            if not line.strip():
                continue
            try:
                row = json.loads(line)
            except json.JSONDecodeError as error:
                raise HarnessError(f"invalid JSONL at {path}:{line_number}: {error}") from error
            if not isinstance(row, dict):
                raise HarnessError(f"invalid JSONL at {path}:{line_number}: expected object")
            rows.append(row)
    return rows


def parse_jsonl_text(raw: bytes) -> tuple[str, list[dict[str, Any]], list[dict[str, Any]]]:
    text = raw.decode("utf-8", errors="replace")
    parsed: list[dict[str, Any]] = []
    invalid: list[dict[str, Any]] = []
    for line_number, line in enumerate(text.splitlines(), start=1):
        if not line.strip():
            continue
        try:
            value = json.loads(line)
        except json.JSONDecodeError as error:
            invalid.append({"line_number": line_number, "error": str(error), "line": line})
            continue
        if isinstance(value, dict):
            parsed.append(value)
        else:
            invalid.append(
                {
                    "line_number": line_number,
                    "error": "line did not decode to an object",
                    "line": line,
                }
            )
    return text, parsed, invalid


def resolve_fingerprint_binary(explicit: str | None) -> Path:
    candidates: list[Path] = []
    if explicit:
        candidates.append(repo_resolve(explicit))
    env_bin = os.environ.get("FINGERPRINT_BIN")
    if env_bin:
        candidates.append(repo_resolve(env_bin))
    target_bin = REPO_ROOT / "target" / "debug" / "fingerprint"
    if target_bin.exists():
        candidates.append(target_bin.resolve())

    for candidate in candidates:
        if candidate.exists():
            return candidate
    raise HarnessError("could not resolve fingerprint binary; set --fingerprint-bin or FINGERPRINT_BIN")


def load_inventory(path: Path) -> dict[str, dict[str, Any]]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    by_path: dict[str, dict[str, Any]] = {}
    for raw_fixture in payload.get("fixtures", []):
        absolute_path = repo_resolve(raw_fixture["path"])
        by_path[str(absolute_path)] = {
            "id": raw_fixture["id"],
            "path": str(absolute_path),
            "family": raw_fixture.get("family"),
            "categories": list(raw_fixture.get("categories", [])),
        }
    return by_path


def synthetic_fixture(path: str) -> dict[str, Any]:
    return {"id": Path(path).stem, "path": path, "family": None, "categories": []}


def fixture_selected(
    fixture: dict[str, Any],
    fixture_ids: set[str],
    families: set[str],
    categories: set[str],
) -> bool:
    if fixture_ids and fixture["id"] not in fixture_ids:
        return False
    if families and fixture.get("family") not in families:
        return False
    if categories and not categories.intersection(fixture.get("categories", [])):
        return False
    return True


def select_manifest_records(
    manifest_path: Path,
    inventory_by_path: dict[str, dict[str, Any]],
    fixture_ids: set[str],
    families: set[str],
    categories: set[str],
    limit: int | None,
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    records: list[dict[str, Any]] = []
    fixtures: list[dict[str, Any]] = []
    for record in read_jsonl(manifest_path):
        raw_path = record.get("path")
        if not isinstance(raw_path, str):
            raise HarnessError("manifest record missing string path")
        absolute_path = repo_resolve(raw_path)
        fixture = inventory_by_path.get(str(absolute_path), synthetic_fixture(str(absolute_path)))
        if not fixture_selected(fixture, fixture_ids, families, categories):
            continue
        selected = dict(record)
        selected["path"] = str(absolute_path)
        records.append(selected)
        fixtures.append(fixture)
        if limit is not None and len(records) >= limit:
            break
    if not records:
        raise HarnessError("selection produced zero manifest records")
    return records, fixtures


def load_legacy_results(path: Path) -> dict[str, dict[str, Any]]:
    routes: dict[str, dict[str, Any]] = {}
    for row in read_jsonl(path):
        raw_path = row.get("path")
        if not isinstance(raw_path, str):
            raise HarnessError(f"legacy row missing string path in {path}")
        normalized = dict(row)
        normalized["path"] = str(repo_resolve(raw_path))
        normalized["source"] = str(path)
        routes[normalized["path"]] = normalized
    return routes


def build_template_command(template: str, record_path: str) -> list[str]:
    try:
        parts = shlex.split(template)
    except ValueError as error:
        raise HarnessError(f"invalid legacy command template: {error}") from error
    if not parts:
        raise HarnessError("legacy command template produced an empty command")
    if not any("{path}" in part for part in parts):
        raise HarnessError("legacy command template must include a {path} placeholder")
    return [part.replace("{path}", record_path) for part in parts]


def run_legacy_command(template: str, record_path: str) -> dict[str, Any]:
    command = build_template_command(template, record_path)
    try:
        process = subprocess.run(
            command,
            cwd=str(REPO_ROOT),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
            text=True,
            timeout=COMMAND_TIMEOUT_SECONDS,
        )
    except subprocess.TimeoutExpired as error:
        raise HarnessError(
            f"legacy SOI command timed out after {COMMAND_TIMEOUT_SECONDS}s\n"
            f"path: {record_path}\ncommand: {shlex.join(command)}"
        ) from error
    if process.returncode != 0:
        raise HarnessError(
            "legacy SOI command failed\n"
            f"path: {record_path}\ncommand: {shlex.join(command)}\nstatus: {process.returncode}\n"
            f"stdout:\n{process.stdout}\nstderr:\n{process.stderr}"
        )
    try:
        payload = json.loads(process.stdout.strip())
    except json.JSONDecodeError as error:
        raise HarnessError(f"legacy command did not emit JSON for {record_path}: {error}") from error
    if not isinstance(payload, dict):
        raise HarnessError(f"legacy command emitted non-object JSON for {record_path}")
    payload["path"] = record_path
    payload["source"] = "command_template"
    payload["command"] = shlex.join(command)
    return payload


def run_fingerprint(
    binary: Path,
    manifest_path: Path,
    fingerprints: list[str],
    definitions_dir: Path,
) -> tuple[subprocess.CompletedProcess[bytes], int, list[str]]:
    command = [str(binary), str(manifest_path)]
    for fingerprint_id in fingerprints:
        command.extend(["--fp", fingerprint_id])
    command.extend(["--progress", "--no-witness"])

    env = os.environ.copy()
    env["FINGERPRINT_DEFINITIONS"] = str(definitions_dir)
    trust_path: Path | None = None
    with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", suffix=".yaml", delete=False) as handle:
        handle.write('trust:\n  - "installed:*"\n')
        trust_path = Path(handle.name)
    env["FINGERPRINT_TRUST"] = str(trust_path)

    started = time.perf_counter()
    try:
        process = subprocess.run(
            command,
            cwd=str(REPO_ROOT),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=env,
            check=False,
            timeout=COMMAND_TIMEOUT_SECONDS,
        )
    except subprocess.TimeoutExpired as error:
        raise HarnessError(
            f"fingerprint run timed out after {COMMAND_TIMEOUT_SECONDS}s\n"
            f"command: {shlex.join(command)}"
        ) from error
    finally:
        trust_path.unlink(missing_ok=True)
    duration_ms = int((time.perf_counter() - started) * 1000)
    return process, duration_ms, command


def collect_legacy_routes(
    records: list[dict[str, Any]],
    legacy_results: dict[str, dict[str, Any]] | None,
    legacy_command_template: str | None,
) -> list[dict[str, Any]]:
    routes: list[dict[str, Any]] = []
    for record in records:
        record_path = record["path"]
        if legacy_results is not None:
            route = legacy_results.get(record_path)
            if route is None:
                raise HarnessError(f"legacy results did not include {record_path}")
            routes.append(route)
            continue
        if legacy_command_template is None:
            raise HarnessError("either --legacy-results or --legacy-command-template is required")
        routes.append(run_legacy_command(legacy_command_template, record_path))
    return routes


def extracted_regions(record: dict[str, Any]) -> list[dict[str, Any]]:
    fingerprint = record.get("fingerprint")
    if not isinstance(fingerprint, dict) or fingerprint.get("matched") is not True:
        return []
    extracted = fingerprint.get("extracted")
    if not isinstance(extracted, dict):
        return []
    payload = extracted.get("schedule_region")
    if not isinstance(payload, dict):
        return []
    regions = payload.get("regions")
    if isinstance(regions, list):
        return [region for region in regions if isinstance(region, dict)]
    return [payload]


def select_period_region(regions: list[dict[str, Any]], period_end: Any) -> dict[str, Any] | None:
    if isinstance(period_end, str) and period_end:
        for region in regions:
            if region.get("as_of") == period_end:
                return region
    return regions[0] if regions else None


def count_holding_rows(path: str, byte_offsets: Any, table_count: int) -> int | None:
    if not isinstance(byte_offsets, dict):
        return None
    start = byte_offsets.get("start")
    end = byte_offsets.get("end")
    if not isinstance(start, int) or not isinstance(end, int) or start > end:
        return None
    raw = Path(path).read_text(encoding="utf-8", errors="replace")
    chunk = raw[start:end]
    tr_count = len(re.findall(r"<tr\b", chunk, flags=re.IGNORECASE))
    return max(0, tr_count - table_count)


def observed_metrics(record: dict[str, Any], legacy: dict[str, Any]) -> dict[str, Any]:
    regions = extracted_regions(record)
    selected = select_period_region(regions, legacy.get("period_end") or legacy.get("as_of"))
    if selected is None:
        return {
            "matched": False,
            "region_count": len(regions),
            "selected_as_of": None,
            "table_count": 0,
            "page_span": None,
            "holding_row_count": None,
            "byte_offsets_present": False,
        }
    table_indices = selected.get("table_indices")
    table_count = len(table_indices) if isinstance(table_indices, list) else 0
    byte_offsets = selected.get("byte_offsets")
    return {
        "matched": True,
        "region_count": len(regions),
        "selected_as_of": selected.get("as_of"),
        "table_count": table_count,
        "page_span": selected.get("page_span"),
        "holding_row_count": count_holding_rows(record["path"], byte_offsets, table_count),
        "byte_offsets_present": isinstance(byte_offsets, dict),
    }


def compare_metrics(
    observed: dict[str, Any],
    legacy: dict[str, Any],
    tolerance: int,
) -> list[dict[str, Any]]:
    comparisons: list[tuple[str, str, bool]] = [
        ("table_count", "table_count", False),
        ("page_span", "page_span", False),
        ("holding_row_count", "holding_row_count", True),
        ("selected_as_of", "period_end", False),
    ]
    mismatches: list[dict[str, Any]] = []
    for observed_key, legacy_key, numeric_tolerance in comparisons:
        if legacy_key not in legacy:
            continue
        expected = legacy.get(legacy_key)
        actual = observed.get(observed_key)
        if numeric_tolerance and isinstance(actual, int) and isinstance(expected, int):
            matched = abs(actual - expected) <= tolerance
        else:
            matched = actual == expected
        if not matched:
            mismatches.append(
                {
                    "field": observed_key,
                    "expected": expected,
                    "observed": actual,
                    "tolerance": tolerance if numeric_tolerance else 0,
                }
            )
    return mismatches


def build_parity_rows(
    records: list[dict[str, Any]],
    fixtures: list[dict[str, Any]],
    legacy_routes: list[dict[str, Any]],
    tolerance: int,
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    rows: list[dict[str, Any]] = []
    mismatches: list[dict[str, Any]] = []
    for record, fixture, legacy in zip(records, fixtures, legacy_routes):
        observed = observed_metrics(record, legacy)
        metric_mismatches = compare_metrics(observed, legacy, tolerance)
        if not observed["matched"]:
            metric_mismatches.append(
                {"field": "matched", "expected": True, "observed": False, "tolerance": 0}
            )
        row = {
            "fixture_id": fixture["id"],
            "family": fixture.get("family"),
            "path": record["path"],
            "legacy": {
                key: legacy.get(key)
                for key in ("period_end", "table_count", "page_span", "holding_row_count")
                if key in legacy
            },
            "observed": observed,
            "matched": not metric_mismatches,
        }
        rows.append(row)
        if metric_mismatches:
            mismatches.append(
                {
                    "schema": PARITY_MISMATCH_SCHEMA,
                    "fixture_id": fixture["id"],
                    "family": fixture.get("family"),
                    "path": record["path"],
                    "mismatches": metric_mismatches,
                    "legacy": row["legacy"],
                    "observed": observed,
                }
            )
    return rows, mismatches


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Check SOI region parity against legacy metrics")
    parser.add_argument("--definitions-dir", default=str(DEFAULT_DEFINITIONS))
    parser.add_argument("--manifest", default=str(DEFAULT_MANIFEST))
    parser.add_argument("--inventory", default=str(DEFAULT_INVENTORY))
    parser.add_argument("--artifact-root", default=str(DEFAULT_ARTIFACT_ROOT))
    parser.add_argument("--label", default="soi-parity")
    parser.add_argument("--fingerprint-bin")
    parser.add_argument("--legacy-results")
    parser.add_argument("--legacy-command-template")
    parser.add_argument("--tolerance", type=int, default=0)
    parser.add_argument("--limit", type=int)
    parser.add_argument("--fixture-id", action="append", default=[])
    parser.add_argument("--family", action="append", default=[])
    parser.add_argument("--category", action="append", default=[])
    parser.add_argument("--fp", action="append", default=[DEFAULT_FP])
    return parser


def main(argv: list[str]) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)

    artifact_root = repo_resolve(args.artifact_root)
    artifact_dir = artifact_root / "parity" / args.label
    artifact_dir.mkdir(parents=True, exist_ok=True)

    fingerprint_binary = resolve_fingerprint_binary(args.fingerprint_bin)
    definitions_dir = repo_resolve(args.definitions_dir)
    manifest_path = repo_resolve(args.manifest)
    inventory_path = repo_resolve(args.inventory)
    inventory = load_inventory(inventory_path)

    selected_records, selected_fixtures = select_manifest_records(
        manifest_path,
        inventory,
        set(args.fixture_id),
        set(args.family),
        set(args.category),
        args.limit,
    )
    selected_manifest = artifact_dir / "selected.manifest.jsonl"
    write_jsonl(selected_manifest, selected_records)

    legacy_results = load_legacy_results(repo_resolve(args.legacy_results)) if args.legacy_results else None
    legacy_routes = collect_legacy_routes(selected_records, legacy_results, args.legacy_command_template)
    write_jsonl(
        artifact_dir / "legacy.routes.jsonl",
        [{"schema": LEGACY_ROUTES_SCHEMA, **route} for route in legacy_routes],
    )

    process, duration_ms, command = run_fingerprint(
        fingerprint_binary,
        selected_manifest,
        args.fp,
        definitions_dir,
    )
    stdout_text, stdout_records, stdout_invalid = parse_jsonl_text(process.stdout)
    stderr_text, stderr_events, stderr_invalid = parse_jsonl_text(process.stderr)
    write_json(
        artifact_dir / "stdout.records.json",
        {
            "schema": STDOUT_RECORDS_SCHEMA,
            "records": stdout_records,
            "invalid_lines": stdout_invalid,
            "raw_text": stdout_text,
        },
    )
    write_json(
        artifact_dir / "stderr.events.json",
        {
            "schema": STDERR_EVENTS_SCHEMA,
            "events": stderr_events,
            "invalid_lines": stderr_invalid,
            "raw_text": stderr_text,
        },
    )

    if process.returncode not in (0, 1):
        raise HarnessError(
            "fingerprint run failed unexpectedly\n"
            f"status: {process.returncode}\nstdout:\n{stdout_text}\nstderr:\n{stderr_text}"
        )
    if len(stdout_records) != len(selected_records):
        raise HarnessError(
            f"fingerprint emitted {len(stdout_records)} records for {len(selected_records)} inputs"
        )

    parity_rows, mismatches = build_parity_rows(
        stdout_records,
        selected_fixtures,
        legacy_routes,
        args.tolerance,
    )
    write_jsonl(artifact_dir / "parity.mismatches.jsonl", mismatches)

    summary = {
        "schema": PARITY_SUMMARY_SCHEMA,
        "label": args.label,
        "fingerprints": args.fp,
        "selected_count": len(selected_records),
        "region_found_count": sum(1 for row in parity_rows if row["observed"]["matched"]),
        "parity_match_count": sum(1 for row in parity_rows if row["matched"]),
        "mismatch_count": len(mismatches),
        "tolerance": args.tolerance,
        "rows": parity_rows,
    }
    write_json(artifact_dir / "parity.summary.json", summary)
    write_json(
        artifact_dir / "run.summary.json",
        {
            "schema": RUN_SUMMARY_SCHEMA,
            "label": args.label,
            "exit_code": 0 if not mismatches else 1,
            "duration_ms": duration_ms,
            "command": command,
            "artifact_dir": str(artifact_dir),
            "selected_count": len(selected_records),
            "mismatch_count": len(mismatches),
        },
    )

    print(
        f"[soi-parity] label={args.label} selected={len(selected_records)} "
        f"matched={summary['parity_match_count']} mismatches={len(mismatches)} "
        f"artifact_dir={artifact_dir}"
    )
    return 0 if not mismatches else 1


if __name__ == "__main__":
    try:
        raise SystemExit(main(sys.argv[1:]))
    except HarnessError as error:
        print(f"soi_parity: {error}", file=sys.stderr)
        raise SystemExit(2)
