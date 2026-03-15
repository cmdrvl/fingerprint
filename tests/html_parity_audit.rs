use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn script_path(name: &str) -> PathBuf {
    repo_path(&format!("scripts/{name}"))
}

fn rules_dir() -> PathBuf {
    repo_path("rules")
}

fn fixture_path(id: &str) -> PathBuf {
    repo_path(&format!("tests/fixtures/html/{id}.html"))
}

fn artifact_dir(root: &Path, mode: &str, label: &str) -> PathBuf {
    root.join(mode).join(label)
}

fn run_script(script_name: &str, args: &[&str]) -> Output {
    Command::new("bash")
        .arg(script_path(script_name))
        .args(args)
        .env("FINGERPRINT_BIN", env!("CARGO_BIN_EXE_fingerprint"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run parity script")
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).expect("read json file")).expect("parse json")
}

fn read_jsonl(path: &Path) -> Vec<Value> {
    fs::read_to_string(path)
        .expect("read jsonl file")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("parse jsonl line"))
        .collect()
}

fn assert_success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{context} failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn assert_failure(output: &Output, context: &str, expected_exit: i32) {
    assert_eq!(
        output.status.code(),
        Some(expected_exit),
        "{context} exit mismatch\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn write_legacy_results(path: &Path, rows: &[(&str, &str)]) {
    let payload = rows
        .iter()
        .map(|(fixture_id, family)| {
            json!({
                "path": fixture_path(fixture_id).display().to_string(),
                "legacy_family": family,
            })
        })
        .map(|row| serde_json::to_string(&row).expect("serialize json row"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(path, format!("{payload}\n")).expect("write legacy results");
}

const FAMILY_CASES: &[(&str, &str)] = &[
    ("bdc_soi_ares_like", "ares"),
    ("bdc_soi_bxsl_like", "bxsl"),
    ("bdc_soi_pennant_like", "pennant"),
    ("bdc_soi_golub_like", "golub"),
    ("bdc_soi_blackrock_like", "blackrock"),
];

#[test]
fn html_parity_audit_matches_legacy_results_file() {
    let artifacts = TempDir::new().expect("create artifacts dir");
    let legacy_results = artifacts.path().join("legacy-results.jsonl");
    write_legacy_results(&legacy_results, FAMILY_CASES);

    let output = run_script(
        "html_parity_audit.sh",
        &[
            "--definitions-dir",
            &rules_dir().display().to_string(),
            "--legacy-results",
            &legacy_results.display().to_string(),
            "--artifact-root",
            &artifacts.path().join("artifacts").display().to_string(),
            "--label",
            "parity-happy",
            "--fixture-id",
            "bdc_soi_ares_like",
            "--fixture-id",
            "bdc_soi_bxsl_like",
            "--fixture-id",
            "bdc_soi_pennant_like",
            "--fixture-id",
            "bdc_soi_golub_like",
            "--fixture-id",
            "bdc_soi_blackrock_like",
        ],
    );
    assert_success(&output, "html parity audit happy path");

    let dir = artifact_dir(
        &artifacts.path().join("artifacts"),
        "parity",
        "parity-happy",
    );
    let summary = read_json(&dir.join("parity.summary.json"));
    assert_eq!(summary["parity_match_count"], 5);
    assert_eq!(summary["mismatch_count"], 0);
    assert_eq!(summary["diagnosed_mismatch_count"], 0);

    let mismatches = read_jsonl(&dir.join("parity.mismatches.jsonl"));
    assert!(mismatches.is_empty(), "expected zero parity mismatches");

    let legacy_routes = read_jsonl(&dir.join("legacy.routes.jsonl"));
    assert_eq!(legacy_routes.len(), 5);
}

#[test]
fn html_parity_audit_reports_mismatches_with_diagnose_artifacts() {
    let artifacts = TempDir::new().expect("create artifacts dir");
    let legacy_results = artifacts.path().join("legacy-results.jsonl");
    let mut mismatched_rows = FAMILY_CASES.to_vec();
    mismatched_rows[0] = ("bdc_soi_ares_like", "bxsl");
    write_legacy_results(&legacy_results, &mismatched_rows);

    let output = run_script(
        "html_parity_audit.sh",
        &[
            "--definitions-dir",
            &rules_dir().display().to_string(),
            "--legacy-results",
            &legacy_results.display().to_string(),
            "--diagnose-mismatches",
            "--artifact-root",
            &artifacts.path().join("artifacts").display().to_string(),
            "--label",
            "parity-mismatch",
            "--fixture-id",
            "bdc_soi_ares_like",
            "--fixture-id",
            "bdc_soi_bxsl_like",
            "--fixture-id",
            "bdc_soi_pennant_like",
            "--fixture-id",
            "bdc_soi_golub_like",
            "--fixture-id",
            "bdc_soi_blackrock_like",
        ],
    );
    assert_failure(&output, "html parity audit mismatch path", 1);

    let dir = artifact_dir(
        &artifacts.path().join("artifacts"),
        "parity",
        "parity-mismatch",
    );
    let summary = read_json(&dir.join("parity.summary.json"));
    assert_eq!(summary["parity_match_count"], 4);
    assert_eq!(summary["mismatch_count"], 1);
    assert_eq!(summary["diagnosed_mismatch_count"], 1);

    let mismatches = read_jsonl(&dir.join("parity.mismatches.jsonl"));
    assert_eq!(mismatches.len(), 1);
    assert_eq!(mismatches[0]["legacy_family"], "bxsl");
    assert_eq!(mismatches[0]["observed_family"], "ares");
    assert_eq!(mismatches[0]["observed_fingerprint_id"], "bdc-soi-ares.v1");
    assert!(
        mismatches[0]["diagnose_artifact_dir"]
            .as_str()
            .is_some_and(|path| Path::new(path).exists()),
        "mismatch should retain diagnose artifact path"
    );
    let failed_children = mismatches[0]["failed_children"]
        .as_array()
        .expect("failed children array");
    assert!(
        failed_children
            .iter()
            .any(|child| child["fingerprint_id"] == "bdc-soi-bxsl.v1"),
        "mismatch report should include the failed legacy-expected child"
    );
}

#[test]
fn html_parity_audit_supports_legacy_command_template_mode() {
    let artifacts = TempDir::new().expect("create artifacts dir");
    let mock_router = artifacts.path().join("mock_router.py");
    fs::write(
        &mock_router,
        r#"#!/usr/bin/env python3
import json
import pathlib
import sys

name = pathlib.Path(sys.argv[1]).stem
if "ares" in name:
    family = "ares"
elif "bxsl" in name:
    family = "bxsl"
elif "pennant" in name:
    family = "pennant"
elif "golub" in name:
    family = "golub"
else:
    family = "blackrock"

print(json.dumps({"family": family}))
"#,
    )
    .expect("write mock router");

    let template = format!("python3 {} {{path}}", mock_router.display());
    let output = run_script(
        "html_parity_audit.sh",
        &[
            "--definitions-dir",
            &rules_dir().display().to_string(),
            "--legacy-command-template",
            &template,
            "--artifact-root",
            &artifacts.path().join("artifacts").display().to_string(),
            "--label",
            "parity-command-template",
            "--fixture-id",
            "bdc_soi_ares_like",
        ],
    );
    assert_success(&output, "html parity audit command-template path");

    let dir = artifact_dir(
        &artifacts.path().join("artifacts"),
        "parity",
        "parity-command-template",
    );
    let summary = read_json(&dir.join("parity.summary.json"));
    assert_eq!(summary["parity_match_count"], 1);
    assert_eq!(summary["mismatch_count"], 0);
}
