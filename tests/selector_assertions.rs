use fingerprint::document::{Document, HtmlDocument};
use fingerprint::dsl::assertions::{
    Assertion, NamedAssertion, evaluate_named_assertions_with_diagnose,
};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::{NamedTempFile, TempDir};

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn html_document(relative: &str) -> HtmlDocument {
    HtmlDocument::open(&repo_path(relative)).expect("open html fixture")
}

fn evaluate(
    assertion: Assertion,
    document: HtmlDocument,
) -> fingerprint::registry::AssertionResult {
    evaluate_named_assertions_with_diagnose(
        &[NamedAssertion {
            name: None,
            assertion,
        }],
        &Document::Html(document),
        true,
    )
    .into_iter()
    .next()
    .expect("one assertion result")
}

fn write_temp_html(contents: &str) -> NamedTempFile {
    let file = NamedTempFile::with_suffix(".html").expect("create temp html");
    fs::write(file.path(), contents).expect("write temp html");
    file
}

fn run_fingerprint(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fingerprint"))
        .args(args)
        .output()
        .expect("run fingerprint binary")
}

fn run_fingerprint_with_env(args: &[&str], envs: &[(&str, &Path)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fingerprint"));
    command.args(args);
    for (key, value) in envs {
        command.env(key, value);
    }
    command.output().expect("run fingerprint binary")
}

fn manifest_for(relative: &str) -> NamedTempFile {
    let path = repo_path(relative);
    let file = NamedTempFile::with_suffix(".jsonl").expect("create manifest");
    let record = json!({
        "version": "hash.v0",
        "path": path,
        "extension": ".html",
        "bytes_hash": "blake3:selector-test",
        "hash_algorithm": "blake3",
        "tool_versions": {"hash": "0.1.0"}
    });
    fs::write(
        file.path(),
        format!(
            "{}\n",
            serde_json::to_string(&record).expect("serialize manifest record")
        ),
    )
    .expect("write manifest");
    file
}

fn selector_snapshot(document: &HtmlDocument, selector: &str) -> Value {
    let nodes = document.select_nodes(selector).expect("select nodes");
    let matches: Vec<Value> = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            json!({
                "index": index,
                "name": node.value().name(),
                "text": node.text().collect::<Vec<_>>().join(" ").split_whitespace().collect::<Vec<_>>().join(" ")
            })
        })
        .collect();
    json!({
        "selector": selector,
        "match_count": matches.len(),
        "matches": matches
    })
}

#[test]
fn sel_u01_node_text_regex_matches_styled_ares_title_without_heading_tags() {
    let document = html_document("tests/fixtures/html/styled_heading_ares.html");
    assert!(
        document.headings.is_empty(),
        "fixture must prove selector support is not heading-normalizer support"
    );

    let result = evaluate(
        Assertion::NodeTextRegex {
            selector: "div[style*=\"text-align:center\"]".to_owned(),
            pattern: "(?i)schedules? of investments".to_owned(),
            min_matches: 1,
        },
        document,
    );

    assert!(result.passed, "unexpected failure: {:?}", result.detail);
}

#[test]
fn sel_u02_node_exists_matches_plain_h1_too() {
    let result = evaluate(
        Assertion::NodeExists {
            selector: "h1".to_owned(),
        },
        html_document("tests/fixtures/html/bdc_soi_ares_like.html"),
    );

    assert!(result.passed, "unexpected failure: {:?}", result.detail);
}

#[test]
fn sel_u03_node_exists_no_match_is_clean_assertion_failure() {
    let result = evaluate(
        Assertion::NodeExists {
            selector: "table".to_owned(),
        },
        html_document("tests/fixtures/html/minimal_empty_shell.html"),
    );

    assert!(!result.passed);
    assert_eq!(
        result.detail.as_deref(),
        Some("selector 'table' matched no nodes")
    );
    assert_eq!(result.name, "node_exists");
}

#[test]
fn sel_u04_node_count_matches_large_table_set() {
    let tables = (0..64)
        .map(|index| format!("<table><tr><td>table-{index}</td></tr></table>"))
        .collect::<String>();
    let html = write_temp_html(&format!("<html><body>{tables}</body></html>"));
    let document = HtmlDocument::open(html.path()).expect("open temp table fixture");

    let result = evaluate(
        Assertion::NodeCount {
            selector: "table".to_owned(),
            min: Some(60),
            max: None,
        },
        document,
    );

    assert!(result.passed, "unexpected failure: {:?}", result.detail);
}

#[test]
fn sel_u05_node_count_matches_attribute_substring_pagebreaks() {
    let result = evaluate(
        Assertion::NodeCount {
            selector: "[style*=\"page-break-after\"]".to_owned(),
            min: Some(60),
            max: Some(68),
        },
        html_document("tests/fixtures/html/oxsq_pagebreaks.html"),
    );

    assert!(result.passed, "unexpected failure: {:?}", result.detail);
}

#[test]
fn attr_regex_matches_selected_node_attributes() {
    let result = evaluate(
        Assertion::AttrRegex {
            selector: "p".to_owned(),
            attr: "class".to_owned(),
            pattern: "(?i)RRH".to_owned(),
            min_matches: 1,
        },
        html_document("tests/fixtures/html/styled_heading_ares.html"),
    );

    assert!(result.passed, "unexpected failure: {:?}", result.detail);
}

