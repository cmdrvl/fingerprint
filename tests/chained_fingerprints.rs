use fingerprint::document::Document;
use fingerprint::dsl::assertions::{Assertion, evaluate};
use fingerprint::pipeline::enricher::enrich_record_with_fingerprints;
use fingerprint::registry::{
    AssertionResult, Fingerprint, FingerprintInfo, FingerprintRegistry, FingerprintResult,
};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tempfile::NamedTempFile;

struct AlwaysMatchParent {
    id: &'static str,
    format: &'static str,
}

impl Fingerprint for AlwaysMatchParent {
    fn id(&self) -> &str {
        self.id
    }

    fn format(&self) -> &str {
        self.format
    }

    fn fingerprint(&self, _doc: &Document) -> FingerprintResult {
        FingerprintResult {
            matched: true,
            reason: None,
            assertions: vec![AssertionResult {
                name: "parent_match".to_owned(),
                passed: true,
                detail: None,
                context: None,
            }],
            extracted: None,
            content_hash: None,
        }
    }
}

struct NeverMatchParent {
    id: &'static str,
    format: &'static str,
}

impl Fingerprint for NeverMatchParent {
    fn id(&self) -> &str {
        self.id
    }

    fn format(&self) -> &str {
        self.format
    }

    fn fingerprint(&self, _doc: &Document) -> FingerprintResult {
        FingerprintResult {
            matched: false,
            reason: Some("parent no-match".to_owned()),
            assertions: vec![AssertionResult {
                name: "parent_match".to_owned(),
                passed: false,
                detail: Some("forced no-match".to_owned()),
                context: None,
            }],
            extracted: None,
            content_hash: None,
        }
    }
}

struct TextChild {
    id: &'static str,
    parent: &'static str,
    token: &'static str,
    with_extract_hash: bool,
}

impl Fingerprint for TextChild {
    fn id(&self) -> &str {
        self.id
    }

    fn format(&self) -> &str {
        "text"
    }

    fn parent(&self) -> Option<&str> {
        Some(self.parent)
    }

    fn fingerprint(&self, doc: &Document) -> FingerprintResult {
        let matched = match doc {
            Document::Text(text) => text.content().contains(self.token),
            _ => false,
        };

        FingerprintResult {
            matched,
            reason: (!matched).then(|| format!("missing token '{}': no match", self.token)),
            assertions: vec![AssertionResult {
                name: format!("contains_{}", self.token),
                passed: matched,
                detail: (!matched).then(|| format!("missing token '{}': no match", self.token)),
                context: None,
            }],
            extracted: (matched && self.with_extract_hash)
                .then_some(HashMap::from([("token".to_owned(), json!(self.token))])),
            content_hash: (matched && self.with_extract_hash)
                .then_some("blake3:child-token-hash".to_owned()),
        }
    }
}

struct PdfStructuralParent {
    id: &'static str,
}

impl Fingerprint for PdfStructuralParent {
    fn id(&self) -> &str {
        self.id
    }

    fn format(&self) -> &str {
        "pdf"
    }

    fn fingerprint(&self, doc: &Document) -> FingerprintResult {
        let assertion = Assertion::PageCount {
            min: Some(1),
            max: Some(10),
        };
        let assertion_result = evaluate(&assertion, doc);
        FingerprintResult {
            matched: assertion_result.passed,
            reason: (!assertion_result.passed).then(|| {
                assertion_result
                    .detail
                    .clone()
                    .unwrap_or_else(|| "no match".to_owned())
            }),
            assertions: vec![assertion_result],
            extracted: None,
            content_hash: None,
        }
    }
}

struct PdfContentChild {
    id: &'static str,
    parent: &'static str,
}

impl Fingerprint for PdfContentChild {
    fn id(&self) -> &str {
        self.id
    }

    fn format(&self) -> &str {
        "pdf"
    }

    fn parent(&self) -> Option<&str> {
        Some(self.parent)
    }

    fn fingerprint(&self, doc: &Document) -> FingerprintResult {
        let assertion = Assertion::TextContains("rent roll".to_owned());
        let assertion_result = evaluate(&assertion, doc);
        FingerprintResult {
            matched: assertion_result.passed,
            reason: (!assertion_result.passed).then(|| {
                assertion_result
                    .detail
                    .clone()
                    .unwrap_or_else(|| "no match".to_owned())
            }),
            assertions: vec![assertion_result],
            extracted: None,
            content_hash: None,
        }
    }
}

fn registry_with(entries: Vec<(Box<dyn Fingerprint>, FingerprintInfo)>) -> FingerprintRegistry {
    let mut registry = FingerprintRegistry::new();
    for (fingerprint, info) in entries {
        registry.register_with_info(fingerprint, info);
    }
    registry
}

