use crate::document::Document;
use crate::dsl::assertions::{Assertion, NamedAssertion};
use crate::dsl::parser::{ContentHashConfig, ExtractSection, FingerprintDefinition};
use crate::infer::frankensearch::{HybridSearcher, SearchDocument};
use regex::escape;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// One requested schema field for infer-schema mode.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct SchemaField {
    pub name: String,
    pub value: String,
}

/// Result from schema-driven infer.
#[derive(Debug, Clone, PartialEq)]
pub struct SchemaInferResult {
    pub definition: FingerprintDefinition,
    pub located_fields: usize,
    pub missing_fields: Vec<String>,
}

/// Parse schema fields YAML from disk.
pub fn parse_fields_file(path: &Path) -> Result<Vec<SchemaField>, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|error| format!("failed reading fields YAML '{}': {error}", path.display()))?;
    parse_fields_str(&raw).map_err(|error| format!("failed parsing fields YAML: {error}"))
}

/// Parse schema fields YAML content.
pub fn parse_fields_str(raw: &str) -> Result<Vec<SchemaField>, String> {
    let fields: Vec<SchemaField> =
        serde_yaml::from_str(raw).map_err(|error| format!("invalid fields YAML: {error}"))?;

    if fields.is_empty() {
        return Err("fields YAML must include at least one field".to_owned());
    }

    for field in &fields {
        if field.name.trim().is_empty() {
            return Err("field name cannot be empty".to_owned());
        }
        if field.value.trim().is_empty() {
            return Err(format!("field '{}' value cannot be empty", field.name));
        }
    }

    Ok(fields)
}

/// Infer a draft fingerprint definition from one document and schema fields.
pub fn infer_schema(
    document: &Document,
    fields: &[SchemaField],
    fingerprint_id: &str,
) -> Result<SchemaInferResult, String> {
    if fields.is_empty() {
        return Err("infer-schema requires at least one field".to_owned());
    }

    let context = build_text_context(document)?;
    let search_documents = context_to_search_documents(&context);
    let searcher = if search_documents.is_empty() {
        None
    } else {
        HybridSearcher::new(&search_documents).ok()
    };
    let mut assertions = Vec::new();
    let mut extract = Vec::new();
    let mut missing_fields = Vec::new();

    for field in fields {
        let located_line = searcher
            .as_ref()
            .and_then(|searcher| locate_line_with_hybrid(searcher, &field.value, &context.text))
            .or_else(|| {
                find_case_insensitive(&context.text, &field.value)
                    .map(|index| line_number_for_index(&context.text, index))
            });

        let Some(line) = located_line else {
            missing_fields.push(field.name.clone());
            continue;
        };

        let nearest_heading = nearest_heading_for_line(&context.headings, line).cloned();

        let field_name = sanitize_name(&field.name);
        let field_pattern = escape(&field.value);

        let assertion = if let Some(heading) = &nearest_heading {
            Assertion::TextNear {
                anchor: format!("(?i){}", escape(heading)),
                pattern: field_pattern.clone(),
                within_chars: 400,
            }
        } else {
            Assertion::TextContains(field.value.clone())
        };

        assertions.push(NamedAssertion {
            name: Some(format!("field_{field_name}")),
            assertion,
        });

        extract.push(ExtractSection {
            name: field_name,
            r#type: "text_match".to_owned(),
            anchor_heading: None,
            index: None,
            anchor: nearest_heading.map(|heading| format!("(?i){}", escape(&heading))),
            pattern: Some(field_pattern),
            within_chars: Some(400),
            sheet: None,
            range: None,
        });
    }

    if assertions.is_empty() {
        return Err("no requested fields were located in the document".to_owned());
    }

    let over = extract.iter().map(|section| section.name.clone()).collect();
    let definition = FingerprintDefinition {
        fingerprint_id: fingerprint_id.to_owned(),
        format: context.format,
        valid_from: None,
        valid_until: None,
        parent: None,
        assertions,
        extract,
        content_hash: Some(ContentHashConfig {
            algorithm: "blake3".to_owned(),
            over,
        }),
    };

    Ok(SchemaInferResult {
        definition,
        located_fields: fields.len() - missing_fields.len(),
        missing_fields,
    })
}

#[derive(Debug, Clone)]
struct TextContext {
    format: String,
    text: String,
    headings: Vec<(String, usize)>,
}

fn build_text_context(document: &Document) -> Result<TextContext, String> {
    match document {
        Document::Markdown(markdown) => Ok(TextContext {
            format: "markdown".to_owned(),
            text: markdown.normalized.clone(),
            headings: markdown
                .headings
                .iter()
                .map(|heading| (heading.text.clone(), heading.line))
                .collect(),
        }),
        Document::Text(text) => Ok(TextContext {
            format: "text".to_owned(),
            text: text.content().to_owned(),
            headings: Vec::new(),
        }),
        Document::Pdf(pdf) => {
            let Some(markdown) = &pdf.text else {
                return Err(
                    "infer-schema for PDF requires pre-extracted markdown text (text_path)"
                        .to_owned(),
                );
            };
            Ok(TextContext {
                format: "pdf".to_owned(),
                text: markdown.normalized.clone(),
                headings: markdown
                    .headings
                    .iter()
                    .map(|heading| (heading.text.clone(), heading.line))
                    .collect(),
            })
        }
        _ => Err("infer-schema supports markdown, text, and pdf(+text_path) only".to_owned()),
    }
}

