use crate::dsl::assertions::Assertion;
use serde::Deserialize;
use std::path::Path;

/// Parsed `.fp.yaml` fingerprint definition.
#[derive(Debug, Clone, Deserialize)]
pub struct FingerprintDefinition {
    pub fingerprint_id: String,
    pub format: String,
    pub assertions: Vec<Assertion>,
    #[serde(default)]
    pub extract: Vec<ExtractSection>,
    pub content_hash: Option<ContentHashConfig>,
}

/// A named content extraction section.
#[derive(Debug, Clone, Deserialize)]
pub struct ExtractSection {
    pub name: String,
    pub sheet: Option<String>,
    pub range: Option<String>,
}

/// Content hash configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct ContentHashConfig {
    pub algorithm: String,
    pub over: Vec<String>,
}

/// Parse a `.fp.yaml` file into a fingerprint definition.
pub fn parse(_path: &Path) -> Result<FingerprintDefinition, String> {
    todo!()
}
