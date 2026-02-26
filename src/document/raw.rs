use crate::document::RawDocument;
use std::path::Path;

impl RawDocument {
    /// Read raw bytes from a file.
    pub fn open(_path: &Path) -> Result<Self, String> {
        todo!()
    }
}
