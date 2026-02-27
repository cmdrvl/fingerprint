use fingerprint::document::{Document, PdfDocument, XlsxDocument};
use fingerprint::dsl::assertions::{Assertion, evaluate_assertion};
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::NamedTempFile;

fn fixture(path: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

fn run_fingerprint(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fingerprint"))
        .args(args)
        .output()
        .expect("run fingerprint binary")
}

fn create_manifest(content: &str) -> NamedTempFile {
    let file = NamedTempFile::new().expect("create manifest");
    fs::write(file.path(), content).expect("write manifest");
    file
}

#[test]
fn malformed_jsonl_returns_bad_input_refusal() {
    let manifest = fixture("tests/fixtures/manifests/malformed_input.jsonl");
    let output = run_fingerprint(&["--fp", "csv.v0", manifest.to_str().expect("manifest path")]);

    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8(output.stdout).expect("stdout utf8");
    let refusal: Value = serde_json::from_str(stdout.trim()).expect("parse refusal envelope");
    assert_eq!(refusal["outcome"], "REFUSAL");
    assert_eq!(refusal["refusal"]["code"], "E_BAD_INPUT");
}

#[test]
fn unknown_fingerprint_returns_unknown_fp_refusal() {
    let manifest_content = format!(
        r#"{{"version":"hash.v0","path":"{}","extension":".csv","bytes_hash":"sha256:test","tool_versions":{{"hash":"0.1.0"}}}}"#,
        fixture("tests/fixtures/files/sample.csv").display()
    );
    let manifest = create_manifest(&manifest_content);
    let output = run_fingerprint(&[
        "--fp",
        "nonexistent-fingerprint.v999",
        manifest.path().to_str().expect("manifest path"),
    ]);

    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8(output.stdout).expect("stdout utf8");
    let refusal: Value = serde_json::from_str(stdout.trim()).expect("parse refusal envelope");
    assert_eq!(refusal["refusal"]["code"], "E_UNKNOWN_FP");
}

#[test]
fn parse_fail_manifest_returns_partial_exit() {
    let manifest = create_manifest(
        r#"{"version":"hash.v0","path":"/definitely/missing.md","extension":".md","bytes_hash":"sha256:deadbeef","tool_versions":{"hash":"0.1.0"}}"#,
    );
    let output = run_fingerprint(&[
        "--fp",
        "markdown.v0",
        manifest.path().to_str().expect("manifest path"),
    ]);
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn sheet_not_found_assertion_reports_detail() {
    let path = fixture("tests/fixtures/files/sample.xlsx");
    let doc = Document::Xlsx(XlsxDocument { path });
    let assertion = Assertion::SheetExists("NonexistentSheet".to_owned());

    let result = evaluate_assertion(&doc, &assertion).expect("evaluate sheet_exists");
    assert!(!result.passed);
    let detail = result.detail.unwrap_or_default();
    assert!(detail.contains("NonexistentSheet"));
}

#[test]
fn text_assertions_fail_without_pdf_text_path() {
    let path = fixture("tests/fixtures/test_files/report.pdf");
    let doc = Document::Pdf(PdfDocument { path, text: None });
    let assertion = Assertion::TextContains("any text".to_owned());

    let result = evaluate_assertion(&doc, &assertion).expect("evaluate text assertion");
    assert!(!result.passed);
}
