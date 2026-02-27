/// Pipeline outcome determining exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// All records matched a fingerprint (exit 0).
    AllMatched,
    /// Some records unmatched or skipped (exit 1).
    Partial,
    /// Pipeline-level failure or CLI error (exit 2).
    Refusal,
}

impl Outcome {
    pub fn exit_code(self) -> u8 {
        match self {
            Outcome::AllMatched => 0,
            Outcome::Partial => 1,
            Outcome::Refusal => 2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Outcome;

    #[test]
    fn all_matched_maps_to_zero() {
        assert_eq!(Outcome::AllMatched.exit_code(), 0);
    }

    #[test]
    fn partial_maps_to_one() {
        assert_eq!(Outcome::Partial.exit_code(), 1);
    }

    #[test]
    fn refusal_maps_to_two() {
        assert_eq!(Outcome::Refusal.exit_code(), 2);
    }
}
