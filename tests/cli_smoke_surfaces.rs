use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::NamedTempFile;

fn fixture(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

fn run_fingerprint(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fingerprint"))
        .args(args)
        .output()
        .expect("run fingerprint binary")
}

fn run_fingerprint_with_witness(args: &[&str], witness_path: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fingerprint"))
        .args(args)
        .env("EPISTEMIC_WITNESS", witness_path)
        .output()
        .expect("run fingerprint binary with witness path")
}

fn manifest_with_record(path: &Path, extension: &str) -> NamedTempFile {
    let manifest = NamedTempFile::new().expect("create temp manifest");
    let record = format!(
        r#"{{"version":"hash.v0","path":"{}","extension":"{}","bytes_hash":"sha256:test","tool_versions":{{"hash":"0.1.0"}}}}"#,
        path.display(),
        extension
    );
    fs::write(manifest.path(), record).expect("write manifest");
    manifest
}

fn manifest_with_content(content: &str) -> NamedTempFile {
    let manifest = NamedTempFile::new().expect("create temp manifest");
    fs::write(manifest.path(), content).expect("write manifest");
    manifest
}

#[test]
fn smoke_describe_schema_and_list_exit_zero() {
    let describe = run_fingerprint(&["--describe"]);
    assert_eq!(describe.status.code(), Some(0));
    let describe_json: Value =
        serde_json::from_slice(&describe.stdout).expect("describe should be valid JSON");
    assert_eq!(describe_json["name"], "fingerprint");

    let schema = run_fingerprint(&["--schema"]);
    assert_eq!(schema.status.code(), Some(0));
    let schema_json: Value =
        serde_json::from_slice(&schema.stdout).expect("schema should be valid JSON");
    assert!(schema_json.get("properties").is_some());

    let list = run_fingerprint(&["--list"]);
    assert_eq!(list.status.code(), Some(0));
    let list_stdout = String::from_utf8(list.stdout).expect("list output utf8");
    assert!(list_stdout.contains("csv.v0"));
}

#[test]
fn smoke_compile_invocation_and_check() {
    let dsl = r#"
fingerprint_id: smoke-compile.v1
format: csv
assertions:
  - filename_regex:
      pattern: "(?i).*\\.csv$"
"#
    .trim();
    let yaml = NamedTempFile::with_suffix(".fp.yaml").expect("create yaml");
    fs::write(yaml.path(), dsl).expect("write yaml");

    let check = run_fingerprint(&[
        "compile",
        yaml.path().to_str().expect("yaml path"),
        "--check",
    ]);
    assert_eq!(check.status.code(), Some(0));

    let compile = run_fingerprint(&["compile", yaml.path().to_str().expect("yaml path")]);
    assert_eq!(compile.status.code(), Some(0));
    let compile_stdout = String::from_utf8(compile.stdout).expect("compile output utf8");
    assert!(compile_stdout.contains("GeneratedFingerprint"));
}

#[test]
fn smoke_process_boundary_exit_codes_0_1_2() {
    let csv_manifest = manifest_with_record(&fixture("tests/fixtures/files/sample.csv"), ".csv");
    let ok = run_fingerprint(&[
        "--no-witness",
        "--fp",
        "csv.v0",
        csv_manifest.path().to_str().expect("manifest path"),
    ]);
    assert_eq!(ok.status.code(), Some(0));

    let missing_dir = tempfile::tempdir().expect("create missing tempdir");
    let missing_path = missing_dir.path().join("missing.md");
    let missing_manifest = manifest_with_content(&format!(
        r#"{{"version":"hash.v0","path":"{}","extension":".md","bytes_hash":"sha256:deadbeef","tool_versions":{{"hash":"0.1.0"}}}}"#,
        missing_path.display()
    ));
    let partial = run_fingerprint(&[
        "--no-witness",
        "--fp",
        "markdown.v0",
        missing_manifest.path().to_str().expect("missing manifest"),
    ]);
    assert_eq!(partial.status.code(), Some(1));

    let malformed = fixture("tests/fixtures/manifests/malformed_input.jsonl");
    let refusal = run_fingerprint(&[
        "--no-witness",
        "--fp",
        "csv.v0",
        malformed.to_str().expect("malformed manifest"),
    ]);
    assert_eq!(refusal.status.code(), Some(2));
}

#[test]
fn smoke_witness_query_last_count() {
    let tempdir = tempfile::tempdir().expect("create tempdir");
    let witness_path = tempdir.path().join("witness.jsonl");
    let csv_manifest = manifest_with_record(&fixture("tests/fixtures/files/sample.csv"), ".csv");

    let run = run_fingerprint_with_witness(
        &[
            "--fp",
            "csv.v0",
            csv_manifest.path().to_str().expect("manifest path"),
        ],
        &witness_path,
    );
    assert_eq!(run.status.code(), Some(0));

    let count = run_fingerprint_with_witness(&["witness", "count"], &witness_path);
    assert_eq!(count.status.code(), Some(0));
    let count_stdout = String::from_utf8(count.stdout).expect("count output utf8");
    assert_eq!(count_stdout.trim(), "1");

    let last = run_fingerprint_with_witness(&["witness", "last"], &witness_path);
    assert_eq!(last.status.code(), Some(0));
    let last_json: Value = serde_json::from_slice(&last.stdout).expect("last should be JSON");
    assert_eq!(last_json["tool"], "fingerprint");

    let query = run_fingerprint_with_witness(&["witness", "query"], &witness_path);
    assert_eq!(query.status.code(), Some(0));
    let query_line = String::from_utf8(query.stdout).expect("query output utf8");
    let first_line = query_line
        .lines()
        .next()
        .expect("query should return at least one witness record");
    let query_json: Value = serde_json::from_str(first_line).expect("query line should be JSON");
    assert_eq!(query_json["tool"], "fingerprint");
}
