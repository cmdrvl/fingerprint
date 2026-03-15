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
        .expect("run route consumer script")
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

fn parse_stdout_jsonl(output: &Output) -> Vec<Value> {
    String::from_utf8(output.stdout.clone())
        .expect("stdout utf8")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("parse stdout json line"))
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
fn html_route_consumer_emits_authoritative_fingerprint_routes() {
    let artifacts = TempDir::new().expect("create artifacts dir");
    let output = run_script(
        "html_route_consumer.sh",
        &[
            "--definitions-dir",
            &rules_dir().display().to_string(),
            "--artifact-root",
            &artifacts.path().join("artifacts").display().to_string(),
            "--label",
            "consumer-fingerprint",
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
    assert_success(&output, "fingerprint-only route consumer");

    let stdout_rows = parse_stdout_jsonl(&output);
    assert_eq!(stdout_rows.len(), 5);
    let rows_by_fixture = stdout_rows
        .iter()
        .map(|row| {
            (
                row["fixture_id"]
                    .as_str()
                    .expect("fixture_id should be present")
                    .to_owned(),
                row,
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    for (fixture_id, expected_family) in FAMILY_CASES {
        let row = rows_by_fixture
            .get(*fixture_id)
            .unwrap_or_else(|| panic!("missing route row for fixture '{fixture_id}'"));
        assert_eq!(row["effective_family"], *expected_family);
        assert_eq!(row["authoritative_family"], *expected_family);
        assert_eq!(row["route_source"], "fingerprint");
        assert_eq!(row["fingerprint_route_status"], "selected");
        assert_eq!(row["fallback_applied"], false);
    }

    let dir = artifact_dir(
        &artifacts.path().join("artifacts"),
        "consumer",
        "consumer-fingerprint",
    );
    let summary = read_json(&dir.join("consumer.summary.json"));
    assert_eq!(summary["authoritative_route_count"], 5);
    assert_eq!(summary["effective_route_count"], 5);
    assert_eq!(summary["fallback_route_count"], 0);
    assert_eq!(summary["dual_run_diff_count"], 0);
}

#[test]
fn html_route_consumer_dual_run_reports_file_level_diffs() {
    let artifacts = TempDir::new().expect("create artifacts dir");
    let legacy_results = artifacts.path().join("legacy-results.jsonl");
    let mut rows = FAMILY_CASES.to_vec();
    rows[0] = ("bdc_soi_ares_like", "bxsl");
    write_legacy_results(&legacy_results, &rows);

    let output = run_script(
        "html_route_consumer.sh",
        &[
            "--definitions-dir",
            &rules_dir().display().to_string(),
            "--legacy-results",
            &legacy_results.display().to_string(),
            "--diagnose-diffs",
            "--artifact-root",
            &artifacts.path().join("artifacts").display().to_string(),
            "--label",
            "consumer-dual-run",
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
    assert_failure(&output, "dual-run route consumer mismatch", 1);

    let dir = artifact_dir(
        &artifacts.path().join("artifacts"),
        "consumer",
        "consumer-dual-run",
    );
    let summary = read_json(&dir.join("consumer.summary.json"));
    assert_eq!(summary["authoritative_route_count"], 5);
    assert_eq!(summary["dual_run_diff_count"], 1);
    assert_eq!(summary["diagnosed_diff_count"], 1);

    let diffs = read_jsonl(&dir.join("route.diffs.jsonl"));
    assert_eq!(diffs.len(), 1);
    assert_eq!(diffs[0]["legacy_family"], "bxsl");
    assert_eq!(diffs[0]["authoritative_family"], "ares");
    assert_eq!(diffs[0]["reason"], "family_mismatch");
    assert_eq!(diffs[0]["route_source"], "fingerprint");
    assert!(
        diffs[0]["diagnose_artifact_dir"]
            .as_str()
            .is_some_and(|path| Path::new(path).exists()),
        "route diff should retain diagnose artifact path"
    );
}

#[test]
fn html_route_consumer_can_fallback_to_legacy_for_unresolved_routes() {
    let artifacts = TempDir::new().expect("create artifacts dir");
    let legacy_results = artifacts.path().join("legacy-results.jsonl");
    write_legacy_results(&legacy_results, &[("minimal_empty_shell", "ares")]);

    let output = run_script(
        "html_route_consumer.sh",
        &[
            "--definitions-dir",
            &rules_dir().display().to_string(),
            "--legacy-results",
            &legacy_results.display().to_string(),
            "--legacy-fallback-on-unresolved",
            "--allow-diffs",
            "--artifact-root",
            &artifacts.path().join("artifacts").display().to_string(),
            "--label",
            "consumer-fallback",
            "--fixture-id",
            "minimal_empty_shell",
        ],
    );
    assert_success(&output, "legacy fallback route consumer");

    let stdout_rows = parse_stdout_jsonl(&output);
    assert_eq!(stdout_rows.len(), 1);
    assert_eq!(stdout_rows[0]["authoritative_family"], Value::Null);
    assert_eq!(stdout_rows[0]["effective_family"], "ares");
    assert_eq!(stdout_rows[0]["route_source"], "legacy_fallback");
    assert_eq!(stdout_rows[0]["fallback_applied"], true);
    assert_eq!(stdout_rows[0]["fingerprint_route_status"], "unmatched");

    let dir = artifact_dir(
        &artifacts.path().join("artifacts"),
        "consumer",
        "consumer-fallback",
    );
    let summary = read_json(&dir.join("consumer.summary.json"));
    assert_eq!(summary["authoritative_route_count"], 0);
    assert_eq!(summary["effective_route_count"], 1);
    assert_eq!(summary["fallback_route_count"], 1);
    assert_eq!(summary["unresolved_authoritative_count"], 1);
    assert_eq!(summary["unresolved_effective_count"], 0);
    assert_eq!(summary["dual_run_diff_count"], 1);
}

#[test]
fn consumer_cutover_doc_marks_legacy_as_non_authoritative() {
    let doc = fs::read_to_string(repo_path("docs/HTML_CONSUMER_CUTOVER.md"))
        .expect("read HTML consumer cutover doc");
    for required in [
        "html_route_consumer.sh",
        "diagnostic/fallback only",
        "route.diffs.jsonl",
        "--legacy-fallback-on-unresolved",
        "`fingerprint` is the source of truth",
    ] {
        assert!(
            doc.contains(required),
            "docs/HTML_CONSUMER_CUTOVER.md should mention '{required}'"
        );
    }
}
