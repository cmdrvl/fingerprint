use fingerprint::dsl::{Assertion, FingerprintDefinition};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::NamedTempFile;

fn run_fingerprint(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fingerprint"))
        .args(args)
        .output()
        .expect("run fingerprint binary")
}

fn fixture(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(relative)
}

fn temp_file(contents: &str, suffix: &str) -> NamedTempFile {
    let mut file = NamedTempFile::with_suffix(suffix).expect("create temp file");
    file.write_all(contents.as_bytes())
        .expect("write temp file");
    file.flush().expect("flush temp file");
    file
}

#[test]
fn infer_schema_emits_valid_fp_yaml_with_located_assertions() {
    let markdown = temp_file(
        "# Summary\n\nAs of date: June 15, 2024\nCap rate: 6.25%\n",
        ".md",
    );
    let fields = temp_file(
        r#"
- name: as_of_date
  value: "June 15, 2024"
- name: cap_rate
  value: "6.25%"
"#,
        ".yaml",
    );

    let output = run_fingerprint(&[
        "--no-witness",
        "infer-schema",
        "--doc",
        markdown.path().to_str().expect("doc str"),
        "--fields",
        fields.path().to_str().expect("fields str"),
        "--id",
        "schema-test.v1",
    ]);

    assert_eq!(output.status.code(), Some(0));
    let yaml = String::from_utf8(output.stdout).expect("stdout utf8");
    let definition: FingerprintDefinition = serde_yaml::from_str(&yaml).expect("parse yaml");

    assert_eq!(definition.fingerprint_id, "schema-test.v1");
    assert_eq!(definition.format, "markdown");
    assert_eq!(definition.assertions.len(), 2);
    assert!(
        definition
            .assertions
            .iter()
            .any(|assertion| { assertion.name.as_deref() == Some("field_as_of_date") })
    );
    assert!(
        definition
            .assertions
            .iter()
            .any(|assertion| { assertion.name.as_deref() == Some("field_cap_rate") })
    );
}

#[test]
fn infer_schema_returns_partial_exit_when_some_fields_missing() {
    let markdown = temp_file("# Summary\n\nAs of date: June 15, 2024\n", ".md");
    let fields = temp_file(
        r#"
- name: as_of_date
  value: "June 15, 2024"
- name: missing
  value: "not present"
"#,
        ".yaml",
    );

    let output = run_fingerprint(&[
        "--no-witness",
        "infer-schema",
        "--doc",
        markdown.path().to_str().expect("doc str"),
        "--fields",
        fields.path().to_str().expect("fields str"),
        "--id",
        "schema-test.v1",
    ]);

    assert_eq!(output.status.code(), Some(1));
    let yaml = String::from_utf8(output.stdout).expect("stdout utf8");
    let definition: FingerprintDefinition = serde_yaml::from_str(&yaml).expect("parse yaml");
    assert_eq!(definition.assertions.len(), 1);
    assert_eq!(
        definition.assertions[0].name.as_deref(),
        Some("field_as_of_date")
    );
}

#[test]
fn infer_schema_accepts_pdf_with_text_path() {
    let pdf = fixture("files/sample.pdf");
    let markdown = fixture("files/sample.md");
    let fields = temp_file(
        r#"
- name: as_of_date
  value: "June 15, 2024"
- name: cap_rate
  value: "6.25%"
"#,
        ".yaml",
    );

    let output = run_fingerprint(&[
        "--no-witness",
        "infer-schema",
        "--doc",
        pdf.to_str().expect("pdf str"),
        "--text-path",
        markdown.to_str().expect("markdown str"),
        "--fields",
        fields.path().to_str().expect("fields str"),
        "--id",
        "sample-pdf.v1",
    ]);

    assert_eq!(output.status.code(), Some(0));
    let yaml = String::from_utf8(output.stdout).expect("stdout utf8");
    let definition: FingerprintDefinition = serde_yaml::from_str(&yaml).expect("parse yaml");

    assert_eq!(definition.fingerprint_id, "sample-pdf.v1");
    assert_eq!(definition.format, "pdf");
    assert_eq!(definition.assertions.len(), 2);
    assert!(
        definition
            .assertions
            .iter()
            .any(|assertion| assertion.name.as_deref() == Some("field_as_of_date"))
    );
    assert!(
        definition
            .assertions
            .iter()
            .any(|assertion| assertion.name.as_deref() == Some("field_cap_rate"))
    );
}

#[test]
fn infer_schema_accepts_html_documents() {
    let html = fixture("html/generic_page_sections_schedule.html");
    let fields = temp_file(
        r#"
- name: as_of_date
  value: "December 31, 2025"
- name: issuer
  value: "Alpha Lending LLC"
"#,
        ".yaml",
    );

    let output = run_fingerprint(&[
        "--no-witness",
        "infer-schema",
        "--doc",
        html.to_str().expect("html str"),
        "--fields",
        fields.path().to_str().expect("fields str"),
        "--id",
        "html-schema.v1",
    ]);

    assert_eq!(output.status.code(), Some(0));
    let yaml = String::from_utf8(output.stdout).expect("stdout utf8");
    let definition: FingerprintDefinition = serde_yaml::from_str(&yaml).expect("parse yaml");

    assert_eq!(definition.fingerprint_id, "html-schema.v1");
    assert_eq!(definition.format, "html");
    assert_eq!(definition.assertions.len(), 2);
    assert_eq!(definition.extract.len(), 2);
    assert!(definition.assertions.iter().all(|assertion| {
        matches!(
            assertion.assertion,
            Assertion::TextNear { .. } | Assertion::TextContains(_)
        )
    }));
}
