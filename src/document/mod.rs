pub mod csv;
pub mod dispatch;
pub mod pdf;
pub mod raw;
pub mod xlsx;

use std::path::{Path, PathBuf};

/// Format-specific document access.
///
/// All variants carry the original file path so that metadata assertions
/// like `filename_regex` can operate without a separate context parameter.
pub enum Document {
    Xlsx(XlsxDocument),
    Csv(CsvDocument),
    Pdf(PdfDocument),
    Unknown(RawDocument),
}

impl Document {
    pub fn path(&self) -> &Path {
        match self {
            Document::Xlsx(d) => &d.path,
            Document::Csv(d) => &d.path,
            Document::Pdf(d) => &d.path,
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
}

pub struct RawDocument {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
}
