use fingerprint::FingerprintResult;
use fingerprint::document::open_document_from_path;
use fingerprint::dsl::assertions::evaluate_named_assertions;
use fingerprint::dsl::content_hash::content_hash;
use fingerprint::dsl::extract::extract;
use fingerprint::dsl::parser::parse;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::{NamedTempFile, TempDir};

fn fixture(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

fn run_fingerprint(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fingerprint"))
        .args(args)
        .output()
        .expect("run fingerprint binary")
}

fn write_yaml(contents: &str) -> NamedTempFile {
    let file = NamedTempFile::with_suffix(".fp.yaml").expect("create yaml");
    fs::write(file.path(), contents).expect("write yaml");
    file
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

fn assert_failure(output: &Output, context: &str) {
    assert!(
        !output.status.success(),
        "{context} unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn html_compile_rule() -> String {
    r#"
fingerprint_id: compile-html.v1
format: html
assertions:
  - header_token_search:
      page: 2
      index: 0
      tokens:
        - "(?i)business\\s+description"
        - "(?i)coupon"
      min_matches: 2
  - dominant_column_count:
      count: 6
      tolerance: 0
      sample_pages: 2
  - full_width_row:
      pattern: "(?i)^(software|healthcare)$"
      min_cells: 6
  - page_section_count:
      min: 3
      max: 3
extract:
  - name: schedule_table
    type: table
    anchor_heading: "(?i)schedule of investments"
    index: 0
content_hash:
  algorithm: blake3
  over: [schedule_table]
"#
    .trim()
    .to_owned()
}

fn sanitize_crate_name(fingerprint_id: &str) -> String {
    let sanitized = fingerprint_id
        .replace('.', "-")
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();

    if sanitized
        .chars()
        .next()
        .is_none_or(|character| !character.is_ascii_alphabetic())
    {
        format!("fp-{sanitized}")
    } else {
        sanitized
    }
}

fn evaluate_dsl(yaml_path: &Path, doc_path: &Path) -> Value {
    let definition = parse(yaml_path).expect("parse DSL");
    let document = open_document_from_path(doc_path).expect("open document");
    let assertions = evaluate_named_assertions(&definition.assertions, &document);
    let matched = assertions.iter().all(|result| result.passed);
    let reason = assertions
        .iter()
        .find(|result| !result.passed)
        .and_then(|result| result.detail.clone());
    let extracted = if matched && !definition.extract.is_empty() {
        Some(extract(&document, &definition.extract).expect("extract from DSL"))
    } else {
        None
    };
    let content_hash = if matched {
        definition.content_hash.as_ref().and_then(|config| {
            extracted
                .as_ref()
                .map(|extracted| content_hash(extracted, &config.over))
        })
    } else {
        None
    };

    serde_json::to_value(FingerprintResult {
        matched,
        reason,
        assertions,
        extracted,
        content_hash,
    })
    .expect("serialize fingerprint result")
}

fn write_parity_harness(
    root: &TempDir,
    generated_dir: &Path,
    generated_package: &str,
) -> (PathBuf, PathBuf) {
    let harness_dir = root.path().join("harness");
    let src_dir = harness_dir.join("src");
    fs::create_dir_all(&src_dir).expect("create harness src");

    let cargo_toml = format!(
        r#"[package]
name = "compile-html-parity-harness"
version = "0.1.0"
edition = "2024"

[dependencies]
fingerprint = {{ path = "{repo}" }}
generated_fp = {{ path = "{generated}", package = "{generated_package}" }}
serde_json = "1.0"

[patch.crates-io]
fingerprint = {{ path = "{repo}" }}
"#,
        repo = env!("CARGO_MANIFEST_DIR"),
        generated = generated_dir.display(),
        generated_package = generated_package,
    );
    fs::write(harness_dir.join("Cargo.toml"), cargo_toml).expect("write harness Cargo.toml");

    let main_rs = r#"use fingerprint::document::open_document_from_path;
use fingerprint::Fingerprint;
use generated_fp::GeneratedFingerprint;
use std::path::Path;

fn main() {
    let doc_path = std::env::args().nth(1).expect("expected document path");
    let document = open_document_from_path(Path::new(&doc_path)).expect("open html document");
    let result = GeneratedFingerprint {}.fingerprint(&document);
    println!("{}", serde_json::to_string(&result).expect("serialize result"));
}
"#;
    fs::write(src_dir.join("main.rs"), main_rs).expect("write harness main.rs");

    let target_dir = root.path().join("cargo-target");
    (harness_dir, target_dir)
}

fn cargo_build_harness(manifest_path: &Path, target_dir: &Path) {
    let output = Command::new("cargo")
        .arg("build")
        .arg("--offline")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(manifest_path)
        .env("CARGO_TARGET_DIR", target_dir)
        .output()
        .expect("build parity harness");
    assert_success(&output, "cargo build parity harness");
}

fn run_parity_harness(binary_path: &Path, doc_path: &Path) -> Value {
    let output = Command::new(binary_path)
        .arg(doc_path)
        .output()
        .expect("run parity harness");
    assert_success(&output, "execute parity harness");
    serde_json::from_slice(&output.stdout).expect("parse parity harness JSON")
}

#[test]
fn compile_mode_html_stdout_is_deterministic_for_same_rule() {
    let yaml = write_yaml(&html_compile_rule());

    let first = run_fingerprint(&["compile", yaml.path().to_str().expect("yaml path")]);
    let second = run_fingerprint(&["compile", yaml.path().to_str().expect("yaml path")]);

    assert_success(&first, "first html compile to stdout");
    assert_success(&second, "second html compile to stdout");
    assert_eq!(
        first.stdout, second.stdout,
        "html codegen must be deterministic"
    );

    let generated = String::from_utf8(first.stdout).expect("generated rust should be utf8");
    assert!(generated.contains("HeaderTokenSearch"));
    assert!(generated.contains("DominantColumnCount"));
    assert!(generated.contains("FullWidthRow"));
    assert!(generated.contains("PageSectionCount"));
    assert!(generated.contains("fn format(&self) -> &str {\n        \"html\""));
}

#[test]
fn compile_mode_html_crate_builds_and_matches_dsl_runtime() {
    let yaml = write_yaml(&html_compile_rule());
    let root = TempDir::new().expect("create temp root");
    let generated_dir = root.path().join("generated-crate");

    let compile = run_fingerprint(&[
        "compile",
        yaml.path().to_str().expect("yaml path"),
        "--out",
        generated_dir.to_str().expect("generated dir"),
    ]);
    assert_success(&compile, "compile html rule to generated crate");

    let generated_package = sanitize_crate_name("compile-html.v1");
    let (harness_dir, target_dir) = write_parity_harness(&root, &generated_dir, &generated_package);
    cargo_build_harness(&harness_dir.join("Cargo.toml"), &target_dir);

    let binary_name = if cfg!(windows) {
        "compile-html-parity-harness.exe"
    } else {
        "compile-html-parity-harness"
    };
    let binary_path = target_dir.join("debug").join(binary_name);
    assert!(
        binary_path.exists(),
        "expected built parity harness at {}",
        binary_path.display()
    );

    let matching_doc = fixture("tests/fixtures/html/bdc_soi_ares_like.html");
    let non_matching_doc = fixture("tests/fixtures/html/bdc_soi_pennant_like.html");

    let compiled_match = run_parity_harness(&binary_path, &matching_doc);
    let dsl_match = evaluate_dsl(yaml.path(), &matching_doc);
    assert_eq!(
        compiled_match, dsl_match,
        "compiled html fingerprint must match DSL runtime on matching fixture"
    );

    let compiled_non_match = run_parity_harness(&binary_path, &non_matching_doc);
    let dsl_non_match = evaluate_dsl(yaml.path(), &non_matching_doc);
    assert_eq!(
        compiled_non_match, dsl_non_match,
        "compiled html fingerprint must match DSL runtime on non-matching fixture"
    );
}

#[test]
fn compile_mode_emits_compile_refusal_codes_for_html_definition_errors() {
    let missing_field = write_yaml(
        r#"
fingerprint_id: missing-format.v1
assertions:
  - page_section_count:
      min: 1
"#
        .trim(),
    );
    let unknown_assertion = write_yaml(
        r#"
fingerprint_id: unknown-assertion.v1
format: html
assertions:
  - html_magic:
      enabled: true
"#
        .trim(),
    );
    let invalid_html_params = write_yaml(
        r#"
fingerprint_id: invalid-html-params.v1
format: html
assertions:
  - page_section_count: {}
"#
        .trim(),
    );

    let missing_field_output = run_fingerprint(&[
        "compile",
        missing_field.path().to_str().expect("missing field path"),
        "--check",
    ]);
    let unknown_assertion_output = run_fingerprint(&[
        "compile",
        unknown_assertion
            .path()
            .to_str()
            .expect("unknown assertion path"),
        "--check",
    ]);
    let invalid_html_params_output = run_fingerprint(&[
        "compile",
        invalid_html_params
            .path()
            .to_str()
            .expect("invalid html params path"),
        "--check",
    ]);

    assert_failure(&missing_field_output, "compile missing field");
    assert_failure(&unknown_assertion_output, "compile unknown assertion");
    assert_failure(&invalid_html_params_output, "compile invalid html params");

    let missing_field_json: Value =
        serde_json::from_slice(&missing_field_output.stdout).expect("parse missing field refusal");
    let unknown_assertion_json: Value = serde_json::from_slice(&unknown_assertion_output.stdout)
        .expect("parse unknown assertion refusal");
    let invalid_html_params_json: Value =
        serde_json::from_slice(&invalid_html_params_output.stdout)
            .expect("parse invalid params refusal");

    assert_eq!(missing_field_json["refusal"]["code"], "E_MISSING_FIELD");
    assert_eq!(
        missing_field_json["refusal"]["detail"]["missing_field"],
        "format"
    );
    assert_eq!(
        unknown_assertion_json["refusal"]["code"],
        "E_UNKNOWN_ASSERTION"
    );
    assert_eq!(
        invalid_html_params_json["refusal"]["code"],
        "E_INVALID_YAML"
    );
    assert!(
        invalid_html_params_json["refusal"]["detail"]["error"]
            .as_str()
            .expect("invalid yaml detail error")
            .contains("requires at least one of 'min' or 'max'")
    );
}
