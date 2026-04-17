use crate::document::{
    CsvDocument, Document, HtmlDocument, MarkdownDocument, PdfDocument, RawDocument, TextDocument,
    XlsxDocument,
};
use std::path::Path;

/// Open a document using format dispatch from extension.
pub fn open_document(path: &Path, extension: &str) -> Result<Document, String> {
    open_document_with_text_path(path, extension, None)
}

/// Open a document using format dispatch from extension, with optional text_path.
pub fn open_document_with_text_path(
    path: &Path,
    extension: &str,
    text_path: Option<&Path>,
) -> Result<Document, String> {
    let extension = extension.to_lowercase();

    match extension.as_str() {
        "xlsx" | "xls" => Ok(Document::Xlsx(XlsxDocument::open(path)?)),
        "csv" => Ok(Document::Csv(CsvDocument {
            path: path.to_path_buf(),
        })),
        "pdf" => Ok(Document::Pdf(PdfDocument::open(path, text_path)?)),
        "html" | "htm" => {
            let doc = HtmlDocument::open(path)?;
            Ok(Document::Html(doc))
        }
        "md" | "markdown" => {
            let doc = MarkdownDocument::open(path)?;
            Ok(Document::Markdown(doc))
        }
        "txt" | "text" => {
            let doc = TextDocument::open(path)?;
            Ok(Document::Text(doc))
        }
        _ => {
            // Fallback to raw bytes for unknown extensions
            let doc = RawDocument::open(path)?;
            Ok(Document::Unknown(doc))
        }
    }
}

/// Open a document using format dispatch from file extension inference.
pub fn open_document_from_path(path: &Path) -> Result<Document, String> {
    open_document_from_path_with_text_path(path, None)
}

