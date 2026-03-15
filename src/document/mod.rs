pub mod csv;
pub mod dispatch;
pub mod html;
pub mod markdown;
pub mod pdf;
pub mod raw;
pub mod text;
pub mod xlsx;

pub use dispatch::{
    open_document, open_document_from_path, open_document_from_path_with_text_path,
    open_document_with_text_path,
};
pub use html::HtmlDocument;
pub use markdown::{Heading, MarkdownDocument, Section, Table};
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
    Html(HtmlDocument),
    Markdown(MarkdownDocument),
    Text(TextDocument),
    Unknown(RawDocument),
}

#[derive(Clone, Copy)]
pub struct StructuredDocument<'a> {
    pub normalized: &'a str,
    pub headings: &'a [Heading],
    pub sections: &'a [Section],
    pub tables: &'a [Table],
}

impl Document {
    pub fn path(&self) -> &Path {
        match self {
            Document::Xlsx(d) => &d.path,
            Document::Csv(d) => &d.path,
            Document::Pdf(d) => &d.path,
            Document::Html(d) => &d.path,
            Document::Markdown(d) => &d.path,
            Document::Text(d) => &d.path,
            Document::Unknown(d) => &d.path,
        }
    }
}

impl<'a> StructuredDocument<'a> {
    pub fn from_markdown(document: &'a MarkdownDocument) -> Self {
        Self {
            normalized: document.normalized.as_str(),
            headings: &document.headings,
            sections: &document.sections,
            tables: &document.tables,
        }
    }

    pub fn from_html(document: &'a HtmlDocument) -> Self {
        Self {
            normalized: document.normalized.as_str(),
            headings: &document.headings,
            sections: &document.sections,
            tables: &document.tables,
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
