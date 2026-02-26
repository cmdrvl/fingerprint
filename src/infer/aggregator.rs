use crate::dsl::assertions::Assertion;
use crate::infer::observer::Observation;

/// Aggregated result from observing multiple documents of the same type.
#[derive(Debug, Clone)]
pub struct AggregatedProfile {
    /// Common sheet names across all observations.
    pub common_sheets: Vec<String>,
    /// Inferred assertions from the corpus.
    pub assertions: Vec<Assertion>,
    /// Detected format (xlsx, csv, pdf).
    pub format: String,
}

/// Aggregate observations from multiple documents into a fingerprint profile.
pub fn aggregate(_observations: &[Observation]) -> AggregatedProfile {
    todo!()
}
