use crate::document::Document;
use crate::registry::AssertionResult;
use serde::Deserialize;

/// DSL assertion types. Each variant maps to one assertion in a `.fp.yaml` file.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Assertion {
    FilenameRegex {
        pattern: String,
    },
    SheetExists(String),
    SheetNameRegex {
        pattern: String,
    },
    CellEq {
        sheet: String,
        cell: String,
        value: String,
    },
    CellRegex {
        sheet: String,
        cell: String,
        pattern: String,
    },
    RangeNonNull {
        sheet: String,
        range: String,
    },
    RangePopulated {
        sheet: String,
        range: String,
        min_pct: f64,
    },
    SheetMinRows {
        sheet: String,
        min_rows: u64,
    },
    SumEq {
        range: String,
        equals_cell: String,
        tolerance: f64,
    },
    WithinTolerance {
        cell: String,
        min: f64,
        max: f64,
    },
}

/// Evaluate a single assertion against a document.
pub fn evaluate(_assertion: &Assertion, _doc: &Document) -> AssertionResult {
    todo!()
}