fn sanitize_name(value: &str) -> String {
    let mut output = String::new();
    let mut previous_underscore = false;

    for character in value
        .chars()
        .map(|character| character.to_ascii_lowercase())
    {
        if character.is_ascii_alphanumeric() {
            output.push(character);
            previous_underscore = false;
        } else if !previous_underscore {
            output.push('_');
            previous_underscore = true;
        }
    }

    let trimmed = output.trim_matches('_');
    if trimmed.is_empty() {
        "field".to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn context_to_search_documents(context: &TextContext) -> Vec<SearchDocument> {
    context
        .text
        .lines()
        .enumerate()
        .filter_map(|(index, raw_line)| {
            let line = raw_line.trim();
            if line.is_empty() {
                return None;
            }

            let line_number = index + 1;
            let heading = nearest_heading_for_line(&context.headings, line_number).cloned();
            let content = if let Some(heading) = &heading {
                format!("{heading}\n{line}")
            } else {
                line.to_owned()
            };

            Some(SearchDocument {
                id: format!("line-{line_number:06}"),
                title: heading,
                content,
            })
        })
        .collect()
}

fn locate_line_with_hybrid(searcher: &HybridSearcher, query: &str, text: &str) -> Option<usize> {
    let lower_query = query.to_ascii_lowercase();
    searcher.search(query, 5).ok()?.into_iter().find_map(|hit| {
        let line_number = hit
            .doc_id
            .strip_prefix("line-")
            .and_then(|value| value.parse::<usize>().ok())?;
        let line = text.lines().nth(line_number.saturating_sub(1))?;
        line.to_ascii_lowercase()
            .contains(&lower_query)
            .then_some(line_number)
    })
}

fn nearest_heading_for_line(headings: &[(String, usize)], line: usize) -> Option<&String> {
    headings
        .iter()
        .filter(|(_, heading_line)| *heading_line <= line)
        .max_by_key(|(_, heading_line)| *heading_line)
        .map(|(heading, _)| heading)
}

fn find_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    haystack
        .to_ascii_lowercase()
        .find(&needle.to_ascii_lowercase())
}

fn line_number_for_index(content: &str, index: usize) -> usize {
    content[..index]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1
}

#[cfg(test)]
mod tests {
    use super::{SchemaField, infer_schema, parse_fields_str};
    use crate::document::{Document, MarkdownDocument};
    use std::path::Path;

    fn fixture(path: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
    }

    #[test]
    fn parse_fields_yaml_accepts_valid_list() {
        let raw = r#"
- name: cap_rate
  value: "6.25%"
- name: address
  value: "123 Example Avenue, New York, NY."
"#;
        let fields = parse_fields_str(raw).expect("parse valid fields");
        assert_eq!(
            fields,
            vec![
                SchemaField {
                    name: "cap_rate".to_owned(),
                    value: "6.25%".to_owned()
                },
                SchemaField {
                    name: "address".to_owned(),
                    value: "123 Example Avenue, New York, NY.".to_owned()
                }
            ]
        );
    }

    #[test]
    fn parse_fields_yaml_rejects_empty_values() {
        let raw = r#"
- name: cap_rate
  value: ""
"#;
        let error = parse_fields_str(raw).expect_err("empty values should fail");
        assert!(error.contains("value cannot be empty"));
    }

    #[test]
    fn infer_schema_locates_fields_in_markdown_document() {
        let path = fixture("tests/fixtures/test_files/cbre_appraisal.md");
        let markdown = MarkdownDocument::open(&path).expect("open markdown");
        let document = Document::Markdown(markdown);
        let fields = vec![
            SchemaField {
                name: "cap_rate".to_owned(),
                value: "6.25%".to_owned(),
            },
            SchemaField {
                name: "tenant".to_owned(),
                value: "Example Co".to_owned(),
            },
            SchemaField {
                name: "missing".to_owned(),
                value: "does not exist".to_owned(),
            },
        ];

        let result = infer_schema(&document, &fields, "cbre-appraisal.inferred.v1")
            .expect("infer schema from markdown");

        assert_eq!(result.located_fields, 2);
        assert_eq!(result.missing_fields, vec!["missing".to_owned()]);
        assert_eq!(
            result.definition.fingerprint_id,
            "cbre-appraisal.inferred.v1"
        );
        assert_eq!(result.definition.format, "markdown");
        assert_eq!(result.definition.assertions.len(), 2);
        assert_eq!(result.definition.extract.len(), 2);
        assert!(result.definition.content_hash.is_some());
    }
}
