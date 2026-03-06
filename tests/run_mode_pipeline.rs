use serde_json::{Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::{NamedTempFile, tempdir};

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn run_fingerprint(manifest_path: &Path, extra_args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fingerprint"));
    command.arg(manifest_path);
    command.args(extra_args);
    command.output().expect("run fingerprint binary")
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

fn parse_jsonl(stdout: &[u8]) -> Vec<Value> {
    let text = String::from_utf8(stdout.to_vec()).expect("stdout UTF-8");
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("parse JSON line"))
        .collect()
}

fn parse_witness_ledger(path: &Path) -> Vec<Value> {
    let contents = std::fs::read_to_string(path).expect("read witness ledger");
    contents
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("parse witness line"))
        .collect()
}

#[test]
fn run_mode_progress_flag_emits_structured_progress_events() {
    let csv_path = repo_path("tests/fixtures/files/sample.csv");
    let manifest = write_jsonl(&[json!({
        "version": "hash.v0",
        "path": csv_path.display().to_string(),
        "extension": ".csv",
        "bytes_hash": "blake3:csv",
        "tool_versions": { "hash": "0.1.0" }
    })]);

    let output = run_fingerprint(
        manifest.path(),
        &["--fp", "csv.v0", "--progress", "--no-witness"],
    );

    assert_eq!(output.status.code(), Some(0));
    let stderr_lines = parse_jsonl(&output.stderr);
    assert!(
        stderr_lines
            .iter()
            .any(|line| line["type"] == "progress" && line["tool"] == "fingerprint"),
        "progress mode should emit structured progress events to stderr"
    );
}

#[test]
fn run_mode_progress_flag_keeps_witness_failures_structured() {
    let csv_path = repo_path("tests/fixtures/files/sample.csv");
    let manifest = write_jsonl(&[json!({
        "version": "hash.v0",
        "path": csv_path.display().to_string(),
        "extension": ".csv",
        "bytes_hash": "blake3:csv",
        "tool_versions": { "hash": "0.1.0" }
    })]);
    let temp_dir = tempdir().expect("create tempdir");
    let blocker = temp_dir.path().join("blocked-parent");
    std::fs::write(&blocker, "not a directory").expect("create blocker file");
    let witness_path = blocker.join("witness.jsonl");

    let output = Command::new(env!("CARGO_BIN_EXE_fingerprint"))
        .arg(manifest.path())
        .args(["--fp", "csv.v0", "--progress"])
        .env("EPISTEMIC_WITNESS", &witness_path)
        .output()
        .expect("run fingerprint binary");

    assert_eq!(output.status.code(), Some(0));
    let stderr_lines = parse_jsonl(&output.stderr);
    assert!(
        stderr_lines.iter().any(|line| {
            line["type"] == "warning"
                && line["path"] == witness_path.display().to_string()
                && line["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("witness append failed"))
        }),
        "witness append failures should stay structured in progress mode"
    );
}

#[test]
fn run_mode_all_matched_exit_zero_and_preserves_order_with_jobs() {
    let csv_path = repo_path("tests/fixtures/files/sample.csv");
    let xlsx_path = repo_path("tests/fixtures/files/sample.xlsx");
    let pdf_path = repo_path("tests/fixtures/files/sample.pdf");
    let markdown_path = repo_path("tests/fixtures/files/sample.md");

    let records = vec![
        json!({
            "version": "hash.v0",
            "path": csv_path.display().to_string(),
            "extension": ".csv",
            "bytes_hash": "blake3:csv",
            "tool_versions": { "hash": "0.1.0" }
        }),
        json!({
            "version": "hash.v0",
            "path": xlsx_path.display().to_string(),
            "extension": ".xlsx",
            "bytes_hash": "blake3:xlsx",
            "tool_versions": { "hash": "0.1.0" }
        }),
        json!({
            "version": "hash.v0",
            "path": pdf_path.display().to_string(),
            "text_path": markdown_path.display().to_string(),
            "extension": ".pdf",
            "bytes_hash": "blake3:pdf",
            "tool_versions": { "hash": "0.1.0" }
        }),
    ];
    let manifest = write_jsonl(&records);

    let output = run_fingerprint(
        manifest.path(),
        &[
            "--fp",
            "csv.v0",
            "--fp",
            "xlsx.v0",
            "--fp",
            "pdf.v0",
            "--jobs",
            "4",
            "--no-witness",
        ],
    );

    assert_eq!(output.status.code(), Some(0));
    let lines = parse_jsonl(&output.stdout);
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0]["path"], records[0]["path"]);
    assert_eq!(lines[1]["path"], records[1]["path"]);
    assert_eq!(lines[2]["path"], records[2]["path"]);
    assert_eq!(lines[0]["fingerprint"]["matched"], true);
    assert_eq!(lines[1]["fingerprint"]["matched"], true);
    assert_eq!(lines[2]["fingerprint"]["matched"], true);
}

