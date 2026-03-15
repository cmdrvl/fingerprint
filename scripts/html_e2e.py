#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_MANIFEST = REPO_ROOT / "tests" / "fixtures" / "manifests" / "html_corpus.jsonl"
DEFAULT_INVENTORY = REPO_ROOT / "tests" / "fixtures" / "html" / "inventory.json"
DEFAULT_ARTIFACT_ROOT = REPO_ROOT / "artifacts" / "html-e2e"

RUN_SUMMARY_SCHEMA = "fingerprint.html-e2e.run-summary.v1"
STDOUT_RECORDS_SCHEMA = "fingerprint.html-e2e.stdout-records.v1"
STDERR_EVENTS_SCHEMA = "fingerprint.html-e2e.stderr-events.v1"
DIAGNOSTICS_SCHEMA = "fingerprint.html-e2e.diagnostics.v1"
FAMILY_SUMMARY_SCHEMA = "fingerprint.html-e2e.family-summary.v1"


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
        for line in handle:
            if not line.strip():
                continue
            rows.append(json.loads(line))
    return rows


def parse_jsonl_text(raw: bytes) -> tuple[str, list[dict[str, Any]], list[dict[str, Any]]]:
    text = raw.decode("utf-8", errors="replace")
    parsed: list[dict[str, Any]] = []
    invalid: list[dict[str, Any]] = []
    for index, line in enumerate(text.splitlines(), start=1):
        if not line.strip():
            continue
        try:
            value = json.loads(line)
        except json.JSONDecodeError as error:
            invalid.append(
                {
                    "line_number": index,
                    "error": str(error),
                    "line": line,
                }
            )
            continue
        if isinstance(value, dict):
            parsed.append(value)
        else:
            invalid.append(
                {
                    "line_number": index,
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

    raise HarnessError(
        "could not resolve fingerprint binary; set --fingerprint-bin or FINGERPRINT_BIN"
    )


def load_inventory(path: Path) -> tuple[dict[str, Any], dict[str, dict[str, Any]]]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    fixtures: list[dict[str, Any]] = []
    by_abs_path: dict[str, dict[str, Any]] = {}

    for raw_fixture in payload.get("fixtures", []):
        absolute_path = repo_resolve(raw_fixture["path"])
        fixture = {
            "id": raw_fixture["id"],
            "path": str(absolute_path),
            "path_relative": raw_fixture["path"],
            "family": raw_fixture.get("family"),
            "categories": list(raw_fixture.get("categories", [])),
            "expected_headings": raw_fixture.get("expected_headings"),
            "expected_tables": raw_fixture.get("expected_tables"),
            "expected_pages": raw_fixture.get("expected_pages"),
        }
        fixtures.append(fixture)
        by_abs_path[fixture["path"]] = fixture

    inventory_payload = {
        "schema_version": payload.get("schema_version"),
        "fixtures": fixtures,
        "hash_pairs": payload.get("hash_pairs", []),
    }
    return inventory_payload, by_abs_path


def synthetic_fixture(path: str) -> dict[str, Any]:
    file_path = Path(path)
    return {
        "id": file_path.stem,
        "path": path,
        "path_relative": path,
        "family": None,
        "categories": [],
        "expected_headings": None,
        "expected_tables": None,
        "expected_pages": None,
    }


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
    inventory_by_abs_path: dict[str, dict[str, Any]],
    fixture_ids: set[str],
    families: set[str],
    categories: set[str],
    limit: int | None,
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    selected_records: list[dict[str, Any]] = []
    selected_fixtures: list[dict[str, Any]] = []

    for record in read_jsonl(manifest_path):
        absolute_path = repo_resolve(record["path"])
        fixture = inventory_by_abs_path.get(str(absolute_path), synthetic_fixture(str(absolute_path)))
        if not fixture_selected(fixture, fixture_ids, families, categories):
            continue

        selected_record = dict(record)
        selected_record["path"] = str(absolute_path)
        selected_records.append(selected_record)
        selected_fixtures.append(fixture)

        if limit is not None and len(selected_records) >= limit:
            break

    if not selected_records:
        raise HarnessError("selection produced zero manifest records")

    return selected_records, selected_fixtures


def run_fingerprint(
    binary: Path,
    manifest_path: Path,
    fingerprints: list[str],
    definitions_dir: Path | None,
    diagnose: bool,
    with_witness: bool,
) -> tuple[subprocess.CompletedProcess[bytes], int, list[str]]:
    command = [str(binary), str(manifest_path)]
    for fingerprint_id in fingerprints:
        command.extend(["--fp", fingerprint_id])
    command.append("--progress")
    if diagnose:
        command.append("--diagnose")
    if not with_witness:
        command.append("--no-witness")

    env = os.environ.copy()
    trust_path: Path | None = None
    if definitions_dir is not None:
        env["FINGERPRINT_DEFINITIONS"] = str(definitions_dir)
        with tempfile.NamedTemporaryFile(
            mode="w",
            encoding="utf-8",
            suffix=".yaml",
            delete=False,
        ) as handle:
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
        )
    finally:
        if trust_path is not None:
            trust_path.unlink(missing_ok=True)
    duration_ms = int((time.perf_counter() - started) * 1000)
    return process, duration_ms, command


def build_fixture_rows(
    stdout_records: list[dict[str, Any]],
    inventory_by_abs_path: dict[str, dict[str, Any]],
) -> tuple[list[dict[str, Any]], list[dict[str, Any]], list[str]]:
    fixture_rows: list[dict[str, Any]] = []
    diagnostic_rows: list[dict[str, Any]] = []
    refusal_codes: list[str] = []

    for record in stdout_records:
        if record.get("outcome") == "REFUSAL":
            refusal_code = record.get("refusal", {}).get("code")
            if isinstance(refusal_code, str):
                refusal_codes.append(refusal_code)
            fixture_rows.append(
                {
                    "fixture_id": None,
                    "family": None,
                    "categories": [],
                    "path": None,
                    "matched": False,
                    "route_resolved": False,
                    "skipped": False,
                    "fingerprint_id": None,
                    "resolved_fingerprint_id": None,
                    "selected_child_fingerprint_id": None,
                    "matched_child_fingerprint_ids": [],
                    "matched_child_count": 0,
                    "child_routing_status": None,
                    "reason": None,
                    "content_hash": None,
                    "diagnostics_present": False,
                    "attempted_fingerprint_ids": [],
                    "refusal_code": refusal_code,
                }
            )
            continue

        absolute_path = str(repo_resolve(record["path"]))
        fixture = inventory_by_abs_path.get(absolute_path, synthetic_fixture(absolute_path))
        fingerprint = record.get("fingerprint")
        matched = bool(isinstance(fingerprint, dict) and fingerprint.get("matched") is True)
        child_routing = fingerprint.get("child_routing") if isinstance(fingerprint, dict) else None
        selected_child_fingerprint_id = None
        matched_child_fingerprint_ids: list[str] = []
        matched_child_count = 0
        child_routing_status = None
        route_resolved = matched
        resolved_fingerprint_id = fingerprint.get("fingerprint_id") if isinstance(fingerprint, dict) else None

        if isinstance(child_routing, dict):
            child_routing_status = child_routing.get("status")
            selected_child_fingerprint_id = child_routing.get("selected_child_fingerprint_id")
            matched_child_fingerprint_ids = [
                child_id
                for child_id in child_routing.get("matched_child_fingerprint_ids", [])
                if isinstance(child_id, str)
            ]
            raw_matched_child_count = child_routing.get("matched_child_count")
            matched_child_count = (
                int(raw_matched_child_count)
                if isinstance(raw_matched_child_count, int)
                else len(matched_child_fingerprint_ids)
            )
            route_resolved = matched and isinstance(selected_child_fingerprint_id, str)
            resolved_fingerprint_id = (
                selected_child_fingerprint_id if route_resolved else None
            )

        skipped = bool(record.get("_skipped", False))
        reason = fingerprint.get("reason") if isinstance(fingerprint, dict) else None
        diagnostics = fingerprint.get("diagnostics") if isinstance(fingerprint, dict) else None
        attempted_ids = []
        if isinstance(diagnostics, dict):
            for attempt in diagnostics.get("attempts", []):
                if isinstance(attempt, dict):
                    fingerprint_id = attempt.get("fingerprint_id")
                    if isinstance(fingerprint_id, str):
                        attempted_ids.append(fingerprint_id)

        row = {
            "fixture_id": fixture["id"],
            "family": fixture.get("family"),
            "categories": fixture.get("categories", []),
            "path": absolute_path,
            "matched": matched,
            "route_resolved": route_resolved,
            "skipped": skipped,
            "fingerprint_id": fingerprint.get("fingerprint_id") if isinstance(fingerprint, dict) else None,
            "resolved_fingerprint_id": resolved_fingerprint_id,
            "selected_child_fingerprint_id": selected_child_fingerprint_id,
            "matched_child_fingerprint_ids": matched_child_fingerprint_ids,
            "matched_child_count": matched_child_count,
            "child_routing_status": child_routing_status,
            "reason": reason,
            "content_hash": fingerprint.get("content_hash") if isinstance(fingerprint, dict) else None,
            "diagnostics_present": diagnostics is not None,
            "attempted_fingerprint_ids": attempted_ids,
            "refusal_code": None,
        }
        fixture_rows.append(row)

        if diagnostics is not None:
            diagnostic_rows.append(
                {
                    "fixture_id": fixture["id"],
                    "path": absolute_path,
                    "fingerprint_id": row["fingerprint_id"],
                    "diagnostics": diagnostics,
                }
            )

    return fixture_rows, diagnostic_rows, sorted(refusal_codes)


def build_family_summary(fixture_rows: list[dict[str, Any]]) -> dict[str, Any]:
    by_expected_family: dict[str, dict[str, Any]] = {}
    by_fingerprint: dict[str, int] = {}

    for row in fixture_rows:
        family_key = row["family"] if row["family"] is not None else "_unclassified"
        family_entry = by_expected_family.setdefault(
            family_key,
            {
                "expected_family": None if family_key == "_unclassified" else family_key,
                "records": 0,
                "matched_records": 0,
                "unmatched_records": 0,
                "ambiguous_child_records": 0,
                "skipped_records": 0,
                "refusal_records": 0,
                "matched_fingerprint_counts": {},
            },
        )
        family_entry["records"] += 1

        if row["refusal_code"] is not None:
            family_entry["refusal_records"] += 1
            by_fingerprint["_refusal"] = by_fingerprint.get("_refusal", 0) + 1
            continue

        if row["skipped"]:
            family_entry["skipped_records"] += 1
            by_fingerprint["_skipped"] = by_fingerprint.get("_skipped", 0) + 1
            continue

        if row["route_resolved"]:
            family_entry["matched_records"] += 1
            matched_id = row["resolved_fingerprint_id"] or "_matched_unknown"
            matched_counts = family_entry["matched_fingerprint_counts"]
            matched_counts[matched_id] = matched_counts.get(matched_id, 0) + 1
            by_fingerprint[matched_id] = by_fingerprint.get(matched_id, 0) + 1
            continue

        if row["child_routing_status"] == "ambiguous":
            family_entry["ambiguous_child_records"] += 1
            by_fingerprint["_ambiguous_child"] = by_fingerprint.get("_ambiguous_child", 0) + 1

        family_entry["unmatched_records"] += 1
        by_fingerprint["_unmatched"] = by_fingerprint.get("_unmatched", 0) + 1

    return {
        "schema_version": FAMILY_SUMMARY_SCHEMA,
        "families": [
            by_expected_family[key]
            for key in sorted(by_expected_family.keys(), key=lambda item: (item == "_unclassified", item))
        ],
        "matched_fingerprint_counts": {
            key: by_fingerprint[key] for key in sorted(by_fingerprint.keys())
        },
    }


def build_run_summary(
    *,
    mode: str,
    label: str,
    artifact_dir: Path,
    command: list[str],
    selected_fixtures: list[dict[str, Any]],
    fingerprints: list[str],
    definitions_dir: Path | None,
    manifest_path: Path,
    inventory_path: Path,
    with_witness: bool,
    exit_code: int,
    duration_ms: int,
    fixture_rows: list[dict[str, Any]],
    stderr_events: list[dict[str, Any]],
    stderr_invalid: list[dict[str, Any]],
    stdout_invalid: list[dict[str, Any]],
    refusal_codes: list[str],
    diagnostic_rows: list[dict[str, Any]],
) -> dict[str, Any]:
    matched_count = sum(1 for row in fixture_rows if row["route_resolved"])
    skipped_count = sum(1 for row in fixture_rows if row["skipped"])
    refusal_count = sum(1 for row in fixture_rows if row["refusal_code"] is not None)
    unmatched_count = sum(
        1
        for row in fixture_rows
        if not row["route_resolved"] and not row["skipped"] and row["refusal_code"] is None
    )
    ambiguous_route_count = sum(1 for row in fixture_rows if row["child_routing_status"] == "ambiguous")
    selected_child_count = sum(1 for row in fixture_rows if row["selected_child_fingerprint_id"] is not None)
    progress_event_count = sum(1 for event in stderr_events if event.get("type") == "progress")
    warning_event_count = sum(1 for event in stderr_events if event.get("type") == "warning")

    return {
        "schema_version": RUN_SUMMARY_SCHEMA,
        "mode": mode,
        "label": label,
        "artifact_dir": str(artifact_dir),
        "command": command,
        "selected_fixture_ids": [fixture["id"] for fixture in selected_fixtures],
        "fingerprints": fingerprints,
        "definitions_dir": str(definitions_dir) if definitions_dir is not None else None,
        "manifest": str(manifest_path),
        "inventory": str(inventory_path),
        "with_witness": with_witness,
        "exit_code": exit_code,
        "duration_ms": duration_ms,
        "selected_fixture_count": len(selected_fixtures),
        "stdout_record_count": len(fixture_rows),
        "matched_count": matched_count,
        "unmatched_count": unmatched_count,
        "ambiguous_route_count": ambiguous_route_count,
        "selected_child_count": selected_child_count,
        "skipped_count": skipped_count,
        "refusal_count": refusal_count,
        "refusal_codes": refusal_codes,
        "diagnostic_record_count": len(diagnostic_rows),
        "progress_event_count": progress_event_count,
        "warning_event_count": warning_event_count,
        "stdout_invalid_line_count": len(stdout_invalid),
        "stderr_invalid_line_count": len(stderr_invalid),
        "artifact_files": {
            "request": "request.json",
            "manifest": "manifest.jsonl",
            "stdout": "stdout.jsonl",
            "stderr": "stderr.jsonl",
            "stdout_records": "stdout.records.json",
            "stderr_events": "stderr.events.json",
            "diagnostics": "diagnostics.json",
            "fixture_summary": "fixture.summary.jsonl",
            "family_summary": "family.summary.json",
            "exit_code": "exit_code.txt",
            "duration_ms": "duration_ms.txt",
            "run_summary": "run.summary.json",
        },
    }


def prepare_artifact_dir(artifact_dir: Path) -> None:
    if artifact_dir.exists():
        shutil.rmtree(artifact_dir)
    artifact_dir.mkdir(parents=True, exist_ok=True)


def touch_files(artifact_dir: Path) -> None:
    for filename in [
        "stdout.jsonl",
        "stderr.jsonl",
        "exit_code.txt",
        "duration_ms.txt",
        "stdout.records.json",
        "stderr.events.json",
        "diagnostics.json",
        "fixture.summary.jsonl",
        "family.summary.json",
        "run.summary.json",
    ]:
        (artifact_dir / filename).write_text("", encoding="utf-8")


def run(args: argparse.Namespace) -> int:
    mode = args.mode
    artifact_root = repo_resolve(args.artifact_root)
    artifact_dir = artifact_root / mode / args.label
    manifest_path = repo_resolve(args.manifest)
    inventory_path = repo_resolve(args.inventory)
    definitions_dir = repo_resolve(args.definitions_dir) if args.definitions_dir else None
    fingerprint_binary = resolve_fingerprint_binary(args.fingerprint_bin)

    prepare_artifact_dir(artifact_dir)
    touch_files(artifact_dir)

    inventory_payload, inventory_by_abs_path = load_inventory(inventory_path)
    selected_records, selected_fixtures = select_manifest_records(
        manifest_path,
        inventory_by_abs_path,
        set(args.fixture_id),
        set(args.family),
        set(args.category),
        args.limit,
    )

    artifact_manifest_path = artifact_dir / "manifest.jsonl"
    write_jsonl(artifact_manifest_path, selected_records)

    request_payload = {
        "mode": mode,
        "label": args.label,
        "artifact_dir": str(artifact_dir),
        "fingerprints": args.fp,
        "definitions_dir": str(definitions_dir) if definitions_dir is not None else None,
        "manifest": str(manifest_path),
        "inventory": str(inventory_path),
        "selected_fixture_ids": [fixture["id"] for fixture in selected_fixtures],
        "with_witness": args.with_witness,
        "fingerprint_bin": str(fingerprint_binary),
        "inventory_schema_version": inventory_payload.get("schema_version"),
    }
    write_json(artifact_dir / "request.json", request_payload)

    diagnose = mode == "diagnose" or getattr(args, "diagnose", False)
    process, duration_ms, command = run_fingerprint(
        fingerprint_binary,
        artifact_manifest_path,
        args.fp,
        definitions_dir,
        diagnose,
        args.with_witness,
    )

    (artifact_dir / "stdout.jsonl").write_bytes(process.stdout)
    (artifact_dir / "stderr.jsonl").write_bytes(process.stderr)
    (artifact_dir / "exit_code.txt").write_text(f"{process.returncode}\n", encoding="utf-8")
    (artifact_dir / "duration_ms.txt").write_text(f"{duration_ms}\n", encoding="utf-8")

    _, stdout_records, stdout_invalid = parse_jsonl_text(process.stdout)
    _, stderr_events, stderr_invalid = parse_jsonl_text(process.stderr)
    write_json(
        artifact_dir / "stdout.records.json",
        {
            "schema_version": STDOUT_RECORDS_SCHEMA,
            "records": stdout_records,
            "invalid_lines": stdout_invalid,
        },
    )
    write_json(
        artifact_dir / "stderr.events.json",
        {
            "schema_version": STDERR_EVENTS_SCHEMA,
            "events": stderr_events,
            "invalid_lines": stderr_invalid,
        },
    )

    fixture_rows, diagnostic_rows, refusal_codes = build_fixture_rows(
        stdout_records, inventory_by_abs_path
    )
    write_json(
        artifact_dir / "diagnostics.json",
        {
            "schema_version": DIAGNOSTICS_SCHEMA,
            "records": diagnostic_rows,
        },
    )
    write_jsonl(artifact_dir / "fixture.summary.jsonl", fixture_rows)
    write_json(artifact_dir / "family.summary.json", build_family_summary(fixture_rows))

    summary = build_run_summary(
        mode=mode,
        label=args.label,
        artifact_dir=artifact_dir,
        command=command,
        selected_fixtures=selected_fixtures,
        fingerprints=args.fp,
        definitions_dir=definitions_dir,
        manifest_path=manifest_path,
        inventory_path=inventory_path,
        with_witness=args.with_witness,
        exit_code=process.returncode,
        duration_ms=duration_ms,
        fixture_rows=fixture_rows,
        stderr_events=stderr_events,
        stderr_invalid=stderr_invalid,
        stdout_invalid=stdout_invalid,
        refusal_codes=refusal_codes,
        diagnostic_rows=diagnostic_rows,
    )
    write_json(artifact_dir / "run.summary.json", summary)

    print(
        f"[{mode}] label={args.label} exit={process.returncode} duration_ms={duration_ms} "
        f"selected={summary['selected_fixture_count']} matched={summary['matched_count']} "
        f"unmatched={summary['unmatched_count']} skipped={summary['skipped_count']} "
        f"refusal={summary['refusal_count']} artifact_dir={artifact_dir}"
    )
    if refusal_codes:
        print(f"[{mode}] refusal_codes={','.join(refusal_codes)}")
    elif diagnostic_rows:
        print(f"[{mode}] diagnostics={len(diagnostic_rows)} path={artifact_dir / 'diagnostics.json'}")
    else:
        print(f"[{mode}] summary={artifact_dir / 'run.summary.json'}")

    return process.returncode


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Run reusable HTML end-to-end harness commands for fingerprint.",
        epilog=(
            "Local example:\n"
            "  scripts/html_smoke.sh --definitions-dir tmp/defs --fp html-smoke.v1 "
            "--fixture-id generic_page_sections_schedule --label local\n\n"
            "CI-safe example:\n"
            "  FINGERPRINT_BIN=target/debug/fingerprint scripts/html_family_matrix.sh "
            "--definitions-dir tmp/defs --fp family-a.v1 --fp family-b.v1 "
            "--artifact-root artifacts/html-e2e --label ci"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    subparsers = parser.add_subparsers(dest="mode", required=True)

    for mode in ("smoke", "matrix", "diagnose"):
        subparser = subparsers.add_parser(mode)
        subparser.add_argument("--artifact-root", default=str(DEFAULT_ARTIFACT_ROOT))
        subparser.add_argument("--label", default=mode)
        subparser.add_argument("--manifest", default=str(DEFAULT_MANIFEST))
        subparser.add_argument("--inventory", default=str(DEFAULT_INVENTORY))
        subparser.add_argument("--definitions-dir")
        subparser.add_argument("--fingerprint-bin")
        subparser.add_argument("--fixture-id", action="append", default=[])
        subparser.add_argument("--family", action="append", default=[])
        subparser.add_argument("--category", action="append", default=[])
        subparser.add_argument("--limit", type=int)
        subparser.add_argument("--with-witness", action="store_true")
        subparser.add_argument("--fp", action="append", required=True, default=[])
        if mode == "matrix":
            subparser.add_argument("--diagnose", action="store_true")

    return parser


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()

    try:
        return run(args)
    except HarnessError as error:
        mode = getattr(args, "mode", "unknown")
        label = getattr(args, "label", "error")
        artifact_root = repo_resolve(getattr(args, "artifact_root", str(DEFAULT_ARTIFACT_ROOT)))
        artifact_dir = artifact_root / mode / label
        prepare_artifact_dir(artifact_dir)
        touch_files(artifact_dir)
        summary = {
            "schema_version": RUN_SUMMARY_SCHEMA,
            "mode": mode,
            "label": label,
            "artifact_dir": str(artifact_dir),
            "exit_code": 2,
            "setup_error": str(error),
        }
        write_json(artifact_dir / "run.summary.json", summary)
        print(
            f"[{mode}] label={label} exit=2 setup_error={error} artifact_dir={artifact_dir}",
            file=sys.stderr,
        )
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
