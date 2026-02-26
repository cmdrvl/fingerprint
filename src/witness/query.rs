use std::path::Path;

/// Query witness records matching filter criteria.
pub fn query(_ledger_path: &Path) -> Result<Vec<serde_json::Value>, String> {
    todo!()
}

/// Return the last witness record from the ledger.
pub fn last(_ledger_path: &Path) -> Result<Option<serde_json::Value>, String> {
    todo!()
}

/// Count witness records matching filter criteria.
pub fn count(_ledger_path: &Path) -> Result<u64, String> {
    todo!()
}
