#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import shutil
import sys
from pathlib import Path
from typing import Any

import html_parity_audit as parity

ROUTE_SCHEMA = "fingerprint.html-route-consumer.route.v1"
SUMMARY_SCHEMA = "fingerprint.html-route-consumer.summary.v1"
DIFF_SCHEMA = "fingerprint.html-route-consumer.diff.v1"


class ConsumerError(RuntimeError):
    pass


def write_json(path: Path, payload: Any) -> None:
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def write_jsonl(path: Path, rows: list[dict[str, Any]]) -> None:
    with path.open("w", encoding="utf-8") as handle:
        for row in rows:
            handle.write(json.dumps(row, sort_keys=True))
            handle.write("\n")


def reset_artifact_dir(path: Path) -> None:
    if path.exists():
        shutil.rmtree(path)
    path.mkdir(parents=True, exist_ok=True)


def fingerprint_route_status(row: dict[str, Any]) -> str:
    if row.get("refusal_code"):
        return "refusal"
    if row.get("skipped"):
        return "skipped"
    child_status = row.get("child_routing_status")
    if isinstance(child_status, str):
        return child_status
    if row.get("route_resolved") is True:
        return "selected"
    if row.get("matched") is True:
        return "matched_without_child_route"
    return "unmatched"


def diff_reason(row: dict[str, Any], legacy_family: str | None) -> str | None:
    authoritative_family = parity.normalize_family(row.get("resolved_fingerprint_id"))
    status = fingerprint_route_status(row)
    if legacy_family is None and authoritative_family is None:
        return None
    if authoritative_family == legacy_family:
        return None
    if status == "refusal":
        return "fingerprint_refusal"
    if status == "skipped":
        return "fingerprint_skipped"
    if authoritative_family is None:
        return "fingerprint_unresolved"
    if legacy_family is None:
        return "legacy_missing_family"
    return "family_mismatch"


