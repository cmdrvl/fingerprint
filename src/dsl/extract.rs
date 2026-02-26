use crate::document::Document;
use crate::dsl::parser::ExtractSection;
use serde_json::Value;
use std::collections::HashMap;

/// Extract content sections from a matched document.
pub fn extract(
    _doc: &Document,
    _sections: &[ExtractSection],
) -> Result<HashMap<String, Value>, String> {
    todo!()
}
