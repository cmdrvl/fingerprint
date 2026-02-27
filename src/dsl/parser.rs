use crate::dsl::assertions::NamedAssertion;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Parsed `.fp.yaml` fingerprint definition.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct FingerprintDefinition {
    pub fingerprint_id: String,
    pub format: String,
    pub valid_from: Option<String>,
    pub valid_until: Option<String>,
    pub parent: Option<String>,
    pub assertions: Vec<NamedAssertion>,
    #[serde(default)]
    pub extract: Vec<ExtractSection>,
    pub content_hash: Option<ContentHashConfig>,
}

/// A named content extraction section.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ExtractSection {
    pub name: String,
    #[serde(rename = "type")]
    pub r#type: String,
    pub anchor_heading: Option<String>,
    pub index: Option<usize>,
    pub anchor: Option<String>,
    pub pattern: Option<String>,
    pub within_chars: Option<u32>,
    pub sheet: Option<String>,
    pub range: Option<String>,
}

/// Content hash configuration.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ContentHashConfig {
    pub algorithm: String,
    pub over: Vec<String>,
}

/// Parse a `.fp.yaml` file into a fingerprint definition.
pub fn parse(path: &Path) -> Result<FingerprintDefinition, String> {
    let yaml = fs::read_to_string(path)
        .map_err(|error| format!("Failed reading '{}': {error}", path.display()))?;
    let mut definition: FingerprintDefinition = serde_yaml::from_str(&yaml)
        .map_err(|error| format!("Failed parsing '{}': {error}", path.display()))?;
    auto_name_assertions(&mut definition.assertions);
    Ok(definition)
}

fn auto_name_assertions(assertions: &mut [NamedAssertion]) {
    let mut seen: HashMap<String, usize> = HashMap::new();

    for assertion in assertions {
        if assertion.name.is_none() {
            let base = assertion_base_name(&assertion.assertion);
            let counter = seen.entry(base.clone()).or_insert(0);
            let generated = if *counter == 0 {
                base
            } else {
                format!("{base}__{}", *counter)
            };
            *counter += 1;
            assertion.name = Some(generated);
        } else if let Some(existing) = &assertion.name {
            let counter = seen.entry(existing.clone()).or_insert(0);
            *counter += 1;
        }
    }
}

fn assertion_base_name(assertion: &crate::dsl::assertions::Assertion) -> String {
    use crate::dsl::assertions::Assertion;

    match assertion {
        Assertion::HeadingRegex { pattern } => {
            format!("heading_regex__{}", regex_excerpt(pattern, 20))
        }
        Assertion::TableExists { heading, index } => format!(
            "table_exists__{}__{}",
            regex_excerpt(heading, 20),
            index.unwrap_or(0)
        ),
        Assertion::CellEq { sheet, cell, .. } => format!(
            "cell_eq__{}__{}",
            literal_excerpt(sheet, 20, false),
            literal_excerpt(cell, 20, false)
        ),
        Assertion::TextNear { anchor, .. } => {
            format!("text_near__{}", regex_excerpt(anchor, 20))
        }
        Assertion::TextRegex { pattern } => format!("text_regex__{}", regex_excerpt(pattern, 20)),
        Assertion::TextContains(text) => {
            format!("text_contains__{}", literal_excerpt(text, 20, true))
        }
        Assertion::HeadingExists(text) => {
            format!("heading_exists__{}", literal_excerpt(text, 20, true))
        }
        Assertion::HeadingLevel { level, pattern } => {
            format!("heading_level__h{}__{}", level, regex_excerpt(pattern, 20))
        }
        Assertion::SectionNonEmpty { heading } => {
            format!("section_non_empty__{}", regex_excerpt(heading, 20))
        }
        Assertion::SectionMinLines { heading, .. } => {
            format!("section_min_lines__{}", regex_excerpt(heading, 20))
        }
        Assertion::TableColumns { heading, index, .. } => format!(
            "table_columns__{}__{}",
            regex_excerpt(heading, 20),
            index.unwrap_or(0)
        ),
        Assertion::TableShape { heading, index, .. } => format!(
            "table_shape__{}__{}",
            regex_excerpt(heading, 20),
            index.unwrap_or(0)
        ),
        Assertion::TableMinRows { heading, index, .. } => format!(
            "table_min_rows__{}__{}",
            regex_excerpt(heading, 20),
            index.unwrap_or(0)
        ),
        Assertion::SheetExists(sheet) => {
            format!("sheet_exists__{}", literal_excerpt(sheet, 20, false))
        }
        Assertion::SheetNameRegex { pattern, .. } => {
            format!("sheet_name_regex__{}", regex_excerpt(pattern, 20))
        }
        Assertion::CellRegex { sheet, cell, .. } => format!(
            "cell_regex__{}__{}",
            literal_excerpt(sheet, 20, false),
            literal_excerpt(cell, 20, false)
        ),
        Assertion::RangeNonNull { sheet, range } => format!(
            "range_non_null__{}__{}",
            literal_excerpt(sheet, 20, false),
            literal_excerpt(range, 20, false)
        ),
        Assertion::RangePopulated { sheet, range, .. } => format!(
            "range_populated__{}__{}",
            literal_excerpt(sheet, 20, false),
            literal_excerpt(range, 20, false)
        ),
        Assertion::SheetMinRows { sheet, .. } => {
            format!("sheet_min_rows__{}", literal_excerpt(sheet, 20, false))
        }
        Assertion::ColumnSearch { sheet, column, .. } => format!(
            "column_search__{}__{}",
            literal_excerpt(sheet, 20, false),
            literal_excerpt(column, 20, false)
        ),
        Assertion::HeaderRowMatch { sheet, .. } => {
            format!("header_row_match__{}", literal_excerpt(sheet, 20, false))
        }
        Assertion::SumEq {
            range, equals_cell, ..
        } => format!(
            "sum_eq__{}__{}",
            literal_excerpt(range, 20, false),
            literal_excerpt(equals_cell, 20, false)
        ),
        Assertion::WithinTolerance { cell, .. } => {
            format!("within_tolerance__{}", literal_excerpt(cell, 20, false))
        }
        Assertion::PageCount { .. } => "page_count".to_owned(),
        Assertion::MetadataRegex { key, .. } => {
            format!("metadata_regex__{}", literal_excerpt(key, 20, false))
        }
        Assertion::FilenameRegex { pattern } => {
            format!("filename_regex__{}", regex_excerpt(pattern, 20))
        }
    }
}