#[test]
fn sel_u06_malformed_selector_is_compile_refusal() {
    let yaml = NamedTempFile::with_suffix(".fp.yaml").expect("create yaml");
    fs::write(
        yaml.path(),
        r#"
fingerprint_id: malformed-selector.v1
format: html
assertions:
  - node_exists:
      selector: "div["
"#,
    )
    .expect("write yaml");

    let output = run_fingerprint(&[
        "compile",
        yaml.path().to_str().expect("yaml path"),
        "--check",
    ]);

    assert_eq!(output.status.code(), Some(2));
    let refusal: Value =
        serde_json::from_slice(&output.stdout).expect("compile refusal should be JSON");
    assert_eq!(refusal["refusal"]["code"], "E_INVALID_SELECTOR");
}

#[test]
fn sel_u06_malformed_installed_selector_is_load_refusal() {
    let definitions = TempDir::new().expect("create definitions dir");
    fs::write(
        definitions.path().join("malformed-selector.fp.yaml"),
        r#"
fingerprint_id: malformed-selector.v1
format: html
assertions:
  - node_exists:
      selector: "div["
"#,
    )
    .expect("write definition");
    let manifest = manifest_for("tests/fixtures/html/styled_heading_ares.html");

    let output = run_fingerprint_with_env(
        &[
            "--no-witness",
            "--fp",
            "malformed-selector.v1",
            manifest.path().to_str().expect("manifest path"),
        ],
        &[("FINGERPRINT_DEFINITIONS", definitions.path())],
    );

    assert_eq!(output.status.code(), Some(2));
    let refusal: Value =
        serde_json::from_slice(&output.stdout).expect("load refusal should be JSON");
    assert_eq!(refusal["refusal"]["code"], "E_INVALID_SELECTOR");
}

#[test]
fn sel_u07_selector_assertion_is_html_only_validation_error() {
    let yaml = NamedTempFile::with_suffix(".fp.yaml").expect("create yaml");
    fs::write(
        yaml.path(),
        r#"
fingerprint_id: selector-on-xlsx.v1
format: xlsx
assertions:
  - node_exists:
      selector: "h1"
"#,
    )
    .expect("write yaml");

    let output = run_fingerprint(&[
        "compile",
        yaml.path().to_str().expect("yaml path"),
        "--check",
    ]);

    assert_eq!(output.status.code(), Some(2));
    let refusal: Value =
        serde_json::from_slice(&output.stdout).expect("validation refusal should be JSON");
    assert_eq!(refusal["refusal"]["code"], "E_INVALID_YAML");
    assert!(
        refusal["refusal"]["detail"]["error"]
            .as_str()
            .expect("detail error")
            .contains("html-only")
    );
}

#[test]
fn sel_u08_selector_match_snapshot_is_deterministic() {
    let tables = (0..64)
        .map(|index| format!("<table><tr><td>table-{index}</td></tr></table>"))
        .collect::<String>();
    let html = write_temp_html(&format!("<html><body>{tables}</body></html>"));
    let document = HtmlDocument::open(html.path()).expect("open temp table fixture");

    let first = selector_snapshot(&document, "table");
    let second = selector_snapshot(&document, "table");
    assert_eq!(first["match_count"], 64);
    assert_eq!(
        serde_json::to_vec(&first).expect("serialize first snapshot"),
        serde_json::to_vec(&second).expect("serialize second snapshot")
    );
}

#[test]
fn selector_smoke_script_writes_expected_artifacts() {
    let definitions = TempDir::new().expect("create definitions dir");
    fs::write(
        definitions.path().join("selector-styled.fp.yaml"),
        r#"
fingerprint_id: selector-styled.v1
format: html
assertions:
  - node_text_regex:
      selector: "p.RRH"
      pattern: "(?i)styled title fixture"
"#,
    )
    .expect("write styled selector definition");
    fs::write(
        definitions.path().join("selector-pagebreaks.fp.yaml"),
        r#"
fingerprint_id: selector-pagebreaks.v1
format: html
assertions:
  - node_count:
      selector: 'span[style*="page-break-after"]'
      min: 68
      max: 68
"#,
    )
    .expect("write pagebreak selector definition");

    let artifacts = TempDir::new().expect("create artifacts dir");
    let output = Command::new("bash")
        .arg(repo_path("scripts/selector_smoke.sh"))
        .args([
            "--definitions-dir",
            definitions.path().to_str().expect("definitions path"),
            "--fp",
            "selector-styled.v1",
            "--fp",
            "selector-pagebreaks.v1",
            "--fixture-id",
            "styled_heading_ares",
            "--fixture-id",
            "oxsq_pagebreaks",
            "--artifact-root",
            artifacts.path().to_str().expect("artifact path"),
            "--label",
            "selector-test",
        ])
        .env("FINGERPRINT_BIN", env!("CARGO_BIN_EXE_fingerprint"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run selector smoke script");

    assert!(
        output.status.success(),
        "selector smoke failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let artifact_dir = artifacts.path().join("selector").join("selector-test");
    let summary_path = artifact_dir.join("run.summary.json");
    let events_path = artifact_dir.join("stderr.events.json");
    assert!(summary_path.is_file(), "missing {}", summary_path.display());
    assert!(events_path.is_file(), "missing {}", events_path.display());

    let summary: Value =
        serde_json::from_str(&fs::read_to_string(summary_path).expect("read summary"))
            .expect("parse summary");
    assert_eq!(summary["exit_code"], 0);
    assert_eq!(summary["matched_count"], 2);
    assert_eq!(summary["selected_fixture_count"], 2);
}
