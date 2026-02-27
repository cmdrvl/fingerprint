include!("run_mode_pipeline.rs");
/*
//! Integration tests for pipeline run mode - testing full CLI behavior from JSONL input to output.
//!
//! This covers all exit codes, passthrough behavior, ordering preservation, and refusal envelopes.

use serde_json::{Value, json};
use std::io::Write;
use std::process::Command;
use tempfile::NamedTempFile;

/// Helper to run the fingerprint CLI with given input and capture output.
fn run_fingerprint_cli(input_content: &str, fingerprints: &[&str]) -> (i32, String, String) {
    let mut input_file = NamedTempFile::new().expect("create input temp file");
    writeln!(input_file, "{}", input_content).expect("write input");
    input_file.flush().expect("flush input");

    let mut cmd = Command::new("cargo");
    cmd.args(["run", "--"]);

    // Add fingerprint IDs with --fp flags
    for fp_id in fingerprints {
        cmd.args(["--fp", fp_id]);
    }

    // Add input file path
    cmd.arg(input_file.path().to_str().unwrap());

    let output = cmd.output().expect("execute fingerprint command");
    let exit_code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    (exit_code, stdout, stderr)
}

/// Parse JSONL output into a vector of JSON values.
fn parse_jsonl_output(output: &str) -> Vec<Value> {
    output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("parse JSONL line"))
        .collect()
}

/// Create a valid hash record for testing.
fn hash_record(path: &str, extension: &str, bytes_hash: &str) -> Value {
    json!({
        "version": "hash.v0",
        "path": path,
        "extension": extension,
        "bytes_hash": bytes_hash,
        "hash_algorithm": "sha256",
        "tool_versions": { "hash": "0.1.0" }
    })
}

/// Create a hash record that's already marked as skipped upstream.
fn skipped_hash_record(path: &str, extension: &str, reason: &str) -> Value {
    json!({
        "version": "hash.v0",
        "path": path,
        "extension": extension,
        "bytes_hash": "blake3:skipped",
        "hash_algorithm": "sha256",
        "tool_versions": { "hash": "0.1.0" },
        "_skipped": true,
        "_skip_reason": reason
    })
}

#[test]
fn all_matched_path_returns_exit_zero() {
    let input = format!(
        "{}\n{}\n{}",
        hash_record(
            "tests/fixtures/files/sample.csv",
            ".csv",
            "sha256:67914d1705427997d9944e42cbc7960ead2f6d00450461841dce639f00acb98b"
        ),
        hash_record(
            "tests/fixtures/files/sample.xlsx",
            ".xlsx",
            "sha256:a803cf50170b512382815005de013609aede86a9ff7297a1e32cf6ffe3200552"
        ),
        hash_record(
            "tests/fixtures/files/sample.pdf",
            ".pdf",
            "sha256:35b6642920d50b818b7ec6ca534e17388a6088c8e5557d69ce2e779f9fb96b9b"
        )
    );

    let (exit_code, stdout, _stderr) =
        run_fingerprint_cli(&input, &["csv.v0", "xlsx.v0", "pdf.v0"]);

    assert_eq!(exit_code, 0, "All matched should return exit 0");

    let output_records = parse_jsonl_output(&stdout);
    assert_eq!(output_records.len(), 3, "Should have 3 output records");

    // Verify all records have fingerprint matches
    for record in &output_records {
        assert!(
            record.get("fingerprint").is_some(),
            "Should have fingerprint field"
        );
        assert_eq!(
            record["fingerprint"]["matched"], true,
            "All fingerprints should match"
        );
    }
}

#[test]
fn partial_no_match_returns_exit_one() {
    let input = format!(
        "{}\n{}",
        hash_record(
            "tests/fixtures/files/sample.csv",
            ".csv",
            "sha256:67914d1705427997d9944e42cbc7960ead2f6d00450461841dce639f00acb98b"
        ),
        hash_record(
            "tests/fixtures/files/nonexistent.xyz",
            ".xyz",
            "sha256:fake"
        )
    );

    let (exit_code, stdout, _stderr) = run_fingerprint_cli(&input, &["csv.v0", "xlsx.v0"]);

    assert_eq!(exit_code, 1, "Partial matches should return exit 1");

    let output_records = parse_jsonl_output(&stdout);
    assert_eq!(output_records.len(), 2, "Should have 2 output records");

    // First record should match
    assert_eq!(output_records[0]["fingerprint"]["matched"], true);

    // Second record should not match or be skipped
    let second_fingerprint = output_records[1].get("fingerprint");
    let is_skipped = output_records[1]
        .get("_skipped")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    assert!(
        second_fingerprint.is_none()
            || second_fingerprint.unwrap()["matched"] == false
            || is_skipped,
        "Second record should not match or be skipped"
    );
}

#[test]
fn upstream_skipped_records_pass_through_unchanged() {
    let input = format!(
        "{}\n{}",
        hash_record(
            "tests/fixtures/files/sample.csv",
            ".csv",
            "sha256:67914d1705427997d9944e42cbc7960ead2f6d00450461841dce639f00acb98b"
        ),
        skipped_hash_record(
            "tests/fixtures/files/skipped.txt",
            ".txt",
            "upstream filter"
        )
    );

    let (exit_code, stdout, _stderr) = run_fingerprint_cli(&input, &["csv.v0"]);

    assert_eq!(
        exit_code, 0,
        "Upstream-skipped passthrough should keep ALL_MATCHED"
    );

    let output_records = parse_jsonl_output(&stdout);
    assert_eq!(output_records.len(), 2, "Should have 2 output records");

    // First record should be processed normally
    assert!(output_records[0].get("fingerprint").is_some());

    // Second record should pass through as skipped
    assert_eq!(output_records[1]["_skipped"], true);
    assert_eq!(output_records[1]["_skip_reason"], "upstream filter");
    assert_eq!(output_records[1]["fingerprint"], Value::Null);
}

#[test]
fn parse_failures_create_new_skipped_records() {
    let input = hash_record("/definitely/missing.md", ".md", "sha256:deadbeef").to_string();

    let (exit_code, stdout, _stderr) = run_fingerprint_cli(&input, &["markdown.v0"]);

    assert_eq!(exit_code, 1, "Parse failures should return exit 1");

    let output_records = parse_jsonl_output(&stdout);
    assert_eq!(output_records.len(), 1, "Should have 1 output record");
    assert_eq!(output_records[0]["_skipped"], true);
    assert_eq!(output_records[0]["fingerprint"], Value::Null);
    assert_eq!(output_records[0]["_warnings"][0]["code"], "E_PARSE");
}

#[test]
fn malformed_input_triggers_refusal_exit_two() {
    let malformed_jsonl = r#"{"version":"hash.v0","path":"tests/fixtures/test_files/simple.csv","bytes_hash":"blake3:5555555555555555555555555555555555555555555555555555555555555555"}
{"version":"hash.v0","path":"tests/fixtures/test_files/report.pdf","bytes_hash":"#;

    let (exit_code, stdout, _stderr) = run_fingerprint_cli(malformed_jsonl, &["csv.v0"]);

    assert_eq!(
        exit_code, 2,
        "Malformed input should return exit 2 (refusal)"
    );

    let output_records = parse_jsonl_output(&stdout);
    assert_eq!(output_records.len(), 1, "Should emit one refusal envelope");
    assert_eq!(output_records[0]["outcome"], "REFUSAL");
    assert_eq!(output_records[0]["refusal"]["code"], "E_BAD_INPUT");
}

#[test]
fn unknown_fingerprint_id_triggers_refusal_exit_two() {
    let input = hash_record(
        "tests/fixtures/files/sample.csv",
        ".csv",
        "sha256:67914d1705427997d9944e42cbc7960ead2f6d00450461841dce639f00acb98b",
    )
    .to_string();

    let (exit_code, stdout, _stderr) = run_fingerprint_cli(&input, &["unknown-fingerprint.v99"]);

    assert_eq!(
        exit_code, 2,
        "Unknown fingerprint should return exit 2 (refusal)"
    );

    let output_records = parse_jsonl_output(&stdout);
    assert_eq!(output_records.len(), 1, "Should emit one refusal envelope");
    assert_eq!(output_records[0]["outcome"], "REFUSAL");
    assert_eq!(output_records[0]["refusal"]["code"], "E_UNKNOWN_FP");
}

#[test]
fn output_ordering_preserved_with_multiple_workers() {
    // Create input with identifiable ordering
    let input = (0..10)
        .map(|i| {
            hash_record(
                &format!("tests/fixtures/files/record_{}.csv", i),
                ".csv",
                "sha256:fake",
            )
            .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");

    let (exit_code, stdout, _stderr) = run_fingerprint_cli(&input, &["csv.v0"]);

    // Note: exit code may be 1 due to fake files not existing, but ordering should still be preserved
    assert!(exit_code <= 1, "Should not refuse processing");

    let output_records = parse_jsonl_output(&stdout);
    assert_eq!(output_records.len(), 10, "Should have 10 output records");

    // Verify ordering is preserved by checking paths
    for (i, record) in output_records.iter().enumerate() {
        let expected_path = format!("tests/fixtures/files/record_{}.csv", i);
        assert_eq!(
            record["path"], expected_path,
            "Record {} should maintain original ordering",
            i
        );
    }
}

#[test]
fn worker_setting_does_not_affect_determinism() {
    let input = format!(
        "{}\n{}\n{}",
        hash_record(
            "tests/fixtures/files/sample.csv",
            ".csv",
            "sha256:67914d1705427997d9944e42cbc7960ead2f6d00450461841dce639f00acb98b"
        ),
        hash_record(
            "tests/fixtures/files/sample.xlsx",
            ".xlsx",
            "sha256:a803cf50170b512382815005de013609aede86a9ff7297a1e32cf6ffe3200552"
        ),
        hash_record(
            "tests/fixtures/files/sample.pdf",
            ".pdf",
            "sha256:35b6642920d50b818b7ec6ca534e17388a6088c8e5557d69ce2e779f9fb96b9b"
        )
    );

    // Run with default workers
    let (exit_code_1, stdout_1, _) = run_fingerprint_cli(&input, &["csv.v0", "xlsx.v0", "pdf.v0"]);

    // For now, just verify both runs produce the same number of records and exit codes
    // In future iterations, we could add --workers flag and test multiple worker counts
    let (exit_code_2, stdout_2, _) = run_fingerprint_cli(&input, &["csv.v0", "xlsx.v0", "pdf.v0"]);

    assert_eq!(
        exit_code_1, exit_code_2,
        "Exit codes should be deterministic"
    );

    let records_1 = parse_jsonl_output(&stdout_1);
    let records_2 = parse_jsonl_output(&stdout_2);

    assert_eq!(
        records_1.len(),
        records_2.len(),
        "Record counts should be deterministic"
    );

    // Verify same paths in same order
    for (r1, r2) in records_1.iter().zip(records_2.iter()) {
        assert_eq!(
            r1["path"], r2["path"],
            "Record ordering should be deterministic"
        );
    }
}

#[test]
fn mixed_success_and_failure_scenario() {
    let input = format!(
        "{}\n{}\n{}\n{}",
        hash_record(
            "tests/fixtures/files/sample.csv",
            ".csv",
            "sha256:67914d1705427997d9944e42cbc7960ead2f6d00450461841dce639f00acb98b"
        ),
        skipped_hash_record(
            "tests/fixtures/files/skipped.txt",
            ".txt",
            "upstream filter"
        ),
        hash_record(
            "/definitely/missing.md",
            ".md",
            "sha256:deadbeef"
        ),
        hash_record(
            "tests/fixtures/files/sample.pdf",
            ".pdf",
            "sha256:35b6642920d50b818b7ec6ca534e17388a6088c8e5557d69ce2e779f9fb96b9b"
        )
    );

    let (exit_code, stdout, _stderr) =
        run_fingerprint_cli(&input, &["csv.v0", "markdown.v0", "pdf.v0"]);

    assert_eq!(exit_code, 1, "Mixed results should return exit 1");

    let output_records = parse_jsonl_output(&stdout);
    assert_eq!(output_records.len(), 4, "Should have 4 output records");

    // Record 0: CSV should match
    assert_eq!(output_records[0]["fingerprint"]["matched"], true);

    // Record 1: Should pass through as skipped
    assert_eq!(output_records[1]["_skipped"], true);
    assert_eq!(output_records[1]["_skip_reason"], "upstream filter");
    assert_eq!(output_records[1]["fingerprint"], Value::Null);

    // Record 2: Should be skipped due to parse failure
    assert_eq!(output_records[2]["_skipped"], true);
    assert_eq!(output_records[2]["fingerprint"], Value::Null);
    assert_eq!(output_records[2]["_warnings"][0]["code"], "E_PARSE");

    // Record 3: PDF should match
    assert_eq!(output_records[3]["fingerprint"]["matched"], true);
}
*/
