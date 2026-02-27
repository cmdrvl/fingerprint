use crate::document::Document;
use crate::registry::AssertionResult;
use calamine::{Reader, open_workbook_auto};
use chrono::NaiveDate;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

type CellRef = (usize, usize);
type CellRange = (CellRef, CellRef);
static DIAGNOSE_MODE: AtomicBool = AtomicBool::new(false);

/// Enable or disable diagnostic context mode for assertion evaluation.
pub fn set_diagnose_mode(enabled: bool) {
    DIAGNOSE_MODE.store(enabled, Ordering::Relaxed);
}

/// Return whether diagnostic mode is currently active.
pub fn diagnose_mode() -> bool {
    DIAGNOSE_MODE.load(Ordering::Relaxed)
}

/// DSL assertion types. Each variant maps to one assertion in a `.fp.yaml` file.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Assertion {
    FilenameRegex {
        pattern: String,
    },
    SheetExists(String),
    SheetNameRegex {
        pattern: String,
        #[serde(default)]
        bind: Option<String>,
    },
    CellEq {
        sheet: String,
        cell: String,
        value: String,
    },
    CellRegex {
        sheet: String,
        cell: String,
        pattern: String,
    },
    RangeNonNull {
        sheet: String,
        range: String,
    },
    RangePopulated {
        sheet: String,
        range: String,
        min_pct: f64,
    },
    SheetMinRows {
        sheet: String,
        min_rows: u64,
    },
    ColumnSearch {
        sheet: String,
        column: String,
        row_range: String,
        pattern: String,
    },
    HeaderRowMatch {
        sheet: String,
        row_range: String,
        min_match: u64,
        columns: Vec<ColumnPattern>,
    },
    SumEq {
        range: String,
        equals_cell: String,
        tolerance: f64,
    },
    WithinTolerance {
        cell: String,
        min: f64,
        max: f64,
    },
    HeadingExists(String),
    HeadingRegex {
        pattern: String,
    },
    HeadingLevel {
        level: u8,
        pattern: String,
    },
    TextContains(String),
    TextRegex {
        pattern: String,
    },
    TextNear {
        anchor: String,
        pattern: String,
        within_chars: u32,
    },
    SectionNonEmpty {
        heading: String,
    },
    SectionMinLines {
        heading: String,
        min_lines: u64,
    },
    TableExists {
        heading: String,
        index: Option<usize>,
    },
    TableColumns {
        heading: String,
        index: Option<usize>,
        patterns: Vec<String>,
    },
    TableShape {
        heading: String,
        index: Option<usize>,
        min_columns: usize,
        column_types: Vec<String>,
    },
    TableMinRows {
        heading: String,
        index: Option<usize>,
        min_rows: u64,
    },
    PageCount {
        min: Option<u64>,
        max: Option<u64>,
    },
    MetadataRegex {
        key: String,
        pattern: String,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ColumnPattern {
    pub pattern: String,
}

#[derive(Debug, Default, Clone)]
struct EvaluationContext {
    sheet_bindings: HashMap<String, String>,
}

/// A DSL assertion entry with an optional human-readable name.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct NamedAssertion {
    pub name: Option<String>,
    #[serde(flatten)]
    pub assertion: Assertion,
}

/// Evaluate a named assertion and preserve its DSL-level name in output.
pub fn evaluate_named(named_assertion: &NamedAssertion, doc: &Document) -> AssertionResult {
    let mut context = EvaluationContext::default();
    evaluate_named_with_diagnose_and_context(named_assertion, doc, diagnose_mode(), &mut context)
}

/// Evaluate a named assertion with explicit diagnostic mode.
pub fn evaluate_named_with_diagnose(
    named_assertion: &NamedAssertion,
    doc: &Document,
    diagnose: bool,
) -> AssertionResult {
    let mut context = EvaluationContext::default();
    evaluate_named_with_diagnose_and_context(named_assertion, doc, diagnose, &mut context)
}

fn evaluate_named_with_diagnose_and_context(
    named_assertion: &NamedAssertion,
    doc: &Document,
    diagnose: bool,
    context: &mut EvaluationContext,
) -> AssertionResult {
    let mut result =
        evaluate_with_diagnose_and_context(&named_assertion.assertion, doc, diagnose, context);
    if let Some(name) = &named_assertion.name {
        result.name = name.clone();
    }
    result
}

/// Evaluate assertions in declaration order.
///
/// In normal mode, evaluation short-circuits after the first failure.
/// In diagnose mode, all assertions are evaluated.
pub fn evaluate_named_assertions(
    assertions: &[NamedAssertion],
    doc: &Document,
) -> Vec<AssertionResult> {
    evaluate_named_assertions_with_diagnose(assertions, doc, diagnose_mode())
}

/// Evaluate assertions in declaration order with explicit diagnostic mode.
pub fn evaluate_named_assertions_with_diagnose(
    assertions: &[NamedAssertion],
    doc: &Document,
    diagnose: bool,
) -> Vec<AssertionResult> {
    let mut context = EvaluationContext::default();
    let mut results = Vec::with_capacity(assertions.len());
    for assertion in assertions {
        let result =
            evaluate_named_with_diagnose_and_context(assertion, doc, diagnose, &mut context);
        let failed = !result.passed;
        results.push(result);
        if failed && !diagnose {
            break;
        }
    }
    results
}

/// Evaluate a single assertion against a document.
pub fn evaluate(assertion: &Assertion, doc: &Document) -> AssertionResult {
    evaluate_with_diagnose(assertion, doc, diagnose_mode())
}

/// Evaluate an assertion with explicit diagnostic mode.
pub fn evaluate_with_diagnose(
    assertion: &Assertion,
    doc: &Document,
    diagnose: bool,
) -> AssertionResult {
    let mut context = EvaluationContext::default();
    evaluate_with_diagnose_and_context(assertion, doc, diagnose, &mut context)
}

fn evaluate_with_diagnose_and_context(
    assertion: &Assertion,
    doc: &Document,
    diagnose: bool,
    context: &mut EvaluationContext,
) -> AssertionResult {
    let name = assertion_type_name(assertion).to_owned();
    let result = if is_content_assertion(assertion) {
        evaluate_content_assertion(assertion, doc)
    } else {
        match assertion {
            Assertion::FilenameRegex { pattern } => evaluate_filename_regex(doc, pattern),
            Assertion::SheetExists(sheet) => resolve_sheet_name(sheet, context)
                .and_then(|resolved| evaluate_sheet_exists(doc, &resolved)),
            Assertion::SheetNameRegex { pattern, bind } => evaluate_sheet_name_regex(doc, pattern)
                .and_then(|matched_sheet| {
                    if let Some(bind_name) = bind {
                        bind_sheet_name(context, bind_name, &matched_sheet)?;
                    }
                    Ok(())
                }),
            Assertion::CellEq { sheet, cell, value } => resolve_sheet_name(sheet, context)
                .and_then(|resolved| evaluate_cell_eq(doc, &resolved, cell, value)),
            Assertion::CellRegex {
                sheet,
                cell,
                pattern,
            } => resolve_sheet_name(sheet, context)
                .and_then(|resolved| evaluate_cell_regex(doc, &resolved, cell, pattern)),
            Assertion::RangeNonNull { sheet, range } => resolve_sheet_name(sheet, context)
                .and_then(|resolved| evaluate_range_non_null(doc, &resolved, range)),
            Assertion::SheetMinRows { sheet, min_rows } => resolve_sheet_name(sheet, context)
                .and_then(|resolved| evaluate_sheet_min_rows(doc, &resolved, *min_rows)),
            Assertion::ColumnSearch {
                sheet,
                column,
                row_range,
                pattern,
            } => resolve_sheet_name(sheet, context).and_then(|resolved| {
                evaluate_column_search(doc, &resolved, column, row_range, pattern)
            }),
            Assertion::HeaderRowMatch {
                sheet,
                row_range,
                min_match,
                columns,
            } => resolve_sheet_name(sheet, context).and_then(|resolved| {
                evaluate_header_row_match(doc, &resolved, row_range, *min_match, columns)
            }),
            Assertion::PageCount { min, max } => evaluate_page_count(doc, *min, *max),
            Assertion::MetadataRegex { key, pattern } => evaluate_metadata_regex(doc, key, pattern),
            _ => Err(format!(
                "assertion '{}' is not implemented in v0.1",
                assertion_type_name(assertion)
            )),
        }
    };

    match result {
        Ok(()) => AssertionResult {
            name,
            passed: true,
            detail: None,
            context: None,
        },
        Err(detail) => {
            let context = if diagnose {
                diagnostic_context(assertion, doc, context)
            } else {
                None
            };
            AssertionResult {
                name,
                passed: false,
                detail: Some(detail),
                context,
            }
        }
    }
}

/// Adapter used by generated crates.
pub fn evaluate_assertion(
    doc: &Document,
    assertion: &Assertion,
) -> Result<AssertionResult, String> {
    Ok(evaluate(assertion, doc))
}

fn normalize_binding_name(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("binding name cannot be empty".to_owned());
    }
    let normalized = trimmed.strip_prefix('$').unwrap_or(trimmed).trim();
    if normalized.is_empty() {
        return Err(format!("binding name '{value}' is invalid"));
    }
    Ok(normalized.to_owned())
}

fn bind_sheet_name(
    context: &mut EvaluationContext,
    binding: &str,
    sheet_name: &str,
) -> Result<(), String> {
    let key = normalize_binding_name(binding)?;
    context.sheet_bindings.insert(key, sheet_name.to_owned());
    Ok(())
}

fn resolve_sheet_name(sheet: &str, context: &EvaluationContext) -> Result<String, String> {
    if !sheet.starts_with('$') {
        return Ok(sheet.to_owned());
    }

    let key = normalize_binding_name(sheet)?;
    context
        .sheet_bindings
        .get(&key)
        .cloned()
        .ok_or_else(|| format!("sheet binding '{sheet}' was not found"))
}

fn diagnostic_context(
    assertion: &Assertion,
    doc: &Document,
    context: &EvaluationContext,
) -> Option<Value> {
    match assertion {
        Assertion::HeadingExists(heading) => heading_diagnostic_context(doc, heading),
        Assertion::HeadingRegex { pattern } | Assertion::HeadingLevel { pattern, .. } => {
            heading_diagnostic_context(doc, &regex_hint(pattern))
        }
        Assertion::TextContains(text) => text_diagnostic_context(doc, text),
        Assertion::TextRegex { pattern } => text_diagnostic_context(doc, &regex_hint(pattern)),
        Assertion::TextNear {
            anchor,
            pattern,
            within_chars,
        } => text_near_diagnostic_context(doc, anchor, pattern, *within_chars),
        Assertion::TableExists { heading, .. }
        | Assertion::TableColumns { heading, .. }
        | Assertion::TableShape { heading, .. } => table_diagnostic_context(doc, heading),
        Assertion::SectionNonEmpty { heading } | Assertion::SectionMinLines { heading, .. } => {
            section_diagnostic_context(doc, heading)
        }
        Assertion::ColumnSearch {
            sheet,
            column,
            row_range,
            pattern,
        } => column_search_diagnostic_context(doc, sheet, column, row_range, pattern, context),
        Assertion::HeaderRowMatch {
            sheet,
            row_range,
            min_match,
            columns,
        } => {
            header_row_match_diagnostic_context(doc, sheet, row_range, *min_match, columns, context)
        }
        _ => None,
    }
}

fn column_search_diagnostic_context(
    doc: &Document,
    sheet: &str,
    column: &str,
    row_range: &str,
    pattern: &str,
    context: &EvaluationContext,
) -> Option<Value> {
    let resolved_sheet = resolve_sheet_name(sheet, context).ok()?;
    let column_index = parse_column_ref(column).ok()?;
    let (start_row, end_row) = parse_row_range_ref(row_range).ok()?;
    let rows = spreadsheet_rows(doc, &resolved_sheet).ok()?;

    let mut scanned_cells = Vec::new();
    let mut partial_matches = Vec::new();
    let tokens = tokenize_hint(&regex_hint(pattern));

    for row_index in start_row..=end_row {
        if scanned_cells.len() >= 60 {
            break;
        }

        let value = rows
            .get(row_index)
            .and_then(|row| row.get(column_index))
            .cloned()
            .unwrap_or_default();
        let cell_ref = to_cell_ref(row_index, column_index);
        scanned_cells.push(json!({
            "cell": cell_ref,
            "value": value
        }));

        if partial_matches.len() < 5 && !tokens.is_empty() {
            let candidate = value.to_ascii_lowercase();
            if tokens.iter().any(|token| candidate.contains(token)) {
                partial_matches.push(value);
            }
        }
    }

    Some(json!({
        "sheet": resolved_sheet,
        "column": column,
        "row_range": row_range,
        "scanned_cells": scanned_cells,
        "partial_matches": partial_matches
    }))
}

