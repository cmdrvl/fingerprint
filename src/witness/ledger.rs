use crate::witness::record::WitnessRecord;
use std::path::Path;

/// Append a witness record to the ledger file.
pub fn append(_ledger_path: &Path, _record: &WitnessRecord) -> Result<(), String> {
    todo!()
}

/// Resolve the witness ledger path from `$EPISTEMIC_WITNESS` or default.
pub fn ledger_path() -> std::path::PathBuf {
    todo!()
}
