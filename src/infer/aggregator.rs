use crate::dsl::assertions::{Assertion, NamedAssertion};
use crate::dsl::parser::{ContentHashConfig, ExtractSection};
use crate::infer::frankensearch::HybridSearcher;
use crate::infer::observer::Observation;
use std::collections::{BTreeMap, BTreeSet};

/// One inferred assertion with support statistics.
#[derive(Debug, Clone, PartialEq)]
pub struct InferredAssertion {
    pub assertion: NamedAssertion,
    pub confidence: f64,
    pub support: usize,
    pub total: usize,
}

/// Aggregated profile inferred from a document corpus.
#[derive(Debug, Clone, PartialEq)]
pub struct AggregatedProfile {
    pub fingerprint_id: String,
    pub format: String,
    pub assertions: Vec<InferredAssertion>,
    pub extract: Vec<ExtractSection>,
    pub content_hash: Option<ContentHashConfig>,
}

#[derive(Debug, Clone)]
struct CandidateAssertion {
    assertion: Assertion,
    support: usize,
}

/// Aggregate observations into an inferred profile.
pub fn aggregate(
    observations: &[Observation],
    format: &str,
    fingerprint_id: &str,
    min_confidence: f64,
    include_extract: bool,
    searcher: Option<&HybridSearcher>,
) -> Result<AggregatedProfile, String> {
    if observations.is_empty() {
        return Err("infer requires at least one matching document".to_owned());
    }
    if !(0.0..=1.0).contains(&min_confidence) {
        return Err(format!(
            "--min-confidence must be within [0.0, 1.0], got {min_confidence}"
        ));
    }

    let normalized_format = format.to_ascii_lowercase();
    let total = observations.len();

    let mut candidates = match normalized_format.as_str() {
        "xlsx" => aggregate_xlsx(observations),
        "csv" => aggregate_csv(observations),
        "pdf" => aggregate_pdf(observations),
        _ => {
            return Err(format!(
                "unsupported infer format '{normalized_format}' (expected xlsx|csv|pdf)"
            ));
        }
    };
    candidates.sort_by(|left, right| {
        assertion_sort_key(&left.assertion).cmp(&assertion_sort_key(&right.assertion))
    });

    let mut assertions = Vec::new();
    for candidate in candidates {
        let support = calibrated_support(searcher, &candidate.assertion, candidate.support, total);
        let confidence = support as f64 / total as f64;
        if confidence + f64::EPSILON < min_confidence {
            continue;
        }

        assertions.push(InferredAssertion {
            assertion: NamedAssertion {
                name: None,
                assertion: candidate.assertion,
            },
            confidence,
            support,
            total,
        });
    }

    if assertions.is_empty() {
        return Err(format!(
            "no assertions met confidence threshold {min_confidence:.2}"
        ));
    }

    let (extract, content_hash) = if include_extract {
        suggested_extract(normalized_format.as_str(), observations)
    } else {
        (Vec::new(), None)
    };

    Ok(AggregatedProfile {
        fingerprint_id: fingerprint_id.to_owned(),
        format: normalized_format,
        assertions,
        extract,
        content_hash,
    })
}

fn aggregate_xlsx(observations: &[Observation]) -> Vec<CandidateAssertion> {
    let mut candidates = vec![CandidateAssertion {
        assertion: Assertion::FilenameRegex {
            pattern: "(?i).*\\.xlsx$".to_owned(),
        },
        support: observations.len(),
    }];

    let mut sheet_support: BTreeMap<String, usize> = BTreeMap::new();
    for observation in observations {
        for sheet in observation
            .sheet_names
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>()
        {
            *sheet_support.entry(sheet).or_insert(0) += 1;
        }
    }
    for (sheet, support) in sheet_support {
        candidates.push(CandidateAssertion {
            assertion: Assertion::SheetExists(sheet),
            support,
        });
    }

    let mut row_counts: BTreeMap<String, Vec<u64>> = BTreeMap::new();
    for observation in observations {
        for (sheet, count) in &observation.row_counts {
            row_counts.entry(sheet.clone()).or_default().push(*count);
        }
    }
    for (sheet, counts) in row_counts {
        if counts.is_empty() {
            continue;
        }
        let support = counts.len();
        let min_rows = counts.into_iter().min().unwrap_or(0);
        candidates.push(CandidateAssertion {
            assertion: Assertion::SheetMinRows { sheet, min_rows },
            support,
        });
    }

    let mut cell_support: BTreeMap<(String, String, String), usize> = BTreeMap::new();
    for observation in observations {
        for (key, value) in &observation.cell_values {
            let Some((sheet, cell)) = key.split_once('!') else {
                continue;
            };
            *cell_support
                .entry((sheet.to_owned(), cell.to_owned(), value.clone()))
                .or_insert(0) += 1;
        }
    }
    for ((sheet, cell, value), support) in cell_support {
        candidates.push(CandidateAssertion {
            assertion: Assertion::CellEq { sheet, cell, value },
            support,
        });
    }

    candidates
}