def build_route_rows(
    fixture_rows: list[dict[str, Any]],
    legacy_routes: list[dict[str, Any]] | None,
    legacy_fallback_on_unresolved: bool,
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    legacy_by_path = {route["path"]: route for route in legacy_routes or []}
    route_rows: list[dict[str, Any]] = []
    diff_rows: list[dict[str, Any]] = []

    for row in fixture_rows:
        path = row.get("path")
        if not isinstance(path, str):
            continue

        authoritative_family = (
            parity.normalize_family(row.get("resolved_fingerprint_id"))
            if row.get("route_resolved") is True
            else None
        )
        status = fingerprint_route_status(row)
        legacy = legacy_by_path.get(path)
        legacy_family = legacy.get("legacy_family") if legacy else None
        effective_family = authoritative_family
        route_source = "fingerprint"
        fallback_applied = False
        if (
            legacy_fallback_on_unresolved
            and authoritative_family is None
            and isinstance(legacy_family, str)
        ):
            effective_family = legacy_family
            route_source = "legacy_fallback"
            fallback_applied = True

        route_row = {
            "schema_version": ROUTE_SCHEMA,
            "fixture_id": row.get("fixture_id"),
            "path": path,
            "expected_family": row.get("family"),
            "fingerprint_id": row.get("fingerprint_id"),
            "resolved_fingerprint_id": row.get("resolved_fingerprint_id"),
            "authoritative_family": authoritative_family,
            "effective_family": effective_family,
            "route_source": route_source,
            "fallback_applied": fallback_applied,
            "fingerprint_route_status": status,
            "legacy_family": legacy_family,
            "matched_child_count": row.get("matched_child_count"),
            "matched_child_fingerprint_ids": row.get("matched_child_fingerprint_ids", []),
            "selected_child_fingerprint_id": row.get("selected_child_fingerprint_id"),
            "skipped": row.get("skipped"),
            "refusal_code": row.get("refusal_code"),
            "attempted_fingerprint_ids": row.get("attempted_fingerprint_ids", []),
            "diagnostics_present": row.get("diagnostics_present"),
            "content_hash": row.get("content_hash"),
        }
        route_rows.append(route_row)

        reason = diff_reason(row, legacy_family) if legacy_routes is not None else None
        if reason is None:
            continue
        diff_rows.append(
            {
                "schema_version": DIFF_SCHEMA,
                "fixture_id": row.get("fixture_id"),
                "path": path,
                "expected_family": row.get("family"),
                "legacy_family": legacy_family,
                "authoritative_family": authoritative_family,
                "effective_family": effective_family,
                "route_source": route_source,
                "fallback_applied": fallback_applied,
                "fingerprint_route_status": status,
                "reason": reason,
                "diagnose_artifact_dir": None,
                "failed_children": [],
            }
        )

    return route_rows, diff_rows


def attach_diagnose_artifacts(
    args: argparse.Namespace,
    artifact_root: Path,
    fingerprint_binary: Path,
    fixture_rows: list[dict[str, Any]],
    diff_rows: list[dict[str, Any]],
) -> None:
    by_fixture_id = {
        row.get("fixture_id"): row
        for row in fixture_rows
        if isinstance(row.get("fixture_id"), str)
    }
    for diff in diff_rows:
        fixture_id = diff.get("fixture_id")
        if not isinstance(fixture_id, str):
            continue
        fixture_row = by_fixture_id.get(fixture_id)
        if fixture_row is None:
            continue
        diagnose_dir, failed_children = parity.diagnose_mismatch(
            args,
            artifact_root,
            fingerprint_binary,
            fixture_row,
        )
        diff["diagnose_artifact_dir"] = diagnose_dir
        diff["failed_children"] = failed_children


def build_summary(
    *,
    args: argparse.Namespace,
    artifact_dir: Path,
    matrix_exit_code: int,
    matrix_artifact_dir: Path,
    route_rows: list[dict[str, Any]],
    diff_rows: list[dict[str, Any]],
    legacy_route_count: int,
) -> dict[str, Any]:
    authoritative_route_count = sum(
        1 for row in route_rows if isinstance(row.get("authoritative_family"), str)
    )
    effective_route_count = sum(
        1 for row in route_rows if isinstance(row.get("effective_family"), str)
    )
    unresolved_authoritative_count = len(route_rows) - authoritative_route_count
    unresolved_effective_count = len(route_rows) - effective_route_count
    fallback_route_count = sum(
        1 for row in route_rows if row.get("route_source") == "legacy_fallback"
    )
    refusal_count = sum(1 for row in route_rows if row.get("refusal_code") is not None)
    skipped_count = sum(1 for row in route_rows if row.get("skipped") is True)

    return {
        "schema_version": SUMMARY_SCHEMA,
        "label": args.label,
        "artifact_dir": str(artifact_dir),
        "matrix_artifact_dir": str(matrix_artifact_dir),
        "manifest": str(parity.repo_resolve(args.manifest)),
        "inventory": str(parity.repo_resolve(args.inventory)),
        "definitions_dir": str(parity.repo_resolve(args.definitions_dir)),
        "fingerprints": args.fp,
        "selected_fixture_count": len(route_rows),
        "matrix_exit_code": matrix_exit_code,
        "legacy_route_count": legacy_route_count,
        "authoritative_route_count": authoritative_route_count,
        "effective_route_count": effective_route_count,
        "unresolved_authoritative_count": unresolved_authoritative_count,
        "unresolved_effective_count": unresolved_effective_count,
        "fallback_route_count": fallback_route_count,
        "dual_run_diff_count": len(diff_rows),
        "diagnosed_diff_count": sum(
            1 for diff in diff_rows if diff.get("diagnose_artifact_dir")
        ),
        "skipped_count": skipped_count,
        "refusal_count": refusal_count,
        "legacy_fallback_on_unresolved": args.legacy_fallback_on_unresolved,
        "allow_diffs": args.allow_diffs,
        "artifact_files": {
            "request": "request.json",
            "consumer_routes": "consumer.routes.jsonl",
            "route_diffs": "route.diffs.jsonl",
            "consumer_summary": "consumer.summary.json",
            "legacy_routes": "legacy.routes.jsonl",
        },
    }


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=(
            "Emit downstream-consumer HTML family routes from fingerprint output, "
            "with optional dual-run legacy diff logging and constrained fallback."
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=(
            "Authoritative fingerprint routing:\n"
            "  scripts/html_route_consumer.sh --definitions-dir rules --label fingerprint-only\n\n"
            "Dual-run diff logging:\n"
            "  scripts/html_route_consumer.sh --definitions-dir rules "
            "--legacy-results /tmp/legacy-routes.jsonl --label dual-run --diagnose-diffs\n\n"
            "Temporary unresolved-route fallback:\n"
            "  scripts/html_route_consumer.sh --definitions-dir rules "
            "--legacy-command-template 'python /path/to/fingerprint_schedule_family.py {path}' "
            "--legacy-fallback-on-unresolved --allow-diffs --label rollback-window"
        ),
    )
    parser.add_argument("--artifact-root", default=str(parity.DEFAULT_ARTIFACT_ROOT))
    parser.add_argument("--label", default="consumer")
    parser.add_argument("--manifest", default=str(parity.DEFAULT_MANIFEST))
    parser.add_argument("--inventory", default=str(parity.DEFAULT_INVENTORY))
    parser.add_argument("--definitions-dir", default=str(parity.DEFAULT_DEFINITIONS))
    parser.add_argument("--fingerprint-bin")
    parser.add_argument("--fixture-id", action="append", default=[])
    parser.add_argument("--family", action="append", default=[])
    parser.add_argument("--category", action="append", default=[])
    parser.add_argument("--limit", type=int)
    parser.add_argument("--fp", action="append", default=[])
    parser.add_argument("--legacy-results")
    parser.add_argument("--legacy-command-template")
    parser.add_argument("--diagnose-diffs", action="store_true")
    parser.add_argument("--legacy-fallback-on-unresolved", action="store_true")
    parser.add_argument("--allow-diffs", action="store_true")
    return parser


def run(args: argparse.Namespace) -> int:
    if args.legacy_results and args.legacy_command_template:
        raise ConsumerError(
            "pass either --legacy-results or --legacy-command-template, not both"
        )
    if args.legacy_fallback_on_unresolved and not (
        args.legacy_results or args.legacy_command_template
    ):
        raise ConsumerError(
            "--legacy-fallback-on-unresolved requires a legacy route source"
        )

    artifact_root = parity.repo_resolve(args.artifact_root)
    artifact_dir = artifact_root / "consumer" / args.label
    reset_artifact_dir(artifact_dir)

    fingerprint_binary = parity.resolve_fingerprint_binary(args.fingerprint_bin)
    legacy_results = (
        parity.load_legacy_results(parity.repo_resolve(args.legacy_results))
        if args.legacy_results
        else None
    )

    request_payload = {
        "label": args.label,
        "artifact_dir": str(artifact_dir),
        "manifest": str(parity.repo_resolve(args.manifest)),
        "inventory": str(parity.repo_resolve(args.inventory)),
        "definitions_dir": str(parity.repo_resolve(args.definitions_dir)),
        "fingerprints": args.fp or parity.DEFAULT_FINGERPRINTS,
        "fixture_id": args.fixture_id,
        "family": args.family,
        "category": args.category,
        "limit": args.limit,
        "legacy_results": args.legacy_results,
        "legacy_command_template": args.legacy_command_template,
        "diagnose_diffs": args.diagnose_diffs,
        "legacy_fallback_on_unresolved": args.legacy_fallback_on_unresolved,
        "allow_diffs": args.allow_diffs,
    }
    write_json(artifact_dir / "request.json", request_payload)

    matrix_exit_code, matrix_artifact_dir = parity.run_matrix(args, artifact_root, fingerprint_binary)
    fixture_rows = parity.read_jsonl(matrix_artifact_dir / "fixture.summary.jsonl")

    legacy_routes = None
    if legacy_results is not None or args.legacy_command_template:
        legacy_routes = parity.collect_legacy_routes(
            fixture_rows,
            legacy_results,
            args.legacy_command_template,
        )
        write_jsonl(artifact_dir / "legacy.routes.jsonl", legacy_routes)

    route_rows, diff_rows = build_route_rows(
        fixture_rows,
        legacy_routes,
        args.legacy_fallback_on_unresolved,
    )

    if args.diagnose_diffs and diff_rows:
        attach_diagnose_artifacts(args, artifact_root, fingerprint_binary, fixture_rows, diff_rows)

    write_jsonl(artifact_dir / "consumer.routes.jsonl", route_rows)
    write_jsonl(artifact_dir / "route.diffs.jsonl", diff_rows)
    summary = build_summary(
        args=args,
        artifact_dir=artifact_dir,
        matrix_exit_code=matrix_exit_code,
        matrix_artifact_dir=matrix_artifact_dir,
        route_rows=route_rows,
        diff_rows=diff_rows,
        legacy_route_count=len(legacy_routes or []),
    )
    write_json(artifact_dir / "consumer.summary.json", summary)

    for row in route_rows:
        sys.stdout.write(json.dumps(row, sort_keys=True))
        sys.stdout.write("\n")

    print(
        f"[consumer] label={args.label} selected={summary['selected_fixture_count']} "
        f"authoritative={summary['authoritative_route_count']} "
        f"effective={summary['effective_route_count']} "
        f"fallback={summary['fallback_route_count']} "
        f"diffs={summary['dual_run_diff_count']} "
        f"artifact_dir={artifact_dir}",
        file=sys.stderr,
    )
    if diff_rows:
        print(
            f"[consumer] diffs={artifact_dir / 'route.diffs.jsonl'}",
            file=sys.stderr,
        )
    else:
        print(
            f"[consumer] summary={artifact_dir / 'consumer.summary.json'}",
            file=sys.stderr,
        )

    if summary["unresolved_effective_count"] > 0:
        return 1
    if diff_rows and not args.allow_diffs:
        return 1
    return 0


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()
    if not args.fp:
        args.fp = list(parity.DEFAULT_FINGERPRINTS)
    try:
        return run(args)
    except (ConsumerError, parity.HarnessError) as error:
        print(f"[consumer] setup_error={error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
