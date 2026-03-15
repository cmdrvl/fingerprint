use serde_json::{Value, json};
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

fn write_witness_ledger(witness_path: &Path, records: &[Value]) {
    let mut contents = records
        .iter()
        .map(|record| serde_json::to_string(record).expect("serialize witness fixture"))
        .collect::<Vec<_>>()
        .join("\n");
    contents.push('\n');
    fs::write(witness_path, contents).expect("write witness ledger");
}

#[test]
fn smoke_describe_schema_and_list_exit_zero() {
    let describe = run_fingerprint(&["--describe"]);
    assert_eq!(describe.status.code(), Some(0));
    let describe_json: Value =
        serde_json::from_slice(&describe.stdout).expect("describe should be valid JSON");
    assert_eq!(describe_json["name"], "fingerprint");
    assert_eq!(
        describe_json["version"],
        env!("CARGO_PKG_VERSION"),
        "--describe version should stay aligned with Cargo.toml"
    );
    assert_eq!(
        describe_json["schema_version"], "operator.v0",
        "--describe must emit operator.v0 schema"
    );
    assert!(
        describe_json.get("exit_codes").is_some(),
        "--describe must include exit_codes"
    );
    assert!(
        describe_json.get("refusals").is_some(),
        "--describe must include refusals"
    );
    assert!(
        describe_json.get("pipeline").is_some(),
        "--describe must include pipeline"
    );
    assert_eq!(
        describe_json["capabilities"]["formats"],
        json!(["csv", "xlsx", "pdf", "html", "markdown", "text"]),
        "--describe must advertise every supported runtime format, including html"
    );
    let options = describe_json["options"]
        .as_array()
        .expect("--describe options should be an array");
    assert!(
        options.iter().any(|option| option["flag"] == "--diagnose"),
        "--describe must advertise the --diagnose flag"
    );

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
fn smoke_display_flags_short_circuit_before_arg_validation() {
    let describe = run_fingerprint(&["--describe", "--jobs", "nope"]);
    assert_eq!(describe.status.code(), Some(0));
    let describe_json: Value =
        serde_json::from_slice(&describe.stdout).expect("describe should be valid JSON");
    assert_eq!(describe_json["name"], "fingerprint");

    let schema = run_fingerprint(&["--schema", "--jobs", "nope"]);
    assert_eq!(schema.status.code(), Some(0));
    let schema_json: Value =
        serde_json::from_slice(&schema.stdout).expect("schema should be valid JSON");
    assert!(schema_json.get("properties").is_some());

    let list = run_fingerprint(&["--list", "--jobs", "nope"]);
    assert_eq!(list.status.code(), Some(0));
    let list_stdout = String::from_utf8(list.stdout).expect("list output utf8");
    assert!(list_stdout.contains("csv.v0"));

    let version = run_fingerprint(&["--version", "--jobs", "nope"]);
    assert_eq!(version.status.code(), Some(0));
    let version_stdout = String::from_utf8(version.stdout).expect("version output utf8");
    assert!(
        version_stdout.starts_with("fingerprint "),
        "version output should still be the semver banner"
    );
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

    let compile_schema = run_fingerprint(&["compile", "--schema"]);
    assert_eq!(compile_schema.status.code(), Some(0));
    let compile_schema_json: Value =
        serde_json::from_slice(&compile_schema.stdout).expect("compile schema should be JSON");
    assert_eq!(
        compile_schema_json["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    let format_enum = compile_schema_json["properties"]["format"]["enum"]
        .as_array()
        .expect("compile schema format enum");
    assert!(
        format_enum.contains(&Value::String("html".to_owned())),
        "compile schema must advertise html format"
    );
    for key in [
        "assertion_header_token_search",
        "assertion_dominant_column_count",
        "assertion_full_width_row",
        "assertion_page_section_count",
    ] {
        assert!(
            compile_schema_json["$defs"].get(key).is_some(),
            "compile schema missing html assertion key '{key}'"
        );
    }
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

#[test]
fn smoke_witness_filters_and_exit_codes() {
    let tempdir = tempfile::tempdir().expect("create tempdir");
    let witness_path = tempdir.path().join("witness.jsonl");
    write_witness_ledger(
        &witness_path,
        &[
            json!({
                "id": "blake3:one",
                "tool": "fingerprint",
                "version": "0.1.0",
                "binary_hash": "blake3:binary",
                "inputs": [{ "path": "stdin", "hash": "blake3:keep-1", "bytes": 10 }],
                "params": { "fingerprints": ["csv.v0"] },
                "outcome": "ALL_MATCHED",
                "exit_code": 0,
                "output_hash": "blake3:out-1",
                "ts": "2026-02-01T00:00:00Z"
            }),
            json!({
                "id": "blake3:two",
                "tool": "fingerprint",
                "version": "0.1.0",
                "binary_hash": "blake3:binary",
                "inputs": [{ "path": "stdin", "hash": "blake3:keep-2", "bytes": 20 }],
                "params": { "fingerprints": ["csv.v0"] },
                "outcome": "PARTIAL",
                "exit_code": 1,
                "output_hash": "blake3:out-2",
                "ts": "2026-02-02T00:00:00Z"
            }),
            json!({
                "id": "blake3:three",
                "tool": "hash",
                "version": "0.1.0",
                "binary_hash": "blake3:binary",
                "inputs": [{ "path": "stdin", "hash": "blake3:keep-3", "bytes": 30 }],
                "params": {},
                "outcome": "ALL_HASHED",
                "exit_code": 0,
                "output_hash": "blake3:out-3",
                "ts": "2026-02-03T00:00:00Z"
            }),
        ],
    );

    let query = run_fingerprint_with_witness(
        &[
            "witness",
            "query",
            "--tool",
            "fingerprint",
            "--outcome",
            "PARTIAL",
            "--json",
        ],
        &witness_path,
    );
    assert_eq!(query.status.code(), Some(0));
    let query_json: Vec<Value> =
        serde_json::from_slice(&query.stdout).expect("query output should be JSON");
    assert_eq!(query_json.len(), 1);
    assert_eq!(query_json[0]["id"], "blake3:two");

    let last = run_fingerprint_with_witness(
        &[
            "witness",
            "last",
            "--tool",
            "fingerprint",
            "--since",
            "2026-02-02T00:00:00Z",
            "--json",
        ],
        &witness_path,
    );
    assert_eq!(last.status.code(), Some(0));
    let last_json: Value =
        serde_json::from_slice(&last.stdout).expect("last output should be JSON");
    assert_eq!(last_json["id"], "blake3:two");

    let count = run_fingerprint_with_witness(
        &[
            "witness",
            "count",
            "--tool",
            "fingerprint",
            "--input-hash",
            "keep",
            "--json",
        ],
        &witness_path,
    );
    assert_eq!(count.status.code(), Some(0));
    let count_json: Value =
        serde_json::from_slice(&count.stdout).expect("count output should be JSON");
    assert_eq!(count_json["count"], 2);

    let no_matches = run_fingerprint_with_witness(
        &[
            "witness",
            "query",
            "--tool",
            "fingerprint",
            "--since",
            "2027-01-01T00:00:00Z",
            "--json",
        ],
        &witness_path,
    );
    assert_eq!(no_matches.status.code(), Some(1));
    let no_match_json: Vec<Value> =
        serde_json::from_slice(&no_matches.stdout).expect("no-match query should be JSON");
    assert!(no_match_json.is_empty());

    let zero_count = run_fingerprint_with_witness(
        &[
            "witness",
            "count",
            "--tool",
            "fingerprint",
            "--since",
            "2027-01-01T00:00:00Z",
        ],
        &witness_path,
    );
    assert_eq!(zero_count.status.code(), Some(1));
    let zero_count_stdout = String::from_utf8(zero_count.stdout).expect("count output utf8");
    assert_eq!(zero_count_stdout.trim(), "0");
}

#[test]
fn smoke_witness_invalid_timestamp_filter_exits_two() {
    let tempdir = tempfile::tempdir().expect("create tempdir");
    let witness_path = tempdir.path().join("witness.jsonl");
    write_witness_ledger(
        &witness_path,
        &[json!({
            "id": "blake3:one",
            "tool": "fingerprint",
            "version": "0.1.0",
            "binary_hash": "blake3:binary",
            "inputs": [],
            "params": {},
            "outcome": "ALL_MATCHED",
            "exit_code": 0,
            "output_hash": "blake3:out-1",
            "ts": "2026-02-01T00:00:00Z"
        })],
    );

    let query = run_fingerprint_with_witness(
        &["witness", "query", "--since", "not-a-timestamp", "--json"],
        &witness_path,
    );
    assert_eq!(query.status.code(), Some(2));
    let stderr = String::from_utf8(query.stderr).expect("stderr utf8");
    assert!(stderr.contains("invalid --since"));
}
