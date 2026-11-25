//! Native `xml` document format (bd-yq3, XML slice) on three REAL SEC filings (see
//! tests/fixtures/xml/README.md): an N-PORT-P (nested, namespaced), an ABS-EE EX-102 (wide
//! repeated asset records) and an ABS-EE EX-103 (flat prose key/value comment records).
//!
//! Expected counts, roots and namespaces were computed independently with Python's
//! xml.etree on a copy with the leading carriage return removed. The files as stored are not
//! strictly well-formed XML (a CR precedes the declaration); fingerprint must accept them.

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/xml")
        .join(name)
}

struct Scratch(PathBuf);
impl Scratch {
    fn new(tag: &str) -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let unique = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("fp-xml-{tag}-{}-{unique}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

fn run_definition(
    yaml: &str,
    id: &str,
    file: &Path,
    diagnose: bool,
) -> (Option<i32>, Value, String) {
    let dir = Scratch::new("defs");
    fs::write(dir.0.join(format!("{id}.fp.yaml")), yaml).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_fingerprint"));
    command.env("FINGERPRINT_DEFINITIONS", &dir.0).args([
        "match",
        s(file),
        "--fp",
        id,
        "--no-witness",
    ]);
    if diagnose {
        command.arg("--diagnose");
    }
    let out = command.output().unwrap();
    let record: Value = String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .map(|line| serde_json::from_str(line).unwrap())
        .unwrap_or(Value::Null);
    (
        out.status.code(),
        record,
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn failed_details(record: &Value) -> Vec<String> {
    record["fingerprint"]["assertions"]
        .as_array()
        .map(|assertions| {
            assertions
                .iter()
                .filter(|a| a["passed"] == false)
                .map(|a| a["detail"].as_str().unwrap_or("").to_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// A one-assertion definition for `format: xml`.
fn single(assertion: &str) -> String {
    format!("fingerprint_id: neg.v1\nformat: xml\nassertions:\n{assertion}")
}

fn expect_fail(file: &str, assertion: &str, detail: &str) {
    let (code, record, stderr) =
        run_definition(&single(assertion), "neg.v1", &fixture(file), false);
    assert_eq!(code, Some(1), "{assertion}: {stderr}");
    assert_eq!(record["fingerprint"]["matched"], false, "{assertion}");
    assert_eq!(
        failed_details(&record),
        vec![detail.to_owned()],
        "{assertion}"
    );
}

const NPORT: &str = r#"
fingerprint_id: nport-p.v1
format: xml
assertions:
  - xml_root: edgarSubmission
  - xml_namespace: "http://www.sec.gov/edgar/nport"
  - xml_path_exists: /edgarSubmission/headerData/submissionType
  - xml_path_exists: /edgarSubmission/formData/genInfo/regName
  - xml_text_regex:
      path: /edgarSubmission/headerData/submissionType
      pattern: "^NPORT-P$"
  - xml_path_count:
      path: /edgarSubmission/formData/invstOrSecs/invstOrSec
      min: 45
      max: 45
  - xml_attr_regex:
      path: /edgarSubmission/formData/genInfo/regStateConditional
      attr: regCountry
      pattern: "^US$"
"#;

const ASSETS: &str = r#"
fingerprint_id: absee-cmbs-assets.v1
format: xml
assertions:
  - xml_root: assetData
  - xml_namespace: "http://www.sec.gov/edgar/document/absee/cmbs/assetdata"
  - xml_path_count:
      path: /assetData/assets
      min: 47
      max: 47
  - xml_path_count:
      path: /assetData/assets/property
      min: 53
      max: 53
  - xml_path_exists: /assetData/assets/originalLoanAmount
  - xml_text_regex:
      path: /assetData/assets/originalLoanAmount
      pattern: "^[0-9]+\\.[0-9]+$"
      min_matches: 47
"#;

const COMMENTS: &str = r#"
fingerprint_id: absee-comments.v1
format: xml
assertions:
  - xml_root: comments
  - xml_namespace: ""
  - xml_path_count:
      path: /comments/commentData
      min: 32
      max: 32
  - xml_path_exists: /comments/commentData/comment
  - xml_text_regex:
      path: /comments/commentData/fieldName
      pattern: "^Loan Structure$"
      min_matches: 1
  - xml_text_regex:
      path: /comments/commentData/itemNumber
      pattern: "^Item 2\\(c\\)\\([0-9]+\\)"
      min_matches: 1
"#;

// ---- one positive test per real fixture ----------------------------------------------------

#[test]
fn nport_p_nested_namespaced_filing_matches() {
    let (code, record, stderr) = run_definition(
        NPORT,
        "nport-p.v1",
        &fixture("nport-p/primary_doc.xml"),
        false,
    );
    assert_eq!(code, Some(0), "{stderr}\n{record}");
    assert_eq!(record["fingerprint"]["matched"], true, "{record}");
    assert_eq!(
        record["fingerprint"]["assertions"]
            .as_array()
            .unwrap()
            .len(),
        7
    );
}

#[test]
fn abs_ee_exh_102_wide_repeated_asset_records_match() {
    let (code, record, stderr) = run_definition(
        ASSETS,
        "absee-cmbs-assets.v1",
        &fixture("abs-ee/exh_102.xml"),
        false,
    );
    assert_eq!(code, Some(0), "{stderr}\n{record}");
    assert_eq!(record["fingerprint"]["matched"], true, "{record}");
}

#[test]
fn abs_ee_exh_103_prose_comment_records_match() {
    let (code, record, stderr) = run_definition(
        COMMENTS,
        "absee-comments.v1",
        &fixture("abs-ee/exh_103.xml"),
        false,
    );
    assert_eq!(code, Some(0), "{stderr}\n{record}");
    assert_eq!(record["fingerprint"]["matched"], true, "{record}");
}

#[test]
fn the_real_files_are_not_strict_xml_but_are_accepted() {
    // Pin the quirk: a carriage return before the declaration (strict parsers reject this).
    for file in [
        "nport-p/primary_doc.xml",
        "abs-ee/exh_102.xml",
        "abs-ee/exh_103.xml",
    ] {
        let bytes = fs::read(fixture(file)).unwrap();
        assert_eq!(&bytes[..6], b"\r<?xml", "{file}");
        assert!(!bytes.contains(&b'\n'), "{file}: CR-only line endings");
    }
}

// ---- planted negatives: each assertion kind fails, on the right fixture -------------------

#[test]
fn every_xml_assertion_fails_when_its_condition_is_false() {
    expect_fail(
        "nport-p/primary_doc.xml",
        "  - xml_root: edgarSubmissions\n",
        "xml root element is 'edgarSubmission', expected 'edgarSubmissions'",
    );
    expect_fail(
        "nport-p/primary_doc.xml",
        "  - xml_namespace: \"http://www.sec.gov/edgar/nport2\"\n",
        "xml root namespace is 'http://www.sec.gov/edgar/nport', expected 'http://www.sec.gov/edgar/nport2'",
    );
    expect_fail(
        "abs-ee/exh_103.xml",
        "  - xml_namespace: \"urn:anything\"\n",
        "xml root namespace is '', expected 'urn:anything'",
    );
    expect_fail(
        "nport-p/primary_doc.xml",
        "  - xml_path_exists: /edgarSubmission/headerData/submissionTyp\n",
        "xml path '/edgarSubmission/headerData/submissionTyp' not found",
    );
    expect_fail(
        "abs-ee/exh_102.xml",
        "  - xml_path_count:\n      path: /assetData/assets\n      min: 48\n",
        "element count at '/assetData/assets' is 47, expected at least 48",
    );
    expect_fail(
        "abs-ee/exh_102.xml",
        "  - xml_path_count:\n      path: /assetData/assets/property\n      max: 52\n",
        "element count at '/assetData/assets/property' is 53, expected at most 52",
    );
    expect_fail(
        "nport-p/primary_doc.xml",
        "  - xml_attr_regex:\n      path: /edgarSubmission/formData/genInfo/regStateConditional\n      attr: regCountry\n      pattern: \"^CA$\"\n",
        "attribute 'regCountry' at '/edgarSubmission/formData/genInfo/regStateConditional': 0 of 1 value(s) matched /^CA$/, expected at least 1",
    );
    expect_fail(
        "nport-p/primary_doc.xml",
        "  - xml_attr_regex:\n      path: /edgarSubmission/formData/genInfo/regStateConditional\n      attr: noSuchAttr\n      pattern: \".\"\n",
        "attribute 'noSuchAttr' at '/edgarSubmission/formData/genInfo/regStateConditional': 0 of 0 value(s) matched /./, expected at least 1",
    );
    expect_fail(
        "abs-ee/exh_102.xml",
        "  - xml_text_regex:\n      path: /assetData/assets/originalLoanAmount\n      pattern: \"^[0-9]+$\"\n      min_matches: 47\n",
        "text at '/assetData/assets/originalLoanAmount': 0 of 47 element(s) matched /^[0-9]+$/, expected at least 47",
    );
    expect_fail(
        "abs-ee/exh_103.xml",
        "  - xml_text_regex:\n      path: /comments/commentData/fieldName\n      pattern: \"^Loan Structure$\"\n      min_matches: 2\n",
        "text at '/comments/commentData/fieldName': 1 of 32 element(s) matched /^Loan Structure$/, expected at least 2",
    );
}

#[test]
fn an_xml_fingerprint_does_not_apply_to_other_formats() {
    let (code, record, _) =
        run_definition(NPORT, "nport-p.v1", &fixture("../json/loans.jsonl"), false);
    assert_eq!(code, Some(1));
    assert!(record["fingerprint"].is_null(), "{record}");
}

#[test]
fn malformed_xml_is_an_open_failure_naming_the_byte_not_the_content() {
    let dir = Scratch::new("bad");
    let bad = dir.0.join("bad.xml");
    fs::write(&bad, "<a><b>SECRET-TEXT-VALUE</a>").unwrap();
    let (code, record, stderr) = run_definition(NPORT, "nport-p.v1", &bad, false);
    assert_eq!(code, Some(1));
    assert!(record["fingerprint"].is_null(), "{record}");
    assert!(
        stderr.contains("invalid xml") && stderr.contains("at byte"),
        "{stderr}"
    );
    assert!(!stderr.contains("SECRET-TEXT-VALUE"), "{stderr}");
}

// ---- zero retention: key names and types only -----------------------------------------------

/// Distinctive fragments of real free text and values in the fixtures. None may ever appear in
/// the output of a failed assertion.
const EXH_103_TEXT: &[&str] = &[
    "Hilton Hawaiian Village",
    "pari passu",
    "anticipated repayment",
    "Cut-off Date",
    "Item 2(c)(15)",
    "Original Loan Term Number",
];

#[test]
fn exh_103_failure_context_names_elements_and_never_the_comment_text() {
    // Several failing assertions at once (diagnose evaluates all): a typo'd element name, a
    // text pattern that matches nothing, a count out of range, a missing attribute.
    let yaml = "fingerprint_id: diag.v1\nformat: xml\nassertions:\n  - xml_path_exists: /comments/commentData/commnt\n  - xml_text_regex:\n      path: /comments/commentData/comment\n      pattern: \"^zzz-never-matches$\"\n  - xml_text_regex:\n      path: /comments/commentData/fieldName\n      pattern: \"^zzz$\"\n  - xml_path_count:\n      path: /comments/commentData\n      max: 1\n  - xml_attr_regex:\n      path: /comments/commentData\n      attr: id\n      pattern: \".\"\n";
    let (code, record, _) = run_definition(yaml, "diag.v1", &fixture("abs-ee/exh_103.xml"), true);
    assert_eq!(code, Some(1));
    let assertions = record["fingerprint"]["assertions"].as_array().unwrap();
    assert_eq!(assertions.len(), 5);
    assert!(assertions.iter().all(|a| a["passed"] == false));

    let first = &assertions[0]["context"];
    assert_eq!(first["root"], "comments");
    assert_eq!(first["resolved_prefix"], "/comments/commentData");
    assert_eq!(
        first["child_elements"],
        serde_json::json!(["comment", "fieldName", "itemNumber"])
    );
    assert_eq!(first["nearest_match"], "comment");
    // Counts and names, for the text and count assertions too.
    assert_eq!(assertions[1]["context"]["elements_at_path"], 32);
    assert_eq!(assertions[3]["context"]["elements_at_path"], 32);

    let rendered = record.to_string();
    for fragment in EXH_103_TEXT {
        assert!(
            !rendered.contains(fragment),
            "comment text leaked: {fragment}\n{rendered}"
        );
    }
}

#[test]
fn nport_and_abs_ee_failure_context_never_includes_values() {
    let cases = [
        (
            "nport-p/primary_doc.xml",
            "  - xml_attr_regex:\n      path: /edgarSubmission/formData/genInfo/regStateConditional\n      attr: regCountry\n      pattern: \"^CA$\"\n  - xml_text_regex:\n      path: /edgarSubmission/formData/genInfo/regName\n      pattern: \"^nope$\"\n  - xml_path_exists: /edgarSubmission/formData/genInfo/regNme\n",
            vec![
                "SEASONS SERIES TRUST",
                "811-07725",
                "549300O318N6OBWGKN57",
                "Memorial Drive",
                "US-TX",
                "S000008051",
            ],
        ),
        (
            "abs-ee/exh_102.xml",
            "  - xml_text_regex:\n      path: /assetData/assets/originalLoanAmount\n      pattern: \"^nope$\"\n  - xml_path_exists: /assetData/assets/originalLoanAmnt\n",
            vec![
                "95000000.00000000",
                "75000000.00000000",
                "65000000.00000000",
                "Prospectus Loan ID",
                "CREFI",
            ],
        ),
    ];
    for (file, assertions, values) in cases {
        let yaml = format!("fingerprint_id: diag.v1\nformat: xml\nassertions:\n{assertions}");
        let (code, record, _) = run_definition(&yaml, "diag.v1", &fixture(file), true);
        assert_eq!(code, Some(1), "{file}");
        let rendered = record.to_string();
        for value in values {
            assert!(
                !rendered.contains(value),
                "{file}: value {value} leaked: {rendered}"
            );
        }
        // The attribute NAME is reported for the attribute assertion's path; its value is not.
        if file.starts_with("nport") {
            let context = &record["fingerprint"]["assertions"][0]["context"];
            assert_eq!(
                context["attribute_names"],
                serde_json::json!(["regCountry", "regState"])
            );
        }
    }
}

// ---- compile --check validation ------------------------------------------------------------

#[test]
fn compile_check_validates_xml_definitions() {
    let dir = Scratch::new("check");
    let check = |yaml: &str| -> Output {
        let path = dir.0.join("def.fp.yaml");
        fs::write(&path, yaml).unwrap();
        Command::new(env!("CARGO_BIN_EXE_fingerprint"))
            .args(["compile", s(&path), "--check"])
            .output()
            .unwrap()
    };
    for good in [NPORT, ASSETS, COMMENTS] {
        let ok = check(good);
        assert_eq!(
            ok.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&ok.stderr)
        );
    }
    for (assertion, needle) in [
        (
            "format: csv\nassertions:\n  - xml_root: a\n",
            "requires format 'xml'",
        ),
        (
            "format: xml\nassertions:\n  - xml_path_exists: a/b\n",
            "must start with '/'",
        ),
        (
            "format: xml\nassertions:\n  - xml_path_exists: /a/b[1]\n",
            "local element names only",
        ),
        (
            "format: xml\nassertions:\n  - xml_path_exists: /a//b\n",
            "no empty segment",
        ),
        (
            "format: xml\nassertions:\n  - xml_root: \"p:a\"\n",
            "local element name",
        ),
        (
            "format: xml\nassertions:\n  - xml_path_count:\n      path: /a\n",
            "at least one of 'min' or 'max'",
        ),
        (
            "format: xml\nassertions:\n  - xml_attr_regex:\n      path: /a\n      attr: \"x:y\"\n      pattern: \".\"\n",
            "local attribute name",
        ),
        (
            "format: xml\nassertions:\n  - xml_text_regex:\n      path: /a\n      pattern: \"(\"\n",
            "not a valid regex",
        ),
        (
            "format: xml\nassertions:\n  - xml_text_regex:\n      path: /a\n      pattern: \".\"\n      min_matches: 0\n",
            "min_matches must be >= 1",
        ),
        (
            "format: parquet\nassertions:\n  - xml_root: a\n",
            "requires format 'xml'",
        ),
    ] {
        let yaml = format!("fingerprint_id: x.v1\n{assertion}");
        let out = check(&yaml);
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.status.code(), Some(2), "{needle}: {text}");
        assert!(text.contains(needle), "expected '{needle}' in: {text}");
    }
}
