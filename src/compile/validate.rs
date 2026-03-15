use crate::dsl::assertions::{Assertion, NamedAssertion};
use crate::dsl::parser::{ContentHashConfig, ExtractSection, FingerprintDefinition};
use std::collections::BTreeSet;

const SUPPORTED_FORMATS: &[&str] = &["xlsx", "csv", "pdf", "markdown", "text", "html"];
const SUPPORTED_EXTRACT_TYPES: &[&str] = &["range", "table", "section", "text_match"];

pub fn validate_definition(definition: &FingerprintDefinition) -> Result<(), String> {
    validate_format(&definition.format)?;

    for assertion in &definition.assertions {
        validate_assertion(&definition.format, assertion)?;
    }

    validate_extract_sections(&definition.extract)?;
    validate_content_hash(&definition.extract, definition.content_hash.as_ref())?;

    Ok(())
}

fn validate_format(format: &str) -> Result<(), String> {
    if SUPPORTED_FORMATS.contains(&format) {
        Ok(())
    } else {
        Err(format!(
            "unsupported format '{format}'; supported formats are {}",
            SUPPORTED_FORMATS.join(", ")
        ))
    }
}

fn validate_assertion(format: &str, assertion: &NamedAssertion) -> Result<(), String> {
    match &assertion.assertion {
        Assertion::HeaderTokenSearch {
            page,
            tokens,
            min_matches,
            max_matches,
            ..
        } => {
            require_html_format("header_token_search", format)?;
            if matches!(page, Some(0)) {
                return Err("header_token_search.page must be >= 1".to_owned());
            }
            if tokens.is_empty() {
                return Err("header_token_search.tokens must contain at least one token".to_owned());
            }
            if tokens.iter().any(|token| token.trim().is_empty()) {
                return Err(
                    "header_token_search.tokens must not contain empty token patterns".to_owned(),
                );
            }
            let token_count = tokens.len() as u64;
            if *min_matches > token_count {
                return Err(format!(
                    "header_token_search.min_matches ({min_matches}) exceeds token count ({token_count})"
                ));
            }
            if let Some(max_matches) = max_matches {
                if *max_matches < *min_matches {
                    return Err(format!(
                        "header_token_search.max_matches ({max_matches}) must be >= min_matches ({min_matches})"
                    ));
                }
                if *max_matches > token_count {
                    return Err(format!(
                        "header_token_search.max_matches ({max_matches}) exceeds token count ({token_count})"
                    ));
                }
            }
        }
        Assertion::DominantColumnCount {
            count,
            sample_pages,
            ..
        } => {
            require_html_format("dominant_column_count", format)?;
            if *count == 0 {
                return Err("dominant_column_count.count must be >= 1".to_owned());
            }
            if *sample_pages == 0 {
                return Err("dominant_column_count.sample_pages must be >= 1".to_owned());
            }
        }
        Assertion::FullWidthRow { pattern, min_cells } => {
            require_html_format("full_width_row", format)?;
            if pattern.trim().is_empty() {
                return Err("full_width_row.pattern must not be empty".to_owned());
            }
            if *min_cells == 0 {
                return Err("full_width_row.min_cells must be >= 1".to_owned());
            }
        }
        Assertion::PageSectionCount { min, max } => {
            require_html_format("page_section_count", format)?;
            validate_bounds("page_section_count", *min, *max)?;
        }
        Assertion::PageCount { min, max } => {
            validate_bounds("page_count", *min, *max)?;
        }
        _ => {}
    }

    Ok(())
}

fn require_html_format(assertion_name: &str, format: &str) -> Result<(), String> {
    if format == "html" {
        Ok(())
    } else {
        Err(format!(
            "assertion '{assertion_name}' requires format 'html', found '{format}'"
        ))
    }
}

fn validate_bounds(name: &str, min: Option<u64>, max: Option<u64>) -> Result<(), String> {
    if min.is_none() && max.is_none() {
        return Err(format!("{name} requires at least one of 'min' or 'max'"));
    }

    if let (Some(min), Some(max)) = (min, max)
        && min > max
    {
        return Err(format!("{name}.min ({min}) must be <= {name}.max ({max})"));
    }

    Ok(())
}

fn validate_extract_sections(extract: &[ExtractSection]) -> Result<(), String> {
    for section in extract {
        if section.name.trim().is_empty() {
            return Err("extract.name must not be empty".to_owned());
        }

        match section.r#type.as_str() {
            "range" => {
                require_extract_field(section, section.sheet.as_ref(), "sheet")?;
                require_extract_field(section, section.range.as_ref(), "range")?;
            }
            "table" | "section" => {
                require_extract_field(section, section.anchor_heading.as_ref(), "anchor_heading")?;
            }
            "text_match" => {
                require_extract_field(section, section.anchor.as_ref(), "anchor")?;
                require_extract_field(section, section.pattern.as_ref(), "pattern")?;
                if section.within_chars.is_none() {
                    return Err(format!(
                        "extract '{}' of type '{}' requires field 'within_chars'",
                        section.name, section.r#type
                    ));
                }
            }
            other => {
                return Err(format!(
                    "unsupported extract type '{other}'; supported extract types are {}",
                    SUPPORTED_EXTRACT_TYPES.join(", ")
                ));
            }
        }
    }

    Ok(())
}

