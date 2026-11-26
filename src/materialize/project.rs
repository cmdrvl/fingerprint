#![forbid(unsafe_code)]

//! Projections: turn a matched extract section into a clean rectangular table.
//!
//! This module owns only NON-FLAT document extraction (locate the bound sheet and range, emit a
//! raw bounded slice). It deliberately does not normalize: header canonicalization, preamble and
//! units stripping, key scoping and equivalence rules belong to `profile slice`, which this
//! output is meant to be piped into (seam agreed in bd-uai's notes).

use calamine::{Data, Reader, open_workbook_auto};
use serde_json::{Value, json};
use std::path::Path;

/// A rectangular slice with the facts needed to pin where it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct Projection {
    pub headers: Vec<String>,
    /// Data rows, each exactly `headers.len()` cells; an empty string is an empty source cell.
    pub rows: Vec<Vec<String>>,
    /// Exact source locator (`sheet` + A1 range, or HTML table indices and byte offsets).
    pub locator: Value,
    /// What the projection left out, with counts.
    pub skipped: Value,
}

/// Canonical text of one worksheet cell. `None` means the cell cannot be rendered and the
/// projection must refuse (an Excel error value); the caller turns that into a typed refusal
/// naming the cell, never the value.
fn render_cell(cell: &Data) -> Option<String> {
    Some(match cell {
        Data::Empty => String::new(),
        Data::String(text) => text.clone(),
        Data::Bool(value) => value.to_string(),
        Data::Int(value) => value.to_string(),
        Data::Float(value) => render_float(*value),
        Data::DateTime(value) => {
            if value.is_duration() {
                return None;
            }
            let datetime = value.as_datetime()?;
            if datetime.time() == chrono::NaiveTime::MIN {
                datetime.date().format("%Y-%m-%d").to_string()
            } else {
                datetime.format("%Y-%m-%dT%H:%M:%S%.f").to_string()
            }
        }
        Data::DateTimeIso(text) | Data::DurationIso(text) => text.clone(),
        Data::Error(_) => return None,
    })
}

