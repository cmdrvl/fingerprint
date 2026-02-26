use crate::registry::FingerprintRegistry;
use serde_json::Value;

/// Process records in parallel with bounded reorder buffer, emitting in input order.
pub fn process_parallel(
    _records: Vec<Value>,
    _registry: &FingerprintRegistry,
    _jobs: usize,
) -> Vec<Value> {
    todo!()
}
