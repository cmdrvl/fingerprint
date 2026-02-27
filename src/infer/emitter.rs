use crate::infer::aggregator::{AggregatedProfile, InferredAssertion};
use std::io::Write;

/// Emit a `.fp.yaml` definition from an aggregated profile.
pub fn emit_yaml(profile: &AggregatedProfile, out: &mut dyn Write) -> Result<(), String> {
    writeln!(
        out,
        "fingerprint_id: {}",
        yaml_quote(&profile.fingerprint_id)
    )
    .map_err(|error| format!("failed writing fingerprint_id: {error}"))?;
    writeln!(out, "format: {}", yaml_quote(&profile.format))
        .map_err(|error| format!("failed writing format: {error}"))?;
    writeln!(out, "assertions:").map_err(|error| format!("failed writing assertions: {error}"))?;

    for assertion in &profile.assertions {
        emit_assertion(assertion, out)?;
    }

    if !profile.extract.is_empty() {
        writeln!(out, "extract:").map_err(|error| format!("failed writing extract: {error}"))?;
        for section in &profile.extract {
            let serialized = normalize_yaml(&serde_yaml::to_string(section).map_err(|error| {
                format!(
                    "failed serializing extract section '{}': {error}",
                    section.name
                )
            })?);
            emit_list_item(&serialized, out, 0)?;
        }
    }

    if let Some(content_hash) = &profile.content_hash {
        writeln!(out, "content_hash:")
            .map_err(|error| format!("failed writing content_hash: {error}"))?;
        writeln!(out, "  algorithm: {}", yaml_quote(&content_hash.algorithm))
            .map_err(|error| format!("failed writing content_hash algorithm: {error}"))?;
        writeln!(out, "  over:")
            .map_err(|error| format!("failed writing content_hash over: {error}"))?;
        for section_name in &content_hash.over {
            writeln!(out, "    - {}", yaml_quote(section_name))
                .map_err(|error| format!("failed writing content_hash section: {error}"))?;
        }
    }

    Ok(())
}

fn emit_assertion(assertion: &InferredAssertion, out: &mut dyn Write) -> Result<(), String> {
    writeln!(
        out,
        "  # confidence: {:.3} ({}/{})",
        assertion.confidence, assertion.support, assertion.total
    )
    .map_err(|error| format!("failed writing confidence annotation: {error}"))?;

    let assertion_value = serde_json::to_value(&assertion.assertion.assertion)
        .map_err(|error| format!("failed serializing assertion: {error}"))?;
    let serialized = normalize_yaml(
        &serde_yaml::to_string(&assertion_value)
            .map_err(|error| format!("failed serializing assertion yaml: {error}"))?,
    );
    emit_list_item(&serialized, out, 2)
}

fn emit_list_item(body: &str, out: &mut dyn Write, base_indent: usize) -> Result<(), String> {
    let indent = " ".repeat(base_indent);
    let nested = " ".repeat(base_indent + 2);

    for (index, line) in body.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        if index == 0 {
            writeln!(out, "{indent}- {line}")
                .map_err(|error| format!("failed writing list item: {error}"))?;
        } else {
            writeln!(out, "{nested}{line}")
                .map_err(|error| format!("failed writing list item body: {error}"))?;
        }
    }

    Ok(())
}

fn normalize_yaml(serialized: &str) -> String {
    serialized
        .strip_prefix("---\n")
        .unwrap_or(serialized)
        .trim_end()
        .to_owned()
}

fn yaml_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_owned();
    }
    if value
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '-' || character == '.')
    {
        return value.to_owned();
    }
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::emit_yaml;
    use crate::dsl::FingerprintDefinition;
    use crate::dsl::assertions::{Assertion, NamedAssertion};
    use crate::infer::aggregator::{AggregatedProfile, InferredAssertion};

    fn profile() -> AggregatedProfile {
        AggregatedProfile {
            fingerprint_id: "test-inferred.v1".to_owned(),
            format: "xlsx".to_owned(),
            assertions: vec![
                InferredAssertion {
                    assertion: NamedAssertion {
                        name: None,
                        assertion: Assertion::SheetExists("Sheet1".to_owned()),
                    },
                    confidence: 1.0,
                    support: 3,
                    total: 3,
                },
                InferredAssertion {
                    assertion: NamedAssertion {
                        name: None,
                        assertion: Assertion::SheetMinRows {
                            sheet: "Sheet1".to_owned(),
                            min_rows: 2,
                        },
                    },
                    confidence: 1.0,
                    support: 3,
                    total: 3,
                },
            ],
            extract: Vec::new(),
            content_hash: None,
        }
    }

    #[test]
    fn emits_parseable_yaml_with_confidence_comments() {
        let mut output = Vec::new();
        emit_yaml(&profile(), &mut output).expect("emit yaml");
        let rendered = String::from_utf8(output).expect("utf8");

        assert!(rendered.contains("# confidence: 1.000 (3/3)"));
        let parsed: FingerprintDefinition =
            serde_yaml::from_str(&rendered).expect("parse emitted yaml");
        assert_eq!(parsed.fingerprint_id, "test-inferred.v1");
        assert_eq!(parsed.format, "xlsx");
        assert_eq!(parsed.assertions.len(), 2);
    }

    #[test]
    fn output_is_deterministic() {
        let mut first = Vec::new();
        let mut second = Vec::new();
        emit_yaml(&profile(), &mut first).expect("emit first");
        emit_yaml(&profile(), &mut second).expect("emit second");
        assert_eq!(first, second);
    }
}
