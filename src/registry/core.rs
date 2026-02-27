use crate::document::Document;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::fmt;

/// Core trait for all fingerprint implementations (DSL-compiled or hand-written Rust).
pub trait Fingerprint: Send + Sync {
    /// Fingerprint identifier, e.g. "argus-model.v1".
    fn id(&self) -> &str;

    /// Expected document format, e.g. "xlsx", "csv", "pdf".
    fn format(&self) -> &str;

    /// Parent fingerprint ID for chained fingerprints.
    fn parent(&self) -> Option<&str> {
        None
    }

    /// Test a document against this fingerprint definition.
    fn fingerprint(&self, doc: &Document) -> FingerprintResult;
}

/// Result of testing a document against a fingerprint.
#[derive(Debug, Clone, Serialize)]
pub struct FingerprintResult {
    pub matched: bool,
    pub reason: Option<String>,
    pub assertions: Vec<AssertionResult>,
    pub extracted: Option<HashMap<String, Value>>,
    pub content_hash: Option<String>,
}

/// Result of evaluating a single assertion.
#[derive(Debug, Clone, Serialize)]
pub struct AssertionResult {
    pub name: String,
    pub passed: bool,
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<Value>,
}

/// Resolves fingerprint IDs to implementations; enforces uniqueness and trust.
pub struct FingerprintRegistry {
    fingerprints: Vec<RegisteredFingerprint>,
}

impl FingerprintRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            fingerprints: Vec::new(),
        }
    }

    /// Register a fingerprint implementation.
    pub fn register(&mut self, fp: Box<dyn Fingerprint>) {
        let info = FingerprintInfo {
            id: fp.id().to_owned(),
            crate_name: "unknown".to_owned(),
            version: "0.0.0".to_owned(),
            source: "unknown".to_owned(),
            format: fp.format().to_owned(),
            parent: fp.parent().map(ToOwned::to_owned),
        };
        self.register_with_info(fp, info);
    }

    /// Register a fingerprint implementation with explicit metadata.
    pub fn register_with_info(&mut self, fp: Box<dyn Fingerprint>, mut info: FingerprintInfo) {
        if info.id.is_empty() {
            info.id = fp.id().to_owned();
        }
        if info.format.is_empty() {
            info.format = fp.format().to_owned();
        }
        if info.parent.is_none() {
            info.parent = fp.parent().map(ToOwned::to_owned);
        }

        self.fingerprints.push(RegisteredFingerprint {
            fingerprint: fp,
            info,
        });
    }

    /// Resolve a fingerprint ID to an implementation.
    pub fn get(&self, id: &str) -> Option<&dyn Fingerprint> {
        self.fingerprints
            .iter()
            .find(|entry| entry.fingerprint.id() == id)
            .map(|entry| &*entry.fingerprint)
    }

    /// Iterate registered fingerprints in registration order.
    pub fn iter(&self) -> impl Iterator<Item = &dyn Fingerprint> {
        self.fingerprints.iter().map(|entry| &*entry.fingerprint)
    }

    /// Resolve metadata for a fingerprint by ID.
    pub fn info_for(&self, id: &str) -> Option<&FingerprintInfo> {
        self.fingerprints
            .iter()
            .find(|entry| entry.fingerprint.id() == id)
            .map(|entry| &entry.info)
    }

    /// List all available fingerprints.
    pub fn list(&self) -> Vec<FingerprintInfo> {
        let mut infos: Vec<FingerprintInfo> = self
            .fingerprints
            .iter()
            .map(|entry| entry.info.clone())
            .collect();
        infos.sort_by(|a, b| a.id.cmp(&b.id).then(a.source.cmp(&b.source)));
        infos
    }

    /// Validate registry invariants for duplicate IDs and trust policy.
    pub fn validate(&self, allowlist: &[String]) -> Result<(), RegistryValidationError> {
        self.validate_no_duplicates()?;
        self.validate_trust(allowlist)?;
        Ok(())
    }

    /// Validate that every fingerprint ID is globally unique.
    pub fn validate_no_duplicates(&self) -> Result<(), RegistryValidationError> {
        let mut providers_by_id: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for entry in &self.fingerprints {
            providers_by_id
                .entry(entry.info.id.clone())
                .or_default()
                .push(entry.info.source.clone());
        }

        for (fingerprint_id, providers) in providers_by_id {
            if providers.len() > 1 {
                return Err(RegistryValidationError::DuplicateFpId {
                    fingerprint_id,
                    providers,
                });
            }
        }

        Ok(())
    }

    /// Validate trust policy for non-builtin providers.
    pub fn validate_trust(&self, allowlist: &[String]) -> Result<(), RegistryValidationError> {
        for entry in &self.fingerprints {
            if is_trusted_source(&entry.info.source, allowlist) {
                continue;
            }

            return Err(RegistryValidationError::UntrustedFp {
                fingerprint_id: entry.info.id.clone(),
                provider: entry.info.source.clone(),
                policy: "allowlist_required".to_owned(),
            });
        }

        Ok(())
    }
}

