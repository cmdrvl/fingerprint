use crate::document::Document;
use crate::registry::{AssertionResult, Fingerprint, FingerprintResult};
use serde_json::Value;
use std::collections::HashMap;

/// Register all built-in fingerprints (csv.v0, xlsx.v0, pdf.v0).
pub fn register_builtins() -> Vec<Box<dyn Fingerprint>> {
    vec![
        Box::new(CsvBuiltin),
        Box::new(XlsxBuiltin),
        Box::new(PdfBuiltin),
        Box::new(MarkdownBuiltin),
    ]
}

struct CsvBuiltin;
struct XlsxBuiltin;
struct PdfBuiltin;
struct MarkdownBuiltin;

impl Fingerprint for CsvBuiltin {
    fn id(&self) -> &str {
        "csv.v0"
    }

    fn format(&self) -> &str {
        "csv"
    }

    fn fingerprint(&self, doc: &Document) -> FingerprintResult {
        format_match_result("csv", doc, matches!(doc, Document::Csv(_)))
    }
}

impl Fingerprint for XlsxBuiltin {
    fn id(&self) -> &str {
        "xlsx.v0"
    }

    fn format(&self) -> &str {
        "xlsx"
    }

    fn fingerprint(&self, doc: &Document) -> FingerprintResult {
        format_match_result("xlsx", doc, matches!(doc, Document::Xlsx(_)))
    }
}

impl Fingerprint for PdfBuiltin {
    fn id(&self) -> &str {
        "pdf.v0"
    }

    fn format(&self) -> &str {
        "pdf"
    }

    fn fingerprint(&self, doc: &Document) -> FingerprintResult {
        format_match_result("pdf", doc, matches!(doc, Document::Pdf(_)))
    }
}

impl Fingerprint for MarkdownBuiltin {
    fn id(&self) -> &str {
        "markdown.v0"
    }

    fn format(&self) -> &str {
        "markdown"
    }

    fn fingerprint(&self, doc: &Document) -> FingerprintResult {
        match doc {
            Document::Markdown(markdown) => {
                let has_heading = !markdown.headings.is_empty();
                if has_heading {
                    FingerprintResult {
                        matched: true,
                        reason: None,
                        assertions: vec![
                            AssertionResult {
                                name: "markdown_valid".to_owned(),
                                passed: true,
                                detail: None,
                                context: None,
                            },
                            AssertionResult {
                                name: "has_heading".to_owned(),
                                passed: true,
                                detail: Some("at least one heading found".to_owned()),
                                context: None,
                            },
                        ],
                        extracted: Some(HashMap::<String, Value>::new()),
                        content_hash: None,
                    }
                } else {
                    FingerprintResult {
                        matched: false,
                        reason: Some("Markdown has no headings".to_owned()),
                        assertions: vec![
                            AssertionResult {
                                name: "markdown_valid".to_owned(),
                                passed: true,
                                detail: None,
                                context: None,
                            },
                            AssertionResult {
                                name: "has_heading".to_owned(),
                                passed: false,
                                detail: Some("expected at least one heading".to_owned()),
                                context: None,
                            },
                        ],
                        extracted: None,
                        content_hash: None,
                    }
                }
            }
            _ => format_match_result("markdown", doc, false),
        }
    }
}

