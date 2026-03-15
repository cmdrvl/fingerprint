use serde_json::{Value, json};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::{NamedTempFile, TempDir};

fn run_fingerprint_with_definitions(
    manifest_path: &Path,
    extra_args: &[&str],
    definitions_dir: &Path,
) -> Output {
    let trust_file = NamedTempFile::new().expect("create trust file");
    fs::write(trust_file.path(), "trust:\n  - \"installed:*\"\n").expect("write trust file");
    let mut command = Command::new(env!("CARGO_BIN_EXE_fingerprint"));
    command.arg(manifest_path);
    command.args(extra_args);
    command.env("FINGERPRINT_DEFINITIONS", definitions_dir);
    command.env("FINGERPRINT_TRUST", trust_file.path());
    command.current_dir(env!("CARGO_MANIFEST_DIR"));
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

fn text_record(path: &Path) -> Value {
    json!({
        "version": "hash.v0",
        "path": path.display().to_string(),
        "extension": ".txt",
        "bytes_hash": "blake3:test",
        "tool_versions": { "hash": "0.1.0" }
    })
}

fn write_definition(dir: &Path, filename: &str, contents: &str) {
    fs::write(dir.join(filename), contents).expect("write fingerprint definition");
}

fn routed_parent_rule() -> &'static str {
    r#"
fingerprint_id: routed-parent.v1
format: text
assertions:
  - filename_regex:
      pattern: "(?i)\\.txt$"
"#
    .trim()
}

fn alpha_child_rule() -> &'static str {
    r#"
fingerprint_id: routed-parent.v1/alpha.v1
parent: routed-parent.v1
format: text
assertions:
  - text_contains: "alpha"
"#
    .trim()
}

fn beta_child_rule() -> &'static str {
    r#"
fingerprint_id: routed-parent.v1/beta.v1
parent: routed-parent.v1
format: text
assertions:
  - text_contains: "beta"
"#
    .trim()
}

fn setup_definitions() -> TempDir {
    let definitions = TempDir::new().expect("create definitions dir");
    write_definition(
        definitions.path(),
        "routed-parent.fp.yaml",
        routed_parent_rule(),
    );
    write_definition(
        definitions.path(),
        "alpha-child.fp.yaml",
        alpha_child_rule(),
    );
    write_definition(definitions.path(), "beta-child.fp.yaml", beta_child_rule());
    definitions
}

fn run_case(contents: &str) -> (Output, Value) {
    let definitions = setup_definitions();
    let file = NamedTempFile::with_suffix(".txt").expect("create text file");
    fs::write(file.path(), contents).expect("write text fixture");
    let manifest = write_jsonl(&[text_record(file.path())]);
    let output = run_fingerprint_with_definitions(
        manifest.path(),
        &[
            "--fp",
            "routed-parent.v1",
            "--fp",
            "routed-parent.v1/alpha.v1",
            "--fp",
            "routed-parent.v1/beta.v1",
            "--no-witness",
        ],
        definitions.path(),
    );
    let records = parse_jsonl(&output.stdout);
    assert_eq!(records.len(), 1);
    (
        output,
        records.into_iter().next().expect("single output record"),
    )
}

#[test]
fn run_mode_chained_routing_selects_single_child_and_exits_zero() {
    let (output, record) = run_case("alpha only");

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(record["fingerprint"]["fingerprint_id"], "routed-parent.v1");
    assert_eq!(record["fingerprint"]["child_routing"]["status"], "selected");
    assert_eq!(
        record["fingerprint"]["child_routing"]["selected_child_fingerprint_id"],
        "routed-parent.v1/alpha.v1"
    );
    assert_eq!(
        record["fingerprint"]["child_routing"]["matched_child_count"],
        1
    );
}

#[test]
fn run_mode_chained_routing_zero_child_match_exits_partial() {
    let (output, record) = run_case("gamma only");

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(record["fingerprint"]["matched"], true);
    assert_eq!(
        record["fingerprint"]["child_routing"]["status"],
        "no_child_match"
    );
    assert_eq!(
        record["fingerprint"]["child_routing"]["matched_child_count"],
        0
    );
    assert_eq!(
        record["fingerprint"]["child_routing"]["selected_child_fingerprint_id"],
        Value::Null
    );
}

#[test]
fn run_mode_chained_routing_multiple_child_match_exits_partial() {
    let (output, record) = run_case("alpha beta");

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(record["fingerprint"]["matched"], true);
    assert_eq!(
        record["fingerprint"]["child_routing"]["status"],
        "ambiguous"
    );
    assert_eq!(
        record["fingerprint"]["child_routing"]["matched_child_count"],
        2
    );
    assert_eq!(
        record["fingerprint"]["child_routing"]["matched_child_fingerprint_ids"],
        json!(["routed-parent.v1/alpha.v1", "routed-parent.v1/beta.v1"])
    );
    assert_eq!(
        record["fingerprint"]["child_routing"]["selected_child_fingerprint_id"],
        Value::Null
    );
}
