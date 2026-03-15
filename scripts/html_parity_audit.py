#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import shlex
import subprocess
import sys
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_MANIFEST = REPO_ROOT / "tests" / "fixtures" / "manifests" / "html_corpus.jsonl"
DEFAULT_INVENTORY = REPO_ROOT / "tests" / "fixtures" / "html" / "inventory.json"
DEFAULT_DEFINITIONS = REPO_ROOT / "rules"
DEFAULT_ARTIFACT_ROOT = REPO_ROOT / "artifacts" / "html-e2e"
DEFAULT_FINGERPRINTS = [
    "bdc-soi.v1",
    "bdc-soi-ares.v1",
    "bdc-soi-blackrock.v1",
    "bdc-soi-bxsl.v1",
    "bdc-soi-pennant.v1",
    "bdc-soi-golub.v1",
]

PARITY_SUMMARY_SCHEMA = "fingerprint.html-parity.summary.v1"
PARITY_MISMATCH_SCHEMA = "fingerprint.html-parity.mismatch.v1"
LEGACY_ROUTES_SCHEMA = "fingerprint.html-parity.legacy-routes.v1"


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


def read_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def read_jsonl(path: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    with path.open("r", encoding="utf-8") as handle:
        for line in handle:
            if not line.strip():
                continue
            rows.append(json.loads(line))
    return rows


def ensure_artifact_dir(path: Path) -> None:
    path.mkdir(parents=True, exist_ok=True)


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


def normalize_family(raw: Any) -> str | None:
    if not isinstance(raw, str):
        return None
    text = raw.strip()
    if not text:
        return None
    lowered = text.lower()
    if lowered.startswith("bdc-soi-") and ".v" in lowered:
        return lowered.split("bdc-soi-", 1)[1].split(".v", 1)[0]
    if "/" in lowered and ".v" in lowered:
        return lowered.rsplit("/", 1)[1].split(".v", 1)[0]
    return lowered


def parse_legacy_output(raw: str) -> str | None:
    stripped = raw.strip()
    if not stripped:
        return None
    if stripped.startswith("{"):
        payload = json.loads(stripped)
        for key in ("family", "legacy_family", "resolved_family", "fingerprint_id"):
            family = normalize_family(payload.get(key))
            if family:
                return family
        return None
    return normalize_family(stripped.splitlines()[0])


def build_matrix_command(args: argparse.Namespace, artifact_root: Path, fingerprint_binary: Path) -> list[str]:
    command = [
        "bash",
        str(REPO_ROOT / "scripts" / "html_family_matrix.sh"),
        "--definitions-dir",
        str(repo_resolve(args.definitions_dir)),
        "--artifact-root",
        str(artifact_root),
        "--label",
        args.label,
        "--manifest",
        str(repo_resolve(args.manifest)),
        "--inventory",
        str(repo_resolve(args.inventory)),
    ]
    for fingerprint_id in args.fp:
        command.extend(["--fp", fingerprint_id])
    for fixture_id in args.fixture_id:
        command.extend(["--fixture-id", fixture_id])
    for family in args.family:
        command.extend(["--family", family])
    for category in args.category:
        command.extend(["--category", category])
    if args.limit is not None:
        command.extend(["--limit", str(args.limit)])
    return command


def run_matrix(args: argparse.Namespace, artifact_root: Path, fingerprint_binary: Path) -> tuple[int, Path]:
    command = build_matrix_command(args, artifact_root, fingerprint_binary)
    env = os.environ.copy()
    env["FINGERPRINT_BIN"] = str(fingerprint_binary)
    process = subprocess.run(
        command,
        cwd=str(REPO_ROOT),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=env,
        check=False,
    )
    if process.returncode not in (0, 1):
        raise HarnessError(
            "html family matrix failed unexpectedly\n"
            f"status: {process.returncode}\n"
            f"stdout:\n{process.stdout.decode('utf-8', errors='replace')}\n"
            f"stderr:\n{process.stderr.decode('utf-8', errors='replace')}"
        )
    return process.returncode, artifact_root / "matrix" / args.label


def load_legacy_results(path: Path) -> dict[str, dict[str, Any]]:
    routes: dict[str, dict[str, Any]] = {}
    for row in read_jsonl(path):
        record_path = row.get("path")
        if not isinstance(record_path, str):
            continue
        family = None
        for key in ("legacy_family", "family", "resolved_family", "fingerprint_id"):
            family = normalize_family(row.get(key))
            if family:
                break
        routes[str(repo_resolve(record_path))] = {
            "path": str(repo_resolve(record_path)),
            "legacy_family": family,
            "source": str(path),
        }
    return routes


def run_legacy_command(template: str, record_path: str) -> dict[str, Any]:
    command = template.format(path=shlex.quote(record_path))
    process = subprocess.run(
        command,
        cwd=str(REPO_ROOT),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        shell=True,
        check=False,
        text=True,
    )
    if process.returncode != 0:
        raise HarnessError(
            "legacy router command failed for parity audit\n"
            f"path: {record_path}\n"
            f"command: {command}\n"
            f"status: {process.returncode}\n"
            f"stdout:\n{process.stdout}\n"
            f"stderr:\n{process.stderr}"
        )
    family = parse_legacy_output(process.stdout)
    return {
        "path": record_path,
        "legacy_family": family,
        "source": "command_template",
        "command": command,
    }


def collect_legacy_routes(
    fixture_rows: list[dict[str, Any]],
    legacy_results: dict[str, dict[str, Any]] | None,
    legacy_command_template: str | None,
) -> list[dict[str, Any]]:
    routes: list[dict[str, Any]] = []
    for row in fixture_rows:
        record_path = row.get("path")
        if not isinstance(record_path, str):
            continue
        if legacy_results is not None:
            route = legacy_results.get(record_path)
            if route is None:
                raise HarnessError(
                    f"legacy results file did not include a route for {record_path}"
                )
            routes.append(route)
            continue
        if legacy_command_template is None:
            raise HarnessError("either --legacy-results or --legacy-command-template is required")
        routes.append(run_legacy_command(legacy_command_template, record_path))
    return routes


def diagnose_command(
    args: argparse.Namespace,
    artifact_root: Path,
    fingerprint_binary: Path,
    fixture_id: str,
    label: str,
) -> list[str]:
    command = [
        "bash",
        str(REPO_ROOT / "scripts" / "html_diagnose.sh"),
        "--definitions-dir",
        str(repo_resolve(args.definitions_dir)),
        "--artifact-root",
        str(artifact_root),
        "--label",
        label,
        "--manifest",
        str(repo_resolve(args.manifest)),
        "--inventory",
        str(repo_resolve(args.inventory)),
        "--fixture-id",
        fixture_id,
    ]
    for fingerprint_id in args.fp:
        command.extend(["--fp", fingerprint_id])
    return command


def summarize_failed_children(stdout_records_path: Path) -> list[dict[str, Any]]:
    stdout_records = read_json(stdout_records_path)
    records = stdout_records.get("records", [])
    if not records:
        return []
    fingerprint = records[0].get("fingerprint", {})
    children = fingerprint.get("children", [])
    failed_children: list[dict[str, Any]] = []
    for child in children:
        if child.get("matched") is True:
            continue
        assertions = child.get("assertions", [])
        first_failed = next((assertion for assertion in assertions if assertion.get("passed") is False), None)
        failed_children.append(
            {
                "fingerprint_id": child.get("fingerprint_id"),
                "reason": child.get("reason"),
                "first_failed_assertion": {
                    "name": first_failed.get("name") if isinstance(first_failed, dict) else None,
                    "detail": first_failed.get("detail") if isinstance(first_failed, dict) else None,
                },
            }
        )
    return failed_children


def diagnose_mismatch(
    args: argparse.Namespace,
    artifact_root: Path,
    fingerprint_binary: Path,
    fixture_row: dict[str, Any],
) -> tuple[str | None, list[dict[str, Any]]]:
    fixture_id = fixture_row.get("fixture_id")
    if not isinstance(fixture_id, str):
        return None, []
    label = f"{args.label}-{fixture_id}-diagnose"
    command = diagnose_command(args, artifact_root, fingerprint_binary, fixture_id, label)
    env = os.environ.copy()
    env["FINGERPRINT_BIN"] = str(fingerprint_binary)
    process = subprocess.run(
        command,
        cwd=str(REPO_ROOT),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=env,
        check=False,
    )
    diagnose_dir = artifact_root / "diagnose" / label
    failed_children = []
    if diagnose_dir.exists():
        failed_children = summarize_failed_children(diagnose_dir / "stdout.records.json")
    if process.returncode not in (0, 1):
        raise HarnessError(
            "html diagnose failed unexpectedly while collecting parity mismatch details\n"
            f"fixture_id: {fixture_id}\n"
            f"status: {process.returncode}\n"
            f"stdout:\n{process.stdout.decode('utf-8', errors='replace')}\n"
            f"stderr:\n{process.stderr.decode('utf-8', errors='replace')}"
        )
    return str(diagnose_dir) if diagnose_dir.exists() else None, failed_children


def build_mismatch_rows(
    args: argparse.Namespace,
    fixture_rows: list[dict[str, Any]],
    legacy_routes: list[dict[str, Any]],
    matrix_artifact_dir: Path,
    artifact_root: Path,
    fingerprint_binary: Path,
) -> list[dict[str, Any]]:
    legacy_by_path = {route["path"]: route for route in legacy_routes}
    mismatches: list[dict[str, Any]] = []

    for row in fixture_rows:
        record_path = row.get("path")
        if not isinstance(record_path, str):
            continue
        legacy = legacy_by_path[record_path]
        observed_family = normalize_family(row.get("resolved_fingerprint_id"))
        legacy_family = legacy.get("legacy_family")
        if observed_family == legacy_family:
            continue

        diagnose_artifact_dir = None
        failed_children: list[dict[str, Any]] = []
        if args.diagnose_mismatches:
            diagnose_artifact_dir, failed_children = diagnose_mismatch(
                args, artifact_root, fingerprint_binary, row
            )

        mismatches.append(
            {
                "schema_version": PARITY_MISMATCH_SCHEMA,
                "fixture_id": row.get("fixture_id"),
                "path": record_path,
                "expected_family": row.get("family"),
                "legacy_family": legacy_family,
                "observed_family": observed_family,
                "observed_fingerprint_id": row.get("resolved_fingerprint_id"),
                "child_routing_status": row.get("child_routing_status"),
                "matched_child_count": row.get("matched_child_count"),
                "matrix_artifact_dir": str(matrix_artifact_dir),
                "diagnose_artifact_dir": diagnose_artifact_dir,
                "failed_children": failed_children,
            }
        )

    return mismatches


def build_summary(
    *,
    args: argparse.Namespace,
    artifact_dir: Path,
    matrix_exit_code: int,
    matrix_artifact_dir: Path,
    fixture_rows: list[dict[str, Any]],
    legacy_routes: list[dict[str, Any]],
    mismatches: list[dict[str, Any]],
) -> dict[str, Any]:
    return {
        "schema_version": PARITY_SUMMARY_SCHEMA,
        "label": args.label,
        "artifact_dir": str(artifact_dir),
        "matrix_artifact_dir": str(matrix_artifact_dir),
        "manifest": str(repo_resolve(args.manifest)),
        "inventory": str(repo_resolve(args.inventory)),
        "definitions_dir": str(repo_resolve(args.definitions_dir)),
        "fingerprints": args.fp,
        "selected_fixture_count": len(fixture_rows),
        "matrix_exit_code": matrix_exit_code,
        "legacy_route_count": len(legacy_routes),
        "parity_match_count": len(fixture_rows) - len(mismatches),
        "mismatch_count": len(mismatches),
        "diagnosed_mismatch_count": sum(
            1 for mismatch in mismatches if mismatch.get("diagnose_artifact_dir")
        ),
        "artifact_files": {
            "legacy_routes": "legacy.routes.jsonl",
            "parity_summary": "parity.summary.json",
            "parity_mismatches": "parity.mismatches.jsonl",
            "request": "request.json",
        },
    }


def run(args: argparse.Namespace) -> int:
    artifact_root = repo_resolve(args.artifact_root)
    artifact_dir = artifact_root / "parity" / args.label
    ensure_artifact_dir(artifact_dir)

    fingerprint_binary = resolve_fingerprint_binary(args.fingerprint_bin)
    legacy_results = (
        load_legacy_results(repo_resolve(args.legacy_results))
        if args.legacy_results
        else None
    )

    request_payload = {
        "label": args.label,
        "artifact_dir": str(artifact_dir),
        "manifest": str(repo_resolve(args.manifest)),
        "inventory": str(repo_resolve(args.inventory)),
        "definitions_dir": str(repo_resolve(args.definitions_dir)),
        "fingerprints": args.fp,
        "fixture_id": args.fixture_id,
        "family": args.family,
        "category": args.category,
        "limit": args.limit,
        "legacy_results": args.legacy_results,
        "legacy_command_template": args.legacy_command_template,
        "diagnose_mismatches": args.diagnose_mismatches,
    }
    write_json(artifact_dir / "request.json", request_payload)

    matrix_exit_code, matrix_artifact_dir = run_matrix(args, artifact_root, fingerprint_binary)
    fixture_rows = read_jsonl(matrix_artifact_dir / "fixture.summary.jsonl")
    legacy_routes = collect_legacy_routes(
        fixture_rows,
        legacy_results,
        args.legacy_command_template,
    )
    write_jsonl(artifact_dir / "legacy.routes.jsonl", legacy_routes)

    mismatches = build_mismatch_rows(
        args,
        fixture_rows,
        legacy_routes,
        matrix_artifact_dir,
        artifact_root,
        fingerprint_binary,
    )
    write_jsonl(artifact_dir / "parity.mismatches.jsonl", mismatches)

    summary = build_summary(
        args=args,
        artifact_dir=artifact_dir,
        matrix_exit_code=matrix_exit_code,
        matrix_artifact_dir=matrix_artifact_dir,
        fixture_rows=fixture_rows,
        legacy_routes=legacy_routes,
        mismatches=mismatches,
    )
    write_json(artifact_dir / "parity.summary.json", summary)

    print(
        f"[parity] label={args.label} selected={summary['selected_fixture_count']} "
        f"matches={summary['parity_match_count']} mismatches={summary['mismatch_count']} "
        f"artifact_dir={artifact_dir}"
    )
    if mismatches:
        print(f"[parity] mismatches={artifact_dir / 'parity.mismatches.jsonl'}")
        return 1
    print(f"[parity] summary={artifact_dir / 'parity.summary.json'}")
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=(
            "Compare fingerprint HTML family-routing output against a legacy route source "
            "and emit artifact-rich mismatch reports."
        ),
        epilog=(
            "Committed-fixture example:\n"
            "  scripts/html_parity_audit.sh --definitions-dir rules "
            "--legacy-results /tmp/legacy-routes.jsonl --label committed-fixtures\n\n"
            "External-corpus example:\n"
            "  scripts/html_parity_audit.sh --definitions-dir rules "
            "--manifest /data/bdc/html_corpus.jsonl --inventory tests/fixtures/html/inventory.json "
            "--legacy-command-template 'python /path/to/fingerprint_schedule_family.py {path}' "
            "--artifact-root artifacts/html-e2e --label external-corpus --diagnose-mismatches"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("--artifact-root", default=str(DEFAULT_ARTIFACT_ROOT))
    parser.add_argument("--label", default="parity")
    parser.add_argument("--manifest", default=str(DEFAULT_MANIFEST))
    parser.add_argument("--inventory", default=str(DEFAULT_INVENTORY))
    parser.add_argument("--definitions-dir", default=str(DEFAULT_DEFINITIONS))
    parser.add_argument("--fingerprint-bin")
    parser.add_argument("--fixture-id", action="append", default=[])
    parser.add_argument("--family", action="append", default=[])
    parser.add_argument("--category", action="append", default=[])
    parser.add_argument("--limit", type=int)
    parser.add_argument("--fp", action="append", default=[])
    parser.add_argument("--legacy-results")
    parser.add_argument("--legacy-command-template")
    parser.add_argument("--diagnose-mismatches", action="store_true")
    return parser


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()
    if not args.fp:
        args.fp = list(DEFAULT_FINGERPRINTS)
    try:
        return run(args)
    except HarnessError as error:
        print(f"[parity] setup_error={error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
