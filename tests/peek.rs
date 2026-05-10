use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fixture(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("peek")
        .join(path)
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
        .expect("run fingerprint binary")
}

fn parse_stdout(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("stdout should be JSON")
}

#[test]
fn peek_reports_row_shape_without_run_mode_fingerprints() {
    let path = fixture("preamble.csv");
    let output = run_fingerprint(&[
        "peek",
        path.to_str().expect("fixture path"),
        "--rows",
        "8",
        "--json",
        "--suggest",
        "--no-witness",
    ]);

    assert_eq!(output.status.code(), Some(0));
    let envelope = parse_stdout(&output);
    assert_eq!(envelope["version"], "fingerprint.peek.v0");
    assert_eq!(envelope["outcome"], "SUCCESS");
    assert_eq!(envelope["subcommand"], "peek");
    assert_eq!(envelope["witness_id"], Value::Null);
    assert_eq!(envelope["result"]["summary"]["modal_column_count"], 4);
    assert_eq!(envelope["result"]["summary"]["header_at_row"], 4);
    assert_eq!(
        envelope["result"]["suggestions"]["profile_pre_parse"]["skip_rows"],
        3
    );
}

#[test]
fn peek_detects_units_row_for_profile_suggestions() {
    let path = fixture("units.csv");
    let output = run_fingerprint(&[
        "peek",
        path.to_str().expect("fixture path"),
        "--suggest",
        "--no-witness",
    ]);

    assert_eq!(output.status.code(), Some(0));
    let envelope = parse_stdout(&output);
    assert_eq!(
        envelope["result"]["suggestions"]["profile_pre_parse"]["mode"],
        "preamble_with_units"
    );
    assert_eq!(
        envelope["result"]["suggestions"]["profile_pre_parse"]["unit_rows"],
        Value::Array(vec![Value::from(3)])
    );
}

#[test]
fn peek_never_emits_cell_content_or_witness_params_with_cell_content() {
    let tempdir = tempfile::tempdir().expect("create tempdir");
    let witness_path = tempdir.path().join("witness.jsonl");
    let path = fixture("secret.csv");
    let output = run_fingerprint_with_witness(
        &["peek", path.to_str().expect("fixture path"), "--suggest"],
        &witness_path,
    );

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).expect("stdout utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr utf8");
    assert!(!stdout.contains("SUPER-SECRET-CELL-VALUE"));
    assert!(!stdout.contains("ANOTHER-PRIVATE-VALUE"));
    assert!(!stderr.contains("SUPER-SECRET-CELL-VALUE"));
    assert!(!stderr.contains("ANOTHER-PRIVATE-VALUE"));

    let witness = fs::read_to_string(&witness_path).expect("read witness ledger");
    assert!(!witness.contains("SUPER-SECRET-CELL-VALUE"));
    assert!(!witness.contains("ANOTHER-PRIVATE-VALUE"));
    assert!(parse_stdout_from_str(&stdout)["witness_id"].is_string());
}

#[test]
fn peek_refuses_empty_file_with_json_refusal() {
    let empty = tempfile::NamedTempFile::new().expect("create empty input");
    let path = empty.path();
    let output = run_fingerprint(&["peek", path.to_str().expect("fixture path"), "--no-witness"]);

    assert_eq!(output.status.code(), Some(2));
    let envelope = parse_stdout(&output);
    assert_eq!(envelope["version"], "fingerprint.peek.v0");
    assert_eq!(envelope["outcome"], "REFUSAL");
    assert_eq!(envelope["refusal"]["code"], "E_BAD_INPUT");
}

fn parse_stdout_from_str(stdout: &str) -> Value {
    serde_json::from_str(stdout).expect("stdout should be JSON")
}
