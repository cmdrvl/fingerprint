use std::fmt;

use serde::Serialize;
use serde_json::Value;

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
    /// Child fingerprint references a parent not loaded in --fp.
    #[serde(rename = "E_ORPHAN_CHILD")]
    OrphanChild,
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

impl fmt::Display for RefusalCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::BadInput => "Invalid input stream",
            Self::UnknownFp => "Fingerprint ID not found",
            Self::DuplicateFpId => "Duplicate fingerprint ID discovered",
            Self::UntrustedFp => "Fingerprint provider not allowlisted",
            Self::OrphanChild => "Child fingerprint references unloaded parent",
        };

        f.write_str(message)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BadInputDetail {
    pub line: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub missing_field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnknownFpDetail {
    pub fingerprint_id: String,
    pub available: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DuplicateFpIdDetail {
    pub fingerprint_id: String,
    pub providers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UntrustedFpDetail {
    pub fingerprint_id: String,
    pub provider: String,
    pub policy: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OrphanChildDetail {
    pub child_id: String,
    pub parent_id: String,
    pub loaded: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum RefusalDetail {
    BadInput(BadInputDetail),
    UnknownFp(UnknownFpDetail),
    DuplicateFpId(DuplicateFpIdDetail),
    UntrustedFp(UntrustedFpDetail),
    OrphanChild(OrphanChildDetail),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RefusalEnvelope {
    pub version: String,
    pub outcome: String,
    pub refusal: RefusalBody,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RefusalBody {
    pub code: RefusalCode,
    pub message: String,
    pub detail: Value,
    pub next_command: Option<String>,
}

pub fn build_envelope(
    code: RefusalCode,
    message: impl Into<String>,
    detail: RefusalDetail,
    next_command: Option<String>,
) -> RefusalEnvelope {
    RefusalEnvelope {
        version: "fingerprint.v0".to_owned(),
        outcome: "REFUSAL".to_owned(),
        refusal: RefusalBody {
            code,
            message: message.into(),
            detail: serde_json::to_value(detail)
                .expect("refusal detail serialization should never fail"),
            next_command,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn serializes_refusal_codes() {
        assert_eq!(
            serde_json::to_value(RefusalCode::BadInput).expect("serialize code"),
            json!("E_BAD_INPUT")
        );
        assert_eq!(
            serde_json::to_value(RefusalCode::UnknownFp).expect("serialize code"),
            json!("E_UNKNOWN_FP")
        );
        assert_eq!(
            serde_json::to_value(RefusalCode::DuplicateFpId).expect("serialize code"),
            json!("E_DUPLICATE_FP_ID")
        );
        assert_eq!(
            serde_json::to_value(RefusalCode::UntrustedFp).expect("serialize code"),
            json!("E_UNTRUSTED_FP")
        );
        assert_eq!(
            serde_json::to_value(RefusalCode::OrphanChild).expect("serialize code"),
            json!("E_ORPHAN_CHILD")
        );
    }

    #[test]
    fn serializes_compile_refusal_codes() {
        assert_eq!(
            serde_json::to_value(CompileRefusalCode::InvalidYaml).expect("serialize code"),
            json!("E_INVALID_YAML")
        );
        assert_eq!(
            serde_json::to_value(CompileRefusalCode::UnknownAssertion).expect("serialize code"),
            json!("E_UNKNOWN_ASSERTION")
        );
        assert_eq!(
            serde_json::to_value(CompileRefusalCode::MissingField).expect("serialize code"),
            json!("E_MISSING_FIELD")
        );
    }

    #[test]
    fn serializes_bad_input_detail_variants() {
        assert_eq!(
            serde_json::to_value(RefusalDetail::BadInput(BadInputDetail {
                line: 42,
                error: Some("invalid JSON".to_owned()),
                missing_field: None,
                version: None,
            }))
            .expect("serialize detail"),
            json!({
                "line": 42,
                "error": "invalid JSON"
            })
        );
        assert_eq!(
            serde_json::to_value(RefusalDetail::BadInput(BadInputDetail {
                line: 1,
                error: None,
                missing_field: Some("bytes_hash".to_owned()),
                version: None,
            }))
            .expect("serialize detail"),
            json!({
                "line": 1,
                "missing_field": "bytes_hash"
            })
        );
        assert_eq!(
            serde_json::to_value(RefusalDetail::BadInput(BadInputDetail {
                line: 1,
                error: None,
                missing_field: None,
                version: Some("unknown.v3".to_owned()),
            }))
            .expect("serialize detail"),
            json!({
                "line": 1,
                "version": "unknown.v3"
            })
        );
    }

    #[test]
    fn serializes_other_detail_variants() {
        assert_eq!(
            serde_json::to_value(RefusalDetail::UnknownFp(UnknownFpDetail {
                fingerprint_id: "argus-model.v1".to_owned(),
                available: vec!["csv.v0".to_owned(), "xlsx.v0".to_owned()],
            }))
            .expect("serialize detail"),
            json!({
                "fingerprint_id": "argus-model.v1",
                "available": ["csv.v0", "xlsx.v0"]
            })
        );

        assert_eq!(
            serde_json::to_value(RefusalDetail::DuplicateFpId(DuplicateFpIdDetail {
                fingerprint_id: "argus-model.v1".to_owned(),
                providers: vec![
                    "builtin:argus".to_owned(),
                    "crate:fingerprint-argus".to_owned(),
                ],
            }))
            .expect("serialize detail"),
            json!({
                "fingerprint_id": "argus-model.v1",
                "providers": ["builtin:argus", "crate:fingerprint-argus"]
            })
        );

        assert_eq!(
            serde_json::to_value(RefusalDetail::UntrustedFp(UntrustedFpDetail {
                fingerprint_id: "argus-model.v1".to_owned(),
                provider: "crate:fingerprint-argus".to_owned(),
                policy: "allowlist_required".to_owned(),
            }))
            .expect("serialize detail"),
            json!({
                "fingerprint_id": "argus-model.v1",
                "provider": "crate:fingerprint-argus",
                "policy": "allowlist_required"
            })
        );

        assert_eq!(
            serde_json::to_value(RefusalDetail::OrphanChild(OrphanChildDetail {
                child_id: "cbre-appraisal.v1/rent-roll.v1".to_owned(),
                parent_id: "cbre-appraisal.v1".to_owned(),
                loaded: vec!["csv.v0".to_owned(), "xlsx.v0".to_owned()],
            }))
            .expect("serialize detail"),
            json!({
                "child_id": "cbre-appraisal.v1/rent-roll.v1",
                "parent_id": "cbre-appraisal.v1",
                "loaded": ["csv.v0", "xlsx.v0"]
            })
        );
    }

    #[test]
    fn builds_and_serializes_refusal_envelope() {
        let envelope = build_envelope(
            RefusalCode::UnknownFp,
            "Fingerprint ID not found",
            RefusalDetail::UnknownFp(UnknownFpDetail {
                fingerprint_id: "argus-model.v1".to_owned(),
                available: vec!["csv.v0".to_owned(), "xlsx.v0".to_owned()],
            }),
            Some("cargo install fingerprint-argus".to_owned()),
        );

        let json_value = serde_json::to_value(&envelope).expect("serialize envelope");
        assert_eq!(
            json_value,
            json!({
                "version": "fingerprint.v0",
                "outcome": "REFUSAL",
                "refusal": {
                    "code": "E_UNKNOWN_FP",
                    "message": "Fingerprint ID not found",
                    "detail": {
                        "fingerprint_id": "argus-model.v1",
                        "available": ["csv.v0", "xlsx.v0"]
                    },
                    "next_command": "cargo install fingerprint-argus"
                }
            })
        );
    }
}
