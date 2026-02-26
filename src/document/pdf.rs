use crate::document::PdfDocument;
use std::path::Path;

impl PdfDocument {
    /// Open a PDF file for structural access via lopdf.
    pub fn open(_path: &Path) -> Result<Self, String> {
        todo!()
    }
}
