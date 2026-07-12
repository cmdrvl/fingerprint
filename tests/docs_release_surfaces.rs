use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::NamedTempFile;

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn run_fingerprint(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fingerprint"))
        .args(args)
        .output()
        .expect("run fingerprint binary")
}

fn run_fingerprint_with_rules(args: &[&str]) -> Output {
    let trust_file = NamedTempFile::new().expect("create trust file");
    fs::write(trust_file.path(), "trust:\n  - \"installed:*\"\n").expect("write trust file");
    Command::new(env!("CARGO_BIN_EXE_fingerprint"))
        .args(args)
        .env("FINGERPRINT_DEFINITIONS", repo_path("rules"))
        .env("FINGERPRINT_TRUST", trust_file.path())
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run fingerprint binary with repo rules")
}

fn write_yaml(contents: &str) -> NamedTempFile {
    let file = NamedTempFile::new().expect("create temp yaml");
    fs::write(file.path(), contents).expect("write temp yaml");
    file
}

fn html_manifest_for_fixture(relative: &str) -> NamedTempFile {
    let manifest = NamedTempFile::new().expect("create temp manifest");
    let record = serde_json::json!({
        "version": "hash.v0",
        "path": repo_path(relative).display().to_string(),
        "extension": ".html",
        "bytes_hash": "blake3:docs-smoke",
        "tool_versions": { "hash": "0.1.0" }
    });
    fs::write(
        manifest.path(),
        format!(
            "{}\n",
            serde_json::to_string(&record).expect("serialize manifest record")
        ),
    )
    .expect("write manifest");
    manifest
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

#[test]
fn pdf_authoring_decision_under_normalizer_freeze_is_recorded() {
    let plan = fs::read_to_string(repo_path("docs/PLAN.md")).expect("read docs/PLAN.md");
    for required in [
        "PDF authoring under the normalizer freeze",
        "Decision: **Option B",
        "PDF fingerprints stay on the frozen normalized text path",
        "selector assertion or `region` extract under `format: pdf` is a validation error",
        "page_count",
        "metadata_regex",
        "text_path",
    ] {
        assert!(
            plan.contains(required),
            "docs/PLAN.md should record the PDF authoring decision phrase '{required}'"
        );
    }
}

#[test]
fn pdf_selector_and_region_definitions_are_rejected_with_boundary_message() {
    let selector_yaml = write_yaml(
        r#"
fingerprint_id: pdf-selector-rejected.v1
format: pdf
assertions:
  - node_exists:
      selector: "h1"
"#
        .trim(),
    );
    let selector_output = run_fingerprint(&[
        "compile",
        selector_yaml.path().to_str().expect("selector yaml path"),
        "--check",
    ]);
    assert!(
        !selector_output.status.success(),
        "pdf selector definition should fail validation"
    );
    let selector_refusal: Value =
        serde_json::from_slice(&selector_output.stdout).expect("parse selector refusal");
    assert_eq!(selector_refusal["refusal"]["code"], "E_INVALID_YAML");
    let selector_error = selector_refusal["refusal"]["detail"]["error"]
        .as_str()
        .expect("selector detail error");
    assert!(selector_error.contains("html-only"));
    assert!(selector_error.contains("For pdf fingerprints, use text_regex/heading_regex"));

    let region_yaml = write_yaml(
        r#"
fingerprint_id: pdf-region-rejected.v1
format: pdf
assertions:
  - filename_regex:
      pattern: '.*\.pdf$'
extract:
  - name: schedule_region
    type: region
    anchor_selector: "h1"
    stop_selector: "h2"
"#
        .trim(),
    );
    let region_output = run_fingerprint(&[
        "compile",
        region_yaml.path().to_str().expect("region yaml path"),
        "--check",
    ]);
    assert!(
        !region_output.status.success(),
        "pdf region definition should fail validation"
    );
    let region_refusal: Value =
        serde_json::from_slice(&region_output.stdout).expect("parse region refusal");
    assert_eq!(region_refusal["refusal"]["code"], "E_INVALID_YAML");
    let region_error = region_refusal["refusal"]["detail"]["error"]
        .as_str()
        .expect("region detail error");
    assert!(region_error.contains("html-only"));
    assert!(region_error.contains("text_path-backed section/table/text_match extracts"));
}

#[test]
fn pdf_native_structural_and_text_path_definition_still_compiles() {
    let pdf_yaml = write_yaml(
        r#"
fingerprint_id: pdf-frozen-path-ok.v1
format: pdf
assertions:
  - page_count:
      min: 1
      max: 500
  - metadata_regex:
      key: "Creator"
      pattern: "(?i)(docling|pdf)"
  - text_regex:
      pattern: "(?i)income capitalization approach"
extract:
  - name: income_section
    type: section
    anchor_heading: "(?i)income capitalization approach"
  - name: as_of_date
    type: text_match
    anchor: "(?i)as of"
    pattern: "\\w+ \\d{1,2},? \\d{4}"
    within_chars: 100
content_hash:
  algorithm: blake3
  over: [income_section]
"#
        .trim(),
    );
    let output = run_fingerprint(&[
        "compile",
        pdf_yaml.path().to_str().expect("pdf yaml path"),
        "--check",
    ]);
    assert_success(&output, "pdf frozen-path definition compile --check");
}

#[test]
fn compile_schema_surface_advertises_html_assertions() {
    let output = run_fingerprint(&["compile", "--schema"]);
    assert_success(&output, "compile --schema");

    let schema: Value =
        serde_json::from_slice(&output.stdout).expect("compile schema should be valid json");
    let format_enum = schema["properties"]["format"]["enum"]
        .as_array()
        .expect("format enum should be present");
    assert!(
        format_enum.contains(&Value::String("html".to_owned())),
        "compile schema must advertise html format\nschema:\n{}",
        serde_json::to_string_pretty(&schema).expect("pretty schema"),
    );

    let defs = schema["$defs"]
        .as_object()
        .expect("$defs should be present in compile schema");
    for key in [
        "assertion_header_token_search",
        "assertion_dominant_column_count",
        "assertion_full_width_row",
        "assertion_page_section_count",
        "assertion_node_exists",
        "assertion_node_count",
        "assertion_node_text_regex",
        "assertion_attr_regex",
    ] {
        assert!(
            defs.contains_key(key),
            "compile schema missing html assertion key '{key}'"
        );
    }
}

#[test]
fn documented_bdc_html_command_smoke_selects_expected_child_route() {
    let manifest = html_manifest_for_fixture("tests/fixtures/html/bdc_soi_ares_like.html");
    let output = run_fingerprint_with_rules(&[
        "--no-witness",
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
        manifest.path().to_str().expect("manifest path"),
    ]);
    assert_success(&output, "documented bdc html command");

    let stdout = String::from_utf8(output.stdout).expect("stdout utf8");
    let line = stdout.lines().next().expect("one jsonl record");
    let record: Value = serde_json::from_str(line).expect("parse output record");
    assert_eq!(record["fingerprint"]["fingerprint_id"], "bdc-soi.v1");
    assert_eq!(record["fingerprint"]["matched"], true);
    assert_eq!(record["fingerprint"]["child_routing"]["status"], "selected");
    assert_eq!(
        record["fingerprint"]["child_routing"]["selected_child_fingerprint_id"],
        "bdc-soi-ares.v1"
    );
}

#[test]
fn docs_publish_html_workflow_verification_and_compatibility() {
    let readme = fs::read_to_string(repo_path("README.md")).expect("read README.md");
    for required in [
        "Deterministic assertions across every document structure",
        "header_token_search",
        "node_exists",
        "child_routing",
        "bdc-soi.v1",
        "fingerprint compile --schema",
        "docs/HTML_VERIFICATION.md",
        "E_UNKNOWN_ASSERTION",
        "E_INVALID_YAML",
    ] {
        assert!(
            readme.contains(required),
            "README.md should mention '{required}' so users can discover the HTML workflow"
        );
    }

    let verification = fs::read_to_string(repo_path("docs/HTML_VERIFICATION.md"))
        .expect("read docs/HTML_VERIFICATION.md");
    for required in [
        "stderr.events.json",
        "diagnostics.json",
        "fixture.summary.jsonl",
        "ambiguous_route_count",
        "selected_child_fingerprint_id",
    ] {
        assert!(
            verification.contains(required),
            "docs/HTML_VERIFICATION.md should mention '{required}'"
        );
    }

    let release = fs::read_to_string(repo_path("docs/release.md")).expect("read docs/release.md");
    for required in [
        "bash scripts/html_verify.sh",
        "html_parity_audit.sh",
        "cmdrvl/tap/fingerprint",
        "operator.json",
    ] {
        assert!(
            release.contains(required),
            "docs/release.md should mention '{required}'"
        );
    }
}