fn aggregate_csv(observations: &[Observation]) -> Vec<CandidateAssertion> {
    let mut candidates = vec![
        CandidateAssertion {
            assertion: Assertion::FilenameRegex {
                pattern: "(?i).*\\.csv$".to_owned(),
            },
            support: observations.len(),
        },
        CandidateAssertion {
            assertion: Assertion::SheetExists("Sheet1".to_owned()),
            support: observations.len(),
        },
    ];

    let row_counts = observations
        .iter()
        .filter_map(|observation| observation.csv_row_count)
        .collect::<Vec<_>>();
    if !row_counts.is_empty() {
        candidates.push(CandidateAssertion {
            assertion: Assertion::SheetMinRows {
                sheet: "Sheet1".to_owned(),
                min_rows: row_counts.into_iter().min().unwrap_or(0),
            },
            support: observations.len(),
        });
    }

    let mut header_support: BTreeMap<(usize, String), usize> = BTreeMap::new();
    for observation in observations {
        for (index, header) in observation.csv_headers.iter().enumerate() {
            if header.trim().is_empty() {
                continue;
            }
            *header_support.entry((index, header.clone())).or_insert(0) += 1;
        }
    }
    for ((index, header), support) in header_support {
        candidates.push(CandidateAssertion {
            assertion: Assertion::CellEq {
                sheet: "Sheet1".to_owned(),
                cell: to_cell_ref(0, index),
                value: header,
            },
            support,
        });
    }

    candidates
}

fn aggregate_pdf(observations: &[Observation]) -> Vec<CandidateAssertion> {
    let mut candidates = vec![CandidateAssertion {
        assertion: Assertion::FilenameRegex {
            pattern: "(?i).*\\.pdf$".to_owned(),
        },
        support: observations.len(),
    }];

    let page_counts = observations
        .iter()
        .filter_map(|observation| observation.pdf_page_count)
        .collect::<Vec<_>>();
    if !page_counts.is_empty() {
        candidates.push(CandidateAssertion {
            assertion: Assertion::PageCount {
                min: page_counts.iter().copied().min(),
                max: page_counts.iter().copied().max(),
            },
            support: page_counts.len(),
        });
    }

    let mut metadata_support: BTreeMap<(String, String), usize> = BTreeMap::new();
    for observation in observations {
        for (key, value) in &observation.pdf_metadata {
            if value.trim().is_empty() {
                continue;
            }
            *metadata_support
                .entry((key.clone(), value.clone()))
                .or_insert(0) += 1;
        }
    }
    for ((key, value), support) in metadata_support {
        candidates.push(CandidateAssertion {
            assertion: Assertion::MetadataRegex {
                key,
                pattern: format!("^{}$", regex::escape(&value)),
            },
            support,
        });
    }

    candidates
}

fn suggested_extract(
    format: &str,
    observations: &[Observation],
) -> (Vec<ExtractSection>, Option<ContentHashConfig>) {
    let mut sections = Vec::new();
    if format == "xlsx" {
        let mut sheets = observations
            .iter()
            .flat_map(|observation| observation.sheet_names.iter().cloned())
            .collect::<Vec<_>>();
        sheets.sort_unstable();
        sheets.dedup();

        if let Some(sheet) = sheets.into_iter().next() {
            sections.push(ExtractSection {
                name: "primary_range".to_owned(),
                r#type: "range".to_owned(),
                anchor_heading: None,
                index: None,
                anchor: None,
                pattern: None,
                within_chars: None,
                sheet: Some(sheet),
                range: Some("A1:D20".to_owned()),
            });
        }
    } else if format == "csv" {
        sections.push(ExtractSection {
            name: "primary_rows".to_owned(),
            r#type: "range".to_owned(),
            anchor_heading: None,
            index: None,
            anchor: None,
            pattern: None,
            within_chars: None,
            sheet: Some("Sheet1".to_owned()),
            range: Some("A1:D20".to_owned()),
        });
    }

    let content_hash = if sections.is_empty() {
        None
    } else {
        Some(ContentHashConfig {
            algorithm: "blake3".to_owned(),
            over: sections
                .iter()
                .map(|section| section.name.clone())
                .collect(),
        })
    };

    (sections, content_hash)
}

fn assertion_sort_key(assertion: &Assertion) -> String {
    match assertion {
        Assertion::FilenameRegex { pattern } => format!("00_filename_regex:{pattern}"),
        Assertion::SheetExists(sheet) => format!("10_sheet_exists:{sheet}"),
        Assertion::SheetMinRows { sheet, min_rows } => {
            format!("11_sheet_min_rows:{sheet}:{min_rows}")
        }
        Assertion::CellEq { sheet, cell, value } => format!("12_cell_eq:{sheet}:{cell}:{value}"),
        Assertion::PageCount { min, max } => format!("20_page_count:{min:?}:{max:?}"),
        Assertion::MetadataRegex { key, pattern } => format!("21_metadata_regex:{key}:{pattern}"),
        other => format!("99_other:{other:?}"),
    }
}