fn header_row_match_diagnostic_context(
    doc: &Document,
    sheet: &str,
    row_range: &str,
    min_match: u64,
    columns: &[ColumnPattern],
    context: &EvaluationContext,
) -> Option<Value> {
    let resolved_sheet = resolve_sheet_name(sheet, context).ok()?;
    let (start_row, end_row) = parse_row_range_ref(row_range).ok()?;
    let patterns = compile_column_patterns(columns).ok()?;
    let rows = spreadsheet_rows(doc, &resolved_sheet).ok()?;
    let (best_row, best_match_count, best_pattern_indexes) =
        best_header_row_match(&rows, start_row, end_row, &patterns);

    let best_patterns: Vec<String> = best_pattern_indexes
        .into_iter()
        .filter_map(|index| columns.get(index).map(|column| column.pattern.clone()))
        .collect();

    Some(json!({
        "sheet": resolved_sheet,
        "row_range": row_range,
        "min_match": min_match,
        "best_row": best_row.map(|row| row + 1),
        "best_match_count": best_match_count,
        "best_patterns": best_patterns
    }))
}

fn heading_diagnostic_context(doc: &Document, target: &str) -> Option<Value> {
    let md_doc = get_content_document(doc).ok()?;
    let headings_found: Vec<String> = md_doc
        .headings
        .iter()
        .map(|heading| heading.text.clone())
        .collect();
    let nearest_match = nearest_match_by_edit_distance(target, &headings_found);

    Some(json!({
        "headings_found": headings_found,
        "nearest_match": nearest_match
    }))
}

fn text_diagnostic_context(doc: &Document, target: &str) -> Option<Value> {
    let source = content_source_text(doc)?;
    let partial_matches = collect_partial_matches(source, target, 5);
    Some(json!({
        "partial_matches": partial_matches
    }))
}

fn text_near_diagnostic_context(
    doc: &Document,
    anchor_pattern: &str,
    value_pattern: &str,
    within_chars: u32,
) -> Option<Value> {
    let source = content_source_text(doc)?;
    let anchor_regex = Regex::new(anchor_pattern).ok()?;
    let value_regex = Regex::new(value_pattern).ok()?;

    let anchors: Vec<_> = anchor_regex.find_iter(source).collect();
    let values: Vec<_> = value_regex.find_iter(source).collect();
    let mut matches_outside_range = Vec::new();

    for anchor_match in &anchors {
        for value_match in &values {
            let distance = if value_match.start() >= anchor_match.end() {
                distance_with_whitespace_tolerance(source, anchor_match.end(), value_match.start())
            } else if anchor_match.start() >= value_match.end() {
                distance_with_whitespace_tolerance(source, value_match.end(), anchor_match.start())
            } else {
                0
            };

            if distance > within_chars as usize && matches_outside_range.len() < 5 {
                matches_outside_range.push(json!({
                    "anchor": excerpt_around(source, anchor_match.start(), anchor_match.end(), 24),
                    "match": value_match.as_str(),
                    "distance": distance
                }));
            }
        }
    }

    Some(json!({
        "anchor_found": !anchors.is_empty(),
        "matches_outside_range": matches_outside_range
    }))
}

fn table_diagnostic_context(doc: &Document, heading_pattern: &str) -> Option<Value> {
    let md_doc = get_content_document(doc).ok()?;
    let heading_regex = Regex::new(heading_pattern).ok()?;
    let heading_found = md_doc
        .headings
        .iter()
        .any(|heading| heading_regex.is_match(&heading.text));

    let tables_found: Vec<Value> = md_doc
        .tables
        .iter()
        .filter(|table| {
            table
                .heading_ref
                .as_deref()
                .is_some_and(|heading| heading_regex.is_match(heading))
        })
        .take(5)
        .map(|table| {
            json!({
                "heading": table.heading_ref.clone(),
                "index": table.index,
                "columns": table.headers.clone(),
                "rows": table.rows.len()
            })
        })
        .collect();

    Some(json!({
        "tables_found": tables_found,
        "heading_found": heading_found
    }))
}

fn section_diagnostic_context(doc: &Document, heading_pattern: &str) -> Option<Value> {
    let md_doc = get_content_document(doc).ok()?;
    let heading_regex = Regex::new(heading_pattern).ok()?;

    let section = md_doc.sections.iter().find(|section| {
        section
            .heading
            .as_ref()
            .is_some_and(|heading| heading_regex.is_match(&heading.text))
    });

    let section_lines = section
        .map(|found| {
            section_body_lines(found)
                .filter(|line| !line.trim().is_empty())
                .count()
        })
        .unwrap_or(0);

    Some(json!({
        "section_lines": section_lines,
        "heading_found": section.is_some()
    }))
}

fn content_source_text(doc: &Document) -> Option<&str> {
    match doc {
        Document::Markdown(markdown) => Some(markdown.normalized.as_str()),
        Document::Text(text) => Some(text.content()),
        Document::Pdf(pdf) => pdf
            .text
            .as_ref()
            .map(|markdown| markdown.normalized.as_str()),
        _ => None,
    }
}

fn collect_partial_matches(source: &str, target: &str, limit: usize) -> Vec<String> {
    let tokens = tokenize_hint(target);
    let mut matches = Vec::new();

    for line in source.lines() {
        if matches.len() >= limit {
            break;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let candidate = trimmed.to_ascii_lowercase();
        if tokens.iter().any(|token| candidate.contains(token)) {
            matches.push(trimmed.to_owned());
        }
    }

    if !matches.is_empty() {
        return matches;
    }

    let normalized_target = normalize_for_distance(target);
    if normalized_target.is_empty() {
        return matches;
    }

    let mut scored: Vec<(usize, String)> = source
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            (!trimmed.is_empty()).then(|| {
                (
                    levenshtein(&normalized_target, &normalize_for_distance(trimmed)),
                    trimmed.to_owned(),
                )
            })
        })
        .collect();

    scored.sort_by_key(|(distance, _)| *distance);
    for (_, line) in scored.into_iter().take(limit) {
        matches.push(line);
    }
    matches
}

fn tokenize_hint(value: &str) -> Vec<String> {
    normalize_for_distance(value)
        .split_whitespace()
        .filter(|token| token.len() >= 3)
        .take(6)
        .map(ToOwned::to_owned)
        .collect()
}

fn regex_hint(pattern: &str) -> String {
    let mut stripped = pattern.to_owned();
    for inline_flag in ["(?i)", "(?m)", "(?s)", "(?x)"] {
        stripped = stripped.replace(inline_flag, " ");
    }
    normalize_for_distance(&stripped)
}

fn normalize_for_distance(value: &str) -> String {
    let mut normalized = String::new();
    let mut previous_space = false;

    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            normalized.push(character.to_ascii_lowercase());
            previous_space = false;
        } else if !previous_space {
            normalized.push(' ');
            previous_space = true;
        }
    }

    normalized.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn nearest_match_by_edit_distance(target: &str, headings: &[String]) -> Option<String> {
    let normalized_target = normalize_for_distance(target);
    if normalized_target.is_empty() {
        return None;
    }

    headings
        .iter()
        .map(|heading| {
            (
                levenshtein(&normalized_target, &normalize_for_distance(heading)),
                heading,
            )
        })
        .min_by_key(|(distance, heading)| (*distance, heading.len()))
        .map(|(_, heading)| heading.clone())
}

fn levenshtein(left: &str, right: &str) -> usize {
    let right_chars: Vec<char> = right.chars().collect();
    let mut previous: Vec<usize> = (0..=right_chars.len()).collect();
    let mut current = vec![0; right_chars.len() + 1];

    for (left_index, left_char) in left.chars().enumerate() {
        current[0] = left_index + 1;

        for (right_index, right_char) in right_chars.iter().enumerate() {
            let substitution_cost = usize::from(left_char != *right_char);
            let delete_cost = previous[right_index + 1] + 1;
            let insert_cost = current[right_index] + 1;
            let replace_cost = previous[right_index] + substitution_cost;
            current[right_index + 1] = delete_cost.min(insert_cost).min(replace_cost);
        }

        previous.clone_from_slice(&current);
    }

    previous[right_chars.len()]
}

fn excerpt_around(source: &str, start: usize, end: usize, radius: usize) -> String {
    let from = start.saturating_sub(radius);
    let to = (end + radius).min(source.len());
    source[from..to].trim().to_owned()
}

fn is_content_assertion(assertion: &Assertion) -> bool {
    matches!(
        assertion,
        Assertion::HeadingExists(_)
            | Assertion::HeadingRegex { .. }
            | Assertion::HeadingLevel { .. }
            | Assertion::TextContains(_)
            | Assertion::TextRegex { .. }
            | Assertion::TextNear { .. }
            | Assertion::SectionNonEmpty { .. }
            | Assertion::SectionMinLines { .. }
            | Assertion::TableExists { .. }
            | Assertion::TableColumns { .. }
            | Assertion::TableShape { .. }
            | Assertion::TableMinRows { .. }
    )
}

fn evaluate_content_assertion(assertion: &Assertion, doc: &Document) -> Result<(), String> {
    if let Document::Pdf(pdf_doc) = doc
        && pdf_doc.text.is_none()
    {
        return Err("No text_path provided (E_NO_TEXT)".to_owned());
    }

    match assertion {
        Assertion::HeadingExists(text) => evaluate_heading_exists(doc, text),
        Assertion::HeadingRegex { pattern } => evaluate_heading_regex(doc, pattern),
        Assertion::HeadingLevel { level, pattern } => evaluate_heading_level(doc, *level, pattern),
        Assertion::TextContains(text) => evaluate_text_contains(doc, text),
        Assertion::TextRegex { pattern } => evaluate_text_regex(doc, pattern),
        Assertion::TextNear {
            anchor,
            pattern,
            within_chars,
        } => evaluate_text_near(doc, anchor, pattern, *within_chars),
        Assertion::SectionNonEmpty { heading } => evaluate_section_non_empty(doc, heading),
        Assertion::SectionMinLines { heading, min_lines } => {
            evaluate_section_min_lines(doc, heading, *min_lines)
        }
        Assertion::TableExists { heading, index } => evaluate_table_exists(doc, heading, *index),
        Assertion::TableColumns {
            heading,
            index,
            patterns,
        } => evaluate_table_columns(doc, heading, *index, patterns),
        Assertion::TableShape {
            heading,
            index,
            min_columns,
            column_types,
        } => evaluate_table_shape(doc, heading, *index, *min_columns, column_types),
        Assertion::TableMinRows {
            heading,
            index,
            min_rows,
        } => evaluate_table_min_rows(doc, heading, *index, *min_rows),
        _ => Err(format!(
            "assertion '{}' is not implemented in v0.1",
            assertion_type_name(assertion)
        )),
    }
}

fn assertion_type_name(assertion: &Assertion) -> &'static str {
    match assertion {
        Assertion::FilenameRegex { .. } => "filename_regex",
        Assertion::SheetExists(_) => "sheet_exists",
        Assertion::SheetNameRegex { .. } => "sheet_name_regex",
        Assertion::CellEq { .. } => "cell_eq",
        Assertion::CellRegex { .. } => "cell_regex",
        Assertion::RangeNonNull { .. } => "range_non_null",
        Assertion::RangePopulated { .. } => "range_populated",
        Assertion::SheetMinRows { .. } => "sheet_min_rows",
        Assertion::ColumnSearch { .. } => "column_search",
        Assertion::HeaderRowMatch { .. } => "header_row_match",
        Assertion::SumEq { .. } => "sum_eq",
        Assertion::WithinTolerance { .. } => "within_tolerance",
        Assertion::HeadingExists(_) => "heading_exists",
        Assertion::HeadingRegex { .. } => "heading_regex",
        Assertion::HeadingLevel { .. } => "heading_level",
        Assertion::TextContains(_) => "text_contains",
        Assertion::TextRegex { .. } => "text_regex",
        Assertion::TextNear { .. } => "text_near",
        Assertion::SectionNonEmpty { .. } => "section_non_empty",
        Assertion::SectionMinLines { .. } => "section_min_lines",
        Assertion::TableExists { .. } => "table_exists",
        Assertion::TableColumns { .. } => "table_columns",
        Assertion::TableShape { .. } => "table_shape",
        Assertion::TableMinRows { .. } => "table_min_rows",
        Assertion::PageCount { .. } => "page_count",
        Assertion::MetadataRegex { .. } => "metadata_regex",
    }
}

