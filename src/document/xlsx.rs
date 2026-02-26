use crate::document::XlsxDocument;
use std::path::Path;

impl XlsxDocument {
    /// Open an XLSX file for lazy sheet access via calamine.
    pub fn open(_path: &Path) -> Result<Self, String> {
        todo!()
    }
}
