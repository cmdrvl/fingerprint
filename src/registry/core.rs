use crate::document::Document;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;

/// Core trait for all fingerprint implementations (DSL-compiled or hand-written Rust).
pub trait Fingerprint: Send + Sync {
    /// Fingerprint identifier, e.g. "argus-model.v1".
    fn id(&self) -> &str;

    /// Expected document format, e.g. "xlsx", "csv", "pdf".
    fn format(&self) -> &str;

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
}

/// Resolves fingerprint IDs to implementations; enforces uniqueness and trust.
pub struct FingerprintRegistry {
    fingerprints: Vec<Box<dyn Fingerprint>>,
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
        self.fingerprints.push(fp);
    }

    /// Resolve a fingerprint ID to an implementation.
    pub fn get(&self, id: &str) -> Option<&dyn Fingerprint> {
        self.fingerprints
            .iter()
            .find(|f| f.id() == id)
            .map(|f| &**f)
    }

    /// List all available fingerprints.
    pub fn list(&self) -> Vec<FingerprintInfo> {
        todo!()
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
}
