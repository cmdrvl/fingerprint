use crate::document::Document;
use crate::dsl::assertions::evaluate_named_assertions;
use crate::dsl::content_hash::content_hash;
use crate::dsl::extract::extract;
use crate::dsl::parser::FingerprintDefinition;
use crate::registry::core::{Fingerprint, FingerprintInfo, FingerprintResult};
use std::collections::HashMap;
use std::path::PathBuf;

/// A fingerprint backed by a parsed DSL definition, evaluated at runtime.
struct DslFingerprint {
    def: FingerprintDefinition,
}

impl Fingerprint for DslFingerprint {
    fn id(&self) -> &str {
        &self.def.fingerprint_id
    }

    fn format(&self) -> &str {
        &self.def.format
    }

    fn parent(&self) -> Option<&str> {
        self.def.parent.as_deref()
    }

    fn fingerprint(&self, doc: &Document) -> FingerprintResult {
        let assertion_results = evaluate_named_assertions(&self.def.assertions, doc);

        let all_passed = assertion_results.iter().all(|r| r.passed);
        let first_failure_reason = assertion_results
            .iter()
            .find(|r| !r.passed)
            .and_then(|r| r.detail.clone());

        let extracted: Option<HashMap<String, serde_json::Value>> =
            if all_passed && !self.def.extract.is_empty() {
                extract(doc, &self.def.extract).ok()
            } else {
                None
            };

        let content_hash_value = if all_passed {
            if let Some(ref config) = self.def.content_hash {
                extracted
                    .as_ref()
                    .map(|ext| content_hash(ext, &config.over))
            } else {
                None
            }
        } else {
            None
        };

        FingerprintResult {
            matched: all_passed,
            reason: first_failure_reason,
            assertions: assertion_results,
            extracted,
            content_hash: content_hash_value,
        }
    }
}

/// Default directory for installed fingerprint definitions.
fn definitions_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("FINGERPRINT_DEFINITIONS") {
        return PathBuf::from(dir);
    }
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".fingerprint")
        .join("definitions")
}

/// Discover installed fingerprint definitions from `~/.fingerprint/definitions/`.
///
/// Scans the definitions directory for `.fp.yaml` files, parses each one,
/// and returns fingerprint implementations with metadata.
///
/// Override the scan directory with the `FINGERPRINT_DEFINITIONS` environment variable.
pub fn discover_installed() -> Vec<(Box<dyn Fingerprint>, FingerprintInfo)> {
    discover_from_dir(&definitions_dir())
}

/// Scan a directory for `.fp.yaml` fingerprint definitions.
fn discover_from_dir(dir: &std::path::Path) -> Vec<(Box<dyn Fingerprint>, FingerprintInfo)> {
    if !dir.is_dir() {
        return Vec::new();
    }

    let mut discovered = Vec::new();

    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let is_yaml = path
            .extension()
            .is_some_and(|ext| ext == "yaml" || ext == "yml");
        let has_fp_stem = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.contains(".fp."));

        if !is_yaml || !has_fp_stem {
            continue;
        }

        let def = match crate::dsl::parser::parse(&path) {
            Ok(def) => def,
            Err(error) => {
                eprintln!(
                    "Warning: skipping invalid definition '{}': {}",
                    path.display(),
                    error
                );
                continue;
            }
        };

        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_owned();

        let info = FingerprintInfo {
            id: def.fingerprint_id.clone(),
            crate_name: format!("dsl-runtime:{}", filename),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            source: format!("installed:{}", def.fingerprint_id),
            format: def.format.clone(),
            parent: def.parent.clone(),
        };

        discovered.push((
            Box::new(DslFingerprint { def }) as Box<dyn Fingerprint>,
            info,
        ));
    }

    discovered
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn discover_returns_empty_when_directory_missing() {
        let result = discover_from_dir(std::path::Path::new(
            "/tmp/fingerprint-test-nonexistent-dir",
        ));
        assert!(result.is_empty());
    }

    #[test]
    fn discover_loads_fp_yaml_definitions() {
        let tmp = TempDir::new().expect("create temp dir");
        let yaml = r#"
fingerprint_id: test-discover.v1
format: csv
assertions:
  - filename_regex:
      pattern: "(?i)\\.csv$"
"#;
        fs::write(tmp.path().join("test-discover.fp.yaml"), yaml).expect("write test definition");

        // Also write a non-fp file that should be ignored
        fs::write(tmp.path().join("notes.yaml"), "not a fingerprint").expect("write decoy file");

        let result = discover_from_dir(tmp.path());

        assert_eq!(result.len(), 1);
        let (fp, info) = &result[0];
        assert_eq!(fp.id(), "test-discover.v1");
        assert_eq!(fp.format(), "csv");
        assert_eq!(info.id, "test-discover.v1");
        assert_eq!(info.source, "installed:test-discover.v1");
    }

    #[test]
    fn dsl_fingerprint_evaluates_assertions_at_runtime() {
        let def = FingerprintDefinition {
            fingerprint_id: "csv-test.v1".to_owned(),
            format: "csv".to_owned(),
            valid_from: None,
            valid_until: None,
            parent: None,
            assertions: vec![crate::dsl::assertions::NamedAssertion {
                name: Some("always_filename".to_owned()),
                assertion: crate::dsl::assertions::Assertion::FilenameRegex {
                    pattern: ".*".to_owned(),
                },
            }],
            extract: vec![],
            content_hash: None,
        };

        let fp = DslFingerprint { def };

        // Create a minimal CSV document
        let tmp = tempfile::NamedTempFile::with_suffix(".csv").expect("create csv");
        std::fs::write(tmp.path(), "a,b\n1,2\n").expect("write csv");
        let doc = Document::Csv(crate::document::CsvDocument {
            path: tmp.path().to_owned(),
        });

        let result = fp.fingerprint(&doc);
        assert!(result.matched);
        assert_eq!(result.assertions.len(), 1);
        assert!(result.assertions[0].passed);
    }
}
