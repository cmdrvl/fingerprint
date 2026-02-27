use crate::document::Document;
use std::collections::HashMap;

/// Observed structural facts about a single document.
#[derive(Debug, Clone, Default)]
pub struct Observation {
    /// Normalized document format (`xlsx`, `csv`, `pdf`).
    pub format: String,
    /// File extension (without dot).
    pub extension: String,
    /// Basename only.
    pub filename: String,
    /// Sheet names (spreadsheet formats).
    pub sheet_names: Vec<String>,
    /// Non-empty row counts keyed by sheet name.
    pub row_counts: HashMap<String, u64>,
    /// Sample cell values keyed by `Sheet!A1`.
    pub cell_values: HashMap<String, String>,
    /// CSV header row.
    pub csv_headers: Vec<String>,
    /// CSV non-empty row count (excluding header row).
    pub csv_row_count: Option<u64>,
    /// PDF page count.
    pub pdf_page_count: Option<u64>,
    /// PDF metadata key/value map.
    pub pdf_metadata: HashMap<String, String>,
}

/// Observe structural facts from a document without persisting document text.
pub fn observe(doc: &Document) -> Result<Observation, String> {
    let path = doc.path();
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_owned();

    match doc {
        Document::Xlsx(xlsx) => {
            let mut sheet_names = xlsx.sheet_names()?;
            sheet_names.sort_unstable();

            let mut row_counts = HashMap::new();
            let mut cell_values = HashMap::new();
            for sheet in &sheet_names {
                let row_count = xlsx.sheet_row_count(sheet)? as u64;
                row_counts.insert(sheet.clone(), row_count);

                for cell in ["A1", "B1", "A2", "B2"] {
                    if let Some(value) = xlsx.read_cell(sheet, cell)? {
                        let value = normalize_scalar(&value);
                        if !value.is_empty() {
                            cell_values.insert(format!("{sheet}!{cell}"), value);
                        }
                    }
                }
            }

            Ok(Observation {
                format: "xlsx".to_owned(),
                extension,
                filename,
                sheet_names,
                row_counts,
                cell_values,
                csv_headers: Vec::new(),
                csv_row_count: None,
                pdf_page_count: None,
                pdf_metadata: HashMap::new(),
            })
        }
        Document::Csv(csv) => {
            let headers = csv.headers()?;
            let rows = csv.rows()?;
            let row_count = rows
                .iter()
                .filter(|row| row.iter().any(|value| !value.trim().is_empty()))
                .count() as u64;

            let mut row_counts = HashMap::new();
            row_counts.insert("Sheet1".to_owned(), row_count);

            Ok(Observation {
                format: "csv".to_owned(),
                extension,
                filename,
                sheet_names: vec!["Sheet1".to_owned(), "csv".to_owned()],
                row_counts,
                cell_values: HashMap::new(),
                csv_headers: headers
                    .into_iter()
                    .map(|header| normalize_scalar(&header))
                    .collect(),
                csv_row_count: Some(row_count),
                pdf_page_count: None,
                pdf_metadata: HashMap::new(),
            })
        }
        Document::Pdf(pdf) => {
            let page_count = pdf.page_count()?;
            let metadata = pdf
                .metadata()
                .unwrap_or_default()
                .into_iter()
                .map(|(key, value)| (key, normalize_scalar(&value)))
                .collect::<HashMap<_, _>>();

            Ok(Observation {
                format: "pdf".to_owned(),
                extension,
                filename,
                sheet_names: Vec::new(),
                row_counts: HashMap::new(),
                cell_values: HashMap::new(),
                csv_headers: Vec::new(),
                csv_row_count: None,
                pdf_page_count: Some(page_count),
                pdf_metadata: metadata,
            })
        }
        _ => Err(format!(
            "infer supports xlsx/csv/pdf documents only, got '{}'",
            path.display()
        )),
    }
}

fn normalize_scalar(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::observe;
    use crate::document::{CsvDocument, Document, PdfDocument, XlsxDocument};
    use lopdf::{Object, dictionary};
    use std::path::Path;
    use tempfile::NamedTempFile;

    fn fixture(path: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
    }

    #[test]
    fn observes_xlsx_structural_facts() {
        let path = fixture("tests/fixtures/files/sample.xlsx");
        let doc = Document::Xlsx(XlsxDocument { path });
        let observation = observe(&doc).expect("observe xlsx");

        assert_eq!(observation.format, "xlsx");
        assert!(!observation.sheet_names.is_empty());
        assert!(observation.row_counts.values().all(|count| *count > 0));
    }

    #[test]
    fn observes_csv_structural_facts() {
        let path = fixture("tests/fixtures/files/sample.csv");
        let doc = Document::Csv(CsvDocument { path });
        let observation = observe(&doc).expect("observe csv");

        assert_eq!(observation.format, "csv");
        assert_eq!(observation.sheet_names[0], "Sheet1");
        assert!(!observation.csv_headers.is_empty());
        assert!(observation.csv_row_count.expect("csv row count") > 0);
    }

    #[test]
    fn observes_pdf_structural_facts() {
        let file = write_minimal_pdf_with_metadata();
        let doc = Document::Pdf(PdfDocument {
            path: file.path().to_path_buf(),
            text: None,
        });
        let observation = observe(&doc).expect("observe pdf");

        assert_eq!(observation.format, "pdf");
        assert!(observation.pdf_page_count.expect("pdf page count") > 0);
        assert_eq!(
            observation.pdf_metadata.get("Producer").map(String::as_str),
            Some("infer-observer-test")
        );
    }

    fn write_minimal_pdf_with_metadata() -> NamedTempFile {
        let file = NamedTempFile::with_suffix(".pdf").expect("create pdf temp file");
        let mut document = lopdf::Document::with_version("1.5");

        let pages_id = document.new_object_id();
        let page_id = document.new_object_id();
        let content_id = document.add_object(lopdf::Stream::new(
            lopdf::Dictionary::new(),
            b"BT ET".to_vec(),
        ));
        document.objects.insert(
            page_id,
            Object::Dictionary(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Contents" => content_id,
                "MediaBox" => vec![0.into(), 0.into(), 300.into(), 300.into()],
            }),
        );
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_id.into()],
                "Count" => 1,
            }),
        );
        let info_id = document.add_object(dictionary! {
            "Producer" => Object::string_literal("infer-observer-test"),
        });
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        document.trailer.set("Root", catalog_id);
        document.trailer.set("Info", info_id);
        document.compress();
        document.save(file.path()).expect("save pdf");
        file
    }
}
