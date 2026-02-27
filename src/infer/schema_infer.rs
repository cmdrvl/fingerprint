use crate::document::{Document, open_document_from_path};
use crate::dsl::assertions::{Assertion, NamedAssertion};
use crate::dsl::{ContentHashConfig, ExtractSection, FingerprintDefinition};
use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct SchemaField {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SchemaInferResult {
    pub definition: FingerprintDefinition,
    pub total_fields: usize,
    pub located_fields: usize,
}

#[derive(Debug, Clone, PartialEq)]
struct LocatedField {
    assertion: NamedAssertion,
    extract: Option<ExtractSection>,
}

pub fn infer_schema(
    doc_path: &Path,
    fields_path: &Path,
    fingerprint_id: &str,
) -> Result<SchemaInferResult, String> {
    let fields = parse_schema_fields_file(fields_path)?;
    if fields.is_empty() {
        return Err("schema field list is empty".to_owned());
    }

    let doc = open_document_from_path(doc_path)
        .map_err(|error| format!("failed opening document '{}': {error}", doc_path.display()))?;
    let located = locate_fields(&doc, &fields);

    if located.is_empty() {
        return Err("no schema fields could be located in the document".to_owned());
    }

    let format = match &doc {
        Document::Xlsx(_) => "xlsx",
        Document::Csv(_) => "csv",
        Document::Pdf(_) => "pdf",
        Document::Markdown(_) => "markdown",
        Document::Text(_) => "text",
        Document::Unknown(_) => "raw",
    }
    .to_owned();

    let assertions = located
        .iter()
        .map(|located| located.assertion.clone())
        .collect::<Vec<_>>();
    let extract = located
        .iter()
        .filter_map(|located| located.extract.clone())
        .collect::<Vec<_>>();
    let content_hash = if extract.is_empty() {
        None
    } else {
        Some(ContentHashConfig {
            algorithm: "blake3".to_owned(),
            over: extract.iter().map(|section| section.name.clone()).collect(),
        })
    };

    Ok(SchemaInferResult {
        definition: FingerprintDefinition {
            fingerprint_id: fingerprint_id.to_owned(),
            format,
            valid_from: None,
            valid_until: None,
            parent: None,
            assertions,
            extract,
            content_hash,
        },
        total_fields: fields.len(),
        located_fields: located.len(),
    })
}

pub fn emit_yaml(definition: &FingerprintDefinition) -> Result<String, String> {
    serde_yaml::to_string(definition)
        .map_err(|error| format!("failed serializing infer-schema output: {error}"))
}

pub fn parse_schema_fields_file(path: &Path) -> Result<Vec<SchemaField>, String> {
    let yaml = fs::read_to_string(path)
        .map_err(|error| format!("failed reading schema file '{}': {error}", path.display()))?;
    parse_schema_fields(&yaml)
}

pub fn parse_schema_fields(yaml: &str) -> Result<Vec<SchemaField>, String> {
    let fields: Vec<SchemaField> =
        serde_yaml::from_str(yaml).map_err(|error| format!("invalid fields yaml: {error}"))?;
    if fields.is_empty() {
        return Ok(fields);
    }
    if let Some(invalid) = fields
        .iter()
        .find(|field| field.name.trim().is_empty() || field.value.trim().is_empty())
    {
        return Err(format!(
            "schema field name/value must be non-empty (field '{}')",
            invalid.name
        ));
    }
    Ok(fields)
}

fn locate_fields(doc: &Document, fields: &[SchemaField]) -> Vec<LocatedField> {
    fields
        .iter()
        .filter_map(|field| locate_field(doc, field))
        .collect()
}

fn locate_field(doc: &Document, field: &SchemaField) -> Option<LocatedField> {
    match doc {
        Document::Markdown(markdown) => locate_in_markdown(markdown, field),
        Document::Text(text) => locate_in_text(text, field),
        Document::Csv(csv) => locate_in_csv(csv, field),
        Document::Xlsx(xlsx) => locate_in_xlsx(xlsx, field),
        Document::Pdf(pdf) => {
            if let Some(text) = &pdf.text {
                locate_in_markdown(text, field)
            } else {
                locate_in_pdf_metadata(pdf, field)
            }
        }
        Document::Unknown(_) => None,
    }
}

fn locate_in_markdown(
    markdown: &crate::document::MarkdownDocument,
    field: &SchemaField,
) -> Option<LocatedField> {
    let content = markdown.normalized.to_ascii_lowercase();
    let needle = field.value.to_ascii_lowercase();
    let index = content.find(&needle)?;

    let line = markdown.normalized[..index]
        .chars()
        .filter(|character| *character == '\n')
        .count()
        + 1;
    let nearest_heading = markdown
        .headings
        .iter()
        .filter(|heading| heading.line <= line)
        .max_by_key(|heading| heading.line)
        .map(|heading| heading.text.clone());

    let escaped_value = regex::escape(&field.value);
    let assertion = if let Some(heading) = &nearest_heading {
        let anchor = format!("(?i){}", regex::escape(heading));
        NamedAssertion {
            name: Some(field.name.clone()),
            assertion: Assertion::TextNear {
                anchor: anchor.clone(),
                pattern: escaped_value.clone(),
                within_chars: 400,
            },
        }
    } else {
        NamedAssertion {
            name: Some(field.name.clone()),
            assertion: Assertion::TextRegex {
                pattern: escaped_value.clone(),
            },
        }
    };

    Some(LocatedField {
        assertion,
        extract: Some(ExtractSection {
            name: field.name.clone(),
            r#type: "text_match".to_owned(),
            anchor_heading: nearest_heading
                .map(|heading| format!("(?i){}", regex::escape(&heading))),
            index: None,
            anchor: None,
            pattern: Some(escaped_value),
            within_chars: Some(400),
            sheet: None,
            range: None,
        }),
    })
}

fn locate_in_text(
    text: &crate::document::TextDocument,
    field: &SchemaField,
) -> Option<LocatedField> {
    let target = field.value.to_ascii_lowercase();
    let line_index = text
        .lines
        .iter()
        .position(|line| line.to_ascii_lowercase().contains(&target))?;

    let anchor_line = (0..line_index).rev().find_map(|index| {
        let candidate = text.lines[index].trim();
        (!candidate.is_empty()).then_some(candidate.to_owned())
    });
    let escaped_value = regex::escape(&field.value);

    let assertion = if let Some(anchor) = &anchor_line {
        NamedAssertion {
            name: Some(field.name.clone()),
            assertion: Assertion::TextNear {
                anchor: regex::escape(anchor),
                pattern: escaped_value.clone(),
                within_chars: 400,
            },
        }
    } else {
        NamedAssertion {
            name: Some(field.name.clone()),
            assertion: Assertion::TextRegex {
                pattern: escaped_value.clone(),
            },
        }
    };

    Some(LocatedField {
        assertion,
        extract: Some(ExtractSection {
            name: field.name.clone(),
            r#type: "text_match".to_owned(),
            anchor_heading: None,
            index: None,
            anchor: anchor_line.map(|anchor| regex::escape(&anchor)),
            pattern: Some(escaped_value),
            within_chars: Some(400),
            sheet: None,
            range: None,
        }),
    })
}

fn locate_in_csv(csv: &crate::document::CsvDocument, field: &SchemaField) -> Option<LocatedField> {
    let rows = csv.rows().ok()?;
    for (row, values) in rows.iter().enumerate() {
        for (col, value) in values.iter().enumerate() {
            if value.trim() == field.value.trim() {
                let cell = to_cell_ref(row, col);
                return Some(LocatedField {
                    assertion: NamedAssertion {
                        name: Some(field.name.clone()),
                        assertion: Assertion::CellEq {
                            sheet: "Sheet1".to_owned(),
                            cell: cell.clone(),
                            value: field.value.clone(),
                        },
                    },
                    extract: Some(ExtractSection {
                        name: field.name.clone(),
                        r#type: "range".to_owned(),
                        anchor_heading: None,
                        index: None,
                        anchor: None,
                        pattern: None,
                        within_chars: None,
                        sheet: Some("Sheet1".to_owned()),
                        range: Some(format!("{cell}:{cell}")),
                    }),
                });
            }
        }
    }
    None
}

fn locate_in_xlsx(
    xlsx: &crate::document::XlsxDocument,
    field: &SchemaField,
) -> Option<LocatedField> {
    let sheets = xlsx.sheet_names().ok()?;
    for sheet in sheets {
        for row in 0..128 {
            for col in 0..32 {
                let cell = to_cell_ref(row, col);
                let value = xlsx.read_cell(&sheet, &cell).ok().flatten();
                if value.as_deref().map(str::trim) == Some(field.value.trim()) {
                    return Some(LocatedField {
                        assertion: NamedAssertion {
                            name: Some(field.name.clone()),
                            assertion: Assertion::CellEq {
                                sheet: sheet.clone(),
                                cell: cell.clone(),
                                value: field.value.clone(),
                            },
                        },
                        extract: Some(ExtractSection {
                            name: field.name.clone(),
                            r#type: "range".to_owned(),
                            anchor_heading: None,
                            index: None,
                            anchor: None,
                            pattern: None,
                            within_chars: None,
                            sheet: Some(sheet.clone()),
                            range: Some(format!("{cell}:{cell}")),
                        }),
                    });
                }
            }
        }
    }
    None
}

fn locate_in_pdf_metadata(
    pdf: &crate::document::PdfDocument,
    field: &SchemaField,
) -> Option<LocatedField> {
    let metadata = pdf.metadata().ok()?;
    let (key, value) = metadata
        .into_iter()
        .find(|(_, value)| value.trim() == field.value.trim())?;
    Some(LocatedField {
        assertion: NamedAssertion {
            name: Some(field.name.clone()),
            assertion: Assertion::MetadataRegex {
                key: key.clone(),
                pattern: format!("^{}$", regex::escape(&value)),
            },
        },
        extract: None,
    })
}

fn to_cell_ref(row: usize, col: usize) -> String {
    let mut remaining = col + 1;
    let mut letters = String::new();
    while remaining > 0 {
        let modulo = (remaining - 1) % 26;
        letters.push((b'A' + modulo as u8) as char);
        remaining = (remaining - 1) / 26;
    }
    let column_label: String = letters.chars().rev().collect();
    format!("{column_label}{}", row + 1)
}

#[cfg(test)]
mod tests {
    use super::{emit_yaml, infer_schema, parse_schema_fields};
    use crate::dsl::FingerprintDefinition;
    use std::fs;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn temp_file(contents: &str, suffix: &str) -> NamedTempFile {
        let mut file = NamedTempFile::with_suffix(suffix).expect("create temp file");
        file.write_all(contents.as_bytes())
            .expect("write temp file");
        file.flush().expect("flush temp file");
        file
    }

    #[test]
    fn parses_schema_fields_yaml() {
        let yaml = r#"
- name: as_of_date
  value: "June 15, 2024"
- name: cap_rate
  value: "6.25%"
"#;
        let fields = parse_schema_fields(yaml).expect("parse fields yaml");
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name, "as_of_date");
        assert_eq!(fields[1].value, "6.25%");
    }

    #[test]
    fn infers_schema_from_markdown_fields() {
        let markdown = temp_file(
            "# Summary\n\nAs of date: June 15, 2024\nCap rate: 6.25%\n",
            ".md",
        );
        let fields = temp_file(
            r#"
- name: as_of_date
  value: "June 15, 2024"
- name: cap_rate
  value: "6.25%"
"#,
            ".yaml",
        );

        let inferred =
            infer_schema(markdown.path(), fields.path(), "schema-test.v1").expect("infer schema");
        assert_eq!(inferred.total_fields, 2);
        assert_eq!(inferred.located_fields, 2);
        assert_eq!(inferred.definition.assertions.len(), 2);

        let yaml = emit_yaml(&inferred.definition).expect("emit yaml");
        let parsed: FingerprintDefinition = serde_yaml::from_str(&yaml).expect("parse yaml");
        assert_eq!(parsed.fingerprint_id, "schema-test.v1");
        assert_eq!(parsed.format, "markdown");
    }

    #[test]
    fn partial_location_tracks_missing_fields() {
        let markdown = temp_file("# Summary\n\nAs of date: June 15, 2024\n", ".md");
        let fields = temp_file(
            r#"
- name: as_of_date
  value: "June 15, 2024"
- name: missing_field
  value: "DOES NOT EXIST"
"#,
            ".yaml",
        );
        let inferred =
            infer_schema(markdown.path(), fields.path(), "schema-test.v1").expect("infer schema");
        assert_eq!(inferred.total_fields, 2);
        assert_eq!(inferred.located_fields, 1);
    }

    #[test]
    fn parses_field_file_from_disk() {
        let file = temp_file("- name: sample\n  value: value\n", ".yaml");
        let loaded = fs::read_to_string(file.path()).expect("read file");
        let parsed = parse_schema_fields(&loaded).expect("parse");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "sample");
    }
}