impl Default for FingerprintRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Metadata about an available fingerprint.
#[derive(Debug, Clone, Serialize)]
pub struct FingerprintInfo {
    pub id: String,
    pub crate_name: String,
    pub version: String,
    pub source: String,
    pub format: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
}

struct RegisteredFingerprint {
    fingerprint: Box<dyn Fingerprint>,
    info: FingerprintInfo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryValidationError {
    DuplicateFpId {
        fingerprint_id: String,
        providers: Vec<String>,
    },
    UntrustedFp {
        fingerprint_id: String,
        provider: String,
        policy: String,
    },
}

impl fmt::Display for RegistryValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateFpId {
                fingerprint_id,
                providers,
            } => write!(
                f,
                "duplicate fingerprint ID '{}' from providers {:?}",
                fingerprint_id, providers
            ),
            Self::UntrustedFp {
                fingerprint_id,
                provider,
                policy,
            } => write!(
                f,
                "untrusted fingerprint '{}' from provider '{}' ({})",
                fingerprint_id, provider, policy
            ),
        }
    }
}

impl std::error::Error for RegistryValidationError {}

fn is_trusted_source(source: &str, allowlist: &[String]) -> bool {
    source == "builtin"
        || source.starts_with("builtin:")
        || allowlist.iter().any(|entry| entry == source)
}

#[cfg(test)]
mod tests {
    use super::{
        AssertionResult, Fingerprint, FingerprintInfo, FingerprintRegistry, FingerprintResult,
        RegistryValidationError,
    };
    use crate::document::Document;
    use serde_json::json;
    use std::collections::HashMap;

    struct TestFingerprint {
        id: &'static str,
        format: &'static str,
        parent: Option<&'static str>,
    }

    impl Fingerprint for TestFingerprint {
        fn id(&self) -> &str {
            self.id
        }

        fn format(&self) -> &str {
            self.format
        }

        fn parent(&self) -> Option<&str> {
            self.parent
        }

        fn fingerprint(&self, _doc: &Document) -> FingerprintResult {
            FingerprintResult {
                matched: true,
                reason: None,
                assertions: vec![AssertionResult {
                    name: "test".to_owned(),
                    passed: true,
                    detail: None,
                    context: None,
                }],
                extracted: Some(HashMap::from([("x".to_owned(), json!("y"))])),
                content_hash: Some("blake3:test".to_owned()),
            }
        }
    }

