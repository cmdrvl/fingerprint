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
        "html" => aggregate_html(observations),
        _ => {
            return Err(format!(
                "unsupported infer format '{normalized_format}' (expected xlsx|csv|pdf|html)"
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
    if normalized_format == "html"
        && assertions.iter().all(|entry| {
            matches!(
                entry.assertion.assertion,
                Assertion::FilenameRegex { .. } | Assertion::HeadingExists(_)
            )
        })
    {
        return Err(
            "html corpus did not expose stable structural signals; need page sections, table headers, or full-width rows"
                .to_owned(),
        );
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

fn aggregate_html(observations: &[Observation]) -> Vec<CandidateAssertion> {
    let mut candidates = vec![CandidateAssertion {
        assertion: Assertion::FilenameRegex {
            pattern: "(?i).*\\.html?$".to_owned(),
        },
        support: observations.len(),
    }];

    if let Some((heading, support)) = common_first_heading(observations) {
        candidates.push(CandidateAssertion {
            assertion: Assertion::HeadingExists(heading),
            support,
        });
    }

    let page_section_counts = observations
        .iter()
        .filter_map(|observation| observation.html_page_section_count)
        .filter(|count| *count > 0)
        .collect::<Vec<_>>();
    if !page_section_counts.is_empty() {
        candidates.push(CandidateAssertion {
            assertion: Assertion::PageSectionCount {
                min: page_section_counts.iter().copied().min(),
                max: page_section_counts.iter().copied().max(),
            },
            support: page_section_counts.len(),
        });
    }

    let dominant_counts = observations
        .iter()
        .filter_map(dominant_column_count_for_observation)
        .collect::<Vec<_>>();
    if !dominant_counts.is_empty() {
        let count = dominant_mode_usize(&dominant_counts);
        let tolerance = dominant_counts
            .iter()
            .map(|observed| observed.abs_diff(count))
            .max()
            .unwrap_or(0);
        candidates.push(CandidateAssertion {
            assertion: Assertion::DominantColumnCount {
                count,
                tolerance,
                sample_pages: max_sample_pages(observations),
            },
            support: dominant_counts.len(),
        });
    }

    if let Some((page, index, tokens, support)) = header_token_search_candidate(observations) {
        candidates.push(CandidateAssertion {
            assertion: Assertion::HeaderTokenSearch {
                page,
                index: Some(index),
                min_matches: tokens.len() as u64,
                max_matches: None,
                tokens,
            },
            support,
        });
    }

    if let Some((pattern, min_cells, support)) = full_width_row_candidate(observations) {
        candidates.push(CandidateAssertion {
            assertion: Assertion::FullWidthRow { pattern, min_cells },
            support,
        });
    }

    candidates
}

fn common_first_heading(observations: &[Observation]) -> Option<(String, usize)> {
    let mut support = BTreeMap::new();
    for observation in observations {
        if let Some(heading) = observation.headings.first() {
            *support.entry(heading.clone()).or_insert(0usize) += 1;
        }
    }
    support
        .into_iter()
        .max_by(|left, right| left.1.cmp(&right.1).then(left.0.cmp(&right.0).reverse()))
}

fn dominant_column_count_for_observation(observation: &Observation) -> Option<usize> {
    let counts = observation
        .html_tables
        .iter()
        .map(|table| table.columns)
        .filter(|columns| *columns > 0)
        .collect::<Vec<_>>();
    (!counts.is_empty()).then(|| dominant_mode_usize(&counts))
}

fn dominant_mode_usize(values: &[usize]) -> usize {
    let mut support = BTreeMap::new();
    for value in values {
        *support.entry(*value).or_insert(0usize) += 1;
    }
    support
        .into_iter()
        .max_by(|left, right| left.1.cmp(&right.1).then(left.0.cmp(&right.0)))
        .map(|(value, _)| value)
        .unwrap_or(0)
}

fn max_sample_pages(observations: &[Observation]) -> u32 {
    observations
        .iter()
        .filter_map(|observation| {
            observation
                .html_page_section_count
                .and_then(|count| u32::try_from(count).ok())
        })
        .max()
        .unwrap_or(1)
}

const GENERIC_HTML_HEADERS: &[&str] = &[
    "amount",
    "cost",
    "fair value",
    "issuer",
    "metric",
    "portfolio company",
    "security",
];

#[derive(Debug)]
struct HeaderTokenCandidate {
    page: Option<u32>,
    index: usize,
    tokens: Vec<String>,
}

fn header_token_search_candidate(
    observations: &[Observation],
) -> Option<(Option<u32>, usize, Vec<String>, usize)> {
    let candidates = observations
        .iter()
        .filter_map(best_header_candidate)
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return None;
    }

    let mut placement_support = BTreeMap::new();
    for candidate in &candidates {
        *placement_support
            .entry((candidate.page, candidate.index))
            .or_insert(0usize) += 1;
    }
    let ((page, index), support) = placement_support
        .into_iter()
        .max_by(|left, right| left.1.cmp(&right.1).then(left.0.cmp(&right.0)))
        .unwrap_or(((None, 0usize), 0usize));
    if support == 0 {
        return None;
    }

    let matching = candidates
        .iter()
        .filter(|candidate| candidate.page == page && candidate.index == index)
        .collect::<Vec<_>>();
    let exemplar = matching.first()?;

    let mut token_support = BTreeMap::new();
    for candidate in &matching {
        for token in candidate.tokens.iter().cloned().collect::<BTreeSet<_>>() {
            *token_support.entry(token).or_insert(0usize) += 1;
        }
    }
    let max_token_support = token_support.values().copied().max().unwrap_or(0);
    let tokens = exemplar
        .tokens
        .iter()
        .filter(|token| token_support.get(*token).copied().unwrap_or(0) == max_token_support)
        .take(2)
        .map(|token| format!("(?i){}", regex::escape(token)))
        .collect::<Vec<_>>();
    if tokens.is_empty() {
        return None;
    }

    Some((page, index, tokens, matching.len()))
}

fn best_header_candidate(observation: &Observation) -> Option<HeaderTokenCandidate> {
    observation
        .html_tables
        .iter()
        .filter_map(|table| {
            let tokens = table
                .headers
                .iter()
                .filter(|header| !is_generic_html_header(header))
                .cloned()
                .collect::<Vec<_>>();
            (!tokens.is_empty()).then_some((table, tokens))
        })
        .max_by(|left, right| {
            left.1
                .len()
                .cmp(&right.1.len())
                .then(left.0.page.cmp(&right.0.page))
                .then(left.0.columns.cmp(&right.0.columns))
                .then(right.0.page_index.cmp(&left.0.page_index))
        })
        .map(|(table, tokens)| HeaderTokenCandidate {
            page: table.page,
            index: table.page_index,
            tokens,
        })
}

fn is_generic_html_header(header: &str) -> bool {
    let normalized = header.trim().to_ascii_lowercase();
    GENERIC_HTML_HEADERS
        .iter()
        .any(|value| *value == normalized)
}

fn full_width_row_candidate(observations: &[Observation]) -> Option<(String, usize, usize)> {
    let mut support = BTreeMap::new();
    let mut first_seen = Vec::new();
    let mut min_cells = 0usize;

    for observation in observations {
        let mut seen_in_observation = BTreeSet::new();
        for table in &observation.html_tables {
            min_cells = min_cells.max(table.columns);
            for row in &table.full_width_rows {
                if seen_in_observation.insert(row.clone()) {
                    *support.entry(row.clone()).or_insert(0usize) += 1;
                    if !first_seen.iter().any(|existing| existing == row) {
                        first_seen.push(row.clone());
                    }
                }
            }
        }
    }

    let max_support = support.values().copied().max().unwrap_or(0);
    if max_support == 0 || min_cells == 0 {
        return None;
    }

    let rows = first_seen
        .into_iter()
        .filter(|row| support.get(row).copied().unwrap_or(0) == max_support)
        .take(2)
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return None;
    }

    Some((
        format!(
            "(?i)^({})$",
            rows.iter()
                .map(|row| regex::escape(row))
                .collect::<Vec<_>>()
                .join("|")
        ),
        min_cells,
        max_support,
    ))
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
    } else if format == "html"
        && let Some(heading) = observations
            .iter()
            .find_map(|observation| observation.headings.first().cloned())
    {
        let anchor_heading = format!("(?i){}", regex::escape(&heading));
        sections.push(ExtractSection {
            name: "primary_table".to_owned(),
            r#type: "table".to_owned(),
            anchor_heading: Some(anchor_heading.clone()),
            index: Some(0),
            anchor: None,
            pattern: None,
            within_chars: None,
            sheet: None,
            range: None,
        });
        sections.push(ExtractSection {
            name: "primary_section".to_owned(),
            r#type: "section".to_owned(),
            anchor_heading: Some(anchor_heading),
            index: None,
            anchor: None,
            pattern: None,
            within_chars: None,
            sheet: None,
            range: None,
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
        Assertion::HeadingExists(text) => format!("13_heading_exists:{text}"),
        Assertion::PageCount { min, max } => format!("20_page_count:{min:?}:{max:?}"),
        Assertion::HeaderTokenSearch {
            page,
            index,
            tokens,
            ..
        } => format!("21_header_token_search:{page:?}:{index:?}:{tokens:?}"),
        Assertion::DominantColumnCount {
            count,
            tolerance,
            sample_pages,
        } => format!("22_dominant_column_count:{count}:{tolerance}:{sample_pages}"),
        Assertion::FullWidthRow { pattern, min_cells } => {
            format!("23_full_width_row:{pattern}:{min_cells}")
        }
        Assertion::PageSectionCount { min, max } => {
            format!("24_page_section_count:{min:?}:{max:?}")
        }
        Assertion::MetadataRegex { key, pattern } => format!("30_metadata_regex:{key}:{pattern}"),
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
        Assertion::HeadingExists(text) => Some(text.clone()),
        Assertion::HeaderTokenSearch { tokens, .. } => Some(
            tokens
                .iter()
                .map(|token| regex_literal(token))
                .collect::<Vec<_>>()
                .join(" "),
        ),
        Assertion::FullWidthRow { pattern, .. } => Some(regex_literal(pattern)),
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
    use crate::infer::observer::{HtmlTableObservation, Observation};
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
            headings: Vec::new(),
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
            html_page_section_count: None,
            html_tables: Vec::new(),
        }
    }

    fn html_observation(
        headings: &[&str],
        page_sections: u64,
        tables: &[HtmlTableObservation],
    ) -> Observation {
        Observation {
            format: "html".to_owned(),
            extension: "html".to_owned(),
            filename: "fixture.html".to_owned(),
            headings: headings.iter().map(ToString::to_string).collect(),
            sheet_names: Vec::new(),
            row_counts: HashMap::new(),
            cell_values: HashMap::new(),
            csv_headers: Vec::new(),
            csv_row_count: None,
            pdf_page_count: None,
            pdf_metadata: HashMap::new(),
            html_page_section_count: Some(page_sections),
            html_tables: tables.to_vec(),
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

    #[test]
    fn aggregate_html_emits_html_specific_assertions_and_extracts() {
        let observations = vec![html_observation(
            &["Schedule of Investments"],
            3,
            &[
                HtmlTableObservation {
                    page: Some(1),
                    page_index: 0,
                    columns: 6,
                    headers: vec![
                        "Portfolio Company".to_owned(),
                        "Business Description".to_owned(),
                        "Coupon".to_owned(),
                        "Cash Interest".to_owned(),
                        "PIK".to_owned(),
                        "Fair Value".to_owned(),
                    ],
                    full_width_rows: vec!["Software".to_owned(), "Healthcare".to_owned()],
                },
                HtmlTableObservation {
                    page: Some(2),
                    page_index: 0,
                    columns: 6,
                    headers: vec![
                        "Portfolio Company".to_owned(),
                        "Business Description".to_owned(),
                        "Coupon".to_owned(),
                        "Cash Interest".to_owned(),
                        "PIK".to_owned(),
                        "Fair Value".to_owned(),
                    ],
                    full_width_rows: vec!["Software".to_owned()],
                },
            ],
        )];

        let profile = aggregate(&observations, "html", "html-test.v1", 0.0, true, None)
            .expect("aggregate html");

        assert!(profile.assertions.iter().any(|entry| {
            matches!(
                entry.assertion.assertion,
                Assertion::HeaderTokenSearch { .. }
            )
        }));
        assert!(profile.assertions.iter().any(|entry| {
            matches!(
                entry.assertion.assertion,
                Assertion::DominantColumnCount { count: 6, .. }
            )
        }));
        assert!(
            profile.assertions.iter().any(|entry| {
                matches!(entry.assertion.assertion, Assertion::FullWidthRow { .. })
            })
        );
        assert!(profile.assertions.iter().any(|entry| {
            matches!(
                entry.assertion.assertion,
                Assertion::PageSectionCount {
                    min: Some(3),
                    max: Some(3)
                }
            )
        }));
        assert_eq!(profile.extract.len(), 2);
        assert!(profile.content_hash.is_some());
    }
}
