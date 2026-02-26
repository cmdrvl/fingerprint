use serde::Serialize;

/// Structured progress event emitted to stderr.
#[derive(Debug, Serialize)]
pub struct ProgressEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub tool: String,
    pub processed: u64,
    pub total: Option<u64>,
    pub percent: Option<f64>,
    pub elapsed_ms: u64,
}

/// Report progress to stderr as JSONL.
pub fn report_progress(_event: &ProgressEvent) {
    todo!()
}
