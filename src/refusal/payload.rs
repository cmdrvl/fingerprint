use crate::refusal::codes::RefusalCode;
use serde::Serialize;
use serde_json::Value;

/// Refusal envelope emitted to stdout on exit 2.
#[derive(Debug, Serialize)]
pub struct RefusalPayload {
    pub version: String,
    pub outcome: String,
    pub refusal: RefusalDetail,
}

/// Detail within a refusal envelope.
#[derive(Debug, Serialize)]
pub struct RefusalDetail {
    pub code: RefusalCode,
    pub message: String,
    pub detail: Value,
    pub next_command: Option<String>,
}

impl RefusalPayload {
    /// Build a refusal payload for run-mode errors.
    pub fn new(_code: RefusalCode, _message: &str, _detail: Value) -> Self {
        todo!()
    }
}