fn regex_excerpt(value: &str, max_len: usize) -> String {
    // Drop common inline-regex mode prefixes like (?i), (?m), etc.
    let mut without_flags = value.to_owned();
    for prefix in ["(?i)", "(?m)", "(?s)", "(?x)"] {
        without_flags = without_flags.replace(prefix, "");
    }
    // Replace character classes with separators for readability.
    let mut normalized = String::new();
    let mut in_class = false;
    for character in without_flags.chars() {
        match character {
            '[' => {
                in_class = true;
                normalized.push('_');
            }
            ']' => {
                in_class = false;
            }
            '\\' if !in_class => {}
            _ => normalized.push(character),
        }
    }
    literal_excerpt(&normalized, max_len, true)
}

fn literal_excerpt(value: &str, max_len: usize, lowercase: bool) -> String {
    let mut output = String::new();
    let mut previous_was_underscore = false;

    for character in value.chars() {
        let character = if lowercase {
            character.to_ascii_lowercase()
        } else {
            character
        };

        if character.is_ascii_alphanumeric() {
            output.push(character);
            previous_was_underscore = false;
        } else if !previous_was_underscore {
            output.push('_');
            previous_was_underscore = true;
        }
    }

    let trimmed = output.trim_matches('_');
    if trimmed.is_empty() {
        return "value".to_owned();
    }

    let truncated: String = trimmed.chars().take(max_len).collect();
    truncated.trim_matches('_').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::assertions::Assertion;
    use tempfile::NamedTempFile;

    const SAMPLE_FP_YAML: &str = r#"
fingerprint_id: cbre-appraisal.v1/rent-roll.v1
format: pdf
valid_from: "2021-01-01"
valid_until: "2025-12-31"
parent: cbre-appraisal.v1
assertions:
  - name: assumptions_title
    cell_eq:
      sheet: "Assumptions"
      cell: "A3"
      value: "Market Leasing Assumptions"
  - heading_regex:
      pattern: "(?i)rent roll"
  - name: cap_rate_present
    text_near:
      anchor: "(?i)capitali[sz]ation rate"
      pattern: "\\d+\\.\\d+%"
      within_chars: 200
  - table_shape:
      heading: "(?i)rent roll"
      index: 0
      min_columns: 4
      column_types: [string, string, number, number]
extract:
  - name: rent_roll_range
    type: range
    sheet: "Assumptions"
    range: "A3:D10"
  - name: rent_roll_table
    type: table
    anchor_heading: "(?i)rent roll"
    index: 0
  - name: as_of_date
    type: text_match
    anchor: "(?i)as of"
    pattern: "\\w+ \\d{1,2},? \\d{4}"
    within_chars: 100
content_hash:
  algorithm: blake3
  over: [rent_roll_range, rent_roll_table]
"#;

    #[test]
    fn parse_file_supports_spreadsheet_and_content_assertions() {
        let file = NamedTempFile::new().expect("temp file should be created");
        fs::write(file.path(), SAMPLE_FP_YAML).expect("sample yaml should be written");

        let parsed = parse(file.path()).expect("sample yaml should parse");

        assert_eq!(parsed.fingerprint_id, "cbre-appraisal.v1/rent-roll.v1");
        assert_eq!(parsed.format, "pdf");
        assert_eq!(parsed.valid_from.as_deref(), Some("2021-01-01"));
        assert_eq!(parsed.valid_until.as_deref(), Some("2025-12-31"));
        assert_eq!(parsed.parent.as_deref(), Some("cbre-appraisal.v1"));
        assert_eq!(parsed.assertions.len(), 4);

        assert_eq!(
            parsed.assertions[0].name.as_deref(),
            Some("assumptions_title")
        );
        assert_eq!(
            parsed.assertions[1].name.as_deref(),
            Some("heading_regex__rent_roll")
        );
        match &parsed.assertions[0].assertion {
            Assertion::CellEq { sheet, cell, value } => {
                assert_eq!(sheet, "Assumptions");
                assert_eq!(cell, "A3");
                assert_eq!(value, "Market Leasing Assumptions");
            }
            other => panic!("expected Assertion::CellEq, got {other:?}"),
        }
        match &parsed.assertions[1].assertion {
            Assertion::HeadingRegex { pattern } => {
                assert_eq!(pattern, "(?i)rent roll");
            }
            other => panic!("expected Assertion::HeadingRegex, got {other:?}"),
        }
        match &parsed.assertions[2].assertion {
            Assertion::TextNear {
                anchor,
                pattern,
                within_chars,
            } => {
                assert_eq!(anchor, "(?i)capitali[sz]ation rate");
                assert_eq!(pattern, "\\d+\\.\\d+%");
                assert_eq!(*within_chars, 200);
            }
            other => panic!("expected Assertion::TextNear, got {other:?}"),
        }
        match &parsed.assertions[3].assertion {
            Assertion::TableShape {
                heading,
                index,
                min_columns,
                column_types,
            } => {
                assert_eq!(heading, "(?i)rent roll");
                assert_eq!(*index, Some(0));
                assert_eq!(*min_columns, 4);
                assert_eq!(
                    column_types,
                    &["string", "string", "number", "number"]
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                );
            }
            other => panic!("expected Assertion::TableShape, got {other:?}"),
        }

        assert_eq!(parsed.extract.len(), 3);
        assert_eq!(parsed.extract[0].name, "rent_roll_range");
        assert_eq!(parsed.extract[0].r#type, "range");
        assert_eq!(parsed.extract[0].sheet.as_deref(), Some("Assumptions"));
        assert_eq!(parsed.extract[0].range.as_deref(), Some("A3:D10"));
        assert_eq!(parsed.extract[1].r#type, "table");
        assert_eq!(
            parsed.extract[1].anchor_heading.as_deref(),
            Some("(?i)rent roll")
        );
        assert_eq!(parsed.extract[1].index, Some(0));
        assert_eq!(parsed.extract[2].r#type, "text_match");
        assert_eq!(parsed.extract[2].anchor.as_deref(), Some("(?i)as of"));
        assert_eq!(
            parsed.extract[2].pattern.as_deref(),
            Some("\\w+ \\d{1,2},? \\d{4}")
        );
        assert_eq!(parsed.extract[2].within_chars, Some(100));

        let content_hash = parsed
            .content_hash
            .expect("content hash should be present in sample");
        assert_eq!(content_hash.algorithm, "blake3");
        assert_eq!(
            content_hash.over,
            vec!["rent_roll_range", "rent_roll_table"]
        );
    }

    #[test]
    fn parse_yaml_round_trips_with_named_assertions() {
        let parsed: FingerprintDefinition =
            serde_yaml::from_str(SAMPLE_FP_YAML).expect("sample yaml should deserialize");
        let rendered =
            serde_yaml::to_string(&parsed).expect("parsed struct should serialize to yaml");
        let reparsed: FingerprintDefinition =
            serde_yaml::from_str(&rendered).expect("serialized yaml should deserialize");

        assert_eq!(parsed, reparsed);
    }

    #[test]
    fn parse_auto_generates_names_for_omitted_assertions() {
        let yaml = r#"
fingerprint_id: test.v1
format: pdf
assertions:
  - heading_regex:
      pattern: "(?i)income capitali[sz]ation approach"
  - table_exists:
      heading: "(?i)rent roll"
      index: 0
  - cell_eq:
      sheet: "Assumptions"
      cell: "A3"
      value: "Market Leasing Assumptions"
  - text_near:
      anchor: "(?i)capitali[sz]ation rate"
      pattern: "\\d+\\.\\d+%"
      within_chars: 200
"#;
        let mut file = NamedTempFile::new().expect("create temp file");
        std::io::Write::write_all(&mut file, yaml.as_bytes()).expect("write yaml");
        std::io::Write::flush(&mut file).expect("flush yaml");

        let parsed = parse(file.path()).expect("parse yaml");
        let names: Vec<&str> = parsed
            .assertions
            .iter()
            .map(|assertion| assertion.name.as_deref().expect("name generated"))
            .collect();

        assert_eq!(names[0], "heading_regex__income_capitali_szat");
        assert_eq!(names[1], "table_exists__rent_roll__0");
        assert_eq!(names[2], "cell_eq__Assumptions__A3");
        assert_eq!(names[3], "text_near__capitali_szation_rat");
    }

    #[test]
    fn parse_auto_generated_names_are_deterministic_and_deduplicated() {
        let yaml = r#"
fingerprint_id: test.v2
format: markdown
assertions:
  - heading_regex:
      pattern: "(?i)property description"
  - heading_regex:
      pattern: "(?i)property description"
"#;
        let mut file = NamedTempFile::new().expect("create temp file");
        std::io::Write::write_all(&mut file, yaml.as_bytes()).expect("write yaml");
        std::io::Write::flush(&mut file).expect("flush yaml");

        let parsed = parse(file.path()).expect("parse yaml");
        assert_eq!(
            parsed.assertions[0].name.as_deref(),
            Some("heading_regex__property_description")
        );
        assert_eq!(
            parsed.assertions[1].name.as_deref(),
            Some("heading_regex__property_description__1")
        );
    }

    #[test]
    fn parse_preserves_explicit_assertion_name() {
        let yaml = r#"
fingerprint_id: test.v3
format: text
assertions:
  - name: explicit_name
    text_contains: "hello"
"#;
        let mut file = NamedTempFile::new().expect("create temp file");
        std::io::Write::write_all(&mut file, yaml.as_bytes()).expect("write yaml");
        std::io::Write::flush(&mut file).expect("flush yaml");

        let parsed = parse(file.path()).expect("parse yaml");
        assert_eq!(parsed.assertions[0].name.as_deref(), Some("explicit_name"));
    }

    #[test]
    fn parse_supports_sheet_binding_and_row_scanning_assertions() {
        let parsed = parse(std::path::Path::new(
            "tests/fixtures/cmbs-watl/cmbs-watl-desired.fp.yaml",
        ))
        .expect("parse cmbs-watl desired fixture");

        assert_eq!(parsed.fingerprint_id, "cmbs-watl.v2");
        assert_eq!(parsed.assertions.len(), 4);

        match &parsed.assertions[0].assertion {
            Assertion::SheetNameRegex { pattern, bind } => {
                assert_eq!(pattern, "(?i)watch\\s?list|WATL");
                assert_eq!(bind.as_deref(), Some("$watl_sheet"));
            }
            other => panic!("expected Assertion::SheetNameRegex, got {other:?}"),
        }

        match &parsed.assertions[1].assertion {
            Assertion::ColumnSearch {
                sheet,
                column,
                row_range,
                pattern,
            } => {
                assert_eq!(sheet, "$watl_sheet");
                assert_eq!(column, "A");
                assert_eq!(row_range, "1:20");
                assert!(pattern.contains("CREFC Investor Reporting"));
            }
            other => panic!("expected Assertion::ColumnSearch, got {other:?}"),
        }

        match &parsed.assertions[2].assertion {
            Assertion::HeaderRowMatch {
                sheet,
                row_range,
                min_match,
                columns,
            } => {
                assert_eq!(sheet, "$watl_sheet");
                assert_eq!(row_range, "1:30");
                assert_eq!(*min_match, 5);
                assert_eq!(columns.len(), 7);
            }
            other => panic!("expected Assertion::HeaderRowMatch, got {other:?}"),
        }
    }
}