fn calibrated_support(
    searcher: Option<&HybridSearcher>,
    assertion: &Assertion,
    baseline_support: usize,
    total: usize,
) -> usize {
    let Some(searcher) = searcher else {
        return baseline_support;
    };

    let Some(query) = assertion_support_query(assertion) else {
        return baseline_support;
    };
    if query.trim().len() < 2 {
        return baseline_support;
    }

    let Ok(search_support) = searcher.support_for_query_default(&query) else {
        return baseline_support;
    };

    baseline_support.max(search_support).min(total)
}

fn assertion_support_query(assertion: &Assertion) -> Option<String> {
    match assertion {
        Assertion::SheetExists(sheet) => Some(sheet.clone()),
        Assertion::SheetMinRows { sheet, .. } => Some(sheet.clone()),
        Assertion::CellEq { value, .. } => Some(value.clone()),
        Assertion::MetadataRegex { pattern, .. } => Some(regex_literal(pattern)),
        _ => None,
    }
}

fn regex_literal(pattern: &str) -> String {
    let unanchored = pattern
        .trim_start_matches('^')
        .trim_end_matches('$')
        .to_owned();
    unanchored.replace("\\", "")
}

fn to_cell_ref(row: usize, col: usize) -> String {
    let mut remainder = col + 1;
    let mut letters = String::new();
    while remainder > 0 {
        let modulo = (remainder - 1) % 26;
        letters.push((b'A' + modulo as u8) as char);
        remainder = (remainder - 1) / 26;
    }

    let column = letters.chars().rev().collect::<String>();
    format!("{column}{}", row + 1)
}

#[cfg(test)]
mod tests {
    use super::aggregate;
    use crate::dsl::assertions::Assertion;
    use crate::infer::observer::Observation;
    use std::collections::HashMap;

    fn xlsx_observation(
        sheets: &[&str],
        sheet_rows: &[(&str, u64)],
        cell_values: &[(&str, &str)],
    ) -> Observation {
        Observation {
            format: "xlsx".to_owned(),
            extension: "xlsx".to_owned(),
            filename: "fixture.xlsx".to_owned(),
            sheet_names: sheets.iter().map(ToString::to_string).collect(),
            row_counts: sheet_rows
                .iter()
                .map(|(sheet, rows)| ((*sheet).to_owned(), *rows))
                .collect(),
            cell_values: cell_values
                .iter()
                .map(|(cell, value)| ((*cell).to_owned(), (*value).to_owned()))
                .collect(),
            csv_headers: Vec::new(),
            csv_row_count: None,
            pdf_page_count: None,
            pdf_metadata: HashMap::new(),
        }
    }

    #[test]
    fn min_confidence_filters_low_support_assertions() {
        let observations = vec![
            xlsx_observation(&["Sheet1"], &[("Sheet1", 10)], &[("Sheet1!A1", "Header")]),
            xlsx_observation(&["Sheet1"], &[("Sheet1", 8)], &[]),
        ];

        let profile =
            aggregate(&observations, "xlsx", "test.v1", 1.0, false, None).expect("aggregate");
        assert!(profile.assertions.iter().all(|entry| {
            !matches!(
                entry.assertion.assertion,
                Assertion::CellEq {
                    ref sheet,
                    ref cell,
                    ..
                } if sheet == "Sheet1" && cell == "A1"
            )
        }));
    }

    #[test]
    fn aggregate_is_deterministic_for_input_order() {
        let first = xlsx_observation(&["A"], &[("A", 5)], &[("A!A1", "alpha")]);
        let second = xlsx_observation(&["A"], &[("A", 7)], &[("A!A1", "alpha")]);

        let profile_a = aggregate(
            &[first.clone(), second.clone()],
            "xlsx",
            "test.v1",
            0.5,
            true,
            None,
        )
        .expect("a");
        let profile_b = aggregate(&[second, first], "xlsx", "test.v1", 0.5, true, None).expect("b");

        assert_eq!(profile_a, profile_b);
    }

    #[test]
    fn no_extract_omits_extract_and_content_hash() {
        let observations = vec![xlsx_observation(
            &["Sheet1"],
            &[("Sheet1", 4)],
            &[("Sheet1!A1", "Header")],
        )];
        let profile =
            aggregate(&observations, "xlsx", "test.v1", 0.0, false, None).expect("aggregate");

        assert!(profile.extract.is_empty());
        assert!(profile.content_hash.is_none());
    }
}
