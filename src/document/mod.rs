pub mod csv;
pub mod dispatch;
pub mod markdown;
pub mod pdf;
pub mod raw;
pub mod text;
pub mod xlsx;

pub use dispatch::{open_document, open_document_from_path, open_document_with_text_path};
pub use markdown::MarkdownDocument;
use std::path::{Path, PathBuf};
pub use text::TextDocument;

/// Format-specific document access.
///
/// All variants carry the original file path so that metadata assertions
/// like `filename_regex` can operate without a separate context parameter.
pub enum Document {
    Xlsx(XlsxDocument),
    Csv(CsvDocument),
    Pdf(PdfDocument),
    Markdown(MarkdownDocument),
    Text(TextDocument),
    Unknown(RawDocument),
}

impl Document {
    pub fn path(&self) -> &Path {
        match self {
            Document::Xlsx(d) => &d.path,
            Document::Csv(d) => &d.path,
            Document::Pdf(d) => &d.path,
            Document::Markdown(d) => &d.path,
            Document::Text(d) => &d.path,
            Document::Unknown(d) => &d.path,
        }
    }
}

pub struct XlsxDocument {
    pub path: PathBuf,
}

pub struct CsvDocument {
    pub path: PathBuf,
}

pub struct PdfDocument {
    pub path: PathBuf,
    pub text: Option<MarkdownDocument>,
}

#[derive(Debug)]
pub struct RawDocument {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
}