fn text_record(path: &Path) -> Value {
    json!({
        "version": "hash.v0",
        "path": path.display().to_string(),
        "extension": ".txt",
        "bytes_hash": "blake3:abc",
        "tool_versions": { "hash": "0.1.0" }
    })
}

fn pdf_record(path: &Path) -> Value {
    json!({
        "version": "hash.v0",
        "path": path.display().to_string(),
        "extension": ".pdf",
        "bytes_hash": "blake3:pdf",
        "tool_versions": { "hash": "0.1.0" }
    })
}

fn expected_exit_code(record: &Value) -> u8 {
    if record
        .get("_skipped")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return 1;
    }

    let Some(fingerprint) = record.get("fingerprint") else {
        return 1;
    };
    if fingerprint.is_null()
        || !fingerprint
            .get("matched")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    {
        return 1;
    }

    if fingerprint
        .get("children")
        .and_then(Value::as_array)
        .is_some_and(|children| {
            children.iter().any(|child| {
                !child
                    .get("matched")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            })
        })
    {
        1
    } else {
        0
    }
}

#[test]
fn parent_match_all_children_match_exit_zero_with_children_payload() {
    let file = NamedTempFile::with_suffix(".txt").expect("create temp text file");
    fs::write(file.path(), "alpha beta").expect("write text fixture");

    let registry = registry_with(vec![
        (
            Box::new(AlwaysMatchParent {
                id: "parent.v1",
                format: "text",
            }),
            FingerprintInfo {
                id: "parent.v1".to_owned(),
                crate_name: "fingerprint-parent".to_owned(),
                version: "1.0.0".to_owned(),
                source: "dsl:parent".to_owned(),
                format: "text".to_owned(),
                parent: None,
            },
        ),
        (
            Box::new(TextChild {
                id: "parent.v1/child-a.v1",
                parent: "parent.v1",
                token: "alpha",
                with_extract_hash: false,
            }),
            FingerprintInfo {
                id: "parent.v1/child-a.v1".to_owned(),
                crate_name: "fingerprint-child-a".to_owned(),
                version: "1.0.0".to_owned(),
                source: "dsl:child-a".to_owned(),
                format: "text".to_owned(),
                parent: Some("parent.v1".to_owned()),
            },
        ),
        (
            Box::new(TextChild {
                id: "parent.v1/child-b.v1",
                parent: "parent.v1",
                token: "beta",
                with_extract_hash: true,
            }),
            FingerprintInfo {
                id: "parent.v1/child-b.v1".to_owned(),
                crate_name: "fingerprint-child-b".to_owned(),
                version: "1.0.0".to_owned(),
                source: "dsl:child-b".to_owned(),
                format: "text".to_owned(),
                parent: Some("parent.v1".to_owned()),
            },
        ),
    ]);

    let selected = vec![
        "parent.v1".to_owned(),
        "parent.v1/child-a.v1".to_owned(),
        "parent.v1/child-b.v1".to_owned(),
    ];

    let output = enrich_record_with_fingerprints(&text_record(file.path()), &registry, &selected);

    assert_eq!(output["fingerprint"]["fingerprint_id"], "parent.v1");
    assert_eq!(output["fingerprint"]["matched"], true);
    let children = output["fingerprint"]["children"]
        .as_array()
        .expect("children array");
    assert_eq!(children.len(), 2);
    assert!(children.iter().all(|child| child["matched"] == true));
    assert_eq!(children[1]["extracted"]["token"], "beta");
    assert_eq!(children[1]["content_hash"], "blake3:child-token-hash");
    assert_eq!(expected_exit_code(&output), 0);
}

