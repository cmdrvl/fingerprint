use fingerprint::document::{CsvDocument, Document, MarkdownDocument, PdfDocument};
use fingerprint::dsl::assertions::{Assertion, evaluate_assertion};
use std::fs;
use std::path::Path;
use tempfile::NamedTempFile;

fn fixture(path: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

fn pdf_with_markdown(markdown: &str) -> Document {
    let path = fixture("tests/fixtures/test_files/report.pdf");
    let markdown_file = NamedTempFile::with_suffix(".md").expect("create markdown temp file");
    fs::write(markdown_file.path(), markdown).expect("write markdown content");
    let text = MarkdownDocument::open(markdown_file.path()).expect("open markdown text");
    Document::Pdf(PdfDocument {
        path,
        text: Some(text),
    })
}

#[test]
fn text_near_bidirectional_search() {
    let doc = pdf_with_markdown("Invoice: INV-2023-001\nTotal: $1,234.56\nDue Date: 2023-12-31");

    let forward = Assertion::TextNear {
        anchor: "Invoice".to_owned(),
        pattern: "Total".to_owned(),
        within_chars: 50,
    };
    let reverse = Assertion::TextNear {
        anchor: "Total".to_owned(),
        pattern: "Invoice".to_owned(),
        within_chars: 50,
    };
    let out_of_range = Assertion::TextNear {
        anchor: "Invoice".to_owned(),
        pattern: "Due Date".to_owned(),
        within_chars: 10,
    };

    assert!(
        evaluate_assertion(&doc, &forward)
            .expect("evaluate forward")
            .passed
    );
    assert!(
        evaluate_assertion(&doc, &reverse)
            .expect("evaluate reverse")
            .passed
    );
    assert!(
        !evaluate_assertion(&doc, &out_of_range)
            .expect("evaluate out of range")
            .passed
    );
}

#[test]
fn table_shape_type_inference_edge_cases() {
    let markdown =
        MarkdownDocument::open(&fixture("tests/fixtures/files/sample.md")).expect("open markdown");
    let doc = Document::Markdown(markdown);

    let matching = Assertion::TableShape {
        heading: "(?i)rent roll".to_owned(),
        index: Some(0),
        min_columns: 4,
        column_types: vec![
            "string".to_owned(),
            "number".to_owned(),
            "number".to_owned(),
            "number".to_owned(),
        ],
    };
    let mismatch = Assertion::TableShape {
        heading: "(?i)rent roll".to_owned(),
        index: Some(0),
        min_columns: 4,
        column_types: vec![
            "number".to_owned(),
            "number".to_owned(),
            "number".to_owned(),
            "number".to_owned(),
        ],
    };

    assert!(
        evaluate_assertion(&doc, &matching)
            .expect("evaluate matching table shape")
            .passed
    );
    assert!(
        !evaluate_assertion(&doc, &mismatch)
            .expect("evaluate mismatched table shape")
            .passed
    );
}

#[test]
fn markdown_normalization_edge_cases() {
    let doc = pdf_with_markdown(
        "# Main Title\n\n## Subsection\n\nSome **bold** and *italic* text.\n\n- List item 1\n- List item 2",
    );

    assert!(
        evaluate_assertion(&doc, &Assertion::TextContains("Main Title".to_owned()))
            .expect("evaluate heading")
            .passed
    );
    assert!(
        evaluate_assertion(
            &doc,
            &Assertion::TextContains("Some **bold** and *italic* text.".to_owned())
        )
        .expect("evaluate normalized formatting")
        .passed
    );
    assert!(
        evaluate_assertion(
            &doc,
            &Assertion::TextNear {
                anchor: "List item 1".to_owned(),
                pattern: "List item 2".to_owned(),
                within_chars: 20,
            }
        )
        .expect("evaluate list adjacency")
        .passed
    );
}

#[test]
fn text_regex_boundary_conditions() {
    let doc =
        pdf_with_markdown("Invoice ID: INV-2023-001\nAmount: $1,234.56\nEmail: test@example.com");

    assert!(
        evaluate_assertion(
            &doc,
            &Assertion::TextRegex {
                pattern: r"^Invoice ID:".to_owned()
            }
        )
        .expect("evaluate start anchor")
        .passed
    );
    assert!(
        evaluate_assertion(
            &doc,
            &Assertion::TextRegex {
                pattern: r"\bINV-\d{4}-\d{3}\b".to_owned()
            }
        )
        .expect("evaluate word boundary")
        .passed
    );
    assert!(
        !evaluate_assertion(
            &doc,
            &Assertion::TextRegex {
                pattern: "invoice".to_owned()
            }
        )
        .expect("evaluate case sensitivity")
        .passed
    );
}

#[test]
fn spreadsheet_cell_and_range_boundary_conditions() {
    let doc = Document::Csv(CsvDocument {
        path: fixture("tests/fixtures/files/sample.csv"),
    });

    assert!(
        evaluate_assertion(
            &doc,
            &Assertion::CellEq {
                sheet: "Sheet1".to_owned(),
                cell: "A1".to_owned(),
                value: "Tenant".to_owned(),
            }
        )
        .expect("evaluate A1")
        .passed
    );
    assert!(
        !evaluate_assertion(
            &doc,
            &Assertion::CellEq {
                sheet: "Sheet1".to_owned(),
                cell: "Z99".to_owned(),
                value: "".to_owned(),
            }
        )
        .expect("evaluate empty cell")
        .passed
    );

    assert!(
        !evaluate_assertion(
            &doc,
            &Assertion::RangeNonNull {
                sheet: "Sheet1".to_owned(),
                range: "INVALID".to_owned(),
            }
        )
        .expect("evaluate invalid range")
        .passed
    );
}

#[test]
fn metadata_extraction_edge_cases() {
    let doc = Document::Pdf(PdfDocument {
        path: fixture("tests/fixtures/test_files/report.pdf"),
        text: None,
    });

    assert!(
        !evaluate_assertion(
            &doc,
            &Assertion::MetadataRegex {
                key: "title".to_owned(),
                pattern: ".+".to_owned(),
            }
        )
        .expect("evaluate metadata key case")
        .passed
    );
    assert!(
        !evaluate_assertion(
            &doc,
            &Assertion::MetadataRegex {
                key: "NonexistentKey".to_owned(),
                pattern: ".+".to_owned(),
            }
        )
        .expect("evaluate missing metadata")
        .passed
    );
}