    #[test]
    fn get_and_list_return_registered_fingerprints() {
        let mut registry = FingerprintRegistry::new();
        registry.register_with_info(
            Box::new(TestFingerprint {
                id: "csv.v0",
                format: "csv",
                parent: None,
            }),
            FingerprintInfo {
                id: "csv.v0".to_owned(),
                crate_name: "fingerprint-core".to_owned(),
                version: "0.1.0".to_owned(),
                source: "builtin:core".to_owned(),
                format: "csv".to_owned(),
                parent: None,
            },
        );

        let resolved = registry.get("csv.v0").expect("resolve fingerprint");
        assert_eq!(resolved.id(), "csv.v0");

        let listed = registry.list();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "csv.v0");
        assert_eq!(listed[0].source, "builtin:core");
    }

    #[test]
    fn validate_no_duplicates_detects_duplicate_ids() {
        let mut registry = FingerprintRegistry::new();
        registry.register_with_info(
            Box::new(TestFingerprint {
                id: "argus-model.v1",
                format: "xlsx",
                parent: None,
            }),
            FingerprintInfo {
                id: "argus-model.v1".to_owned(),
                crate_name: "fingerprint-core".to_owned(),
                version: "0.1.0".to_owned(),
                source: "builtin:argus".to_owned(),
                format: "xlsx".to_owned(),
                parent: None,
            },
        );
        registry.register_with_info(
            Box::new(TestFingerprint {
                id: "argus-model.v1",
                format: "xlsx",
                parent: None,
            }),
            FingerprintInfo {
                id: "argus-model.v1".to_owned(),
                crate_name: "fingerprint-argus".to_owned(),
                version: "0.2.0".to_owned(),
                source: "crate:fingerprint-argus".to_owned(),
                format: "xlsx".to_owned(),
                parent: None,
            },
        );

        assert_eq!(
            registry
                .validate_no_duplicates()
                .expect_err("duplicate expected"),
            RegistryValidationError::DuplicateFpId {
                fingerprint_id: "argus-model.v1".to_owned(),
                providers: vec![
                    "builtin:argus".to_owned(),
                    "crate:fingerprint-argus".to_owned()
                ],
            }
        );
    }

    #[test]
    fn validate_trust_allows_builtin_and_allowlisted_sources() {
        let mut registry = FingerprintRegistry::new();
        registry.register_with_info(
            Box::new(TestFingerprint {
                id: "csv.v0",
                format: "csv",
                parent: None,
            }),
            FingerprintInfo {
                id: "csv.v0".to_owned(),
                crate_name: "fingerprint-core".to_owned(),
                version: "0.1.0".to_owned(),
                source: "builtin:core".to_owned(),
                format: "csv".to_owned(),
                parent: None,
            },
        );
        registry.register_with_info(
            Box::new(TestFingerprint {
                id: "argus-model.v1",
                format: "xlsx",
                parent: None,
            }),
            FingerprintInfo {
                id: "argus-model.v1".to_owned(),
                crate_name: "fingerprint-argus".to_owned(),
                version: "0.2.0".to_owned(),
                source: "crate:fingerprint-argus".to_owned(),
                format: "xlsx".to_owned(),
                parent: None,
            },
        );

        registry
            .validate_trust(&["crate:fingerprint-argus".to_owned()])
            .expect("allowlisted external crate should pass");
    }

    #[test]
    fn validate_trust_rejects_unallowlisted_sources() {
        let mut registry = FingerprintRegistry::new();
        registry.register_with_info(
            Box::new(TestFingerprint {
                id: "argus-model.v1",
                format: "xlsx",
                parent: None,
            }),
            FingerprintInfo {
                id: "argus-model.v1".to_owned(),
                crate_name: "fingerprint-argus".to_owned(),
                version: "0.2.0".to_owned(),
                source: "crate:fingerprint-argus".to_owned(),
                format: "xlsx".to_owned(),
                parent: None,
            },
        );

        assert_eq!(
            registry
                .validate_trust(&[])
                .expect_err("untrusted expected"),
            RegistryValidationError::UntrustedFp {
                fingerprint_id: "argus-model.v1".to_owned(),
                provider: "crate:fingerprint-argus".to_owned(),
                policy: "allowlist_required".to_owned(),
            }
        );
    }

    #[test]
    fn register_propagates_trait_parent_metadata() {
        let mut registry = FingerprintRegistry::new();
        registry.register(Box::new(TestFingerprint {
            id: "cbre-appraisal.v1/rent-roll.v1",
            format: "pdf",
            parent: Some("cbre-appraisal.v1"),
        }));

        let listed = registry.list();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "cbre-appraisal.v1/rent-roll.v1");
        assert_eq!(listed[0].parent.as_deref(), Some("cbre-appraisal.v1"));
    }
}