fn require_extract_field(
    section: &ExtractSection,
    value: Option<&String>,
    field: &str,
) -> Result<(), String> {
    if value.is_some() {
        Ok(())
    } else {
        Err(format!(
            "extract '{}' of type '{}' requires field '{field}'",
            section.name, section.r#type
        ))
    }
}

fn validate_content_hash(
    extract: &[ExtractSection],
    content_hash: Option<&ContentHashConfig>,
) -> Result<(), String> {
    let Some(content_hash) = content_hash else {
        return Ok(());
    };

    if content_hash.algorithm != "blake3" {
        return Err(format!(
            "unsupported content_hash.algorithm '{}'; supported algorithms are blake3",
            content_hash.algorithm
        ));
    }

    if content_hash.over.is_empty() {
        return Err("content_hash.over must contain at least one extract name".to_owned());
    }

    let extract_names: BTreeSet<&str> = extract
        .iter()
        .map(|section| section.name.as_str())
        .collect();
    for extract_name in &content_hash.over {
        if !extract_names.contains(extract_name.as_str()) {
            return Err(format!(
                "content_hash.over references unknown extract '{extract_name}'"
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_html_definition() -> FingerprintDefinition {
        FingerprintDefinition {
            fingerprint_id: "compile-html.v1".to_owned(),
            format: "html".to_owned(),
            valid_from: None,
            valid_until: None,
            parent: None,
            assertions: vec![
                NamedAssertion {
                    name: Some("header_tokens".to_owned()),
                    assertion: Assertion::HeaderTokenSearch {
                        page: Some(1),
                        index: Some(0),
                        tokens: vec!["(?i)portfolio company".to_owned(), "(?i)coupon".to_owned()],
                        min_matches: 1,
                        max_matches: Some(2),
                    },
                },
                NamedAssertion {
                    name: Some("dominant_columns".to_owned()),
                    assertion: Assertion::DominantColumnCount {
                        count: 6,
                        tolerance: 1,
                        sample_pages: 3,
                    },
                },
                NamedAssertion {
                    name: Some("full_width_row".to_owned()),
                    assertion: Assertion::FullWidthRow {
                        pattern: "(?i)^software$".to_owned(),
                        min_cells: 6,
                    },
                },
                NamedAssertion {
                    name: Some("page_sections".to_owned()),
                    assertion: Assertion::PageSectionCount {
                        min: Some(2),
                        max: Some(5),
                    },
                },
            ],
            extract: vec![ExtractSection {
                name: "schedule_table".to_owned(),
                r#type: "table".to_owned(),
                anchor_heading: Some("(?i)schedule of investments".to_owned()),
                index: Some(0),
                anchor: None,
                pattern: None,
                within_chars: None,
                sheet: None,
                range: None,
            }],
            content_hash: Some(ContentHashConfig {
                algorithm: "blake3".to_owned(),
                over: vec!["schedule_table".to_owned()],
            }),
        }
    }

    #[test]
    fn validate_definition_accepts_supported_html_rules() {
        validate_definition(&base_html_definition()).expect("html definition should validate");
    }

    #[test]
    fn validate_definition_rejects_unsupported_format() {
        let mut definition = base_html_definition();
        definition.format = "xml".to_owned();

        let error = validate_definition(&definition).expect_err("unsupported format should fail");
        assert!(error.contains("unsupported format 'xml'"));
    }

    #[test]
    fn validate_definition_rejects_html_assertions_on_non_html_format() {
        let mut definition = base_html_definition();
        definition.format = "markdown".to_owned();

        let error = validate_definition(&definition).expect_err("html-only assertions should fail");
        assert!(error.contains("requires format 'html'"));
    }

    #[test]
    fn validate_definition_rejects_invalid_html_assertion_parameters() {
        let mut definition = base_html_definition();
        definition.assertions[0].assertion = Assertion::HeaderTokenSearch {
            page: Some(0),
            index: None,
            tokens: vec!["".to_owned()],
            min_matches: 2,
            max_matches: Some(1),
        };

        let error = validate_definition(&definition).expect_err("invalid params should fail");
        assert!(error.contains("page must be >= 1"));
    }

    #[test]
    fn validate_definition_rejects_extract_and_content_hash_mismatches() {
        let mut definition = base_html_definition();
        definition.extract[0].anchor_heading = None;
        definition.content_hash = Some(ContentHashConfig {
            algorithm: "sha256".to_owned(),
            over: vec!["missing".to_owned()],
        });

        let error = validate_definition(&definition).expect_err("invalid extract should fail");
        assert!(error.contains("requires field 'anchor_heading'"));
    }

    #[test]
    fn validate_definition_rejects_empty_page_section_count_bounds() {
        let mut definition = base_html_definition();
        definition.assertions[3].assertion = Assertion::PageSectionCount {
            min: None,
            max: None,
        };

        let error = validate_definition(&definition).expect_err("missing bounds should fail");
        assert!(error.contains("requires at least one of 'min' or 'max'"));
    }
}
