use serde_json::Value;
use std::io::BufRead;

/// Read JSONL records from an input source, validating structure and version.
pub fn read_records(_input: &mut dyn BufRead) -> Result<Vec<Value>, String> {
    todo!()
}
