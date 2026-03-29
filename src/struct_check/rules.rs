use serde::Deserialize;
use std::path::Path;

/// Top-level rules file structure (`.sf.yaml`).
#[derive(Debug, Clone, Deserialize)]
pub struct RulesFile {
    pub rules: Vec<Rule>,
}

/// A single struct-check rule.
#[derive(Debug, Clone, Deserialize)]
pub struct Rule {
    /// Unique identifier for this rule.
    pub id: String,

    /// Glob pattern matched against directory paths to select groups.
    pub group_by: String,

    /// File patterns that must be present (at least one file matching each).
    #[serde(default)]
    pub required: Vec<String>,

    /// File patterns that are allowed but not required.
    #[serde(default)]
    pub optional: Vec<String>,
}

/// Parse a `.sf.yaml` rules file from a path.
pub fn parse_rules_file(path: &Path) -> Result<RulesFile, String> {
    let contents = std::fs::read_to_string(path)
        .map_err(|error| format!("failed to read rules file '{}': {error}", path.display()))?;
    let rules_file: RulesFile = serde_yaml::from_str(&contents)
        .map_err(|error| format!("failed to parse rules file '{}': {error}", path.display()))?;
    if rules_file.rules.is_empty() {
        return Err(format!("rules file '{}' contains no rules", path.display()));
    }
    Ok(rules_file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_rules_yaml() {
        let yaml = r#"
rules:
  - id: monthly-package.v1
    group_by: "*/packages/P*"
    required:
      - "*.pdf"
      - "*_summary.xlsx"
    optional:
      - "*_notes.txt"
"#;
        let rules_file: RulesFile = serde_yaml::from_str(yaml).expect("parse rules");
        assert_eq!(rules_file.rules.len(), 1);
        assert_eq!(rules_file.rules[0].id, "monthly-package.v1");
        assert_eq!(rules_file.rules[0].group_by, "*/packages/P*");
        assert_eq!(rules_file.rules[0].required.len(), 2);
        assert_eq!(rules_file.rules[0].optional.len(), 1);
    }

    #[test]
    fn defaults_optional_and_required_to_empty() {
        let yaml = r#"
rules:
  - id: bare-rule.v1
    group_by: "stuff/*"
"#;
        let rules_file: RulesFile = serde_yaml::from_str(yaml).expect("parse rules");
        assert!(rules_file.rules[0].required.is_empty());
        assert!(rules_file.rules[0].optional.is_empty());
    }
}