#[test]
fn parent_match_child_failure_yields_partial_exit_one() {
    let file = NamedTempFile::with_suffix(".txt").expect("create temp text file");
    fs::write(file.path(), "alpha only").expect("write text fixture");

    let registry = registry_with(vec![
        (
            Box::new(AlwaysMatchParent {
                id: "parent.v1",
                format: "text",
            }),
            FingerprintInfo {
                id: "parent.v1".to_owned(),
                crate_name: "fingerprint-parent".to_owned(),
                version: "1.0.0".to_owned(),
                source: "dsl:parent".to_owned(),
                format: "text".to_owned(),
                parent: None,
            },
        ),
        (
            Box::new(TextChild {
                id: "parent.v1/child-a.v1",
                parent: "parent.v1",
                token: "alpha",
                with_extract_hash: false,
            }),
            FingerprintInfo {
                id: "parent.v1/child-a.v1".to_owned(),
                crate_name: "fingerprint-child-a".to_owned(),
                version: "1.0.0".to_owned(),
                source: "dsl:child-a".to_owned(),
                format: "text".to_owned(),
                parent: Some("parent.v1".to_owned()),
            },
        ),
        (
            Box::new(TextChild {
                id: "parent.v1/child-b.v1",
                parent: "parent.v1",
                token: "beta",
                with_extract_hash: false,
            }),
            FingerprintInfo {
                id: "parent.v1/child-b.v1".to_owned(),
                crate_name: "fingerprint-child-b".to_owned(),
                version: "1.0.0".to_owned(),
                source: "dsl:child-b".to_owned(),
                format: "text".to_owned(),
                parent: Some("parent.v1".to_owned()),
            },
        ),
    ]);

    let selected = vec![
        "parent.v1".to_owned(),
        "parent.v1/child-a.v1".to_owned(),
        "parent.v1/child-b.v1".to_owned(),
    ];

    let output = enrich_record_with_fingerprints(&text_record(file.path()), &registry, &selected);

    assert_eq!(output["fingerprint"]["matched"], true);
    let children = output["fingerprint"]["children"]
        .as_array()
        .expect("children array");
    assert_eq!(children.len(), 2);
    assert_eq!(children[1]["matched"], false);
    assert_eq!(expected_exit_code(&output), 1);
}

#[test]
fn parent_no_match_skips_children_and_returns_partial() {
    let file = NamedTempFile::with_suffix(".txt").expect("create temp text file");
    fs::write(file.path(), "alpha beta").expect("write text fixture");

    let registry = registry_with(vec![
        (
            Box::new(NeverMatchParent {
                id: "parent.v1",
                format: "text",
            }),
            FingerprintInfo {
                id: "parent.v1".to_owned(),
                crate_name: "fingerprint-parent".to_owned(),
                version: "1.0.0".to_owned(),
                source: "dsl:parent".to_owned(),
                format: "text".to_owned(),
                parent: None,
            },
        ),
        (
            Box::new(TextChild {
                id: "parent.v1/child-a.v1",
                parent: "parent.v1",
                token: "alpha",
                with_extract_hash: false,
            }),
            FingerprintInfo {
                id: "parent.v1/child-a.v1".to_owned(),
                crate_name: "fingerprint-child-a".to_owned(),
                version: "1.0.0".to_owned(),
                source: "dsl:child-a".to_owned(),
                format: "text".to_owned(),
                parent: Some("parent.v1".to_owned()),
            },
        ),
    ]);

    let selected = vec!["parent.v1".to_owned(), "parent.v1/child-a.v1".to_owned()];
    let output = enrich_record_with_fingerprints(&text_record(file.path()), &registry, &selected);

    assert_eq!(output["fingerprint"]["fingerprint_id"], "parent.v1");
    assert_eq!(output["fingerprint"]["matched"], false);
    assert!(output["fingerprint"].get("children").is_none());
    assert_eq!(expected_exit_code(&output), 1);
}

#[test]
fn structural_parent_matches_without_text_path_and_content_child_fails_e_no_text() {
    let pdf_path = Path::new("tests/fixtures/test_files/report.pdf");
    assert!(pdf_path.exists(), "expected pdf fixture at {pdf_path:?}");

    let registry = registry_with(vec![
        (
            Box::new(PdfStructuralParent { id: "parent.v1" }),
            FingerprintInfo {
                id: "parent.v1".to_owned(),
                crate_name: "fingerprint-parent".to_owned(),
                version: "1.0.0".to_owned(),
                source: "dsl:parent".to_owned(),
                format: "pdf".to_owned(),
                parent: None,
            },
        ),
        (
            Box::new(PdfContentChild {
                id: "parent.v1/child-content.v1",
                parent: "parent.v1",
            }),
            FingerprintInfo {
                id: "parent.v1/child-content.v1".to_owned(),
                crate_name: "fingerprint-child".to_owned(),
                version: "1.0.0".to_owned(),
                source: "dsl:child".to_owned(),
                format: "pdf".to_owned(),
                parent: Some("parent.v1".to_owned()),
            },
        ),
    ]);

    let selected = vec![
        "parent.v1".to_owned(),
        "parent.v1/child-content.v1".to_owned(),
    ];
    let output = enrich_record_with_fingerprints(&pdf_record(pdf_path), &registry, &selected);

    assert_eq!(output["fingerprint"]["matched"], true);
    let children = output["fingerprint"]["children"]
        .as_array()
        .expect("children array");
    assert_eq!(children.len(), 1);
    assert_eq!(children[0]["matched"], false);
    let child_assertions = children[0]["assertions"]
        .as_array()
        .expect("child assertions");
    assert!(
        child_assertions[0]["detail"]
            .as_str()
            .expect("detail")
            .contains("E_NO_TEXT")
    );
    assert_eq!(expected_exit_code(&output), 1);
}
