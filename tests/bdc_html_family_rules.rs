use fingerprint::compile::validate::validate_definition;
use fingerprint::dsl::parser::parse;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::NamedTempFile;

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn rules_dir() -> PathBuf {
    repo_path("rules")
}

fn script_path(name: &str) -> PathBuf {
    repo_path(&format!("scripts/{name}"))
}

fn fixture_path(id: &str) -> PathBuf {
    repo_path(&format!("tests/fixtures/html/{id}.html"))
}

fn write_jsonl(records: &[Value]) -> NamedTempFile {
    let mut file = NamedTempFile::new().expect("create temp manifest");
    for record in records {
        serde_json::to_writer(&mut file, record).expect("serialize manifest record");
        file.write_all(b"\n").expect("write newline");
    }
    file.flush().expect("flush manifest file");
    file
}

fn html_record(id: &str) -> Value {
    json!({
        "version": "hash.v0",
        "path": fixture_path(id).display().to_string(),
        "extension": ".html",
        "bytes_hash": format!("blake3:{id}"),
        "tool_versions": { "hash": "0.1.0" }
    })
}

fn parse_jsonl(stdout: &[u8]) -> Vec<Value> {
    String::from_utf8(stdout.to_vec())
        .expect("stdout UTF-8")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("parse JSON line"))
        .collect()
}

