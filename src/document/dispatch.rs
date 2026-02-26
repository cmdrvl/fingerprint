use crate::document::Document;
use std::path::Path;

/// Open a document using format dispatch from extension/mime_guess.
pub fn open_document(_path: &Path, _extension: &str) -> Result<Document, String> {
    todo!()
}