/// Shortest round-trip text; integral values lose the `.0` (2000.0 -> `2000`).
fn render_float(value: f64) -> String {
    if value.is_finite() && value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

fn column_name(index: usize) -> String {
    let mut n = index + 1;
    let mut letters = String::new();
    while n > 0 {
        let rem = (n - 1) % 26;
        letters.insert(0, (b'A' + rem as u8) as char);
        n = (n - 1) / 26;
    }
    letters
}

fn a1(row: usize, col: usize) -> String {
    format!("{}{}", column_name(col), row + 1)
}

/// Project an XLSX/XLS `sheet` + A1 `range` (header-anchored: the first row of the range holds
/// the column headers). Fully empty rows inside the range are skipped and counted; an empty or
/// duplicate header, or an Excel error cell, refuses.
pub fn xlsx_range(path: &Path, sheet: &str, range: &str) -> Result<Projection, String> {
    let ((r0, c0), (r1, c1)) = crate::document::xlsx::parse_range_ref(range)?;
    let mut workbook = open_workbook_auto(path)
        .map_err(|error| format!("failed opening workbook '{}': {error}", path.display()))?;
    let worksheet = workbook
        .worksheet_range(sheet)
        .map_err(|error| format!("failed to read sheet '{sheet}': {error}"))?;

    let cell_text = |row: usize, col: usize| -> Result<String, String> {
        match worksheet.get_value((row as u32, col as u32)) {
            None => Ok(String::new()),
            Some(cell) => render_cell(cell).ok_or_else(|| {
                format!(
                    "cell {} on sheet '{sheet}' holds an Excel error value or an unrepresentable date",
                    a1(row, col)
                )
            }),
        }
    };

    let mut headers = Vec::with_capacity(c1 - c0 + 1);
    for col in c0..=c1 {
        let name = cell_text(r0, col)?.trim().to_owned();
        if name.is_empty() {
            return Err(format!(
                "header cell {} is empty: the first row of range {range} must name every column",
                a1(r0, col)
            ));
        }
        if headers.contains(&name) {
            return Err(format!(
                "header cell {} repeats an earlier column name: headers must be unique",
                a1(r0, col)
            ));
        }
        headers.push(name);
    }

    let mut rows = Vec::new();
    let mut skipped_empty = 0u64;
    for row in (r0 + 1)..=r1 {
        let mut cells = Vec::with_capacity(headers.len());
        for col in c0..=c1 {
            cells.push(cell_text(row, col)?);
        }
        if cells.iter().all(|cell| cell.trim().is_empty()) {
            skipped_empty += 1;
        } else {
            rows.push(cells);
        }
    }

    Ok(Projection {
        headers,
        rows,
        locator: json!({
            "kind": "xlsx_range",
            "sheet": sheet,
            "range": range,
            "header_row": r0 + 1,
            "first_data_row": r0 + 2,
            "last_row": r1 + 1,
        }),
        skipped: json!({ "empty_rows_in_range": skipped_empty }),
    })
}

/// Visible text of an element with all whitespace (including non-breaking spaces) collapsed.
fn collapsed_text(element: scraper::ElementRef<'_>) -> String {
    element
        .text()
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// One table of an HTML region as rows of cell text. `colspan` expands the spanned cells with
/// empty text; `rowspan`, nested tables and a missing row are refused (fail closed) because they
/// cannot be flattened without guessing.
fn html_table_rows(
    table: scraper::ElementRef<'_>,
    table_index: usize,
) -> Result<Vec<Vec<(String, usize)>>, String> {
    let tr = scraper::Selector::parse("tr").map_err(|e| format!("{e:?}"))?;
    let cells = scraper::Selector::parse("th, td").map_err(|e| format!("{e:?}"))?;
    let nested = scraper::Selector::parse("table").map_err(|e| format!("{e:?}"))?;
    if table.select(&nested).next().is_some() {
        return Err(format!(
            "table {table_index} contains a nested table, which cannot be flattened"
        ));
    }
    let mut rows = Vec::new();
    for (row_index, row) in table.select(&tr).enumerate() {
        let mut out = Vec::new();
        for cell in row.select(&cells) {
            if cell
                .value()
                .attr("rowspan")
                .is_some_and(|v| v.trim() != "1")
            {
                return Err(format!(
                    "table {table_index} row {} uses rowspan, which cannot be flattened",
                    row_index + 1
                ));
            }
            let span = cell
                .value()
                .attr("colspan")
                .map(|v| v.trim().parse::<usize>())
                .transpose()
                .map_err(|_| {
                    format!(
                        "table {table_index} row {} has a non-numeric colspan",
                        row_index + 1
                    )
                })?
                .unwrap_or(1)
                .max(1);
            out.push((collapsed_text(cell), span));
        }
        rows.push(out);
    }
    Ok(rows)
}

#[derive(Debug, Clone)]
struct HtmlLayout {
    /// Logical headers and the expanded source columns they own. SEC HTML often uses empty
    /// separator cells between visible columns and splits one rendered value over several cells.
    groups: Vec<(String, std::ops::Range<usize>)>,
    expanded_width: usize,
}

fn expanded_cells(row: &[(String, usize)]) -> Vec<String> {
    row.iter()
        .flat_map(|(text, span)| {
            std::iter::once(text.clone()).chain(std::iter::repeat_n(String::new(), span - 1))
        })
        .collect()
}

/// Recognize the explicit-column layout used by SEC filing HTML. This is deliberately narrower
/// than accepting arbitrary blank headers: at least one `colspan` must be present, and the
/// header names remain the only source of output column names. Blank cells before the first
/// named header are checked against every data row and refuse if they carry data.
fn sec_layout(row: &[(String, usize)]) -> Option<HtmlLayout> {
    if !row.iter().any(|(_, span)| *span > 1) || !row.iter().any(|(text, _)| !text.is_empty()) {
        return None;
    }
    let expanded_width = row.iter().map(|(_, span)| *span).sum::<usize>();
    let mut cursor = 0;
    let mut named = Vec::new();
    for (text, span) in row {
        if !text.is_empty() {
            named.push((text.clone(), cursor));
        }
        cursor += *span;
    }
    let mut groups = Vec::with_capacity(named.len());
    for (index, (name, start)) in named.iter().enumerate() {
        let end = named
            .get(index + 1)
            .map_or(expanded_width, |(_, next_start)| *next_start);
        groups.push((name.clone(), *start..end));
    }
    Some(HtmlLayout {
        groups,
        expanded_width,
    })
}

fn project_sec_layout(
    expanded: &[String],
    layout: &HtmlLayout,
    table_index: usize,
) -> Result<Vec<String>, String> {
    if expanded.len() != layout.expanded_width {
        return Err(format!(
            "table {table_index} row has {} expanded cells, expected {} for the SEC column layout",
            expanded.len(),
            layout.expanded_width
        ));
    }
    let first_start = layout.groups.first().map_or(0, |(_, range)| range.start);
    if expanded[..first_start]
        .iter()
        .any(|cell| !cell.trim().is_empty())
    {
        return Err(format!(
            "table {table_index} has data in unnamed cells before the first named SEC column"
        ));
    }
    Ok(layout
        .groups
        .iter()
        .map(|(_, range)| {
            expanded[range.clone()]
                .iter()
                .filter(|cell| !cell.trim().is_empty())
                .cloned()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect())
}

/// Project an HTML `region` extract: every `<table>` the region covers (`table_indices`, the
/// document-order indices of the matched region), first row = header.
///
/// - Later tables whose first row equals the header (a continued schedule) contribute only
///   their data rows (counted as `repeated_header_rows`); a different header refuses.
/// - A full-width row (one cell spanning every column: an industry or classification label)
///   is not data. It is carried into a leading `_group` column on the rows that follow it, so
///   nothing is dropped and nothing is guessed. The column exists only if the region has one.
/// - Short rows are padded with empty cells and counted; rows wider than the header refuse.
pub fn html_region(
    html: &crate::document::HtmlDocument,
    region: &Value,
) -> Result<Projection, String> {
    let indices: Vec<usize> = region
        .get("table_indices")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|v| v.as_u64().map(|n| n as usize))
                .collect()
        })
        .unwrap_or_default();
    if indices.is_empty() {
        return Err("the region covers no <table>: nothing to project".to_owned());
    }
    let tables = html.select_nodes("table")?;

    let mut header: Option<Vec<String>> = None;
    let mut layout: Option<HtmlLayout> = None;
    let mut grouped: Vec<(Option<String>, Vec<String>)> = Vec::new(); // (group label, cells)
    let (mut repeated_headers, mut padded_rows, mut full_width_rows, mut leading_layout_rows) =
        (0u64, 0u64, 0u64, 0u64);
    let mut current_group: Option<String> = None;

    for &table_index in &indices {
        let table = tables.get(table_index).ok_or_else(|| {
            format!(
                "region names table {table_index} but the document has {} tables",
                tables.len()
            )
        })?;
        let rows = html_table_rows(*table, table_index)?;
        let mut rows = rows.into_iter();
        // SEC's inline-XBRL renderer commonly emits a leading row containing only empty
        // width/layout cells. It is source structure, not a header or data row; skipping only
        // these leading all-empty rows preserves the fail-closed checks for every later row.
        let first_raw: Vec<(String, usize)> = loop {
            let first = rows
                .next()
                .ok_or_else(|| format!("table {table_index} has no non-empty rows"))?;
            let expanded = expanded_cells(&first);
            if expanded.iter().all(|cell| cell.trim().is_empty()) {
                leading_layout_rows += 1;
                continue;
            }
            break first;
        };
        let table_layout = sec_layout(&first_raw);
        let first_cells = match &table_layout {
            Some(table_layout) => table_layout
                .groups
                .iter()
                .map(|(name, _)| name.clone())
                .collect(),
            None => expanded_cells(&first_raw),
        };
        match &header {
            None => {
                if first_cells.iter().any(|name| name.is_empty()) {
                    return Err(format!(
                        "table {table_index}: the first row must name every column (found an empty header cell)"
                    ));
                }
                let mut seen = std::collections::BTreeSet::new();
                if let Some(duplicate) = first_cells.iter().find(|name| !seen.insert(*name)) {
                    let _ = duplicate;
                    return Err(format!(
                        "table {table_index}: the header row repeats a column name; headers must be unique"
                    ));
                }
                header = Some(first_cells);
                layout = table_layout;
            }
            Some(existing) => {
                if &first_cells == existing {
                    repeated_headers += 1;
                } else {
                    return Err(format!(
                        "table {table_index} has a different header row than table {}: a region must be one schedule",
                        indices[0]
                    ));
                }
            }
        }
        let width = header.as_ref().map_or(0, Vec::len);
        let expanded_width = layout
            .as_ref()
            .map_or(width, |table_layout| table_layout.expanded_width);
        for (offset, row) in rows.enumerate() {
            let expanded = expanded_cells(&row);
            let is_full_width =
                row.len() == 1 && row[0].1 >= expanded_width.max(2) && !row[0].0.is_empty();
            if is_full_width {
                current_group = Some(row[0].0.clone());
                full_width_rows += 1;
                continue;
            }
            if expanded.iter().all(|cell| cell.is_empty()) {
                continue;
            }
            if expanded.len() > expanded_width {
                return Err(format!(
                    "table {table_index} row {} has {} cells, wider than the {width}-column header",
                    offset + 2,
                    expanded.len()
                ));
            }
            let mut expanded = expanded;
            if expanded.len() < expanded_width {
                padded_rows += 1;
                expanded.resize(expanded_width, String::new());
            }
            let cells = match &layout {
                Some(layout) => project_sec_layout(&expanded, layout, table_index)?,
                None => expanded,
            };
            grouped.push((current_group.clone(), cells));
        }
    }

    let mut headers = header.unwrap_or_default();
    let has_groups = full_width_rows > 0;
    if has_groups && headers.iter().any(|name| name == "_group") {
        return Err("the header already has a column named _group".to_owned());
    }
    let rows: Vec<Vec<String>> = grouped
        .into_iter()
        .map(|(group, cells)| {
            if has_groups {
                let mut row = vec![group.unwrap_or_default()];
                row.extend(cells);
                row
            } else {
                cells
            }
        })
        .collect();
    if has_groups {
        headers.insert(0, "_group".to_owned());
    }

    let pick = |key: &str| region.get(key).cloned().unwrap_or(Value::Null);
    Ok(Projection {
        headers,
        rows,
        locator: json!({
            "kind": "html_region",
            "table_indices": indices,
            "byte_offsets": pick("byte_offsets"),
            "start_line": pick("start_line"),
            "end_line": pick("end_line"),
            "page_span": pick("page_span"),
        }),
        skipped: json!({
            "leading_layout_rows": leading_layout_rows,
            "repeated_header_rows": repeated_headers,
            "full_width_rows_carried_as_group": full_width_rows,
            "padded_rows": padded_rows,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_render_shortest_and_integral_without_a_fraction() {
        assert_eq!(render_float(2000.0), "2000");
        assert_eq!(render_float(1000.5), "1000.5");
        assert_eq!(render_float(0.045), "0.045");
        assert_eq!(render_float(-3.0), "-3");
        assert_eq!(render_float(1e20), "100000000000000000000");
    }

    #[test]
    fn a1_addresses() {
        assert_eq!(a1(0, 0), "A1");
        assert_eq!(a1(10, 25), "Z11");
        assert_eq!(a1(0, 26), "AA1");
    }
}
