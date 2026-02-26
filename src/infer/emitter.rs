use crate::infer::aggregator::AggregatedProfile;
use std::io::Write;

/// Emit a `.fp.yaml` definition from an aggregated profile.
pub fn emit_yaml(_profile: &AggregatedProfile, _out: &mut dyn Write) -> Result<(), String> {
    todo!()
}
