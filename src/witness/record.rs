use serde::Serialize;
use serde_json::Value;

/// Witness record following the witness.v0 schema.
#[derive(Debug, Serialize)]
pub struct WitnessRecord {
    pub id: String,
    pub tool: String,
    pub version: String,
    pub binary_hash: String,
    pub inputs: Vec<WitnessInput>,
    pub params: Value,
    pub outcome: String,
    pub exit_code: u8,
    pub output_hash: String,
    pub prev: Option<String>,
    pub ts: String,
}

/// An input source referenced in a witness record.
#[derive(Debug, Serialize)]
pub struct WitnessInput {
    pub path: String,
    pub hash: Option<String>,
    pub bytes: Option<u64>,
}
