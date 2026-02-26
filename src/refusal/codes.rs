use serde::Serialize;

/// Run-mode refusal codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum RefusalCode {
    /// Invalid JSONL, missing bytes_hash, or unrecognized upstream version.
    #[serde(rename = "E_BAD_INPUT")]
    BadInput,
    /// Fingerprint ID not found in any installed crate.
    #[serde(rename = "E_UNKNOWN_FP")]
    UnknownFp,
    /// Same fingerprint_id from multiple providers.
    #[serde(rename = "E_DUPLICATE_FP_ID")]
    DuplicateFpId,
    /// External fingerprint crate/plugin not allowlisted.
    #[serde(rename = "E_UNTRUSTED_FP")]
    UntrustedFp,
}

/// Compile-mode refusal codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CompileRefusalCode {
    /// YAML parse error or schema violation.
    #[serde(rename = "E_INVALID_YAML")]
    InvalidYaml,
    /// Assertion type not recognized.
    #[serde(rename = "E_UNKNOWN_ASSERTION")]
    UnknownAssertion,
    /// Required field missing from DSL.
    #[serde(rename = "E_MISSING_FIELD")]
    MissingField,
}