/// Open a document using format dispatch from file extension inference, with optional text_path.
pub fn open_document_from_path_with_text_path(
    path: &Path,
    text_path: Option<&Path>,
) -> Result<Document, String> {
    let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");

    open_document_with_text_path(path, extension, text_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use tempfile::NamedTempFile;

    fn fixture(relative: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
    }

    fn make_temp_file_with_extension(contents: &str, extension: &str) -> NamedTempFile {
        let mut file =
            NamedTempFile::with_suffix(format!(".{}", extension)).expect("create temp file");
        file.write_all(contents.as_bytes())
            .expect("write temp file contents");
        file.flush().expect("flush temp file");
        file
    }

    #[test]
    fn dispatches_xlsx_files() {
        let file = fixture("tests/fixtures/files/sample.xlsx");
        let doc = open_document(&file, "xlsx").expect("open xlsx document");

        match doc {
            Document::Xlsx(_) => {} // Expected
            _ => panic!("Expected Xlsx document"),
        }
    }

    #[test]
    fn dispatches_xls_files() {
        let file = fixture("tests/fixtures/files/sample.xls");
        let doc = open_document(&file, "xls").expect("open xls document");

        match doc {
            Document::Xlsx(_) => {} // Expected
            _ => panic!("Expected Xlsx document"),
        }
    }

    #[test]
    fn dispatches_csv_files() {
        let file = make_temp_file_with_extension("col1,col2\nval1,val2", "csv");
        let doc = open_document(file.path(), "csv").expect("open csv document");

        match doc {
            Document::Csv(_) => {} // Expected
            _ => panic!("Expected Csv document"),
        }
    }

    #[test]
    fn dispatches_pdf_files() {
        let file = make_temp_file_with_extension("dummy pdf content", "pdf");
        let doc = open_document(file.path(), "pdf").expect("open pdf document");

        match doc {
            Document::Pdf(pdf) => {
                assert!(pdf.text.is_none());
            }
            _ => panic!("Expected Pdf document"),
        }
    }

    #[test]
    fn dispatches_pdf_files_with_text_path() {
        let pdf = make_temp_file_with_extension("%PDF-1.4\n", "pdf");
        let markdown = make_temp_file_with_extension("# Extracted\n\nBody", "md");
        let doc = open_document_with_text_path(pdf.path(), "pdf", Some(markdown.path()))
            .expect("open pdf document with text path");

        match doc {
            Document::Pdf(pdf) => {
                let text = pdf.text.expect("pdf text should be loaded");
                assert_eq!(text.path, markdown.path());
                assert_eq!(text.headings[0].text, "Extracted");
            }
            _ => panic!("Expected Pdf document"),
        }
    }

    #[test]
    fn dispatches_markdown_files() {
        let file = make_temp_file_with_extension("# Heading\nContent", "md");
        let doc = open_document(file.path(), "md").expect("open markdown document");

        match doc {
            Document::Markdown(_) => {} // Expected
            _ => panic!("Expected Markdown document"),
        }
    }

    #[test]
    fn dispatches_html_files() {
        let file = make_temp_file_with_extension("<h1>Heading</h1><p>Body</p>", "html");
        let doc = open_document(file.path(), "html").expect("open html document");

        match doc {
            Document::Html(doc) => {
                assert_eq!(doc.headings.len(), 1);
                assert_eq!(doc.headings[0].text, "Heading");
            }
            _ => panic!("Expected Html document"),
        }
    }

    #[test]
    fn dispatches_htm_files() {
        let file = make_temp_file_with_extension("<h1>Heading</h1><p>Body</p>", "htm");
        let doc = open_document(file.path(), "htm").expect("open html document");

        match doc {
            Document::Html(doc) => {
                assert_eq!(doc.headings.len(), 1);
                assert_eq!(doc.headings[0].text, "Heading");
            }
            _ => panic!("Expected Html document"),
        }
    }

    #[test]
    fn dispatches_markdown_files_with_full_extension() {
        let file = make_temp_file_with_extension("# Heading\nContent", "markdown");
        let doc = open_document(file.path(), "markdown").expect("open markdown document");

        match doc {
            Document::Markdown(_) => {} // Expected
            _ => panic!("Expected Markdown document"),
        }
    }

    #[test]
    fn markdown_ignores_optional_text_path() {
        let markdown = make_temp_file_with_extension("# Source\n\nBody", "md");
        let alternate = make_temp_file_with_extension("# Alternate\n\nBody", "md");
        let doc = open_document_with_text_path(markdown.path(), "md", Some(alternate.path()))
            .expect("open markdown document");

        match doc {
            Document::Markdown(doc) => {
                assert_eq!(doc.path, markdown.path());
                assert_eq!(doc.headings[0].text, "Source");
            }
            _ => panic!("Expected Markdown document"),
        }
    }

    #[test]
    fn dispatches_text_files() {
        let file = make_temp_file_with_extension("Plain text content", "txt");
        let doc = open_document(file.path(), "txt").expect("open text document");

        match doc {
            Document::Text(_) => {} // Expected
            _ => panic!("Expected Text document"),
        }
    }

    #[test]
    fn dispatches_text_files_with_text_extension() {
        let file = make_temp_file_with_extension("Plain text content", "text");
        let doc = open_document(file.path(), "text").expect("open text document");

        match doc {
            Document::Text(_) => {} // Expected
            _ => panic!("Expected Text document"),
        }
    }

    #[test]
    fn dispatches_unknown_files_to_raw() {
        let file = make_temp_file_with_extension("binary content", "bin");
        let doc = open_document(file.path(), "bin").expect("open raw document");

        match doc {
            Document::Unknown(_) => {} // Expected
            _ => panic!("Expected Unknown (Raw) document"),
        }
    }

    #[test]
    fn case_insensitive_extension_matching() {
        let file = fixture("tests/fixtures/files/sample.xlsx");
        let doc = open_document(&file, "XLSX").expect("open document");

        match doc {
            Document::Xlsx(_) => {} // Expected
            _ => panic!("Expected Xlsx document"),
        }
    }

    #[test]
    fn rejects_unreadable_spreadsheet_files_during_dispatch() {
        let file = fixture("tests/fixtures/files/corrupt.xlsx");
        let error = match open_document(&file, "xlsx") {
            Ok(_) => panic!("corrupt xlsx should fail early"),
            Err(error) => error,
        };
        assert!(error.contains("failed to open spreadsheet"), "{error}");
    }

    #[test]
    fn open_document_from_path_infers_extension() {
        let file = make_temp_file_with_extension("# Test\nContent", "md");
        let doc = open_document_from_path(file.path()).expect("open document from path");

        match doc {
            Document::Markdown(_) => {} // Expected
            _ => panic!("Expected Markdown document"),
        }
    }

    #[test]
    fn open_document_from_path_loads_pdf_text_path_when_provided() {
        let pdf = make_temp_file_with_extension("%PDF-1.4\n", "pdf");
        let markdown = make_temp_file_with_extension("# Extracted\n\nBody", "md");
        let doc = open_document_from_path_with_text_path(pdf.path(), Some(markdown.path()))
            .expect("open document from path");

        match doc {
            Document::Pdf(pdf) => {
                let text = pdf.text.expect("pdf text should be loaded");
                assert_eq!(text.path, markdown.path());
                assert_eq!(text.headings[0].text, "Extracted");
            }
            _ => panic!("Expected Pdf document"),
        }
    }

    #[test]
    fn open_document_from_path_handles_no_extension() {
        let mut file = NamedTempFile::new().expect("create temp file");
        file.write_all(b"some content").expect("write content");
        file.flush().expect("flush file");

        let doc = open_document_from_path(file.path()).expect("open document");

        match doc {
            Document::Unknown(_) => {} // Expected - no extension defaults to raw
            _ => panic!("Expected Unknown (Raw) document"),
        }
    }
}
