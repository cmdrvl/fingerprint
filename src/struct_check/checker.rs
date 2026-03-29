use std::collections::{BTreeMap, BTreeSet};
use std::io::BufRead;

use globset::{Glob, GlobMatcher};
use serde::Serialize;
use serde_json::Value;

use super::rules::Rule;

/// A single struct-check output record.
#[derive(Debug, Clone, Serialize)]
pub struct CheckRecord {
    pub version: String,
    pub rule_id: String,
    pub group_pattern: String,
    pub matched_directory: String,
    pub outcome: String,
    pub present: Vec<String>,
    pub missing: Vec<String>,
    pub unexpected: Vec<String>,
    pub tool_versions: ToolVersions,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolVersions {
    pub fingerprint: String,
}

/// Error types for struct-check input reading.
#[derive(Debug)]
pub enum InputError {
    /// I/O failure reading the input stream.
    ReadFailure(String),
    /// A record has invalid JSON.
    InvalidJson { line: u64, error: String },
    /// A record is not a JSON object.
    RecordNotObject { line: u64 },
    /// The version field is missing.
    MissingVersion { line: u64 },
    /// The version field is not "vacuum.v0".
    BadVersion { line: u64, version: String },
    /// The relative_path field is missing.
    MissingRelativePath { line: u64 },
    /// The relative_path field is not a string.
    InvalidRelativePath { line: u64 },
}

impl std::fmt::Display for InputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReadFailure(error) => write!(f, "failed reading input: {error}"),
            Self::InvalidJson { line, error } => write!(f, "line {line}: invalid JSON: {error}"),
            Self::RecordNotObject { line } => {
                write!(f, "line {line}: record must be a JSON object")
            }
            Self::MissingVersion { line } => {
                write!(f, "line {line}: missing required field 'version'")
            }
            Self::BadVersion { line, version } => {
                write!(
                    f,
                    "line {line}: expected version \"vacuum.v0\", got \"{version}\""
                )
            }
            Self::MissingRelativePath { line } => {
                write!(f, "line {line}: missing required field 'relative_path'")
            }
            Self::InvalidRelativePath { line } => {
                write!(f, "line {line}: field 'relative_path' must be a string")
            }
        }
    }
}

/// Read vacuum.v0 JSONL records and extract (directory, filename) pairs.
///
/// Returns a map from directory path to the set of filenames in that directory.
pub fn read_vacuum_records(
    input: &mut dyn BufRead,
) -> Result<BTreeMap<String, BTreeSet<String>>, InputError> {
    let mut groups: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut line_number: u64 = 0;
    let mut line = String::new();

    loop {
        line.clear();
        let bytes_read = input
            .read_line(&mut line)
            .map_err(|error| InputError::ReadFailure(error.to_string()))?;
        if bytes_read == 0 {
            break;
        }
        line_number += 1;

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let value: Value =
            serde_json::from_str(trimmed).map_err(|error| InputError::InvalidJson {
                line: line_number,
                error: error.to_string(),
            })?;

        let object = value
            .as_object()
            .ok_or(InputError::RecordNotObject { line: line_number })?;

        // Validate version is "vacuum.v0"
        match object.get("version") {
            Some(Value::String(version)) => {
                if version != "vacuum.v0" {
                    return Err(InputError::BadVersion {
                        line: line_number,
                        version: version.clone(),
                    });
                }
            }
            Some(_) | None => {
                return Err(InputError::MissingVersion { line: line_number });
            }
        }

        // Extract relative_path
        let relative_path = match object.get("relative_path") {
            Some(Value::String(path)) => path.as_str(),
            Some(_) => {
                return Err(InputError::InvalidRelativePath { line: line_number });
            }
            None => {
                return Err(InputError::MissingRelativePath { line: line_number });
            }
        };

        // Split into directory and filename
        let (dir, filename) = match relative_path.rsplit_once('/') {
            Some((d, f)) => (d.to_owned(), f.to_owned()),
            None => (String::new(), relative_path.to_owned()),
        };

        groups.entry(dir).or_default().insert(filename);
    }

    Ok(groups)
}

