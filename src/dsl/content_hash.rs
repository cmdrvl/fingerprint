use serde_json::Value;
use std::collections::HashMap;

/// Compute BLAKE3 content hash over extracted content sections.
pub fn content_hash(_extracted: &HashMap<String, Value>, _over: &[String]) -> String {
    todo!()
}