fn run_fingerprint(manifest_path: &Path, extra_args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fingerprint"));
    command.arg(manifest_path);
    command.args(extra_args);
    command.env("FINGERPRINT_DEFINITIONS", rules_dir());
    command.output().expect("run fingerprint binary")
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

const RULE_FILES: &[&str] = &[
    "bdc-soi.v1.fp.yaml",
    "bdc-soi-ares.v1.fp.yaml",
    "bdc-soi-bxsl.v1.fp.yaml",
    "bdc-soi-pennant.v1.fp.yaml",
    "bdc-soi-golub.v1.fp.yaml",
    "bdc-soi-blackrock.v1.fp.yaml",
];

const FAMILY_CASES: &[(&str, &str)] = &[
    ("bdc_soi_ares_like", "bdc-soi-ares.v1"),
    ("bdc_soi_bxsl_like", "bdc-soi-bxsl.v1"),
    ("bdc_soi_pennant_like", "bdc-soi-pennant.v1"),
    ("bdc_soi_golub_like", "bdc-soi-golub.v1"),
    ("bdc_soi_blackrock_like", "bdc-soi-blackrock.v1"),
];

#[test]
fn bdc_rule_files_parse_validate_and_compile_check() {
    for filename in RULE_FILES {
        let path = rules_dir().join(filename);
        let definition = parse(&path).unwrap_or_else(|error| panic!("parse {filename}: {error}"));
        validate_definition(&definition)
            .unwrap_or_else(|error| panic!("validate {filename}: {error}"));

        let output = Command::new(env!("CARGO_BIN_EXE_fingerprint"))
            .args(["compile", path.to_str().expect("utf-8 path"), "--check"])
            .output()
            .expect("run compile --check");
        assert!(
            output.status.success(),
            "compile --check failed for {filename}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }
}

#[test]
fn bdc_parent_matches_family_fixtures_and_rejects_shared_negatives() {
    let mut records: Vec<Value> = FAMILY_CASES
        .iter()
        .map(|(fixture_id, _)| html_record(fixture_id))
        .collect();
    for negative in [
        "generic_page_sections_schedule",
        "ambiguity_trap_dual_headers",
        "minimal_empty_shell",
        "malformed_static_schedule",
    ] {
        records.push(html_record(negative));
    }

    let manifest = write_jsonl(&records);
    let output = run_fingerprint(manifest.path(), &["--fp", "bdc-soi.v1", "--no-witness"]);
    assert_eq!(output.status.code(), Some(1));

    let lines = parse_jsonl(&output.stdout);
    assert_eq!(lines.len(), records.len());

    for (index, (fixture_id, _)) in FAMILY_CASES.iter().enumerate() {
        assert_eq!(
            lines[index]["path"],
            fixture_path(fixture_id).display().to_string()
        );
        assert_eq!(lines[index]["fingerprint"]["fingerprint_id"], "bdc-soi.v1");
        assert_eq!(lines[index]["fingerprint"]["matched"], true);
    }

    for line in lines.iter().skip(FAMILY_CASES.len()) {
        assert_eq!(line["fingerprint"]["matched"], false);
    }
}

#[test]
fn bdc_family_matrix_resolves_expected_leaf_routes_without_ambiguity() {
    let manifest = write_jsonl(
        &FAMILY_CASES
            .iter()
            .map(|(fixture_id, _)| html_record(fixture_id))
            .collect::<Vec<_>>(),
    );
    let output = run_fingerprint(
        manifest.path(),
        &[
            "--fp",
            "bdc-soi.v1",
            "--fp",
            "bdc-soi-ares.v1",
            "--fp",
            "bdc-soi-blackrock.v1",
            "--fp",
            "bdc-soi-bxsl.v1",
            "--fp",
            "bdc-soi-pennant.v1",
            "--fp",
            "bdc-soi-golub.v1",
            "--no-witness",
        ],
    );
    assert_eq!(output.status.code(), Some(0));

    let lines = parse_jsonl(&output.stdout);
    assert_eq!(lines.len(), FAMILY_CASES.len());

    for ((fixture_id, expected_child_id), line) in FAMILY_CASES.iter().zip(lines.iter()) {
        assert_eq!(line["path"], fixture_path(fixture_id).display().to_string());
        assert_eq!(line["fingerprint"]["fingerprint_id"], "bdc-soi.v1");
        assert_eq!(line["fingerprint"]["matched"], true);
        assert_eq!(line["fingerprint"]["child_routing"]["status"], "selected");
        assert_eq!(
            line["fingerprint"]["child_routing"]["selected_child_fingerprint_id"],
            *expected_child_id
        );
        assert_eq!(
            line["fingerprint"]["child_routing"]["matched_child_count"],
            1
        );

        let children = line["fingerprint"]["children"]
            .as_array()
            .expect("children array");
        let matched_children: Vec<&Value> = children
            .iter()
            .filter(|child| child["matched"] == Value::Bool(true))
            .collect();
        assert_eq!(
            matched_children.len(),
            1,
            "fixture {fixture_id} should resolve exactly one child"
        );
        assert_eq!(matched_children[0]["fingerprint_id"], *expected_child_id);
        assert!(
            matched_children[0]["content_hash"]
                .as_str()
                .is_some_and(|hash| hash.starts_with("blake3:")),
            "fixture {fixture_id} should emit a content hash for the matched child"
        );
        assert!(
            children
                .iter()
                .filter(|child| child["matched"] == Value::Bool(false))
                .all(|child| child["content_hash"] == Value::Null),
            "fixture {fixture_id} should keep unmatched child hashes null"
        );
        assert_ne!(
            line["fingerprint"]["child_routing"]["status"],
            Value::String("ambiguous".to_owned()),
            "fixture {fixture_id} should not be ambiguous"
        );
    }
}

#[test]
fn bdc_harness_family_matrix_reports_expected_child_routes() {
    let artifacts = tempfile::TempDir::new().expect("create artifacts dir");
    let output = run_script(
        "html_family_matrix.sh",
        &[
            "--definitions-dir",
            &rules_dir().display().to_string(),
            "--fp",
            "bdc-soi.v1",
            "--fp",
            "bdc-soi-ares.v1",
            "--fp",
            "bdc-soi-blackrock.v1",
            "--fp",
            "bdc-soi-bxsl.v1",
            "--fp",
            "bdc-soi-pennant.v1",
            "--fp",
            "bdc-soi-golub.v1",
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
            "--artifact-root",
            &artifacts.path().join("artifacts").display().to_string(),
            "--label",
            "bdc-family-routes",
        ],
    );
    assert_success(&output, "bdc family matrix");

    let dir = artifact_dir(
        &artifacts.path().join("artifacts"),
        "matrix",
        "bdc-family-routes",
    );
    let fixture_rows = read_jsonl(&dir.join("fixture.summary.jsonl"));
    assert_eq!(fixture_rows.len(), FAMILY_CASES.len());
    let expected_by_fixture: HashMap<&str, &str> = FAMILY_CASES.iter().copied().collect();
    for row in &fixture_rows {
        let fixture_id = row["fixture_id"].as_str().expect("fixture id");
        let expected_child_id = expected_by_fixture
            .get(fixture_id)
            .copied()
            .expect("expected child route");
        assert_eq!(row["route_resolved"], Value::Bool(true));
        assert_eq!(row["resolved_fingerprint_id"], expected_child_id);
        assert_eq!(row["child_routing_status"], "selected");
    }

    let family_summary = read_json(&dir.join("family.summary.json"));
    for (_, expected_child_id) in FAMILY_CASES {
        assert_eq!(
            family_summary["matched_fingerprint_counts"][expected_child_id],
            1
        );
    }

    let run_summary = read_json(&dir.join("run.summary.json"));
    assert_eq!(run_summary["matched_count"], json!(FAMILY_CASES.len()));
    assert_eq!(run_summary["ambiguous_route_count"], 0);
    assert_eq!(
        run_summary["selected_child_count"],
        json!(FAMILY_CASES.len())
    );
}

#[test]
fn bdc_harness_diagnose_mismatch_keeps_child_route_artifacts() {
    let artifacts = tempfile::TempDir::new().expect("create artifacts dir");
    let output = run_script(
        "html_diagnose.sh",
        &[
            "--definitions-dir",
            &rules_dir().display().to_string(),
            "--fp",
            "bdc-soi.v1",
            "--fp",
            "bdc-soi-bxsl.v1",
            "--fp",
            "bdc-soi-pennant.v1",
            "--fp",
            "bdc-soi-golub.v1",
            "--fp",
            "bdc-soi-blackrock.v1",
            "--fixture-id",
            "bdc_soi_ares_like",
            "--artifact-root",
            &artifacts.path().join("artifacts").display().to_string(),
            "--label",
            "bdc-diagnose-mismatch",
        ],
    );
    assert_failure(&output, "bdc diagnose mismatch", 1);

    let dir = artifact_dir(
        &artifacts.path().join("artifacts"),
        "diagnose",
        "bdc-diagnose-mismatch",
    );
    let run_summary = read_json(&dir.join("run.summary.json"));
    assert_eq!(run_summary["matched_count"], 0);
    assert_eq!(run_summary["unmatched_count"], 1);

    let stdout_records = read_json(&dir.join("stdout.records.json"));
    let record = &stdout_records["records"][0];
    assert_eq!(record["fingerprint"]["fingerprint_id"], "bdc-soi.v1");
    assert_eq!(
        record["fingerprint"]["child_routing"]["status"],
        "no_child_match"
    );
    let children = record["fingerprint"]["children"]
        .as_array()
        .expect("children array");
    assert_eq!(children.len(), 4);
    assert!(
        children
            .iter()
            .all(|child| child["matched"] == Value::Bool(false)),
        "diagnose mismatch should retain failed sibling payloads for rule authoring"
    );
}