#[test]
fn run_mode_parse_failure_creates_new_skipped_and_exit_one() {
    let missing_markdown = repo_path("tests/fixtures/files/does-not-exist.md");
    let manifest = write_jsonl(&[json!({
        "version": "hash.v0",
        "path": missing_markdown.display().to_string(),
        "extension": ".md",
        "bytes_hash": "blake3:missing",
        "tool_versions": { "hash": "0.1.0" }
    })]);

    let output = run_fingerprint(manifest.path(), &["--fp", "markdown.v0", "--no-witness"]);

    assert_eq!(output.status.code(), Some(1));
    let lines = parse_jsonl(&output.stdout);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["_skipped"], true);
    assert_eq!(lines[0]["fingerprint"], Value::Null);
    assert_eq!(lines[0]["_warnings"][0]["code"], "E_PARSE");
}

#[test]
fn run_mode_witness_records_exact_output_hash_and_prev_chain() {
    let csv_path = repo_path("tests/fixtures/files/sample.csv");
    let manifest = write_jsonl(&[json!({
        "version": "hash.v0",
        "path": csv_path.display().to_string(),
        "extension": ".csv",
        "bytes_hash": "blake3:csv",
        "tool_versions": { "hash": "0.1.0" }
    })]);
    let witness_dir = tempdir().expect("create witness tempdir");
    let witness_path = witness_dir.path().join("witness.jsonl");

    let first = Command::new(env!("CARGO_BIN_EXE_fingerprint"))
        .arg(manifest.path())
        .args(["--fp", "csv.v0", "--jobs", "4"])
        .env("EPISTEMIC_WITNESS", &witness_path)
        .output()
        .expect("run fingerprint binary");
    let second = Command::new(env!("CARGO_BIN_EXE_fingerprint"))
        .arg(manifest.path())
        .args(["--fp", "csv.v0", "--jobs", "4"])
        .env("EPISTEMIC_WITNESS", &witness_path)
        .output()
        .expect("run fingerprint binary");

    assert_eq!(first.status.code(), Some(0));
    assert_eq!(second.status.code(), Some(0));

    let witness_rows = parse_witness_ledger(&witness_path);
    assert_eq!(witness_rows.len(), 2);

    let manifest_bytes = std::fs::read(manifest.path()).expect("read manifest bytes");
    let expected_input_hash = format!("blake3:{}", blake3::hash(&manifest_bytes).to_hex());
    let expected_first_output_hash = format!("blake3:{}", blake3::hash(&first.stdout).to_hex());
    let expected_second_output_hash = format!("blake3:{}", blake3::hash(&second.stdout).to_hex());

    assert_eq!(
        witness_rows[0]["inputs"][0]["path"],
        manifest.path().display().to_string()
    );
    assert_eq!(witness_rows[0]["inputs"][0]["hash"], expected_input_hash);
    assert_eq!(
        witness_rows[0]["inputs"][0]["bytes"],
        u64::try_from(manifest_bytes.len()).expect("manifest length fits u64")
    );
    assert_eq!(witness_rows[0]["params"]["jobs"], 4);
    assert_eq!(witness_rows[0]["output_hash"], expected_first_output_hash);
    assert!(
        witness_rows[0]["binary_hash"]
            .as_str()
            .is_some_and(|value| value.starts_with("blake3:"))
    );
    assert!(witness_rows[0]["prev"].is_null());
    assert_eq!(witness_rows[1]["prev"], witness_rows[0]["id"]);
    assert_eq!(witness_rows[1]["params"]["jobs"], 4);
    assert_eq!(witness_rows[1]["output_hash"], expected_second_output_hash);
}