fn evaluate_filename_regex(doc: &Document, pattern: &str) -> Result<(), String> {
    let regex =
        Regex::new(pattern).map_err(|error| format!("invalid regex '{pattern}': {error}"))?;
    let file_name = doc
        .path()
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            format!(
                "unable to read filename from path '{}'",
                doc.path().display()
            )
        })?;
    if regex.is_match(file_name) {
        Ok(())
    } else {
        Err(format!(
            "filename '{}' does not match pattern '{}'",
            file_name, pattern
        ))
    }
}

fn evaluate_sheet_exists(doc: &Document, sheet: &str) -> Result<(), String> {
    match doc {
        Document::Xlsx(xlsx) => {
            let workbook = open_workbook_auto(&xlsx.path).map_err(|error| {
                format!("failed opening workbook '{}': {error}", xlsx.path.display())
            })?;
            if workbook.sheet_names().iter().any(|name| name == sheet) {
                Ok(())
            } else {
                Err(format!("sheet '{sheet}' not found"))
            }
        }
        Document::Csv(csv) => {
            if csv_virtual_sheet_names(&csv.path)
                .iter()
                .any(|name| name.eq_ignore_ascii_case(sheet))
            {
                Ok(())
            } else {
                Err(format!("sheet '{sheet}' not found in csv document"))
            }
        }
        _ => Err("spreadsheet assertion requires xlsx or csv document".to_owned()),
    }
}

fn evaluate_sheet_name_regex(doc: &Document, pattern: &str) -> Result<String, String> {
    let regex =
        Regex::new(pattern).map_err(|error| format!("invalid regex '{pattern}': {error}"))?;

    match doc {
        Document::Xlsx(xlsx) => {
            let workbook = open_workbook_auto(&xlsx.path).map_err(|error| {
                format!("failed opening workbook '{}': {error}", xlsx.path.display())
            })?;
            if let Some(matched) = workbook
                .sheet_names()
                .iter()
                .find(|sheet| regex.is_match(sheet))
                .cloned()
            {
                Ok(matched)
            } else {
                Err(format!("no sheet name matched pattern '{}'", pattern))
            }
        }
        Document::Csv(csv) => {
            if let Some(matched) = csv_virtual_sheet_names(&csv.path)
                .iter()
                .find(|sheet| regex.is_match(sheet))
                .cloned()
            {
                Ok(matched)
            } else {
                Err(format!(
                    "no csv virtual sheet name matched pattern '{}'",
                    pattern
                ))
            }
        }
        _ => Err("spreadsheet assertion requires xlsx or csv document".to_owned()),
    }
}

fn evaluate_cell_eq(doc: &Document, sheet: &str, cell: &str, value: &str) -> Result<(), String> {
    let cell_ref = parse_cell_ref(cell)?;
    let actual = spreadsheet_cell_value(doc, sheet, cell_ref)?;
    match actual {
        Some(actual) if actual == value => Ok(()),
        Some(actual) => Err(format!(
            "cell {cell} expected '{value}' but found '{actual}'"
        )),
        None => Err(format!("cell {cell} is empty or missing")),
    }
}

fn evaluate_cell_regex(
    doc: &Document,
    sheet: &str,
    cell: &str,
    pattern: &str,
) -> Result<(), String> {
    let cell_ref = parse_cell_ref(cell)?;
    let regex =
        Regex::new(pattern).map_err(|error| format!("invalid regex '{pattern}': {error}"))?;
    let actual = spreadsheet_cell_value(doc, sheet, cell_ref)?;
    match actual {
        Some(value) if regex.is_match(&value) => Ok(()),
        Some(value) => Err(format!(
            "cell {cell} value '{value}' did not match pattern '{pattern}'"
        )),
        None => Err(format!("cell {cell} is empty or missing")),
    }
}

fn evaluate_range_non_null(doc: &Document, sheet: &str, range: &str) -> Result<(), String> {
    let (start, end) = parse_range_ref(range)?;
    for row in start.0..=end.0 {
        for col in start.1..=end.1 {
            let value = spreadsheet_cell_value(doc, sheet, (row, col))?;
            if value.is_none_or(|text| text.trim().is_empty()) {
                return Err(format!(
                    "range {range} contains empty or missing cell at {}",
                    to_cell_ref(row, col)
                ));
            }
        }
    }
    Ok(())
}

fn evaluate_sheet_min_rows(doc: &Document, sheet: &str, min_rows: u64) -> Result<(), String> {
    let row_count = spreadsheet_non_empty_row_count(doc, sheet)?;
    if row_count as u64 >= min_rows {
        Ok(())
    } else {
        Err(format!(
            "sheet '{sheet}' has {row_count} non-empty rows, expected at least {min_rows}"
        ))
    }
}

fn evaluate_column_search(
    doc: &Document,
    sheet: &str,
    column: &str,
    row_range: &str,
    pattern: &str,
) -> Result<(), String> {
    let column_index = parse_column_ref(column)?;
    let (start_row, end_row) = parse_row_range_ref(row_range)?;
    let regex =
        Regex::new(pattern).map_err(|error| format!("invalid regex '{pattern}': {error}"))?;
    let rows = spreadsheet_rows(doc, sheet)?;

    for row_index in start_row..=end_row {
        if let Some(value) = rows.get(row_index).and_then(|row| row.get(column_index))
            && regex.is_match(value)
        {
            return Ok(());
        }
    }

    Err(format!(
        "no cell in column {column} rows {row_range} matched pattern '{pattern}'"
    ))
}

fn evaluate_header_row_match(
    doc: &Document,
    sheet: &str,
    row_range: &str,
    min_match: u64,
    columns: &[ColumnPattern],
) -> Result<(), String> {
    if columns.is_empty() {
        return Err("header_row_match requires at least one column pattern".to_owned());
    }

    let (start_row, end_row) = parse_row_range_ref(row_range)?;
    let patterns = compile_column_patterns(columns)?;
    let rows = spreadsheet_rows(doc, sheet)?;
    let (best_row, best_count, _) = best_header_row_match(&rows, start_row, end_row, &patterns);

    if best_count as u64 >= min_match {
        return Ok(());
    }

    let best_row_message = best_row
        .map(|row| (row + 1).to_string())
        .unwrap_or_else(|| "none".to_owned());
    Err(format!(
        "no row in range {row_range} reached min_match {min_match}; best row was {best_row_message} with {best_count} matches"
    ))
}

fn compile_column_patterns(columns: &[ColumnPattern]) -> Result<Vec<Regex>, String> {
    columns
        .iter()
        .enumerate()
        .map(|(index, column)| {
            Regex::new(&column.pattern).map_err(|error| {
                format!(
                    "invalid regex for header_row_match columns[{index}] '{}': {error}",
                    column.pattern
                )
            })
        })
        .collect()
}

fn best_header_row_match(
    rows: &[Vec<String>],
    start_row: usize,
    end_row: usize,
    patterns: &[Regex],
) -> (Option<usize>, usize, Vec<usize>) {
    let mut best_row = None;
    let mut best_count = 0usize;
    let mut best_patterns = Vec::new();

    for row_index in start_row..=end_row {
        let row = rows.get(row_index).map(Vec::as_slice).unwrap_or(&[]);
        let (match_count, matched_pattern_indexes) = row_pattern_matches(row, patterns);
        if match_count > best_count {
            best_row = Some(row_index);
            best_count = match_count;
            best_patterns = matched_pattern_indexes;
        }
    }

    (best_row, best_count, best_patterns)
}

fn row_pattern_matches(row: &[String], patterns: &[Regex]) -> (usize, Vec<usize>) {
    let mut matched_patterns = HashSet::new();

    for value in row {
        if value.trim().is_empty() {
            continue;
        }
        for (pattern_index, pattern) in patterns.iter().enumerate() {
            if matched_patterns.contains(&pattern_index) {
                continue;
            }
            if pattern.is_match(value) {
                matched_patterns.insert(pattern_index);
                break;
            }
        }
    }

    let mut matched_indexes: Vec<usize> = matched_patterns.into_iter().collect();
    matched_indexes.sort_unstable();
    (matched_indexes.len(), matched_indexes)
}

fn spreadsheet_cell_value(
    doc: &Document,
    sheet: &str,
    position: CellRef,
) -> Result<Option<String>, String> {
    match doc {
        Document::Xlsx(xlsx) => {
            let mut workbook = open_workbook_auto(&xlsx.path).map_err(|error| {
                format!("failed opening workbook '{}': {error}", xlsx.path.display())
            })?;
            let worksheet = workbook
                .worksheet_range(sheet)
                .map_err(|error| format!("failed reading sheet '{sheet}': {error}"))?;

            let value = worksheet.get_value((position.0 as u32, position.1 as u32));
            Ok(value
                .map(|cell| cell.to_string())
                .filter(|text| !text.trim().is_empty()))
        }
        Document::Csv(csv) => {
            validate_csv_sheet_name(&csv.path, sheet)?;
            let rows = load_csv_rows(&csv.path)?;
            Ok(rows
                .get(position.0)
                .and_then(|row| row.get(position.1))
                .map(ToOwned::to_owned)
                .filter(|text| !text.trim().is_empty()))
        }
        _ => Err("spreadsheet assertion requires xlsx or csv document".to_owned()),
    }
}

fn spreadsheet_non_empty_row_count(doc: &Document, sheet: &str) -> Result<usize, String> {
    match doc {
        Document::Xlsx(xlsx) => {
            let mut workbook = open_workbook_auto(&xlsx.path).map_err(|error| {
                format!("failed opening workbook '{}': {error}", xlsx.path.display())
            })?;
            let worksheet = workbook
                .worksheet_range(sheet)
                .map_err(|error| format!("failed reading sheet '{sheet}': {error}"))?;

            Ok(worksheet
                .rows()
                .filter(|row| row.iter().any(|cell| !cell.to_string().trim().is_empty()))
                .count())
        }
        Document::Csv(csv) => {
            validate_csv_sheet_name(&csv.path, sheet)?;
            let rows = load_csv_rows(&csv.path)?;
            Ok(rows
                .iter()
                .filter(|row| row.iter().any(|value| !value.trim().is_empty()))
                .count())
        }
        _ => Err("spreadsheet assertion requires xlsx or csv document".to_owned()),
    }
}

fn spreadsheet_rows(doc: &Document, sheet: &str) -> Result<Vec<Vec<String>>, String> {
    match doc {
        Document::Xlsx(xlsx) => {
            let mut workbook = open_workbook_auto(&xlsx.path).map_err(|error| {
                format!("failed opening workbook '{}': {error}", xlsx.path.display())
            })?;
            let worksheet = workbook
                .worksheet_range(sheet)
                .map_err(|error| format!("failed reading sheet '{sheet}': {error}"))?;

            Ok(worksheet
                .rows()
                .map(|row| row.iter().map(ToString::to_string).collect())
                .collect())
        }
        Document::Csv(csv) => {
            validate_csv_sheet_name(&csv.path, sheet)?;
            load_csv_rows(&csv.path)
        }
        _ => Err("spreadsheet assertion requires xlsx or csv document".to_owned()),
    }
}

fn parse_cell_ref(cell: &str) -> Result<CellRef, String> {
    let mut letters = String::new();
    let mut digits = String::new();

    for character in cell.chars() {
        if character.is_ascii_alphabetic() {
            if !digits.is_empty() {
                return Err(format!("invalid cell reference '{cell}'"));
            }
            letters.push(character);
        } else if character.is_ascii_digit() {
            digits.push(character);
        } else {
            return Err(format!("invalid cell reference '{cell}'"));
        }
    }

    if letters.is_empty() || digits.is_empty() {
        return Err(format!("invalid cell reference '{cell}'"));
    }

    let mut column: usize = 0;
    for character in letters.chars() {
        let upper = character.to_ascii_uppercase();
        if !upper.is_ascii_uppercase() {
            return Err(format!("invalid column reference in '{cell}'"));
        }
        column = column.saturating_mul(26) + (upper as usize - 'A' as usize + 1);
    }

    let row: usize = digits
        .parse()
        .map_err(|error| format!("invalid row in cell reference '{cell}': {error}"))?;
    if row == 0 {
        return Err(format!("row number must be >= 1 in '{cell}'"));
    }

    Ok((row - 1, column - 1))
}

fn parse_range_ref(range: &str) -> Result<CellRange, String> {
    let (left, right) = range
        .split_once(':')
        .ok_or_else(|| format!("invalid range reference '{range}'"))?;
    let start = parse_cell_ref(left)?;
    let end = parse_cell_ref(right)?;

    Ok((
        (start.0.min(end.0), start.1.min(end.1)),
        (start.0.max(end.0), start.1.max(end.1)),
    ))
}

