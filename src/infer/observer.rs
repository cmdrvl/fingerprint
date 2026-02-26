use crate::document::Document;
use std::collections::HashMap;

/// Observed structural facts about a document (no content stored).
#[derive(Debug, Clone)]
pub struct Observation {
    /// Sheet names found (xlsx only).
    pub sheet_names: Vec<String>,
    /// Cell addresses with non-null values, keyed by sheet name.
    pub non_null_cells: HashMap<String, Vec<String>>,
    /// Row counts per sheet.
    pub row_counts: HashMap<String, u64>,
    /// File extension.
    pub extension: String,
    /// Filename (basename).
    pub filename: String,
}

/// Observe structural facts from a document without storing content.
pub fn observe(_doc: &Document) -> Result<Observation, String> {
    todo!()
}
