use regex::Regex;
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::Builder;

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn run_fingerprint(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fingerprint"))
        .args(args)
        .output()
        .expect("run fingerprint binary")
}

fn read_reference() -> String {
    fs::read_to_string(repo_path("docs/DSL_REFERENCE.md")).expect("read DSL reference")
}

fn compile_schema() -> Value {
    let output = run_fingerprint(&["compile", "--schema"]);
    assert!(
        output.status.success(),
        "compile --schema failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("compile schema json")
}

fn catalog_values(reference: &str, start_marker: &str, end_marker: &str) -> BTreeSet<String> {
    let mut in_catalog = false;
    let mut values = BTreeSet::new();
    for line in reference.lines() {
        if line.contains(start_marker) {
            in_catalog = true;
            continue;
        }
        if line.contains(end_marker) {
            break;
        }
        if !in_catalog || !line.starts_with('|') {
            continue;
        }
        let Some(first_cell) = line.split('|').nth(1) else {
            continue;
        };
        let value = first_cell.trim();
        if !value.starts_with('`') || !value.ends_with('`') {
            continue;
        }
        values.insert(value.trim_matches('`').to_owned());
    }
    values
}

fn schema_assertion_keys(schema: &Value) -> BTreeSet<String> {
    schema["$defs"]
        .as_object()
        .expect("$defs object")
        .keys()
        .filter_map(|key| key.strip_prefix("assertion_"))
        .map(ToOwned::to_owned)
        .collect()
}

fn schema_extract_types(schema: &Value) -> BTreeSet<String> {
    schema["$defs"]["extractSection"]["properties"]["type"]["enum"]
        .as_array()
        .expect("extract type enum")
        .iter()
        .map(|value| value.as_str().expect("extract type string").to_owned())
        .collect()
}

fn source_refusal_codes() -> BTreeSet<String> {
    let source = fs::read_to_string(repo_path("src/refusal/codes.rs")).expect("read refusal codes");
    let regex = Regex::new(r#"serde\(rename = "(E_[A-Z_]+)"\)"#).expect("compile regex");
    regex
        .captures_iter(&source)
        .map(|captures| captures[1].to_owned())
        .collect()
}

fn tagged_yaml_examples(reference: &str) -> Vec<(String, String)> {
    let mut examples = Vec::new();
    let mut current_name: Option<String> = None;
    let mut current_yaml = Vec::new();

    for line in reference.lines() {
        if let Some(rest) = line.strip_prefix("```yaml fingerprint-example:") {
            current_name = Some(rest.trim().to_owned());
            current_yaml.clear();
            continue;
        }
        if line == "```" {
            if let Some(name) = current_name.take() {
                examples.push((name, current_yaml.join("\n")));
                current_yaml.clear();
            }
            continue;
        }
        if current_name.is_some() {
            current_yaml.push(line.to_owned());
        }
    }

    examples
}

#[test]
fn doc_u01_assertion_key_parity_with_compile_schema() {
    let reference = read_reference();
    let documented = catalog_values(
        &reference,
        "assertion-catalog:start",
        "assertion-catalog:end",
    );
    let expected = schema_assertion_keys(&compile_schema());

    assert_eq!(
        documented, expected,
        "DSL_REFERENCE.md assertion catalog drifted from compile --schema"
    );
}

#[test]
fn doc_u02_extract_type_parity_with_compile_schema() {
    let reference = read_reference();
    let documented = catalog_values(&reference, "extract-catalog:start", "extract-catalog:end");
    let expected = schema_extract_types(&compile_schema());

    assert_eq!(
        documented, expected,
        "DSL_REFERENCE.md extract catalog drifted from compile --schema"
    );
}

#[test]
fn doc_u03_refusal_code_parity_with_source_enums() {
    let reference = read_reference();
    let documented = catalog_values(&reference, "refusal-catalog:start", "refusal-catalog:end");
    let expected = source_refusal_codes();

    assert_eq!(
        documented, expected,
        "DSL_REFERENCE.md refusal catalog drifted from refusal code enums"
    );
}

#[test]
fn doc_u04_tagged_examples_compile_check() {
    let reference = read_reference();
    let examples = tagged_yaml_examples(&reference);
    assert!(
        examples.len() >= 3,
        "expected at least xlsx, html, and pdf examples"
    );

    for (name, yaml) in examples {
        let file = Builder::new()
            .prefix(&format!("dsl-reference-{name}-"))
            .suffix(".fp.yaml")
            .tempfile()
            .expect("create temp yaml");
        fs::write(file.path(), yaml).expect("write example yaml");

        let output = run_fingerprint(&[
            "compile",
            file.path().to_str().expect("example yaml path"),
            "--check",
        ]);
        assert!(
            output.status.success(),
            "example '{name}' failed compile --check\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }
}