fn parse_column_ref(column: &str) -> Result<usize, String> {
    let trimmed = column.trim();
    if trimmed.is_empty() {
        return Err("column reference cannot be empty".to_owned());
    }

    let mut parsed = 0usize;
    for character in trimmed.chars() {
        if !character.is_ascii_alphabetic() {
            return Err(format!("invalid column reference '{column}'"));
        }
        let upper = character.to_ascii_uppercase();
        parsed = parsed.saturating_mul(26) + (upper as usize - 'A' as usize + 1);
    }

    Ok(parsed - 1)
}

fn parse_row_range_ref(row_range: &str) -> Result<(usize, usize), String> {
    let (start, end) = row_range
        .split_once(':')
        .ok_or_else(|| format!("invalid row_range '{row_range}'"))?;

    let start_row: usize = start
        .trim()
        .parse()
        .map_err(|error| format!("invalid row_range start '{}': {error}", start.trim()))?;
    let end_row: usize = end
        .trim()
        .parse()
        .map_err(|error| format!("invalid row_range end '{}': {error}", end.trim()))?;

    if start_row == 0 || end_row == 0 {
        return Err(format!("row_range '{row_range}' must use 1-based rows"));
    }

    Ok((start_row.min(end_row) - 1, start_row.max(end_row) - 1))
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

fn csv_virtual_sheet_names(path: &Path) -> Vec<String> {
    let mut names = vec!["Sheet1".to_owned(), "csv".to_owned()];
    if let Some(stem) = path.file_stem().and_then(|value| value.to_str()) {
        names.push(stem.to_owned());
    }
    names
}

fn validate_csv_sheet_name(path: &Path, sheet: &str) -> Result<(), String> {
    if csv_virtual_sheet_names(path)
        .iter()
        .any(|name| name.eq_ignore_ascii_case(sheet))
    {
        Ok(())
    } else {
        Err(format!(
            "sheet '{sheet}' not found in csv document '{}'",
            path.display()
        ))
    }
}

fn load_csv_rows(path: &Path) -> Result<Vec<Vec<String>>, String> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_path(path)
        .map_err(|error| format!("failed opening csv '{}': {error}", path.display()))?;
    let mut rows = Vec::new();

    for (index, record) in reader.records().enumerate() {
        let record = record.map_err(|error| {
            format!(
                "failed reading csv '{}' row {}: {error}",
                path.display(),
                index + 1
            )
        })?;
        rows.push(record.iter().map(ToOwned::to_owned).collect());
    }

    Ok(rows)
}

fn evaluate_page_count(doc: &Document, min: Option<u64>, max: Option<u64>) -> Result<(), String> {
    let pdf = match doc {
        Document::Pdf(pdf) => pdf,
        _ => return Err("pdf structural assertion requires pdf format".to_owned()),
    };

    let pdf_document = lopdf::Document::load(&pdf.path)
        .map_err(|error| format!("failed reading pdf '{}': {error}", pdf.path.display()))?;
    let page_count = pdf_document.get_pages().len() as u64;

    if let Some(min) = min
        && page_count < min
    {
        return Err(format!(
            "pdf has {page_count} pages, expected at least {min}"
        ));
    }
    if let Some(max) = max
        && page_count > max
    {
        return Err(format!(
            "pdf has {page_count} pages, expected at most {max}"
        ));
    }

    Ok(())
}

fn evaluate_metadata_regex(doc: &Document, key: &str, pattern: &str) -> Result<(), String> {
    let pdf = match doc {
        Document::Pdf(pdf) => pdf,
        _ => return Err("pdf structural assertion requires pdf format".to_owned()),
    };
    let regex =
        Regex::new(pattern).map_err(|error| format!("invalid regex '{pattern}': {error}"))?;

    let pdf_document = lopdf::Document::load(&pdf.path)
        .map_err(|error| format!("failed reading pdf '{}': {error}", pdf.path.display()))?;
    let metadata = pdf_metadata_map(&pdf_document).map_err(|error| {
        format!(
            "failed reading pdf metadata '{}': {error}",
            pdf.path.display()
        )
    })?;

    let value = metadata.iter().find_map(|(candidate, value)| {
        candidate
            .eq_ignore_ascii_case(key)
            .then_some(value.as_str())
    });

    let Some(value) = value else {
        return Err(format!("pdf metadata key '{key}' not found"));
    };

    if regex.is_match(value) {
        Ok(())
    } else {
        Err(format!(
            "pdf metadata key '{key}' value '{value}' does not match '{pattern}'"
        ))
    }
}

fn pdf_metadata_map(document: &lopdf::Document) -> Result<Vec<(String, String)>, String> {
    let info_object = document
        .trailer
        .get(b"Info")
        .map_err(|error| format!("missing Info dictionary in trailer: {error}"))?;

    let info_dictionary = match info_object {
        lopdf::Object::Reference(object_id) => document
            .get_object(*object_id)
            .map_err(|error| format!("unable to resolve Info dictionary reference: {error}"))?,
        object => object,
    };

    let dictionary = info_dictionary
        .as_dict()
        .map_err(|error| format!("Info object is not a dictionary: {error}"))?;

    let mut metadata = Vec::new();
    for (name, object) in dictionary {
        let key = String::from_utf8_lossy(name).to_string();
        let value = pdf_object_as_string(document, object)?;
        metadata.push((key, value));
    }

    Ok(metadata)
}

fn pdf_object_as_string(
    document: &lopdf::Document,
    object: &lopdf::Object,
) -> Result<String, String> {
    match object {
        lopdf::Object::String(bytes, _) => Ok(String::from_utf8_lossy(bytes).to_string()),
        lopdf::Object::Name(bytes) => Ok(String::from_utf8_lossy(bytes).to_string()),
        lopdf::Object::Integer(value) => Ok(value.to_string()),
        lopdf::Object::Real(value) => Ok(value.to_string()),
        lopdf::Object::Boolean(value) => Ok(value.to_string()),
        lopdf::Object::Reference(object_id) => {
            let resolved = document
                .get_object(*object_id)
                .map_err(|error| format!("unable to resolve metadata reference: {error}"))?;
            pdf_object_as_string(document, resolved)
        }
        _ => Ok(format!("{object:?}")),
    }
}

// Content assertion implementations with format-aware dispatch

fn evaluate_heading_exists(doc: &Document, text: &str) -> Result<(), String> {
    let md_doc = get_content_document(doc)?;
    let found = md_doc.headings.iter().any(|h| h.text == text);
    if found {
        Ok(())
    } else {
        Err(format!("Heading '{}' not found", text))
    }
}

fn evaluate_heading_regex(doc: &Document, pattern: &str) -> Result<(), String> {
    let md_doc = get_content_document(doc)?;
    let regex =
        Regex::new(pattern).map_err(|error| format!("invalid regex '{pattern}': {error}"))?;
    let found = md_doc.headings.iter().any(|h| regex.is_match(&h.text));
    if found {
        Ok(())
    } else {
        Err(format!("No heading matches pattern '{}'", pattern))
    }
}

fn evaluate_heading_level(doc: &Document, level: u8, pattern: &str) -> Result<(), String> {
    let md_doc = get_content_document(doc)?;
    let regex =
        Regex::new(pattern).map_err(|error| format!("invalid regex '{pattern}': {error}"))?;
    let found = md_doc
        .headings
        .iter()
        .any(|h| h.level == level && regex.is_match(&h.text));
    if found {
        Ok(())
    } else {
        Err(format!(
            "No level-{level} heading matches pattern '{pattern}'"
        ))
    }
}

fn evaluate_text_contains(doc: &Document, text: &str) -> Result<(), String> {
    match doc {
        Document::Markdown(md_doc) => {
            if md_doc.normalized.contains(text) {
                Ok(())
            } else {
                Err(format!("Text '{}' not found in document", text))
            }
        }
        Document::Text(text_doc) => {
            if text_doc.content().contains(text) {
                Ok(())
            } else {
                Err(format!("Text '{}' not found in document", text))
            }
        }
        Document::Pdf(pdf_doc) => match &pdf_doc.text {
            Some(md_doc) => {
                if md_doc.normalized.contains(text) {
                    Ok(())
                } else {
                    Err(format!("Text '{}' not found in document", text))
                }
            }
            None => Err("No text_path provided (E_NO_TEXT)".to_string()),
        },
        _ => Err("Content assertion 'text_contains' not supported for document type".to_string()),
    }
}

fn evaluate_text_regex(doc: &Document, pattern: &str) -> Result<(), String> {
    let regex =
        Regex::new(pattern).map_err(|error| format!("invalid regex '{pattern}': {error}"))?;

    match doc {
        Document::Markdown(md_doc) => {
            if regex.is_match(&md_doc.normalized) {
                Ok(())
            } else {
                Err(format!("Pattern '{}' not found in document", pattern))
            }
        }
        Document::Text(text_doc) => {
            if regex.is_match(text_doc.content()) {
                Ok(())
            } else {
                Err(format!("Pattern '{}' not found in document", pattern))
            }
        }
        Document::Pdf(pdf_doc) => match &pdf_doc.text {
            Some(md_doc) => {
                if regex.is_match(&md_doc.normalized) {
                    Ok(())
                } else {
                    Err(format!("Pattern '{}' not found in document", pattern))
                }
            }
            None => Err("No text_path provided (E_NO_TEXT)".to_string()),
        },
        _ => Err("Content assertion 'text_regex' not supported for document type".to_string()),
    }
}

fn evaluate_text_near(
    doc: &Document,
    anchor_pattern: &str,
    pattern: &str,
    within_chars: u32,
) -> Result<(), String> {
    let anchor_regex = Regex::new(anchor_pattern)
        .map_err(|error| format!("invalid regex '{}': {error}", anchor_pattern))?;
    let value_regex =
        Regex::new(pattern).map_err(|error| format!("invalid regex '{pattern}': {error}"))?;
    let source = match doc {
        Document::Markdown(markdown) => markdown.normalized.as_str(),
        Document::Text(text) => text.content(),
        Document::Pdf(pdf) => match &pdf.text {
            Some(markdown) => markdown.normalized.as_str(),
            None => return Err("No text_path provided (E_NO_TEXT)".to_string()),
        },
        _ => {
            return Err("Content assertion 'text_near' not supported for document type".to_owned());
        }
    };

    let anchors: Vec<_> = anchor_regex.find_iter(source).collect();
    let values: Vec<_> = value_regex.find_iter(source).collect();
    if anchors.is_empty() || values.is_empty() {
        return Err(format!(
            "No '{}' match found near anchor '{}' within {} chars",
            pattern, anchor_pattern, within_chars
        ));
    }

    for anchor_match in &anchors {
        for value_match in &values {
            let distance = if value_match.start() >= anchor_match.end() {
                distance_with_whitespace_tolerance(source, anchor_match.end(), value_match.start())
            } else if anchor_match.start() >= value_match.end() {
                distance_with_whitespace_tolerance(source, value_match.end(), anchor_match.start())
            } else {
                0
            };
            if distance <= within_chars as usize {
                return Ok(());
            }
        }
    }

    Err(format!(
        "No '{}' match found near anchor '{}' within {} chars",
        pattern, anchor_pattern, within_chars
    ))
}

fn distance_with_whitespace_tolerance(source: &str, start: usize, end: usize) -> usize {
    let gap = &source[start..end];
    if gap.len() < 10 && gap.chars().all(char::is_whitespace) {
        0
    } else {
        gap.len()
    }
}

/// Get the content document (MarkdownDocument) from any document type that supports content assertions.
/// For PDFs, this ensures text_path is present and returns E_NO_TEXT if missing.
fn get_content_document(doc: &Document) -> Result<&crate::document::MarkdownDocument, String> {
    match doc {
        Document::Markdown(md_doc) => Ok(md_doc),
        Document::Pdf(pdf_doc) => match &pdf_doc.text {
            Some(md_doc) => Ok(md_doc),
            None => Err("No text_path provided (E_NO_TEXT)".to_string()),
        },
        Document::Text(_) => Err(
            "Heading assertions require markdown format (text format has no heading structure)"
                .to_string(),
        ),
        _ => Err(
            "Content assertions with heading structure require markdown or pdf with text_path"
                .to_string(),
        ),
    }
}

fn evaluate_section_non_empty(doc: &Document, heading_pattern: &str) -> Result<(), String> {
    let md_doc = get_content_document(doc)?;
    let heading_regex = Regex::new(heading_pattern)
        .map_err(|error| format!("invalid regex '{heading_pattern}': {error}"))?;

    // Find the section with matching heading
    let section = md_doc.sections.iter().find(|section| {
        section
            .heading
            .as_ref()
            .is_some_and(|heading| heading_regex.is_match(&heading.text))
    });

    match section {
        Some(section) => {
            let has_content = section_body_lines(section).any(|line| !line.trim().is_empty());

            if has_content {
                Ok(())
            } else {
                Err("Section is empty".to_string())
            }
        }
        None => Err(format!(
            "heading not found: no section matches '{}'",
            heading_pattern
        )),
    }
}

