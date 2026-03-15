use serde_json::Value;
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

fn run_script(script_name: &str, args: &[&str]) -> Output {
    Command::new("bash")
        .arg(script_path(script_name))
        .args(args)
        .env("FINGERPRINT_BIN", env!("CARGO_BIN_EXE_fingerprint"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run html e2e script")
}

fn write_definition(dir: &Path, filename: &str, contents: &str) {
    fs::write(dir.join(filename), contents).expect("write fingerprint definition");
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

fn artifact_dir(root: &Path, mode: &str, label: &str) -> PathBuf {
    root.join(mode).join(label)
}

fn artifact_listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .expect("read artifact dir")
        .map(|entry| {
            entry
                .expect("read artifact entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
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

fn smoke_rule() -> &'static str {
    r#"
fingerprint_id: html-smoke.v1
format: html
assertions:
  - heading_exists: "Schedule of Investments"
  - page_section_count:
      min: 3
      max: 3
  - table_min_rows:
      heading: "(?i)schedule of investments"
      min_rows: 1
"#
    .trim()
}

fn ares_near_miss_rule() -> &'static str {
    r#"
fingerprint_id: ares-near-miss.v1
format: html
assertions:
  - header_token_search:
      page: 2
      index: 0
      tokens:
        - "(?i)business\\s+description"
        - "(?i)coupon"
      min_matches: 2
  - full_width_row:
      pattern: "(?i)^energy$"
      min_cells: 6
"#
    .trim()
}

fn ares_winner_rule() -> &'static str {
    r#"
fingerprint_id: ares-winner.v1
format: html
assertions:
  - header_token_search:
      page: 2
      index: 0
      tokens:
        - "(?i)business\\s+description"
        - "(?i)coupon"
      min_matches: 2
  - full_width_row:
      pattern: "(?i)^(software|healthcare)$"
      min_cells: 6
"#
    .trim()
}

fn pennant_winner_rule() -> &'static str {
    r#"
fingerprint_id: pennant-winner.v1
format: html
assertions:
  - dominant_column_count:
      count: 5
      tolerance: 0
      sample_pages: 2
  - full_width_row:
      pattern: "(?i)^(first lien debt investments|equity investments)$"
      min_cells: 5
  - page_section_count:
      min: 2
      max: 2
"#
    .trim()
}

#[test]
fn html_smoke_script_happy_path_and_layout_are_stable() {
    let definitions = TempDir::new().expect("create definitions dir");
    write_definition(definitions.path(), "html-smoke.fp.yaml", smoke_rule());

    let artifacts = TempDir::new().expect("create artifacts dir");
    let artifact_root = artifacts.path().join("artifacts");
    let artifact_root_str = artifact_root.display().to_string();
    let definitions_str = definitions.path().display().to_string();

    let args = [
        "--definitions-dir",
        definitions_str.as_str(),
        "--fp",
        "html-smoke.v1",
        "--fixture-id",
        "generic_page_sections_schedule",
        "--artifact-root",
        artifact_root_str.as_str(),
        "--label",
        "smoke-happy",
    ];

    let first = run_script("html_smoke.sh", &args);
    assert_success(&first, "html smoke happy path");
    let second = run_script("html_smoke.sh", &args);
    assert_success(&second, "html smoke happy path rerun");

    let dir = artifact_dir(&artifact_root, "smoke", "smoke-happy");
    let expected_files = vec![
        "diagnostics.json",
        "duration_ms.txt",
        "exit_code.txt",
        "family.summary.json",
        "fixture.summary.jsonl",
        "manifest.jsonl",
        "request.json",
        "run.summary.json",
        "stderr.events.json",
        "stderr.jsonl",
        "stdout.jsonl",
        "stdout.records.json",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<_>>();
    assert_eq!(artifact_listing(&dir), expected_files);

    let summary = read_json(&dir.join("run.summary.json"));
    assert_eq!(summary["mode"], "smoke");
    assert_eq!(summary["matched_count"], 1);
    assert_eq!(summary["unmatched_count"], 0);
    assert_eq!(summary["refusal_count"], 0);
    assert!(
        summary["progress_event_count"]
            .as_u64()
            .expect("progress count")
            >= 1,
        "smoke run should capture progress events"
    );
}

#[test]
fn html_diagnose_script_captures_attempt_history_on_happy_path() {
    let definitions = TempDir::new().expect("create definitions dir");
    write_definition(
        definitions.path(),
        "ares-near-miss.fp.yaml",
        ares_near_miss_rule(),
    );
    write_definition(
        definitions.path(),
        "ares-winner.fp.yaml",
        ares_winner_rule(),
    );

    let artifacts = TempDir::new().expect("create artifacts dir");
    let output = run_script(
        "html_diagnose.sh",
        &[
            "--definitions-dir",
            &definitions.path().display().to_string(),
            "--fp",
            "ares-near-miss.v1",
            "--fp",
            "ares-winner.v1",
            "--fixture-id",
            "bdc_soi_ares_like",
            "--artifact-root",
            &artifacts.path().join("artifacts").display().to_string(),
            "--label",
            "diagnose-happy",
        ],
    );
    assert_success(&output, "html diagnose happy path");

    let dir = artifact_dir(
        &artifacts.path().join("artifacts"),
        "diagnose",
        "diagnose-happy",
    );
    let diagnostics = read_json(&dir.join("diagnostics.json"));
    let records = diagnostics["records"]
        .as_array()
        .expect("diagnostics records should be an array");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["fingerprint_id"], "ares-winner.v1");
    assert_eq!(
        records[0]["diagnostics"]["attempts"][0]["fingerprint_id"],
        "ares-near-miss.v1"
    );
    assert_eq!(
        records[0]["diagnostics"]["attempts"][1]["fingerprint_id"],
        "ares-winner.v1"
    );

    let summary = read_json(&dir.join("run.summary.json"));
    assert_eq!(summary["diagnostic_record_count"], 1);
    assert_eq!(summary["matched_count"], 1);
}

#[test]
fn html_family_matrix_script_writes_fixture_and_family_summaries() {
    let definitions = TempDir::new().expect("create definitions dir");
    write_definition(
        definitions.path(),
        "ares-winner.fp.yaml",
        ares_winner_rule(),
    );
    write_definition(
        definitions.path(),
        "pennant-winner.fp.yaml",
        pennant_winner_rule(),
    );

    let artifacts = TempDir::new().expect("create artifacts dir");
    let output = run_script(
        "html_family_matrix.sh",
        &[
            "--definitions-dir",
            &definitions.path().display().to_string(),
            "--fp",
            "ares-winner.v1",
            "--fp",
            "pennant-winner.v1",
            "--fixture-id",
            "bdc_soi_ares_like",
            "--fixture-id",
            "bdc_soi_pennant_like",
            "--artifact-root",
            &artifacts.path().join("artifacts").display().to_string(),
            "--label",
            "matrix-happy",
        ],
    );
    assert_success(&output, "html family matrix happy path");

    let dir = artifact_dir(
        &artifacts.path().join("artifacts"),
        "matrix",
        "matrix-happy",
    );
    let fixture_rows = read_jsonl(&dir.join("fixture.summary.jsonl"));
    assert_eq!(fixture_rows.len(), 2);
    assert!(
        fixture_rows
            .iter()
            .all(|row| row["matched"] == Value::Bool(true))
    );

    let family_summary = read_json(&dir.join("family.summary.json"));
    let families = family_summary["families"]
        .as_array()
        .expect("family summary should contain families");
    assert_eq!(families.len(), 2);
    assert!(
        families
            .iter()
            .any(|family| family["expected_family"] == "ares")
    );
    assert!(
        families
            .iter()
            .any(|family| family["expected_family"] == "pennant")
    );
}

#[test]
fn html_diagnose_script_negative_path_keeps_progress_and_diagnostics_artifacts() {
    let definitions = TempDir::new().expect("create definitions dir");
    write_definition(
        definitions.path(),
        "ares-winner.fp.yaml",
        ares_winner_rule(),
    );

    let artifacts = TempDir::new().expect("create artifacts dir");
    let output = run_script(
        "html_diagnose.sh",
        &[
            "--definitions-dir",
            &definitions.path().display().to_string(),
            "--fp",
            "ares-winner.v1",
            "--fixture-id",
            "bdc_soi_pennant_like",
            "--artifact-root",
            &artifacts.path().join("artifacts").display().to_string(),
            "--label",
            "diagnose-mismatch",
        ],
    );
    assert_failure(&output, "html diagnose mismatch", 1);

    let dir = artifact_dir(
        &artifacts.path().join("artifacts"),
        "diagnose",
        "diagnose-mismatch",
    );
    let summary = read_json(&dir.join("run.summary.json"));
    assert_eq!(summary["matched_count"], 0);
    assert_eq!(summary["unmatched_count"], 1);
    assert!(
        summary["progress_event_count"]
            .as_u64()
            .expect("progress count")
            >= 1,
        "mismatch runs should still capture progress"
    );

    let diagnostics = read_json(&dir.join("diagnostics.json"));
    let records = diagnostics["records"]
        .as_array()
        .expect("diagnostics records should be an array");
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0]["diagnostics"]["attempts"][0]["fingerprint_id"],
        "ares-winner.v1"
    );
}

#[test]
fn html_smoke_script_refusal_path_writes_summary_artifacts() {
    let artifacts = TempDir::new().expect("create artifacts dir");
    let output = run_script(
        "html_smoke.sh",
        &[
            "--fp",
            "missing-fingerprint.v1",
            "--fixture-id",
            "generic_page_sections_schedule",
            "--artifact-root",
            &artifacts.path().join("artifacts").display().to_string(),
            "--label",
            "smoke-refusal",
        ],
    );
    assert_failure(&output, "html smoke refusal", 2);

    let dir = artifact_dir(
        &artifacts.path().join("artifacts"),
        "smoke",
        "smoke-refusal",
    );
    let summary = read_json(&dir.join("run.summary.json"));
    assert_eq!(summary["refusal_count"], 1);
    assert_eq!(
        summary["refusal_codes"],
        serde_json::json!(["E_UNKNOWN_FP"])
    );

    let stdout_records = read_json(&dir.join("stdout.records.json"));
    assert_eq!(
        stdout_records["records"][0]["refusal"]["code"],
        "E_UNKNOWN_FP"
    );
}