#[test]
fn run_mode_refusal_appends_witness_record() {
    let mut manifest = NamedTempFile::new().expect("create malformed manifest");
    writeln!(
        manifest,
        "{{\"version\":\"hash.v0\",\"path\":\"{}\",\"extension\":\".csv\",\"bytes_hash\":\"blake3:csv\"}}",
        repo_path("tests/fixtures/files/sample.csv").display()
    )
    .expect("write first manifest line");
    writeln!(manifest, "{{\"version\":\"hash.v0\",\"path\":").expect("write malformed line");
    manifest.flush().expect("flush malformed manifest");

    let witness_dir = tempdir().expect("create witness tempdir");
    let witness_path = witness_dir.path().join("witness.jsonl");
    let output = Command::new(env!("CARGO_BIN_EXE_fingerprint"))
        .arg(manifest.path())
        .args(["--fp", "csv.v0"])
        .env("EPISTEMIC_WITNESS", &witness_path)
        .output()
        .expect("run fingerprint binary");

    assert_eq!(output.status.code(), Some(2));

    let witness_rows = parse_witness_ledger(&witness_path);
    assert_eq!(witness_rows.len(), 1);
    assert_eq!(witness_rows[0]["outcome"], "REFUSAL");
    assert_eq!(witness_rows[0]["exit_code"], 2);
    assert_eq!(
        witness_rows[0]["output_hash"],
        format!("blake3:{}", blake3::hash(&output.stdout).to_hex())
    );
    assert_eq!(
        witness_rows[0]["inputs"][0]["path"],
        manifest.path().display().to_string()
    );
}

#[test]
fn run_mode_upstream_skipped_passthrough_keeps_fingerprint_null() {
    let csv_path = repo_path("tests/fixtures/files/sample.csv");
    let manifest = write_jsonl(&[json!({
        "version": "hash.v0",
        "path": csv_path.display().to_string(),
        "_skipped": true,
        "tool_versions": { "hash": "0.1.0" }
    })]);

    let output = run_fingerprint(manifest.path(), &["--fp", "csv.v0", "--no-witness"]);

    assert_eq!(output.status.code(), Some(0));
    let lines = parse_jsonl(&output.stdout);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["_skipped"], true);
    assert_eq!(lines[0]["fingerprint"], Value::Null);
    assert_eq!(lines[0]["tool_versions"]["hash"], "0.1.0");
    assert_eq!(
        lines[0]["tool_versions"]["fingerprint"],
        env!("CARGO_PKG_VERSION")
    );
    assert!(lines[0].get("_warnings").is_none());
}

#[test]
fn run_mode_refusal_unknown_fingerprint_has_envelope_shape() {
    let csv_path = repo_path("tests/fixtures/files/sample.csv");
    let manifest = write_jsonl(&[json!({
        "version": "hash.v0",
        "path": csv_path.display().to_string(),
        "extension": ".csv",
        "bytes_hash": "blake3:csv",
        "tool_versions": { "hash": "0.1.0" }
    })]);

    let output = run_fingerprint(
        manifest.path(),
        &["--fp", "does-not-exist.v9", "--no-witness"],
    );

    assert_eq!(output.status.code(), Some(2));
    let lines = parse_jsonl(&output.stdout);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["version"], "fingerprint.v0");
    assert_eq!(lines[0]["outcome"], "REFUSAL");
    assert_eq!(lines[0]["refusal"]["code"], "E_UNKNOWN_FP");
    assert_eq!(
        lines[0]["refusal"]["detail"]["fingerprint_id"],
        "does-not-exist.v9"
    );
}

#[test]
fn run_mode_refusal_bad_input_has_envelope_shape() {
    let mut manifest = NamedTempFile::new().expect("create malformed manifest");
    writeln!(
        manifest,
        "{{\"version\":\"hash.v0\",\"path\":\"{}\",\"extension\":\".csv\",\"bytes_hash\":\"blake3:csv\"}}",
        repo_path("tests/fixtures/files/sample.csv").display()
    )
    .expect("write first manifest line");
    writeln!(manifest, "{{\"version\":\"hash.v0\",\"path\":").expect("write malformed line");
    manifest.flush().expect("flush malformed manifest");

    let output = run_fingerprint(manifest.path(), &["--fp", "csv.v0", "--no-witness"]);

    assert_eq!(output.status.code(), Some(2));
    let lines = parse_jsonl(&output.stdout);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["version"], "fingerprint.v0");
    assert_eq!(lines[0]["outcome"], "REFUSAL");
    assert_eq!(lines[0]["refusal"]["code"], "E_BAD_INPUT");
    assert!(
        lines[0]["refusal"]["detail"]["error"]
            .as_str()
            .expect("bad input detail error")
            .contains("invalid JSON")
    );
}