fn format_match_result(expected_format: &str, doc: &Document, matched: bool) -> FingerprintResult {
    let path = doc.path();
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("<none>");

    if matched {
        FingerprintResult {
            matched: true,
            reason: None,
            assertions: vec![AssertionResult {
                name: "format_match".to_owned(),
                passed: true,
                detail: Some(format!(
                    "document '{}' matches builtin format '{}'",
                    path.display(),
                    expected_format
                )),
                context: None,
            }],
            extracted: Some(HashMap::<String, Value>::new()),
            content_hash: None,
        }
    } else {
        FingerprintResult {
            matched: false,
            reason: Some(format!(
                "document '{}' is not '{}' (extension '{}')",
                path.display(),
                expected_format,
                extension
            )),
            assertions: vec![AssertionResult {
                name: "format_match".to_owned(),
                passed: false,
                detail: Some(format!(
                    "expected '{}' document, got '{}'",
                    expected_format, extension
                )),
                context: None,
            }],
            extracted: None,
            content_hash: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::register_builtins;
    use crate::document::{CsvDocument, Document, PdfDocument, RawDocument, XlsxDocument};
    use std::fs;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

    fn csv_doc() -> Document {
        Document::Csv(CsvDocument {
            path: PathBuf::from("/tmp/example.csv"),
        })
    }

    fn xlsx_doc() -> Document {
        Document::Xlsx(XlsxDocument {
            path: PathBuf::from("/tmp/example.xlsx"),
        })
    }

    fn pdf_doc() -> Document {
        Document::Pdf(PdfDocument {
            path: PathBuf::from("/tmp/example.pdf"),
            text: None,
        })
    }

    fn unknown_doc() -> Document {
        Document::Unknown(RawDocument {
            path: PathBuf::from("/tmp/example.bin"),
            bytes: vec![0, 1, 2],
        })
    }

    fn markdown_doc(contents: &str) -> Document {
        let file = NamedTempFile::with_suffix(".md").expect("create markdown temp file");
        fs::write(file.path(), contents).expect("write markdown fixture");
        let markdown = crate::document::MarkdownDocument::open(file.path()).expect("open markdown");
        Document::Markdown(markdown)
    }

    #[test]
    fn registers_expected_builtin_ids() {
        let builtins = register_builtins();
        let mut ids: Vec<&str> = builtins.iter().map(|fp| fp.id()).collect();
        ids.sort_unstable();
        assert_eq!(ids, vec!["csv.v0", "markdown.v0", "pdf.v0", "xlsx.v0"]);
    }

    #[test]
    fn builtins_match_expected_document_variants() {
        let builtins = register_builtins();

        let csv = builtins
            .iter()
            .find(|fp| fp.id() == "csv.v0")
            .expect("csv builtin exists");
        let xlsx = builtins
            .iter()
            .find(|fp| fp.id() == "xlsx.v0")
            .expect("xlsx builtin exists");
        let pdf = builtins
            .iter()
            .find(|fp| fp.id() == "pdf.v0")
            .expect("pdf builtin exists");
        let markdown = builtins
            .iter()
            .find(|fp| fp.id() == "markdown.v0")
            .expect("markdown builtin exists");

        assert!(csv.fingerprint(&csv_doc()).matched);
        assert!(!csv.fingerprint(&xlsx_doc()).matched);

        assert!(xlsx.fingerprint(&xlsx_doc()).matched);
        assert!(!xlsx.fingerprint(&pdf_doc()).matched);

        assert!(pdf.fingerprint(&pdf_doc()).matched);
        assert!(!pdf.fingerprint(&unknown_doc()).matched);

        assert!(
            markdown
                .fingerprint(&markdown_doc("# Heading\n\nBody"))
                .matched
        );
        assert!(
            !markdown
                .fingerprint(&markdown_doc("Body without heading"))
                .matched
        );
    }

    #[test]
    fn non_matching_result_contains_reason_and_failed_assertion() {
        let builtins = register_builtins();
        let csv = builtins
            .iter()
            .find(|fp| fp.id() == "csv.v0")
            .expect("csv builtin exists");

        let result = csv.fingerprint(&pdf_doc());
        assert!(!result.matched);
        assert!(result.reason.is_some());
        assert_eq!(result.assertions.len(), 1);
        assert!(!result.assertions[0].passed);
    }

    #[test]
    fn matching_result_has_no_reason_and_passed_assertion() {
        let builtins = register_builtins();
        let pdf = builtins
            .iter()
            .find(|fp| fp.id() == "pdf.v0")
            .expect("pdf builtin exists");

        let result = pdf.fingerprint(&pdf_doc());
        assert!(result.matched);
        assert!(result.reason.is_none());
        assert_eq!(result.assertions.len(), 1);
        assert!(result.assertions[0].passed);
    }
}
