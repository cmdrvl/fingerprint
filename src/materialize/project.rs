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
