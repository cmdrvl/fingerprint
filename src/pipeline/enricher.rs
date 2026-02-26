use crate::registry::FingerprintRegistry;
use serde_json::Value;

/// Enrich a single JSONL record with fingerprint results.
pub fn enrich_record(_record: &Value, _registry: &FingerprintRegistry) -> Value {
    todo!()
}
