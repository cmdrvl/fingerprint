use std::io::Write;
use std::process::Command;

use tempfile::NamedTempFile;

fn fingerprint_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_fingerprint"))
}

fn write_temp_file(content: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().expect("create temp file");
    file.write_all(content.as_bytes()).expect("write temp file");
    file.flush().expect("flush temp file");
    file
}

#[test]
fn struct_check_complete_outcome() {
    let rules = write_temp_file(
        r#"
rules:
  - id: monthly-package.v1
    group_by: "*/packages/P*"
    required:
      - "*.pdf"
      - "*_summary.xlsx"
    optional:
      - "*_notes.txt"
"#,
    );

    let input = write_temp_file(
        &[
            r#"{"version":"vacuum.v0","relative_path":"org/packages/P20240101/report.pdf"}"#,
            r#"{"version":"vacuum.v0","relative_path":"org/packages/P20240101/jan_summary.xlsx"}"#,
        ]
        .join("\n"),
    );

    let output = fingerprint_bin()
        .args([
            "struct-check",
            "--rules",
            rules.path().to_str().unwrap(),
            "--input",
            input.path().to_str().unwrap(),
        ])
        .output()
        .expect("run struct-check");

    assert!(
        output.status.success(),
        "exit code should be 0 for complete: got {}",
        output.status.code().unwrap_or(-1)
    );

    let stdout = String::from_utf8(output.stdout).expect("valid UTF-8");
    let record: serde_json::Value = serde_json::from_str(stdout.trim()).expect("parse output JSON");

    assert_eq!(record["version"], "struct-check.v0");
    assert_eq!(record["rule_id"], "monthly-package.v1");
    assert_eq!(record["group_pattern"], "*/packages/P*");
    assert_eq!(record["matched_directory"], "org/packages/P20240101");
    assert_eq!(record["outcome"], "complete");
    assert!(record["missing"].as_array().unwrap().is_empty());
    assert!(
        record["tool_versions"]["fingerprint"]
            .as_str()
            .unwrap()
            .starts_with("0.")
    );
}

#[test]
fn struct_check_partial_outcome() {
    let rules = write_temp_file(
        r#"
rules:
  - id: monthly-package.v1
    group_by: "*/packages/P*"
    required:
      - "*.pdf"
      - "*_summary.xlsx"
    optional: []
"#,
    );

    // Only the PDF is present, not the summary spreadsheet
    let input = write_temp_file(
        r#"{"version":"vacuum.v0","relative_path":"org/packages/P20240101/report.pdf"}"#,
    );

    let output = fingerprint_bin()
        .args([
            "struct-check",
            "--rules",
            rules.path().to_str().unwrap(),
            "--input",
            input.path().to_str().unwrap(),
        ])
        .output()
        .expect("run struct-check");

    assert_eq!(
        output.status.code(),
        Some(1),
        "exit code should be 1 for partial"
    );

    let stdout = String::from_utf8(output.stdout).expect("valid UTF-8");
    let record: serde_json::Value = serde_json::from_str(stdout.trim()).expect("parse output JSON");

    assert_eq!(record["outcome"], "partial");
    let missing = record["missing"].as_array().unwrap();
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0], "*_summary.xlsx");
}

#[test]
fn struct_check_empty_outcome() {
    let rules = write_temp_file(
        r#"
rules:
  - id: monthly-package.v1
    group_by: "*/packages/P*"
    required:
      - "*.pdf"
      - "*_summary.xlsx"
    optional: []
"#,
    );

    // Only an unexpected file, none of the required patterns match
    let input = write_temp_file(
        r#"{"version":"vacuum.v0","relative_path":"org/packages/P20240101/draft.docx"}"#,
    );

    let output = fingerprint_bin()
        .args([
            "struct-check",
            "--rules",
            rules.path().to_str().unwrap(),
            "--input",
            input.path().to_str().unwrap(),
        ])
        .output()
        .expect("run struct-check");

    assert_eq!(
        output.status.code(),
        Some(1),
        "exit code should be 1 for empty"
    );

    let stdout = String::from_utf8(output.stdout).expect("valid UTF-8");
    let record: serde_json::Value = serde_json::from_str(stdout.trim()).expect("parse output JSON");

    assert_eq!(record["outcome"], "empty");
    let missing = record["missing"].as_array().unwrap();
    assert_eq!(missing.len(), 2);
    let unexpected = record["unexpected"].as_array().unwrap();
    assert_eq!(unexpected.len(), 1);
    assert_eq!(unexpected[0], "draft.docx");
}