fn evaluate_section_min_lines(
    doc: &Document,
    heading_pattern: &str,
    min_lines: u64,
) -> Result<(), String> {
    let md_doc = get_content_document(doc)?;
    let heading_regex = Regex::new(heading_pattern)
        .map_err(|error| format!("invalid regex '{heading_pattern}': {error}"))?;

    // Find the section with matching heading
    let section = md_doc.sections.iter().find(|section| {
        section
            .heading
            .as_ref()
            .is_some_and(|heading| heading_regex.is_match(&heading.text))
    });

    match section {
        Some(section) => {
            let line_count = section_body_lines(section)
                .filter(|line| !line.trim().is_empty())
                .count();

            if line_count >= min_lines as usize {
                Ok(())
            } else {
                Err(format!(
                    "Section has {} non-blank lines, expected at least {}",
                    line_count, min_lines
                ))
            }
        }
        None => Err(format!(
            "heading not found: no section matches '{}'",
            heading_pattern
        )),
    }
}

fn section_body_lines(section: &crate::document::markdown::Section) -> impl Iterator<Item = &str> {
    let mut lines = section.content.lines();
    if section.heading.is_some() {
        let _ = lines.next();
    }
    lines
}

fn evaluate_table_exists(
    doc: &Document,
    heading_pattern: &str,
    index: Option<usize>,
) -> Result<(), String> {
    let _ = find_table(doc, heading_pattern, index)?;
    Ok(())
}

fn evaluate_table_columns(
    doc: &Document,
    heading_pattern: &str,
    index: Option<usize>,
    patterns: &[String],
) -> Result<(), String> {
    let table = find_table(doc, heading_pattern, index)?;
    if table.headers.len() < patterns.len() {
        return Err(format!(
            "table has {} columns but {} patterns were provided",
            table.headers.len(),
            patterns.len()
        ));
    }

    for (column_index, pattern) in patterns.iter().enumerate() {
        let regex =
            Regex::new(pattern).map_err(|error| format!("invalid regex '{pattern}': {error}"))?;
        let header = &table.headers[column_index];
        if !regex.is_match(header) {
            return Err(format!(
                "column {} header '{}' does not match '{}'",
                column_index, header, pattern
            ));
        }
    }
    Ok(())
}

fn evaluate_table_shape(
    doc: &Document,
    heading_pattern: &str,
    index: Option<usize>,
    min_columns: usize,
    column_types: &[String],
) -> Result<(), String> {
    let table = find_table(doc, heading_pattern, index)?;
    if table.headers.len() < min_columns {
        return Err(format!(
            "table has {} columns, expected at least {}",
            table.headers.len(),
            min_columns
        ));
    }

    if table.headers.len() < column_types.len() {
        return Err(format!(
            "table has {} columns but {} expected types were provided",
            table.headers.len(),
            column_types.len()
        ));
    }

    for (column_index, expected_type) in column_types.iter().enumerate() {
        let inferred = infer_column_type(table, column_index);
        if !column_type_matches(expected_type, &inferred) {
            return Err(format!(
                "column {} inferred as '{}' but expected '{}'",
                column_index, inferred, expected_type
            ));
        }
    }

    Ok(())
}

fn evaluate_table_min_rows(
    doc: &Document,
    heading_pattern: &str,
    index: Option<usize>,
    min_rows: u64,
) -> Result<(), String> {
    let table = find_table(doc, heading_pattern, index)?;
    let row_count = table.rows.len() as u64;
    if row_count >= min_rows {
        Ok(())
    } else {
        Err(format!(
            "table has {} rows, expected at least {}",
            row_count, min_rows
        ))
    }
}

fn find_table<'a>(
    doc: &'a Document,
    heading_pattern: &str,
    index: Option<usize>,
) -> Result<&'a crate::document::markdown::Table, String> {
    let md_doc = get_content_document(doc)?;
    let heading_regex = Regex::new(heading_pattern)
        .map_err(|error| format!("invalid regex '{heading_pattern}': {error}"))?;
    let expected_index = index.unwrap_or(0);

    let matching_tables: Vec<&crate::document::markdown::Table> = md_doc
        .tables
        .iter()
        .filter(|table| {
            table
                .heading_ref
                .as_ref()
                .is_some_and(|heading| heading_regex.is_match(heading))
        })
        .collect();

    matching_tables.get(expected_index).copied().ok_or_else(|| {
        format!(
            "table not found for heading '{}' at index {}",
            heading_pattern, expected_index
        )
    })
}

fn infer_column_type(table: &crate::document::markdown::Table, column_index: usize) -> String {
    let mut number = 0usize;
    let mut currency = 0usize;
    let mut percentage = 0usize;
    let mut date = 0usize;
    let mut string = 0usize;
    let mut non_empty = 0usize;

    for row in &table.rows {
        let value = row.get(column_index).map_or("", String::as_str);
        match infer_cell_type(value) {
            "number" => {
                number += 1;
                non_empty += 1;
            }
            "currency" => {
                currency += 1;
                non_empty += 1;
            }
            "percentage" => {
                percentage += 1;
                non_empty += 1;
            }
            "date" => {
                date += 1;
                non_empty += 1;
            }
            "empty" => {}
            _ => {
                string += 1;
                non_empty += 1;
            }
        }
    }

    if non_empty == 0 {
        return "empty".to_owned();
    }

    let majority = (non_empty / 2) + 1;
    if currency >= majority {
        return "currency".to_owned();
    }
    if number >= majority {
        return "number".to_owned();
    }
    if percentage >= majority {
        return "percentage".to_owned();
    }
    if date >= majority {
        return "date".to_owned();
    }
    if string >= majority {
        return "string".to_owned();
    }

    // No strict majority over non-empty cells.
    "string".to_owned()
}

fn infer_cell_type(value: &str) -> &'static str {
    let normalized = normalize_markdown_cell(value);
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        return "empty";
    }

    if is_percentage(trimmed) {
        return "percentage";
    }
    if is_currency(trimmed) {
        return "currency";
    }
    if is_number(trimmed) {
        return "number";
    }
    if is_date(trimmed) {
        return "date";
    }

    "string"
}

fn column_type_matches(expected: &str, inferred: &str) -> bool {
    if expected.eq_ignore_ascii_case(inferred) {
        return true;
    }
    (expected.eq_ignore_ascii_case("number") && inferred.eq_ignore_ascii_case("currency"))
        || (expected.eq_ignore_ascii_case("currency") && inferred.eq_ignore_ascii_case("number"))
}

fn normalize_markdown_cell(value: &str) -> String {
    let mut text = value.trim().to_owned();

    for marker in ["**", "__", "`", "*", "_"] {
        if text.starts_with(marker) && text.ends_with(marker) && text.len() >= marker.len() * 2 {
            let start = marker.len();
            let end = text.len() - marker.len();
            text = text[start..end].trim().to_owned();
        }
    }

    text
}

fn is_number(value: &str) -> bool {
    let cleaned = value.replace(',', "");
    cleaned.parse::<f64>().is_ok()
}

fn is_currency(value: &str) -> bool {
    if !value.contains('$') {
        return false;
    }
    let cleaned = value.replace(['$', ','], "");
    cleaned.parse::<f64>().is_ok()
}

fn is_percentage(value: &str) -> bool {
    if !value.ends_with('%') {
        return false;
    }
    let core = value[..value.len() - 1].trim().replace(',', "");
    core.parse::<f64>().is_ok()
}

