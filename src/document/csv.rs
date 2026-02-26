use crate::document::CsvDocument;
use std::path::Path;

impl CsvDocument {
    /// Open a CSV file for header + streaming record access.
    pub fn open(_path: &Path) -> Result<Self, String> {
        todo!()
    }
}