#[test]
fn struct_check_unexpected_files_detected() {
    let rules = write_temp_file(
        r#"
rules:
  - id: monthly-package.v1
    group_by: "*/packages/P*"
    required:
      - "*.pdf"
    optional:
      - "*_notes.txt"
"#,
    );

    let input = write_temp_file(
        &[
            r#"{"version":"vacuum.v0","relative_path":"org/packages/P001/report.pdf"}"#,
            r#"{"version":"vacuum.v0","relative_path":"org/packages/P001/meeting_notes.txt"}"#,
            r#"{"version":"vacuum.v0","relative_path":"org/packages/P001/draft.docx"}"#,
        ]
        .join("\n"),
    );

    let output = fingerprint_bin()
        .args([
            "struct-check",
            "--rules",
            rules.path().to_str().unwrap(),
            "--input",
            input.path().to_str().unwrap(),
        ])
        .output()
        .expect("run struct-check");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("valid UTF-8");
    let record: serde_json::Value = serde_json::from_str(stdout.trim()).expect("parse output JSON");

    assert_eq!(record["outcome"], "complete");
    assert_eq!(record["unexpected"].as_array().unwrap(), &["draft.docx"]);
}

#[test]
fn struct_check_refuses_non_vacuum_input() {
    let rules = write_temp_file(
        r#"
rules:
  - id: test.v1
    group_by: "*"
    required:
      - "*.pdf"
"#,
    );

    let input =
        write_temp_file(r#"{"version":"hash.v0","path":"a.pdf","bytes_hash":"blake3:abc"}"#);

    let output = fingerprint_bin()
        .args([
            "struct-check",
            "--rules",
            rules.path().to_str().unwrap(),
            "--input",
            input.path().to_str().unwrap(),
        ])
        .output()
        .expect("run struct-check");

    assert_eq!(
        output.status.code(),
        Some(2),
        "exit code should be 2 for refusal"
    );

    let stderr = String::from_utf8(output.stderr).expect("valid UTF-8");
    assert!(
        stderr.contains("vacuum.v0"),
        "stderr should mention expected version: {stderr}"
    );
}

#[test]
fn struct_check_multiple_directories() {
    let rules = write_temp_file(
        r#"
rules:
  - id: pkg.v1
    group_by: "*/packages/P*"
    required:
      - "*.pdf"
    optional: []
"#,
    );

    let input = write_temp_file(
        &[
            r#"{"version":"vacuum.v0","relative_path":"org/packages/P001/report.pdf"}"#,
            r#"{"version":"vacuum.v0","relative_path":"org/packages/P002/summary.pdf"}"#,
            r#"{"version":"vacuum.v0","relative_path":"org/packages/P003/draft.docx"}"#,
        ]
        .join("\n"),
    );

    let output = fingerprint_bin()
        .args([
            "struct-check",
            "--rules",
            rules.path().to_str().unwrap(),
            "--input",
            input.path().to_str().unwrap(),
        ])
        .output()
        .expect("run struct-check");

    // P003 is missing the required *.pdf, so exit code 1
    assert_eq!(output.status.code(), Some(1));

    let stdout = String::from_utf8(output.stdout).expect("valid UTF-8");
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 3, "should emit 3 records");

    let r1: serde_json::Value = serde_json::from_str(lines[0]).expect("parse line 1");
    let r2: serde_json::Value = serde_json::from_str(lines[1]).expect("parse line 2");
    let r3: serde_json::Value = serde_json::from_str(lines[2]).expect("parse line 3");

    // Sorted by (rule_id, matched_directory)
    assert_eq!(r1["matched_directory"], "org/packages/P001");
    assert_eq!(r1["outcome"], "complete");
    assert_eq!(r2["matched_directory"], "org/packages/P002");
    assert_eq!(r2["outcome"], "complete");
    assert_eq!(r3["matched_directory"], "org/packages/P003");
    assert_eq!(r3["outcome"], "empty");
}