fn is_date(value: &str) -> bool {
    const FORMATS: [&str; 6] = [
        "%Y-%m-%d",
        "%m/%d/%Y",
        "%m/%d/%y",
        "%B %d, %Y",
        "%b %d, %Y",
        "%d-%b-%Y",
    ];
    FORMATS
        .iter()
        .any(|format| NaiveDate::parse_from_str(value, format).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{CsvDocument, PdfDocument, RawDocument};
    use lopdf::{Document as LopdfDocument, Object, Stream, dictionary};
    use std::fs;
    use tempfile::NamedTempFile;

    fn csv_document(contents: &str) -> Document {
        let file = NamedTempFile::with_suffix(".csv").expect("create csv temp file");
        fs::write(file.path(), contents).expect("write csv fixture");
        let (_persisted_file, path) = file.keep().expect("persist csv fixture");
        Document::Csv(CsvDocument { path })
    }

    fn pdf_document(page_count: usize, metadata: &[(&str, &str)]) -> Document {
        let file = NamedTempFile::with_suffix(".pdf").expect("create pdf temp file");
        write_pdf_fixture(file.path(), page_count, metadata);
        let (_persisted_file, path) = file.keep().expect("persist pdf fixture");
        let pdf_doc = PdfDocument::open(&path, None).expect("open pdf fixture");
        Document::Pdf(pdf_doc)
    }

    fn write_pdf_fixture(path: &Path, page_count: usize, metadata: &[(&str, &str)]) {
        let mut document = LopdfDocument::with_version("1.5");
        let pages_id = document.new_object_id();
        let mut kids = Vec::new();

        for _ in 0..page_count {
            let page_id = document.new_object_id();
            let content_id = document.new_object_id();
            document.objects.insert(
                content_id,
                Object::Stream(Stream::new(dictionary! {}, Vec::new())),
            );
            document.objects.insert(
                page_id,
                Object::Dictionary(dictionary! {
                    "Type" => "Page",
                    "Parent" => pages_id,
                    "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
                    "Contents" => content_id,
                }),
            );
            kids.push(page_id.into());
        }

        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => kids,
                "Count" => page_count as i64,
            }),
        );

        let catalog_id = document.new_object_id();
        document.objects.insert(
            catalog_id,
            Object::Dictionary(dictionary! {
                "Type" => "Catalog",
                "Pages" => pages_id,
            }),
        );
        document.trailer.set("Root", catalog_id);

        let info_id = document.new_object_id();
        let mut info = lopdf::Dictionary::new();
        for (key, value) in metadata {
            info.set(*key, Object::string_literal(*value));
        }
        document.objects.insert(info_id, Object::Dictionary(info));
        document.trailer.set("Info", info_id);

        document.compress();
        document
            .save(path)
            .unwrap_or_else(|error| panic!("save pdf fixture '{}': {error}", path.display()));
    }

    #[test]
    fn filename_regex_passes_when_basename_matches() {
        let doc = csv_document("a,b\nx,y\n");
        let result = evaluate(
            &Assertion::FilenameRegex {
                pattern: r".*\.csv$".to_owned(),
            },
            &doc,
        );

        assert!(result.passed, "{result:?}");
        assert_eq!(result.name, "filename_regex");
        assert_eq!(result.detail, None);
    }

    #[test]
    fn sheet_exists_uses_csv_virtual_sheet_names() {
        let doc = csv_document("a,b\nx,y\n");
        let result = evaluate(&Assertion::SheetExists("Sheet1".to_owned()), &doc);
        assert!(result.passed, "{result:?}");
    }

    #[test]
    fn sheet_name_regex_matches_csv_virtual_sheet() {
        let doc = csv_document("a,b\nx,y\n");
        let result = evaluate(
            &Assertion::SheetNameRegex {
                pattern: "(?i)^sheet1$".to_owned(),
                bind: None,
            },
            &doc,
        );
        assert!(result.passed);
    }

    #[test]
    fn sheet_name_regex_returns_first_match_when_multiple_names_match() {
        let doc = csv_document("a,b\nx,y\n");
        let matched = evaluate_sheet_name_regex(&doc, "(?i)sheet1|csv").expect("sheet name match");
        assert_eq!(matched, "Sheet1");
    }

    #[test]
    fn sheet_binding_resolves_for_downstream_assertions() {
        let doc = csv_document("metric,value\ncap_rate,6.25%\n");
        let assertions = vec![
            NamedAssertion {
                name: Some("bind_sheet".to_owned()),
                assertion: Assertion::SheetNameRegex {
                    pattern: "(?i)^sheet1$".to_owned(),
                    bind: Some("$watl_sheet".to_owned()),
                },
            },
            NamedAssertion {
                name: Some("bound_cell".to_owned()),
                assertion: Assertion::CellEq {
                    sheet: "$watl_sheet".to_owned(),
                    cell: "A1".to_owned(),
                    value: "metric".to_owned(),
                },
            },
        ];

        let results = evaluate_named_assertions(&assertions, &doc);
        assert_eq!(results.len(), 2);
        assert!(results[0].passed);
        assert!(results[1].passed);
    }

    #[test]
    fn unresolved_sheet_binding_fails_with_clear_message() {
        let doc = csv_document("metric,value\ncap_rate,6.25%\n");
        let assertions = vec![NamedAssertion {
            name: Some("missing_binding".to_owned()),
            assertion: Assertion::CellEq {
                sheet: "$missing_sheet".to_owned(),
                cell: "A1".to_owned(),
                value: "metric".to_owned(),
            },
        }];

        let results = evaluate_named_assertions(&assertions, &doc);
        assert_eq!(results.len(), 1);
        assert!(!results[0].passed);
        assert!(
            results[0]
                .detail
                .as_deref()
                .expect("detail")
                .contains("sheet binding '$missing_sheet' was not found")
        );
    }

    #[test]
    fn cell_eq_validates_cell_contents() {
        let doc = csv_document("metric,value\ncap_rate,6.25%\n");
        let result = evaluate(
            &Assertion::CellEq {
                sheet: "Sheet1".to_owned(),
                cell: "B2".to_owned(),
                value: "6.25%".to_owned(),
            },
            &doc,
        );
        assert!(result.passed);
    }

    #[test]
    fn cell_regex_validates_pattern() {
        let doc = csv_document("metric,value\ncap_rate,6.25%\n");
        let result = evaluate(
            &Assertion::CellRegex {
                sheet: "Sheet1".to_owned(),
                cell: "B2".to_owned(),
                pattern: r"^\d+\.\d+%$".to_owned(),
            },
            &doc,
        );
        assert!(result.passed);
    }

    #[test]
    fn range_non_null_fails_for_empty_cells() {
        let doc = csv_document("a,b,c\nx,,z\n");
        let result = evaluate(
            &Assertion::RangeNonNull {
                sheet: "Sheet1".to_owned(),
                range: "A1:C2".to_owned(),
            },
            &doc,
        );
        assert!(!result.passed, "{result:?}");
        assert!(
            result
                .detail
                .as_deref()
                .expect("failure detail")
                .contains("B2")
        );
    }

    #[test]
    fn sheet_min_rows_checks_non_empty_rows() {
        let doc = csv_document("a,b\nx,y\nz,w\n");
        let result = evaluate(
            &Assertion::SheetMinRows {
                sheet: "Sheet1".to_owned(),
                min_rows: 3,
            },
            &doc,
        );
        assert!(result.passed);
    }

    #[test]
    fn column_search_finds_pattern_in_row_range() {
        let doc = csv_document("header\npreface\nCREFC Investor Reporting Package\nvalue\n");
        let result = evaluate(
            &Assertion::ColumnSearch {
                sheet: "Sheet1".to_owned(),
                column: "A".to_owned(),
                row_range: "1:5".to_owned(),
                pattern: "(?i)crefc investor reporting".to_owned(),
            },
            &doc,
        );
        assert!(result.passed);
    }

    #[test]
    fn column_search_fails_when_no_cells_match() {
        let doc = csv_document("header\npreface\nvalue\n");
        let result = evaluate(
            &Assertion::ColumnSearch {
                sheet: "Sheet1".to_owned(),
                column: "A".to_owned(),
                row_range: "1:3".to_owned(),
                pattern: "(?i)crefc".to_owned(),
            },
            &doc,
        );
        assert!(!result.passed);
        assert!(
            result
                .detail
                .as_deref()
                .expect("detail")
                .contains("no cell in column A rows 1:3 matched")
        );
    }

    #[test]
    fn column_search_diagnose_context_reports_scanned_cells() {
        let doc = csv_document("header\npreface\nvalue\n");
        let result = evaluate_with_diagnose(
            &Assertion::ColumnSearch {
                sheet: "Sheet1".to_owned(),
                column: "A".to_owned(),
                row_range: "1:3".to_owned(),
                pattern: "(?i)crefc".to_owned(),
            },
            &doc,
            true,
        );
        assert!(!result.passed);
        let context = result.context.expect("diagnostic context");
        assert_eq!(context["sheet"], "Sheet1");
        assert_eq!(context["row_range"], "1:3");
        assert_eq!(context["scanned_cells"].as_array().map(Vec::len), Some(3));
    }

    #[test]
    fn column_search_supports_bound_sheet_names() {
        let doc = csv_document("header\nCREFC Investor Reporting Package\nvalue\n");
        let assertions = vec![
            NamedAssertion {
                name: Some("bind_sheet".to_owned()),
                assertion: Assertion::SheetNameRegex {
                    pattern: "(?i)^sheet1$".to_owned(),
                    bind: Some("$sheet_ref".to_owned()),
                },
            },
            NamedAssertion {
                name: Some("search_bound".to_owned()),
                assertion: Assertion::ColumnSearch {
                    sheet: "$sheet_ref".to_owned(),
                    column: "A".to_owned(),
                    row_range: "1:3".to_owned(),
                    pattern: "(?i)crefc".to_owned(),
                },
            },
        ];

        let results = evaluate_named_assertions(&assertions, &doc);
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|result| result.passed), "{results:?}");
    }

    #[test]
    fn header_row_match_passes_when_threshold_is_met() {
        let doc = csv_document(
            "preface\nmetadata\nTransaction ID,Loan ID,Property Name,Comments - Servicer Watchlist\n1,2,Main Plaza,On watch\n",
        );
        let result = evaluate(
            &Assertion::HeaderRowMatch {
                sheet: "Sheet1".to_owned(),
                row_range: "1:5".to_owned(),
                min_match: 3,
                columns: vec![
                    ColumnPattern {
                        pattern: "(?i)transaction\\s*id|^L1$".to_owned(),
                    },
                    ColumnPattern {
                        pattern: "(?i)loan\\s*id|^L3$".to_owned(),
                    },
                    ColumnPattern {
                        pattern: "(?i)property\\s*name|^S55$".to_owned(),
                    },
                    ColumnPattern {
                        pattern: "(?i)comments.*watchlist|^19$".to_owned(),
                    },
                ],
            },
            &doc,
        );
        assert!(result.passed);
    }

    #[test]
    fn header_row_match_fails_below_threshold_and_reports_best_row_in_diagnose() {
        let doc = csv_document(
            "preface\nmetadata\nTransaction ID,Loan ID,Property Name\n1,2,Main Plaza\n",
        );
        let result = evaluate_with_diagnose(
            &Assertion::HeaderRowMatch {
                sheet: "Sheet1".to_owned(),
                row_range: "1:5".to_owned(),
                min_match: 4,
                columns: vec![
                    ColumnPattern {
                        pattern: "(?i)transaction\\s*id|^L1$".to_owned(),
                    },
                    ColumnPattern {
                        pattern: "(?i)loan\\s*id|^L3$".to_owned(),
                    },
                    ColumnPattern {
                        pattern: "(?i)property\\s*name|^S55$".to_owned(),
                    },
                    ColumnPattern {
                        pattern: "(?i)comments.*watchlist|^19$".to_owned(),
                    },
                ],
            },
            &doc,
            true,
        );
        assert!(!result.passed, "{result:?}");
        assert!(
            result
                .detail
                .as_deref()
                .expect("detail")
                .contains("best row was 3 with 3 matches")
        );

        let context = result.context.expect("diagnostic context");
        assert_eq!(context["best_row"], 3);
        assert_eq!(context["best_match_count"], 3);
    }

    #[test]
    fn header_row_match_supports_bound_sheet_names() {
        let doc = csv_document(
            "preface\nmetadata\nTransaction ID,Loan ID,Property Name,Comments - Servicer Watchlist\n",
        );
        let assertions = vec![
            NamedAssertion {
                name: Some("bind_sheet".to_owned()),
                assertion: Assertion::SheetNameRegex {
                    pattern: "(?i)^sheet1$".to_owned(),
                    bind: Some("$watl_sheet".to_owned()),
                },
            },
            NamedAssertion {
                name: Some("header_match".to_owned()),
                assertion: Assertion::HeaderRowMatch {
                    sheet: "$watl_sheet".to_owned(),
                    row_range: "1:4".to_owned(),
                    min_match: 3,
                    columns: vec![
                        ColumnPattern {
                            pattern: "(?i)transaction\\s*id|^L1$".to_owned(),
                        },
                        ColumnPattern {
                            pattern: "(?i)loan\\s*id|^L3$".to_owned(),
                        },
                        ColumnPattern {
                            pattern: "(?i)property\\s*name|^S55$".to_owned(),
                        },
                    ],
                },
            },
        ];

        let results = evaluate_named_assertions(&assertions, &doc);
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|result| result.passed), "{results:?}");
    }

    #[test]
    fn unsupported_assertions_fail_with_clear_message() {
        let doc = csv_document("a,b\nx,y\n");
        let result = evaluate(
            &Assertion::TableShape {
                heading: "(?i)rent roll".to_owned(),
                index: Some(0),
                min_columns: 4,
                column_types: vec!["string".to_owned(), "number".to_owned()],
            },
            &doc,
        );
        assert!(!result.passed);
        assert_eq!(result.name, "table_shape");
        assert!(
            result
                .detail
                .as_deref()
                .expect("failure detail")
                .contains("require markdown or pdf with text_path")
        );
    }

    #[test]
    fn spreadsheet_assertions_fail_for_non_spreadsheet_document() {
        let raw = Document::Unknown(RawDocument {
            path: Path::new("fixture.bin").to_path_buf(),
            bytes: vec![1, 2, 3],
        });
        let result = evaluate(&Assertion::SheetExists("Sheet1".to_owned()), &raw);
        assert!(!result.passed);
        assert!(
            result
                .detail
                .as_deref()
                .expect("failure detail")
                .contains("xlsx or csv")
        );
    }

    #[test]
    fn heading_exists_works_with_markdown() {
        use crate::document::MarkdownDocument;
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut file = NamedTempFile::with_suffix(".md").expect("create markdown temp file");
        file.write_all(b"# Test Heading\n\nSome content")
            .expect("write markdown");
        file.flush().expect("flush file");

        let md_doc = MarkdownDocument::open(file.path()).expect("open markdown");
        let doc = Document::Markdown(md_doc);

        let result = evaluate(&Assertion::HeadingExists("Test Heading".to_string()), &doc);
        assert!(result.passed);

        let result = evaluate(&Assertion::HeadingExists("Test".to_string()), &doc);
        assert!(!result.passed);

        let result = evaluate(&Assertion::HeadingExists("Missing".to_string()), &doc);
        assert!(!result.passed);
        assert!(result.detail.as_ref().unwrap().contains("not found"));
    }

    #[test]
    fn text_contains_works_with_text_document() {
        use crate::document::TextDocument;
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut file = NamedTempFile::with_suffix(".txt").expect("create text temp file");
        file.write_all(b"This is a test document with some text")
            .expect("write text");
        file.flush().expect("flush file");

        let text_doc = TextDocument::open(file.path()).expect("open text");
        let doc = Document::Text(text_doc);

        let result = evaluate(&Assertion::TextContains("test document".to_string()), &doc);
        assert!(result.passed);

        let result = evaluate(&Assertion::TextContains("missing".to_string()), &doc);
        assert!(!result.passed);
    }

    #[test]
    fn text_regex_works_with_markdown() {
        use crate::document::MarkdownDocument;
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut file = NamedTempFile::with_suffix(".md").expect("create markdown temp file");
        file.write_all(b"# Heading\n\nThe rate is 6.25% annually")
            .expect("write markdown");
        file.flush().expect("flush file");

        let md_doc = MarkdownDocument::open(file.path()).expect("open markdown");
        let doc = Document::Markdown(md_doc);

        let result = evaluate(
            &Assertion::TextRegex {
                pattern: r"\d+\.\d+%".to_string(),
            },
            &doc,
        );
        assert!(result.passed);

        let result = evaluate(
            &Assertion::TextRegex {
                pattern: r"missing_pattern".to_string(),
            },
            &doc,
        );
        assert!(!result.passed);
    }

    #[test]
    fn pdf_content_assertions_fail_without_text_path() {
        use crate::document::PdfDocument;
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut pdf_file = NamedTempFile::with_suffix(".pdf").expect("create pdf temp file");
        pdf_file
            .write_all(b"%PDF-1.4\nPDF content")
            .expect("write pdf");
        pdf_file.flush().expect("flush file");

        let pdf_doc = PdfDocument::open(pdf_file.path(), None).expect("open pdf");
        let doc = Document::Pdf(pdf_doc);

        let result = evaluate(&Assertion::HeadingExists("Test".to_string()), &doc);
        assert!(!result.passed);
        assert!(result.detail.as_ref().unwrap().contains("E_NO_TEXT"));

        let result = evaluate(&Assertion::TextContains("test".to_string()), &doc);
        assert!(!result.passed);
        assert!(result.detail.as_ref().unwrap().contains("E_NO_TEXT"));

        let result = evaluate(
            &Assertion::TableExists {
                heading: "(?i)rent roll".to_string(),
                index: Some(0),
            },
            &doc,
        );
        assert!(!result.passed);
        assert!(result.detail.as_ref().unwrap().contains("E_NO_TEXT"));
    }

    #[test]
    fn pdf_content_assertions_work_with_text_path() {
        use crate::document::PdfDocument;
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut pdf_file = NamedTempFile::with_suffix(".pdf").expect("create pdf temp file");
        pdf_file
            .write_all(b"%PDF-1.4\nPDF content")
            .expect("write pdf");
        pdf_file.flush().expect("flush file");

        let mut md_file = NamedTempFile::with_suffix(".md").expect("create markdown temp file");
        md_file
            .write_all(b"# Extracted Heading\n\nExtracted text content")
            .expect("write markdown");
        md_file.flush().expect("flush file");

        let pdf_doc = PdfDocument::open(pdf_file.path(), Some(md_file.path())).expect("open pdf");
        let doc = Document::Pdf(pdf_doc);

        let result = evaluate(
            &Assertion::HeadingExists("Extracted Heading".to_string()),
            &doc,
        );
        assert!(result.passed);

        let result = evaluate(&Assertion::TextContains("Extracted text".to_string()), &doc);
        assert!(result.passed);
    }

    #[test]
    fn heading_assertions_fail_on_text_format() {
        use crate::document::TextDocument;
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut file = NamedTempFile::with_suffix(".txt").expect("create text temp file");
        file.write_all(b"Plain text without heading structure")
            .expect("write text");
        file.flush().expect("flush file");

        let text_doc = TextDocument::open(file.path()).expect("open text");
        let doc = Document::Text(text_doc);

        let result = evaluate(&Assertion::HeadingExists("Test".to_string()), &doc);
        assert!(!result.passed);
        assert!(
            result
                .detail
                .as_ref()
                .unwrap()
                .contains("text format has no heading")
        );
    }

    #[test]
    fn page_count_reads_pdf_structure() {
        let doc = pdf_document(2, &[("Creator", "CBRE Producer")]);
        let result = evaluate(
            &Assertion::PageCount {
                min: Some(1),
                max: Some(3),
            },
            &doc,
        );
        assert!(result.passed);
    }

    #[test]
    fn page_count_fails_when_outside_range() {
        let doc = pdf_document(2, &[("Creator", "CBRE Producer")]);
        let result = evaluate(
            &Assertion::PageCount {
                min: Some(3),
                max: Some(10),
            },
            &doc,
        );
        assert!(!result.passed);
        assert!(result.detail.as_ref().unwrap().contains("at least 3"));
    }

    #[test]
    fn metadata_regex_reads_info_dictionary() {
        let doc = pdf_document(
            1,
            &[("Creator", "CBRE Valuation Engine"), ("Producer", "lopdf")],
        );
        let result = evaluate(
            &Assertion::MetadataRegex {
                key: "Creator".to_string(),
                pattern: "(?i)cbre".to_string(),
            },
            &doc,
        );
        assert!(result.passed);
    }

    #[test]
    fn metadata_regex_fails_when_key_missing_or_value_mismatch() {
        let doc = pdf_document(1, &[("Creator", "CBRE Valuation Engine")]);
        let missing_key = evaluate(
            &Assertion::MetadataRegex {
                key: "Author".to_string(),
                pattern: ".+".to_string(),
            },
            &doc,
        );
        assert!(!missing_key.passed);
        assert!(missing_key.detail.as_ref().unwrap().contains("not found"));

        let mismatch = evaluate(
            &Assertion::MetadataRegex {
                key: "Creator".to_string(),
                pattern: "(?i)jll".to_string(),
            },
            &doc,
        );
        assert!(!mismatch.passed);
        assert!(mismatch.detail.as_ref().unwrap().contains("does not match"));
    }

    #[test]
    fn text_near_supports_text_and_pdf_markdown_sources() {
        use crate::document::{PdfDocument, TextDocument};
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut text_file = NamedTempFile::with_suffix(".txt").expect("create text temp file");
        text_file
            .write_all(b"The capitalization rate is 5.25% based on current NOI.")
            .expect("write text");
        text_file.flush().expect("flush file");
        let text_doc = TextDocument::open(text_file.path()).expect("open text");
        let text_result = evaluate(
            &Assertion::TextNear {
                anchor: "(?i)capitalization rate".to_string(),
                pattern: r"\d+\.\d+%".to_string(),
                within_chars: 20,
            },
            &Document::Text(text_doc),
        );
        assert!(text_result.passed);

        let mut pdf_file = NamedTempFile::with_suffix(".pdf").expect("create pdf temp file");
        pdf_file
            .write_all(b"%PDF-1.4\nPDF content")
            .expect("write pdf");
        pdf_file.flush().expect("flush file");

        let mut md_file = NamedTempFile::with_suffix(".md").expect("create markdown temp file");
        md_file
            .write_all(b"# Extracted\n\nThe capitalization rate is 4.90% for this deal.")
            .expect("write markdown");
        md_file.flush().expect("flush file");

        let pdf_doc = PdfDocument::open(pdf_file.path(), Some(md_file.path())).expect("open pdf");
        let pdf_result = evaluate(
            &Assertion::TextNear {
                anchor: "(?i)capitalization rate".to_string(),
                pattern: r"\d+\.\d+%".to_string(),
                within_chars: 20,
            },
            &Document::Pdf(pdf_doc),
        );
        assert!(pdf_result.passed);
    }

    #[test]
    fn pdf_structural_assertions_do_not_require_text_path() {
        use crate::document::PdfDocument;
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut pdf_file = NamedTempFile::with_suffix(".pdf").expect("create pdf temp file");
        pdf_file
            .write_all(b"%PDF-1.4\nPDF content")
            .expect("write pdf");
        pdf_file.flush().expect("flush file");

        let pdf_doc = PdfDocument::open(pdf_file.path(), None).expect("open pdf");
        let result = evaluate(
            &Assertion::PageCount {
                min: Some(1),
                max: Some(10),
            },
            &Document::Pdf(pdf_doc),
        );
        assert!(!result.passed);
        assert!(!result.detail.as_ref().unwrap().contains("E_NO_TEXT"));
    }

    #[test]
    fn text_near_is_bidirectional() {
        use crate::document::TextDocument;
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut file = NamedTempFile::with_suffix(".txt").expect("create text temp file");
        file.write_all(b"6.25% overall capitalization rate for the deal")
            .expect("write text");
        file.flush().expect("flush file");

        let text_doc = TextDocument::open(file.path()).expect("open text");
        let result = evaluate(
            &Assertion::TextNear {
                anchor: "(?i)capitalization rate".to_string(),
                pattern: r"\d+\.\d+%".to_string(),
                within_chars: 25,
            },
            &Document::Text(text_doc),
        );
        assert!(result.passed);
    }

    #[test]
    fn text_near_passes_when_any_anchor_occurrence_matches() {
        use crate::document::TextDocument;
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut file = NamedTempFile::with_suffix(".txt").expect("create text temp file");
        file.write_all(
            b"capitalization rate unknown\nother text\ncapitalization rate is 5.10% now",
        )
        .expect("write text");
        file.flush().expect("flush file");

        let text_doc = TextDocument::open(file.path()).expect("open text");
        let result = evaluate(
            &Assertion::TextNear {
                anchor: "(?i)capitalization rate".to_string(),
                pattern: r"\d+\.\d+%".to_string(),
                within_chars: 20,
            },
            &Document::Text(text_doc),
        );
        assert!(result.passed);
    }

    #[test]
    fn text_near_ignores_short_whitespace_only_gaps() {
        use crate::document::TextDocument;
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut file = NamedTempFile::with_suffix(".txt").expect("create text temp file");
        file.write_all(b"capitalization rate\n   \n5.00%")
            .expect("write text");
        file.flush().expect("flush file");

        let text_doc = TextDocument::open(file.path()).expect("open text");
        let result = evaluate(
            &Assertion::TextNear {
                anchor: "(?i)capitalization rate".to_string(),
                pattern: r"\d+\.\d+%".to_string(),
                within_chars: 0,
            },
            &Document::Text(text_doc),
        );
        assert!(result.passed);
    }

    #[test]
    fn section_non_empty_works_with_content() {
        use crate::document::MarkdownDocument;
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut file = NamedTempFile::with_suffix(".md").expect("create markdown temp file");
        file.write_all(b"# Main Heading\n\nSome content.\n\n# Empty Section\n\n\n\n# Another Section\n\nMore content here.").expect("write markdown");
        file.flush().expect("flush file");

        let md_doc = MarkdownDocument::open(file.path()).expect("open markdown");
        let doc = Document::Markdown(md_doc);

        // Test with section that has content
        let result = evaluate(
            &Assertion::SectionNonEmpty {
                heading: "Main Heading".to_string(),
            },
            &doc,
        );
        assert!(result.passed);

        // Test with empty section
        let result = evaluate(
            &Assertion::SectionNonEmpty {
                heading: "Empty Section".to_string(),
            },
            &doc,
        );
        assert!(!result.passed);
        assert!(result.detail.as_ref().unwrap().contains("empty"));

        // Test with non-existent section
        let result = evaluate(
            &Assertion::SectionNonEmpty {
                heading: "Non-existent".to_string(),
            },
            &doc,
        );
        assert!(!result.passed);
        assert!(
            result
                .detail
                .as_ref()
                .unwrap()
                .contains("heading not found")
        );
    }

    #[test]
    fn section_min_lines_counts_non_blank_lines() {
        use crate::document::MarkdownDocument;
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut file = NamedTempFile::with_suffix(".md").expect("create markdown temp file");
        file.write_all(
            b"# Test Section\n\nLine 1\n\nLine 2\n\n\nLine 3\n\n# Next Section\nContent",
        )
        .expect("write markdown");
        file.flush().expect("flush file");

        let md_doc = MarkdownDocument::open(file.path()).expect("open markdown");
        let doc = Document::Markdown(md_doc);

        // Test with exact line count (3 non-blank lines)
        let result = evaluate(
            &Assertion::SectionMinLines {
                heading: "Test Section".to_string(),
                min_lines: 3,
            },
            &doc,
        );
        assert!(result.passed);

        // Test with fewer required lines
        let result = evaluate(
            &Assertion::SectionMinLines {
                heading: "Test Section".to_string(),
                min_lines: 2,
            },
            &doc,
        );
        assert!(result.passed);

        // Test with more required lines than available
        let result = evaluate(
            &Assertion::SectionMinLines {
                heading: "Test Section".to_string(),
                min_lines: 5,
            },
            &doc,
        );
        assert!(!result.passed);
        assert!(
            result
                .detail
                .as_ref()
                .unwrap()
                .contains("has 3 non-blank lines")
        );
    }

    #[test]
    fn section_assertions_use_regex_for_heading_matching() {
        use crate::document::MarkdownDocument;
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut file = NamedTempFile::with_suffix(".md").expect("create markdown temp file");
        file.write_all(b"# Income Capitalization Approach\n\nCap rate analysis here.")
            .expect("write markdown");
        file.flush().expect("flush file");

        let md_doc = MarkdownDocument::open(file.path()).expect("open markdown");
        let doc = Document::Markdown(md_doc);

        // Test with regex pattern
        let result = evaluate(
            &Assertion::SectionNonEmpty {
                heading: "(?i)income.+cap".to_string(),
            },
            &doc,
        );
        assert!(result.passed);

        // Test with case-insensitive match
        let result = evaluate(
            &Assertion::SectionNonEmpty {
                heading: "(?i)INCOME".to_string(),
            },
            &doc,
        );
        assert!(result.passed);
    }

    #[test]
    fn section_assertions_fail_on_text_documents() {
        use crate::document::TextDocument;
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut file = NamedTempFile::with_suffix(".txt").expect("create text temp file");
        file.write_all(b"Plain text without sections")
            .expect("write text");
        file.flush().expect("flush file");

        let text_doc = TextDocument::open(file.path()).expect("open text");
        let doc = Document::Text(text_doc);

        let result = evaluate(
            &Assertion::SectionNonEmpty {
                heading: "Test".to_string(),
            },
            &doc,
        );
        assert!(!result.passed);
        assert!(
            result
                .detail
                .as_ref()
                .unwrap()
                .contains("text format has no heading")
        );
    }

    #[test]
    fn evaluate_named_uses_explicit_dsl_name_in_result() {
        use crate::document::TextDocument;
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut file = NamedTempFile::with_suffix(".txt").expect("create text temp file");
        file.write_all(b"cap rate is 5.25%").expect("write text");
        file.flush().expect("flush file");
        let text_doc = TextDocument::open(file.path()).expect("open text");
        let doc = Document::Text(text_doc);

        let named_assertion = NamedAssertion {
            name: Some("cap_rate_marker".to_owned()),
            assertion: Assertion::TextRegex {
                pattern: r"\d+\.\d+%".to_owned(),
            },
        };

        let result = evaluate_named(&named_assertion, &doc);
        assert!(result.passed);
        assert_eq!(result.name, "cap_rate_marker");
    }

    #[test]
    fn table_exists_uses_heading_and_index() {
        use crate::document::MarkdownDocument;
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut file = NamedTempFile::with_suffix(".md").expect("create markdown temp file");
        file.write_all(
            br#"# Rent Roll

| Tenant | SF | Rent |
|---|---|---|
| A | 1000 | $10 |
| B | 900 | $11 |

| Name | Units |
|---|---|
| X | 1 |
"#,
        )
        .expect("write markdown");
        file.flush().expect("flush file");

        let md_doc = MarkdownDocument::open(file.path()).expect("open markdown");
        let doc = Document::Markdown(md_doc);

        let first = evaluate(
            &Assertion::TableExists {
                heading: "(?i)rent roll".to_string(),
                index: None,
            },
            &doc,
        );
        assert!(first.passed);

        let second = evaluate(
            &Assertion::TableExists {
                heading: "(?i)rent roll".to_string(),
                index: Some(1),
            },
            &doc,
        );
        assert!(second.passed);

        let missing = evaluate(
            &Assertion::TableExists {
                heading: "(?i)rent roll".to_string(),
                index: Some(2),
            },
            &doc,
        );
        assert!(!missing.passed);
        assert!(missing.detail.as_ref().unwrap().contains("index 2"));
    }

    #[test]
    fn table_columns_matches_header_patterns() {
        use crate::document::MarkdownDocument;
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut file = NamedTempFile::with_suffix(".md").expect("create markdown temp file");
        file.write_all(
            br#"# Rent Roll

| Tenant Name | Sq. Ft. | Monthly Rent |
|---|---|---|
| A | 1000 | $10 |
"#,
        )
        .expect("write markdown");
        file.flush().expect("flush file");

        let md_doc = MarkdownDocument::open(file.path()).expect("open markdown");
        let doc = Document::Markdown(md_doc);

        let pass = evaluate(
            &Assertion::TableColumns {
                heading: "(?i)rent roll".to_string(),
                index: None,
                patterns: vec![
                    "(?i)tenant".to_string(),
                    "(?i)sq\\.?\\s*ft|sf".to_string(),
                    "(?i)rent".to_string(),
                ],
            },
            &doc,
        );
        assert!(pass.passed);

        let fail = evaluate(
            &Assertion::TableColumns {
                heading: "(?i)rent roll".to_string(),
                index: None,
                patterns: vec![
                    "(?i)tenant".to_string(),
                    "(?i)units".to_string(),
                    "(?i)rent".to_string(),
                ],
            },
            &doc,
        );
        assert!(!fail.passed);
        assert!(fail.detail.as_ref().unwrap().contains("does not match"));
    }

    #[test]
    fn table_min_rows_counts_data_rows() {
        use crate::document::MarkdownDocument;
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut file = NamedTempFile::with_suffix(".md").expect("create markdown temp file");
        file.write_all(
            br#"# Rent Roll

| Tenant | SF |
|---|---|
| A | 1000 |
| B | 900 |
"#,
        )
        .expect("write markdown");
        file.flush().expect("flush file");

        let md_doc = MarkdownDocument::open(file.path()).expect("open markdown");
        let doc = Document::Markdown(md_doc);

        let pass = evaluate(
            &Assertion::TableMinRows {
                heading: "(?i)rent roll".to_string(),
                index: None,
                min_rows: 2,
            },
            &doc,
        );
        assert!(pass.passed);

        let fail = evaluate(
            &Assertion::TableMinRows {
                heading: "(?i)rent roll".to_string(),
                index: None,
                min_rows: 3,
            },
            &doc,
        );
        assert!(!fail.passed);
        assert!(fail.detail.as_ref().unwrap().contains("table has 2 rows"));
    }

    #[test]
    fn table_shape_infers_types_with_majority_and_subtype_rules() {
        use crate::document::MarkdownDocument;
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut file = NamedTempFile::with_suffix(".md").expect("create markdown temp file");
        file.write_all(
            br#"# Rent Roll

| Item | Amount | Rate | As Of |
|---|---|---|---|
| **Base Rent** | $1,200.50 | 5.25% | June 15, 2024 |
| Expense | $950.00 | 4.75% | July 01, 2024 |
| Notes | 1200 | 5.00% | 2024-08-01 |
"#,
        )
        .expect("write markdown");
        file.flush().expect("flush file");

        let md_doc = MarkdownDocument::open(file.path()).expect("open markdown");
        let doc = Document::Markdown(md_doc);

        let pass = evaluate(
            &Assertion::TableShape {
                heading: "(?i)rent roll".to_string(),
                index: None,
                min_columns: 4,
                column_types: vec![
                    "string".to_string(),
                    "number".to_string(),
                    "percentage".to_string(),
                    "date".to_string(),
                ],
            },
            &doc,
        );
        assert!(pass.passed);
    }

    #[test]
    fn table_shape_falls_back_to_string_without_majority() {
        use crate::document::MarkdownDocument;
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut file = NamedTempFile::with_suffix(".md").expect("create markdown temp file");
        file.write_all(
            br#"# Mixed Types

| Value |
|---|
| 100 |
| Alpha |
| 2024-01-01 |
"#,
        )
        .expect("write markdown");
        file.flush().expect("flush file");

        let md_doc = MarkdownDocument::open(file.path()).expect("open markdown");
        let doc = Document::Markdown(md_doc);

        let pass = evaluate(
            &Assertion::TableShape {
                heading: "(?i)mixed types".to_string(),
                index: None,
                min_columns: 1,
                column_types: vec!["string".to_string()],
            },
            &doc,
        );
        assert!(pass.passed);
    }

    #[test]
    fn diagnose_context_is_absent_when_disabled() {
        use crate::document::MarkdownDocument;
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut file = NamedTempFile::with_suffix(".md").expect("create markdown temp file");
        file.write_all(b"# Property Description\n\nBody")
            .expect("write markdown");
        file.flush().expect("flush file");

        let doc = Document::Markdown(MarkdownDocument::open(file.path()).expect("open markdown"));
        let result = evaluate_with_diagnose(
            &Assertion::HeadingRegex {
                pattern: "(?i)rent roll".to_owned(),
            },
            &doc,
            false,
        );
        assert!(!result.passed);
        assert!(result.context.is_none());
    }

    #[test]
    fn diagnose_mode_disables_short_circuit_for_named_assertions() {
        use crate::document::MarkdownDocument;
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut file = NamedTempFile::with_suffix(".md").expect("create markdown temp file");
        file.write_all(b"# Existing Heading\n\nBody")
            .expect("write markdown");
        file.flush().expect("flush file");

        let doc = Document::Markdown(MarkdownDocument::open(file.path()).expect("open markdown"));
        let assertions = vec![
            NamedAssertion {
                name: Some("missing_heading".to_owned()),
                assertion: Assertion::HeadingExists("Missing Heading".to_owned()),
            },
            NamedAssertion {
                name: Some("missing_text".to_owned()),
                assertion: Assertion::TextContains("Missing phrase".to_owned()),
            },
        ];

        let normal = evaluate_named_assertions_with_diagnose(&assertions, &doc, false);
        assert_eq!(normal.len(), 1);
        assert!(!normal[0].passed);
        assert!(normal[0].context.is_none());

        let diagnose = evaluate_named_assertions_with_diagnose(&assertions, &doc, true);
        assert_eq!(diagnose.len(), 2);
        assert!(!diagnose[0].passed);
        assert!(!diagnose[1].passed);
        assert!(diagnose[0].context.is_some());
        assert!(diagnose[1].context.is_some());
    }

    #[test]
    fn diagnose_heading_failure_includes_headings_and_nearest_match() {
        use crate::document::MarkdownDocument;
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut file = NamedTempFile::with_suffix(".md").expect("create markdown temp file");
        file.write_all(
            br#"# Property Description
# Income Approach
# RENT ROLL SUMMARY
"#,
        )
        .expect("write markdown");
        file.flush().expect("flush file");

        let doc = Document::Markdown(MarkdownDocument::open(file.path()).expect("open markdown"));
        let result = evaluate_with_diagnose(
            &Assertion::HeadingRegex {
                pattern: "(?i)rent roll detail".to_owned(),
            },
            &doc,
            true,
        );
        assert!(!result.passed);
        let context = result.context.expect("diagnostic context");
        assert_eq!(context["headings_found"].as_array().map(Vec::len), Some(3));
        assert_eq!(context["nearest_match"], "RENT ROLL SUMMARY");
    }

    #[test]
    fn diagnose_text_failure_includes_partial_matches() {
        use crate::document::TextDocument;
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut file = NamedTempFile::with_suffix(".txt").expect("create text temp file");
        file.write_all(
            b"The capitalization metric is reported monthly.\nDebt service coverage listed separately.",
        )
        .expect("write text");
        file.flush().expect("flush file");

        let doc = Document::Text(TextDocument::open(file.path()).expect("open text"));
        let result = evaluate_with_diagnose(
            &Assertion::TextContains("capitalization ratio".to_owned()),
            &doc,
            true,
        );
        assert!(!result.passed);
        let context = result.context.expect("diagnostic context");
        let partial_matches = context["partial_matches"]
            .as_array()
            .expect("partial matches");
        assert_eq!(partial_matches.len(), 1);
        assert!(
            partial_matches[0]
                .as_str()
                .expect("partial match text")
                .contains("capitalization")
        );
    }

    #[test]
    fn diagnose_text_near_failure_reports_anchor_and_out_of_range_matches() {
        use crate::document::TextDocument;
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut file = NamedTempFile::with_suffix(".txt").expect("create text temp file");
        file.write_all(b"capitalization rate .................................... 6.25%")
            .expect("write text");
        file.flush().expect("flush file");

        let doc = Document::Text(TextDocument::open(file.path()).expect("open text"));
        let result = evaluate_with_diagnose(
            &Assertion::TextNear {
                anchor: "(?i)capitalization rate".to_owned(),
                pattern: r"\d+\.\d+%".to_owned(),
                within_chars: 5,
            },
            &doc,
            true,
        );
        assert!(!result.passed);
        let context = result.context.expect("diagnostic context");
        assert_eq!(context["anchor_found"], true);
        assert_eq!(context["matches_outside_range"][0]["match"], "6.25%");
    }

    #[test]
    fn diagnose_table_failure_includes_heading_and_tables_found() {
        use crate::document::MarkdownDocument;
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut file = NamedTempFile::with_suffix(".md").expect("create markdown temp file");
        file.write_all(
            br#"# Rent Roll

| Tenant | SF |
|---|---|
| A | 1000 |
"#,
        )
        .expect("write markdown");
        file.flush().expect("flush file");

        let doc = Document::Markdown(MarkdownDocument::open(file.path()).expect("open markdown"));
        let result = evaluate_with_diagnose(
            &Assertion::TableExists {
                heading: "(?i)rent roll".to_owned(),
                index: Some(1),
            },
            &doc,
            true,
        );
        assert!(!result.passed);
        let context = result.context.expect("diagnostic context");
        assert_eq!(context["heading_found"], true);
        assert_eq!(context["tables_found"].as_array().map(Vec::len), Some(1));
    }

    #[test]
    fn diagnose_section_failure_includes_section_lines_and_heading_presence() {
        use crate::document::MarkdownDocument;
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut file = NamedTempFile::with_suffix(".md").expect("create markdown temp file");
        file.write_all(
            br#"# Rent Roll

One line.
"#,
        )
        .expect("write markdown");
        file.flush().expect("flush file");

        let doc = Document::Markdown(MarkdownDocument::open(file.path()).expect("open markdown"));
        let result = evaluate_with_diagnose(
            &Assertion::SectionMinLines {
                heading: "(?i)rent roll".to_owned(),
                min_lines: 3,
            },
            &doc,
            true,
        );
        assert!(!result.passed);
        let context = result.context.expect("diagnostic context");
        assert_eq!(context["heading_found"], true);
        assert_eq!(context["section_lines"], 1);
    }
}