/// Check a set of directory groups against a list of rules.
///
/// Returns sorted output records and whether all groups are complete.
pub fn check_groups(
    groups: &BTreeMap<String, BTreeSet<String>>,
    rules: &[Rule],
    version: &str,
) -> (Vec<CheckRecord>, bool) {
    let mut records = Vec::new();
    let mut all_complete = true;

    for rule in rules {
        let group_matcher = match Glob::new(&rule.group_by) {
            Ok(glob) => glob.compile_matcher(),
            Err(error) => {
                eprintln!(
                    "Warning: invalid group_by glob '{}' in rule '{}': {error}",
                    rule.group_by, rule.id
                );
                continue;
            }
        };

        // Build matchers for required patterns
        let required_matchers: Vec<(String, GlobMatcher)> = rule
            .required
            .iter()
            .filter_map(|pattern| {
                Glob::new(pattern)
                    .ok()
                    .map(|glob| (pattern.clone(), glob.compile_matcher()))
            })
            .collect();

        // Build matchers for optional patterns
        let optional_matchers: Vec<GlobMatcher> = rule
            .optional
            .iter()
            .filter_map(|pattern| Glob::new(pattern).ok().map(|glob| glob.compile_matcher()))
            .collect();

        for (dir, filenames) in groups {
            if !group_matcher.is_match(dir) {
                continue;
            }

            let mut present = Vec::new();
            let mut missing = Vec::new();

            // Check each required pattern
            for (pattern, matcher) in &required_matchers {
                let matching_files: Vec<&String> =
                    filenames.iter().filter(|f| matcher.is_match(f)).collect();
                if matching_files.is_empty() {
                    missing.push(pattern.clone());
                } else {
                    for file in matching_files {
                        if !present.contains(file) {
                            present.push(file.clone());
                        }
                    }
                }
            }

            // Find unexpected files (not matching any required or optional pattern)
            let mut unexpected = Vec::new();
            for filename in filenames {
                let matches_required = required_matchers.iter().any(|(_, m)| m.is_match(filename));
                let matches_optional = optional_matchers.iter().any(|m| m.is_match(filename));
                if !matches_required && !matches_optional {
                    unexpected.push(filename.clone());
                }
            }

            let outcome = if missing.is_empty() {
                "complete"
            } else if present.is_empty() {
                all_complete = false;
                "empty"
            } else {
                all_complete = false;
                "partial"
            };

            present.sort();
            missing.sort();
            unexpected.sort();

            records.push(CheckRecord {
                version: "struct-check.v0".to_owned(),
                rule_id: rule.id.clone(),
                group_pattern: rule.group_by.clone(),
                matched_directory: dir.clone(),
                outcome: outcome.to_owned(),
                present,
                missing,
                unexpected,
                tool_versions: ToolVersions {
                    fingerprint: version.to_owned(),
                },
            });
        }
    }

    // Sort by (rule_id, matched_directory) for determinism
    records.sort_by(|a, b| {
        a.rule_id
            .cmp(&b.rule_id)
            .then_with(|| a.matched_directory.cmp(&b.matched_directory))
    });

    (records, all_complete)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn reads_vacuum_records_and_groups_by_directory() {
        let input = r#"{"version":"vacuum.v0","relative_path":"org/packages/P001/report.pdf"}
{"version":"vacuum.v0","relative_path":"org/packages/P001/jan_summary.xlsx"}
{"version":"vacuum.v0","relative_path":"org/packages/P002/report.pdf"}
"#;
        let mut cursor = Cursor::new(input.as_bytes());
        let groups = read_vacuum_records(&mut cursor).expect("read records");

        assert_eq!(groups.len(), 2);
        assert_eq!(groups["org/packages/P001"].len(), 2);
        assert!(groups["org/packages/P001"].contains("report.pdf"));
        assert!(groups["org/packages/P001"].contains("jan_summary.xlsx"));
        assert_eq!(groups["org/packages/P002"].len(), 1);
    }

    #[test]
    fn rejects_non_vacuum_version() {
        let input = r#"{"version":"hash.v0","relative_path":"a.pdf"}"#;
        let mut cursor = Cursor::new(input.as_bytes());
        let error = read_vacuum_records(&mut cursor).expect_err("should reject");
        assert!(error.to_string().contains("vacuum.v0"));
    }

    #[test]
    fn rejects_missing_relative_path() {
        let input = r#"{"version":"vacuum.v0","path":"a.pdf"}"#;
        let mut cursor = Cursor::new(input.as_bytes());
        let error = read_vacuum_records(&mut cursor).expect_err("should reject");
        assert!(error.to_string().contains("relative_path"));
    }

    #[test]
    fn check_groups_complete_outcome() {
        let mut groups = BTreeMap::new();
        let mut files = BTreeSet::new();
        files.insert("report.pdf".to_owned());
        files.insert("jan_summary.xlsx".to_owned());
        groups.insert("org/packages/P001".to_owned(), files);

        let rules = vec![Rule {
            id: "pkg.v1".to_owned(),
            group_by: "*/packages/P*".to_owned(),
            required: vec!["*.pdf".to_owned(), "*_summary.xlsx".to_owned()],
            optional: vec![],
        }];

        let (records, all_complete) = check_groups(&groups, &rules, "0.5.1");
        assert!(all_complete);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].outcome, "complete");
        assert!(records[0].missing.is_empty());
    }

    #[test]
    fn check_groups_partial_outcome() {
        let mut groups = BTreeMap::new();
        let mut files = BTreeSet::new();
        files.insert("report.pdf".to_owned());
        groups.insert("org/packages/P001".to_owned(), files);

        let rules = vec![Rule {
            id: "pkg.v1".to_owned(),
            group_by: "*/packages/P*".to_owned(),
            required: vec!["*.pdf".to_owned(), "*_summary.xlsx".to_owned()],
            optional: vec![],
        }];

        let (records, all_complete) = check_groups(&groups, &rules, "0.5.1");
        assert!(!all_complete);
        assert_eq!(records[0].outcome, "partial");
        assert_eq!(records[0].missing, vec!["*_summary.xlsx"]);
    }

    #[test]
    fn check_groups_empty_outcome() {
        let mut groups = BTreeMap::new();
        let mut files = BTreeSet::new();
        files.insert("draft.docx".to_owned());
        groups.insert("org/packages/P001".to_owned(), files);

        let rules = vec![Rule {
            id: "pkg.v1".to_owned(),
            group_by: "*/packages/P*".to_owned(),
            required: vec!["*.pdf".to_owned(), "*_summary.xlsx".to_owned()],
            optional: vec![],
        }];

        let (records, all_complete) = check_groups(&groups, &rules, "0.5.1");
        assert!(!all_complete);
        assert_eq!(records[0].outcome, "empty");
        assert_eq!(records[0].unexpected, vec!["draft.docx"]);
    }

    #[test]
    fn check_groups_detects_unexpected_files() {
        let mut groups = BTreeMap::new();
        let mut files = BTreeSet::new();
        files.insert("report.pdf".to_owned());
        files.insert("jan_summary.xlsx".to_owned());
        files.insert("draft.docx".to_owned());
        groups.insert("org/packages/P001".to_owned(), files);

        let rules = vec![Rule {
            id: "pkg.v1".to_owned(),
            group_by: "*/packages/P*".to_owned(),
            required: vec!["*.pdf".to_owned(), "*_summary.xlsx".to_owned()],
            optional: vec!["*_notes.txt".to_owned()],
        }];

        let (records, all_complete) = check_groups(&groups, &rules, "0.5.1");
        assert!(all_complete);
        assert_eq!(records[0].unexpected, vec!["draft.docx"]);
    }
}
