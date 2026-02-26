use serde_json::Value;
use std::io::Write;

/// Write JSONL records to an output stream (one JSON object per line).
pub fn write_jsonl(_out: &mut dyn Write, _records: &[Value]) -> Result<(), String> {
    todo!()
}
