use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader};

use serde_json::{Value, json};

use crate::cli::args::{PeekArgs, PeekView};
use crate::redaction::{Kind, Marker, RedactionKey};
use crate::witness::ledger::{append, ledger_path_for_append};
use crate::witness::record::{WitnessInput, WitnessRecord};

const ENVELOPE_VERSION: &str = "fingerprint.peek.v0";

#[derive(Debug, Clone)]
struct RawRowShape {
    row_index: usize,
    column_count: usize,
    non_empty_count: usize,
    type_counts: BTreeMap<&'static str, usize>,
    length_buckets: BTreeMap<&'static str, usize>,
    has_unit_tokens: bool,
}

/// Longest line kept in memory. A longer line is truncated for shape purposes (its length
/// bucket is `65+` either way) and counted in `truncated_lines`; every byte is still hashed.
const MAX_LINE_BYTES: usize = 1024 * 1024;
/// Non-empty lines the delimiter guess looks at.
const GUESS_LINES: usize = 25;

/// Binary containers peek has no reader for: refused up front as "format not applicable"
/// instead of failing later with a UTF-8 error.
const NOT_APPLICABLE_EXTENSIONS: &[&str] = &["zip", "gz", "docx", "pptx", "png", "jpg", "jpeg"];

/// Workbook containers peek reads through calamine (bd-jxd).
const WORKBOOK_EXTENSIONS: &[&str] = &["xlsx", "xlsm", "xls"];

/// What peek reads from the file: only the lines it reports on, never the whole file.
struct PeekInput {
    /// The first `rows` lines (blank lines included), without line terminators.
    row_lines: Vec<Vec<u8>>,
    /// The first [`GUESS_LINES`] non-blank lines, for the delimiter guess.
    guess_lines: Vec<Vec<u8>>,
    total_bytes: u64,
    /// blake3 of every byte of the file; computed only when a witness record needs it.
    blake3: Option<String>,
    truncated_lines: usize,
}

/// Read one line (up to and including `\n`) keeping at most [`MAX_LINE_BYTES`] of it. Every
/// consumed byte is passed to `consumed` (for hashing). Returns `None` at end of file, else
/// the line without its terminator and whether it was truncated.
fn read_capped_line<R: BufRead>(
    reader: &mut R,
    consumed: &mut dyn FnMut(&[u8]),
) -> std::io::Result<Option<(Vec<u8>, bool)>> {
    let mut line = Vec::new();
    let mut truncated = false;
    let mut saw_any = false;
    loop {
        let (used, done) = {
            let available = reader.fill_buf()?;
            if available.is_empty() {
                break;
            }
            saw_any = true;
            let newline = available.iter().position(|b| *b == b'\n');
            let take = newline.map_or(available.len(), |i| i + 1);
            consumed(&available[..take]);
            let keep = take.min(MAX_LINE_BYTES.saturating_sub(line.len()));
            if keep < take {
                truncated = true;
            }
            line.extend_from_slice(&available[..keep]);
            (take, newline.is_some())
        };
        reader.consume(used);
        if done {
            break;
        }
    }
    if !saw_any {
        return Ok(None);
    }
    while matches!(line.last(), Some(b'\n' | b'\r')) {
        line.pop();
    }
    Ok(Some((line, truncated)))
}

fn read_input(
    path: &std::path::Path,
    max_rows: usize,
    want_hash: bool,
) -> std::io::Result<PeekInput> {
    let file = fs::File::open(path)?;
    let total_bytes = file.metadata()?.len();
    let mut reader = BufReader::with_capacity(64 * 1024, file);
    let mut hasher = want_hash.then(blake3::Hasher::new);
    let mut consumed = |bytes: &[u8]| {
        if let Some(hasher) = hasher.as_mut() {
            hasher.update(bytes);
        }
    };

    let mut row_lines = Vec::new();
    let mut guess_lines = Vec::new();
    let mut truncated_lines = 0usize;
    let mut seen = 0usize;
    while seen < max_rows || guess_lines.len() < GUESS_LINES {
        let Some((line, truncated)) = read_capped_line(&mut reader, &mut consumed)? else {
            break;
        };
        if truncated {
            truncated_lines += 1;
        }
        let blank = String::from_utf8_lossy(&line).trim().is_empty();
        if !blank && guess_lines.len() < GUESS_LINES {
            guess_lines.push(line.clone());
        }
        if seen < max_rows {
            row_lines.push(line);
        }
        seen += 1;
    }
    // Hash the rest of the file in chunks (the only full pass, and only for witnesses).
    if want_hash {
        loop {
            let available = reader.fill_buf()?;
            if available.is_empty() {
                break;
            }
            let len = available.len();
            consumed(available);
            reader.consume(len);
        }
    }
    let blake3 = hasher.map(|hasher| format!("blake3:{}", hasher.finalize().to_hex()));
    Ok(PeekInput {
        row_lines,
        guess_lines,
        total_bytes,
        blake3,
        truncated_lines,
    })
}

pub fn run(args: &PeekArgs, append_witness_record: bool) -> Result<u8, String> {
    let key = match RedactionKey::from_env() {
        Ok(key) => key,
        Err(error) => {
            // Fail closed, for every format: a misconfigured key must not quietly downgrade.
            emit_refusal(
                crate::redaction::REFUSAL_CODE,
                "FINGERPRINT_REDACTION_KEY is empty or too short",
                json!({
                    "env": crate::redaction::ENV_KEY,
                    "reason": error.reason(),
                    "min_bytes": crate::redaction::MIN_KEY_BYTES,
                }),
            )?;
            return Ok(2);
        }
    };
    let mut marker = Marker::new(key.as_ref());
    let max_rows = args.rows.max(1);
    let lower_extension = args
        .file
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    if lower_extension
        .as_deref()
        .is_some_and(|extension| WORKBOOK_EXTENSIONS.contains(&extension))
    {
        return run_workbook(args, append_witness_record, max_rows, &mut marker);
    }
    if lower_extension.as_deref() == Some("parquet") {
        return run_parquet(args, append_witness_record, &mut marker);
    }
    if lower_extension.as_deref() == Some("xml") {
        return run_xml(args, append_witness_record, &mut marker);
    }
    if matches!(lower_extension.as_deref(), Some("html" | "htm")) {
        return run_html(args, append_witness_record, &mut marker);
    }
    if lower_extension.as_deref() == Some("pdf") {
        return run_pdf(args, append_witness_record, &mut marker);
    }
    if let Some(kind) = match lower_extension.as_deref() {
        Some("jsonl" | "ndjson") => Some(JsonPeekKind::Jsonl),
        Some("json") => Some(JsonPeekKind::Json),
        _ => None,
    } {
        return run_json(args, append_witness_record, max_rows, kind, &mut marker);
    }
    if let Some(extension) = args
        .file
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        && NOT_APPLICABLE_EXTENSIONS.contains(&extension.as_str())
    {
        emit_refusal(
            "E_BAD_INPUT",
            "Format not applicable: peek inspects CSV-like text only",
            json!({
                "path": args.file.display().to_string(),
                "format": extension,
                "reason": "format_not_applicable",
            }),
        )?;
        return Ok(2);
    }
    let input = match read_input(&args.file, max_rows, append_witness_record) {
        Ok(input) => input,
        Err(error) => {
            emit_refusal(
                "E_BAD_INPUT",
                "Could not read input file",
                json!({
                    "path": args.file.display().to_string(),
                    "error": error.kind().to_string()
                }),
            )?;
            return Ok(2);
        }
    };

    if input.total_bytes == 0 {
        emit_refusal(
            "E_BAD_INPUT",
            "Input file is empty",
            json!({ "path": args.file.display().to_string() }),
        )?;
        return Ok(2);
    }

    let delimiter = guess_delimiter(&input.guess_lines);
    let rows = match inspect_rows(&input.row_lines, delimiter.byte) {
        Ok(rows) if rows.is_empty() => {
            emit_refusal(
                "E_BAD_INPUT",
                "No rows were available to inspect",
                json!({ "path": args.file.display().to_string() }),
            )?;
            return Ok(2);
        }
        Ok(rows) => rows,
        Err(error) => {
            emit_refusal(
                "E_BAD_INPUT",
                "Could not parse delimited rows",
                json!({
                    "path": args.file.display().to_string(),
                    "error": error
                }),
            )?;
            return Ok(2);
        }
    };

    let modal_column_count = modal_positive_column_count(&rows);
    let shape_rows = rows
        .iter()
        .map(|row| row_to_json(row, modal_column_count))
        .collect::<Vec<_>>();
    let summary = summarize_rows(&rows, modal_column_count);
    let format_name = if matches!(lower_extension.as_deref(), Some("tsv" | "tab")) {
        "tsv"
    } else {
        "csv"
    };
    let mut result = json!({
        "file": args.file.display().to_string(),
        // Shared inspection vocabulary (bd-jxd): `format` is csv or tsv for delimited text;
        // `dialect` and `column_count` use profile inspect's names for the same concepts.
        "format": format_name,
        "dialect": dialect_json(
            format_name,
            &delimiter_text(delimiter.byte),
            Some(("confidence", json!(delimiter.confidence))),
        ),
        "column_count": modal_column_count,
        "rows_requested": max_rows,
        "rows_observed": rows.len(),
        "delimiter": {
            "name": delimiter.name,
            "byte": delimiter.byte,
            "confidence": delimiter.confidence
        },
        "summary": summary,
        "rows": shape_rows
    });

    if args.suggest {
        result["suggestions"] = suggestions(&rows, modal_column_count);
    }
    if input.truncated_lines > 0 {
        // Only present when a line exceeded the in-memory cap; ordinary files are unchanged.
        result["truncated_lines"] = json!(input.truncated_lines);
    }
    let result = apply_view(args, result);

    let witness_id = if append_witness_record {
        append_peek_witness(args, input.blake3.clone(), input.total_bytes, &result)
    } else {
        None
    };

    let envelope = json!({
        "version": ENVELOPE_VERSION,
        "outcome": "SUCCESS",
        "exit_code": 0,
        "subcommand": "peek",
        "result": result,
        "witness_id": witness_id
    });
    emit_json(&envelope)?;
    Ok(0)
}

/// What peek sees of one workbook cell: text for classification and an optional fixed class
/// (numbers, booleans and dates are classified by their Excel type, not by their text).
fn workbook_cell(cell: &calamine::Data) -> (String, Option<&'static str>) {
    use calamine::Data;
    match cell {
        Data::Empty => (String::new(), None),
        Data::String(text) => (text.clone(), None),
        Data::Float(value) => (value.to_string(), Some("numeric")),
        Data::Int(value) => (value.to_string(), Some("numeric")),
        Data::Bool(value) => (value.to_string(), Some("boolean")),
        // Rendered length only feeds the length histogram; a date is a 10-character ISO form.
        Data::DateTime(_) => ("0000-00-00".to_owned(), Some("date")),
        Data::DateTimeIso(text) => (text.clone(), Some("date")),
        Data::DurationIso(text) => (text.clone(), None),
        Data::Error(_) => ("#ERR".to_owned(), Some("text")),
    }
}

fn workbook_row_shape(row_index: usize, cells: &[calamine::Data]) -> RawRowShape {
    let views: Vec<(String, Option<&'static str>)> = cells.iter().map(workbook_cell).collect();
    // Trailing empty cells do not count as columns (a worksheet range is rectangular).
    let used = views
        .iter()
        .rposition(|(text, _)| !text.trim().is_empty())
        .map_or(0, |last| last + 1);
    let mut non_empty_count = 0usize;
    let mut type_counts = empty_type_counts();
    let mut length_buckets = empty_length_buckets();
    let mut has_unit_tokens = false;
    for (text, class) in &views[..used] {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            non_empty_count += 1;
        }
        let class = class.unwrap_or_else(|| classify_cell(trimmed));
        *type_counts.entry(class).or_insert(0) += 1;
        *length_buckets
            .entry(length_bucket(trimmed.len()))
            .or_insert(0) += 1;
        if looks_like_unit_token(trimmed) {
            has_unit_tokens = true;
        }
    }
    RawRowShape {
        row_index,
        column_count: used,
        non_empty_count,
        type_counts,
        length_buckets,
        has_unit_tokens,
    }
}

/// Stream the file through blake3 (the workbook is read by calamine anyway; this keeps the
/// witness hash independent of how the reader treats the file).
fn hash_file(path: &std::path::Path) -> std::io::Result<(String, u64)> {
    use std::io::Read;
    let mut file = fs::File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        total += read as u64;
    }
    Ok((format!("blake3:{}", hasher.finalize().to_hex()), total))
}

/// `fingerprint peek book.xlsx [--sheet NAME|INDEX]`: the same zero-retention row-shape report
/// as for CSV (classes, counts and histograms; never a cell value), plus the workbook's sheet
/// list with dimensions. Row numbers are Excel row numbers. A workbook is read by calamine one
/// sheet at a time, so memory is O(largest sheet), not O(rows inspected).
fn run_workbook(
    args: &PeekArgs,
    append_witness_record: bool,
    max_rows: usize,
    marker: &mut Marker<'_>,
) -> Result<u8, String> {
    use calamine::{Reader, open_workbook_auto};

    let refuse = |message: &str, detail: Value| -> Result<u8, String> {
        emit_refusal("E_BAD_INPUT", message, detail)?;
        Ok(2)
    };
    let path = args.file.display().to_string();
    let mut workbook = match open_workbook_auto(&args.file) {
        Ok(workbook) => workbook,
        Err(error) => {
            return refuse(
                "Could not open workbook",
                json!({ "path": path, "error": error.to_string() }),
            );
        }
    };
    let names: Vec<String> = workbook.sheet_names().to_vec();
    if names.is_empty() {
        return refuse("Workbook has no sheets", json!({ "path": path }));
    }

    // Sheet dimensions, one sheet at a time (each range is dropped before the next is read).
    let mut sheets = Vec::with_capacity(names.len());
    for (index, name) in names.iter().enumerate() {
        match workbook.worksheet_range(name) {
            Ok(range) => {
                let (first_row, first_column) = range
                    .start()
                    .map_or((0, 0), |(row, column)| (row + 1, column + 1));
                let mut entry = json!({
                    "index": index,
                    "id": marker.mark(Kind::Sheet, name),
                    "rows": range.height(),
                    "columns": range.width(),
                    "first_row": first_row,
                    "first_column": first_column,
                });
                if args.explicit {
                    entry["name"] = json!(name);
                }
                sheets.push(entry);
            }
            Err(_) => {
                // The reader's error text can quote the sheet name: report the index only.
                return refuse(
                    "Could not read a worksheet",
                    json!({ "path": path, "sheet_index": index }),
                );
            }
        }
    }

    let selected = match &args.sheet {
        None => 0,
        Some(selector) => match names.iter().position(|name| name == selector) {
            Some(index) => index,
            None => match selector.parse::<usize>() {
                Ok(index) if index < names.len() => index,
                _ => {
                    // Sheet names are withheld by default: the count says what indexes exist.
                    let mut detail = json!({
                        "path": path,
                        "reason": "sheet_not_found",
                        "sheet_count": names.len(),
                    });
                    if args.explicit {
                        detail["sheets"] = json!(names);
                    }
                    return refuse("Sheet not found", detail);
                }
            },
        },
    };
    let range = workbook
        .worksheet_range(&names[selected])
        .map_err(|error| format!("failed to read sheet: {error}"))?;
    let first_row = range.start().map_or(1, |(row, _)| row as usize + 1);
    let rows: Vec<RawRowShape> = range
        .rows()
        .take(max_rows)
        .enumerate()
        .map(|(offset, cells)| workbook_row_shape(first_row + offset, cells))
        .collect();
    if rows.is_empty() {
        return refuse(
            "No rows were available to inspect",
            json!({ "path": path, "sheet_index": selected }),
        );
    }

    let modal_column_count = modal_positive_column_count(&rows);
    let shape_rows = rows
        .iter()
        .map(|row| row_to_json(row, modal_column_count))
        .collect::<Vec<_>>();
    let summary = summarize_rows(&rows, modal_column_count);

    // Column type distributions over the inspected DATA rows (those from `data_starts_at` on),
    // by column position. Counts only, in profile inspect's type words for the dominant type.
    let data_starts_at = summary["data_starts_at"].as_u64().unwrap_or(1) as usize;
    let last_inspected_row = first_row + rows.len() - 1;
    let width = range.width();
    let mut per_column: Vec<BTreeMap<&'static str, usize>> =
        (0..width).map(|_| empty_type_counts()).collect();
    for (offset, cells) in range.rows().take(max_rows).enumerate() {
        if first_row + offset < data_starts_at {
            continue;
        }
        for (column, cell) in cells.iter().enumerate() {
            let (text, class) = workbook_cell(cell);
            let trimmed = text.trim();
            let class = if trimmed.is_empty() {
                "empty"
            } else {
                class.unwrap_or_else(|| classify_cell(trimmed))
            };
            *per_column[column].entry(class).or_insert(0) += 1;
        }
    }
    let column_start = range.start().map_or(0, |(_, column)| column as usize);
    let columns: Vec<Value> = per_column
        .iter()
        .enumerate()
        .map(|(index, counts)| {
            let non_empty: usize = counts
                .iter()
                .filter(|(class, _)| **class != "empty")
                .map(|(_, count)| *count)
                .sum();
            let total: usize = counts.values().sum();
            let dominant = counts
                .iter()
                .filter(|(class, count)| **class != "empty" && **count > 0)
                .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)))
                .map(|(class, _)| *class);
            let inspect_type = match dominant {
                Some("numeric") => "number",
                Some("boolean") => "boolean",
                Some(_) => "text",
                None => "unknown",
            };
            json!({
                "index": index,
                "column": column_letters(column_start + index),
                "type": inspect_type,
                "dominant_class": dominant,
                "cell_type_counts": counts,
                "null_count": counts.get("empty").copied().unwrap_or(0),
                "null_rate": if total == 0 { 0.0 } else { round_confidence(counts.get("empty").copied().unwrap_or(0) as f64 / total as f64) },
                "non_empty": non_empty,
            })
        })
        .collect();

    // Merged ranges and formula cells of the selected sheet (counts only: no formula text).
    let sheet_name = names[selected].clone();
    let merged: Option<Vec<(u32, u32, u32, u32)>> = match &mut workbook {
        calamine::Sheets::Xlsx(xlsx) => xlsx
            .worksheet_merge_cells(&sheet_name)
            .and_then(Result::ok)
            .map(|dims| {
                dims.iter()
                    .map(|d| (d.start.0, d.start.1, d.end.0, d.end.1))
                    .collect()
            }),
        calamine::Sheets::Xls(xls) => xls.worksheet_merge_cells(&sheet_name).map(|dims| {
            dims.iter()
                .map(|d| (d.start.0, d.start.1, d.end.0, d.end.1))
                .collect()
        }),
        _ => None,
    };
    let formula_cells = workbook
        .worksheet_formula(&sheet_name)
        .ok()
        .map(|formulas| {
            formulas
                .cells()
                .filter(|(_, _, formula)| !formula.trim().is_empty())
                .count()
        });
    let merged_flags = merged.as_ref().map(|ranges| {
        let in_window = ranges
            .iter()
            .filter(|(start_row, _, end_row, _)| {
                (*start_row as usize) < last_inspected_row && (*end_row as usize) + 1 >= first_row
            })
            .count();
        let area: u64 = ranges
            .iter()
            .map(|(r0, c0, r1, c1)| u64::from(r1 - r0 + 1) * u64::from(c1 - c0 + 1))
            .sum();
        json!({ "ranges": ranges.len(), "cells": area, "ranges_in_inspected_rows": in_window })
    });

    let mut selected_sheet = json!({
        "index": selected,
        "id": marker.mark(Kind::Sheet, &names[selected]),
    });
    if args.explicit {
        selected_sheet["name"] = json!(names[selected]);
    }
    let mut result = json!({
        "file": path,
        "format": "xlsx",
        "sheet_flags": {
            "merged": merged_flags,
            "formula_cells": formula_cells,
        },
        "columns": columns,
        "dialect": dialect_json("xlsx", "none", None),
        "column_count": modal_column_count,
        "sheet": selected_sheet,
        "sheets": sheets,
        "redaction": marker.mode(),
        "rows_requested": max_rows,
        "rows_observed": rows.len(),
        "summary": summary,
        "rows": shape_rows,
    });
    if args.suggest {
        result["suggestions"] = suggestions(&rows, modal_column_count);
    }
    let result = apply_view(args, result);

    let witness_id = if append_witness_record {
        match hash_file(&args.file) {
            Ok((hash, bytes)) => append_peek_witness(args, Some(hash), bytes, &result),
            Err(error) => {
                eprintln!("Warning: Failed to hash workbook for the witness record: {error}");
                None
            }
        }
    } else {
        None
    };
    emit_json(&json!({
        "version": ENVELOPE_VERSION,
        "outcome": "SUCCESS",
        "exit_code": 0,
        "subcommand": "peek",
        "result": result,
        "witness_id": witness_id
    }))?;
    Ok(0)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum JsonPeekKind {
    Json,
    Jsonl,
}

/// Distinct field paths tracked and nesting levels followed; beyond these the report says so.
const MAX_JSON_FIELDS: usize = 500;
const MAX_JSON_DEPTH: usize = 8;
/// Largest single `.json` document parsed in memory (same limit as the `json` DSL format).
const MAX_JSON_PEEK_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Default)]
struct JsonFieldStats {
    path: String,
    /// Records in which the path occurs at least once.
    present: usize,
    /// Occurrences by JSON type (an array element path counts every element).
    types: BTreeMap<&'static str, usize>,
}

#[derive(Default)]
struct JsonShape {
    /// In first-seen FILE order (never name order).
    fields: Vec<JsonFieldStats>,
    index: BTreeMap<String, usize>,
    truncated: bool,
}

impl JsonShape {
    /// Count one occurrence of `kind` at `path`; `None` when the field cap is reached.
    fn record(&mut self, path: &str, kind: &'static str, seen: &mut Vec<usize>) -> Option<usize> {
        let slot = match self.index.get(path) {
            Some(slot) => *slot,
            None if self.fields.len() >= MAX_JSON_FIELDS => {
                self.truncated = true;
                return None;
            }
            None => {
                self.fields.push(JsonFieldStats {
                    path: path.to_owned(),
                    ..JsonFieldStats::default()
                });
                self.index.insert(path.to_owned(), self.fields.len() - 1);
                self.fields.len() - 1
            }
        };
        *self.fields[slot].types.entry(kind).or_insert(0) += 1;
        if !seen.contains(&slot) {
            seen.push(slot);
        }
        Some(slot)
    }

    fn finish_record(&mut self, seen: Vec<usize>) {
        for slot in seen {
            self.fields[slot].present += 1;
        }
    }
}

/// Walks one JSON value in FILE order (serde_json's `Value` would sort object keys by name, and
/// field order must not depend on names), recording each path and type into the shared shape.
/// Nothing but paths and type words is kept; scalar values are dropped as they are read.
struct ShapeSeed<'a> {
    shape: &'a std::cell::RefCell<JsonShape>,
    seen: &'a std::cell::RefCell<Vec<usize>>,
    path: String,
    depth: usize,
}

impl<'a> ShapeSeed<'a> {
    fn child(&self, path: String) -> ShapeSeed<'a> {
        ShapeSeed {
            shape: self.shape,
            seen: self.seen,
            path,
            depth: self.depth + 1,
        }
    }

    fn note(&self, kind: &'static str) -> bool {
        self.shape
            .borrow_mut()
            .record(&self.path, kind, &mut self.seen.borrow_mut())
            .is_some()
    }
}

impl<'de> serde::de::DeserializeSeed<'de> for ShapeSeed<'_> {
    type Value = ();

    fn deserialize<D: serde::Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> serde::de::Visitor<'de> for ShapeSeed<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("any JSON value")
    }

    fn visit_bool<E>(self, _: bool) -> Result<(), E> {
        self.note("boolean");
        Ok(())
    }

    fn visit_i64<E>(self, _: i64) -> Result<(), E> {
        self.note("integer");
        Ok(())
    }

    fn visit_u64<E>(self, _: u64) -> Result<(), E> {
        self.note("integer");
        Ok(())
    }

    fn visit_f64<E>(self, _: f64) -> Result<(), E> {
        self.note("number");
        Ok(())
    }

    fn visit_str<E>(self, _: &str) -> Result<(), E> {
        self.note("string");
        Ok(())
    }

    fn visit_unit<E>(self) -> Result<(), E> {
        self.note("null");
        Ok(())
    }

    fn visit_none<E>(self) -> Result<(), E> {
        self.note("null");
        Ok(())
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        let tracked = self.note("object");
        if !tracked || self.depth >= MAX_JSON_DEPTH {
            if tracked {
                self.shape.borrow_mut().truncated = true;
            }
            while map
                .next_entry::<serde::de::IgnoredAny, serde::de::IgnoredAny>()?
                .is_some()
            {}
            return Ok(());
        }
        while let Some(key) = map.next_key::<String>()? {
            let child = self.child(format!("{}.{key}", self.path));
            map.next_value_seed(child)?;
        }
        Ok(())
    }

    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        let tracked = self.note("array");
        if !tracked || self.depth >= MAX_JSON_DEPTH {
            if tracked {
                self.shape.borrow_mut().truncated = true;
            }
            while seq.next_element::<serde::de::IgnoredAny>()?.is_some() {}
            return Ok(());
        }
        let element_path = format!("{}[]", self.path);
        while seq
            .next_element_seed(self.child(element_path.clone()))?
            .is_some()
        {}
        Ok(())
    }
}

/// A top-level JSON array: its first `limit` elements are records, the rest are skipped.
struct RootArraySeed<'a> {
    shape: &'a std::cell::RefCell<JsonShape>,
    limit: usize,
    records: &'a std::cell::Cell<usize>,
}

impl<'de> serde::de::DeserializeSeed<'de> for RootArraySeed<'_> {
    type Value = ();

    fn deserialize<D: serde::Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_seq(self)
    }
}

impl<'de> serde::de::Visitor<'de> for RootArraySeed<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a JSON array")
    }

    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        loop {
            if self.records.get() >= self.limit {
                while seq.next_element::<serde::de::IgnoredAny>()?.is_some() {}
                return Ok(());
            }
            let seen = std::cell::RefCell::new(Vec::new());
            let element = ShapeSeed {
                shape: self.shape,
                seen: &seen,
                path: "$".to_owned(),
                depth: 0,
            };
            if seq.next_element_seed(element)?.is_none() {
                return Ok(());
            }
            self.shape.borrow_mut().finish_record(seen.into_inner());
            self.records.set(self.records.get() + 1);
        }
    }
}

/// Add one self-contained JSON value (a JSONL line, or a non-array document) as one record.
fn add_json_record(shape: &std::cell::RefCell<JsonShape>, bytes: &[u8]) -> Result<(), ()> {
    use serde::de::DeserializeSeed;
    let seen = std::cell::RefCell::new(Vec::new());
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    ShapeSeed {
        shape,
        seen: &seen,
        path: "$".to_owned(),
        depth: 0,
    }
    .deserialize(&mut deserializer)
    .map_err(|_| ())?;
    deserializer.end().map_err(|_| ())?;
    shape.borrow_mut().finish_record(seen.into_inner());
    Ok(())
}

/// `fingerprint peek x.jsonl|x.json`: the schema tree of the first `--rows` records (a `.json`
/// array contributes its first elements, a `.json` object is one record): per field path the
/// presence rate and the counts by JSON type. Never a value. A field is identified by an ordinal
/// ref in file order (`id`; a keyed marker with a key); `--explicit` adds the path itself (key
/// names). `.jsonl` streams
/// the first lines only; a `.json` document is parsed whole (refused over 256 MiB).
fn run_json(
    args: &PeekArgs,
    append_witness_record: bool,
    max_rows: usize,
    kind: JsonPeekKind,
    marker: &mut Marker<'_>,
) -> Result<u8, String> {
    let path = args.file.display().to_string();
    let refuse = |message: &str, detail: Value| -> Result<u8, String> {
        emit_refusal("E_BAD_INPUT", message, detail)?;
        Ok(2)
    };
    let shape = std::cell::RefCell::new(JsonShape::default());
    let mut records = 0usize;
    let (mut invalid_lines, mut first_invalid_line) = (0usize, None::<usize>);
    let (hash, total_bytes, truncated_lines);

    match kind {
        JsonPeekKind::Jsonl => {
            let input = match read_input(&args.file, max_rows, append_witness_record) {
                Ok(input) => input,
                Err(error) => {
                    return refuse(
                        "Could not read input file",
                        json!({ "path": path, "error": error.kind().to_string() }),
                    );
                }
            };
            if input.total_bytes == 0 {
                return refuse("Input file is empty", json!({ "path": path }));
            }
            for (index, raw) in input.row_lines.iter().enumerate() {
                if raw.iter().all(u8::is_ascii_whitespace) {
                    continue;
                }
                match add_json_record(&shape, raw) {
                    Ok(()) => {
                        records += 1;
                    }
                    Err(()) => {
                        invalid_lines += 1;
                        first_invalid_line.get_or_insert(index + 1);
                    }
                }
            }
            hash = input.blake3;
            total_bytes = input.total_bytes;
            truncated_lines = input.truncated_lines;
        }
        JsonPeekKind::Json => {
            let size = match fs::metadata(&args.file) {
                Ok(metadata) => metadata.len(),
                Err(error) => {
                    return refuse(
                        "Could not read input file",
                        json!({ "path": path, "error": error.kind().to_string() }),
                    );
                }
            };
            if size == 0 {
                return refuse("Input file is empty", json!({ "path": path }));
            }
            if size > MAX_JSON_PEEK_BYTES {
                return refuse(
                    "json file is over the in-memory limit; use jsonl for large inputs",
                    json!({ "path": path, "bytes": size, "limit": MAX_JSON_PEEK_BYTES }),
                );
            }
            let bytes = match fs::read(&args.file) {
                Ok(bytes) => bytes,
                Err(error) => {
                    return refuse(
                        "Could not read input file",
                        json!({ "path": path, "error": error.kind().to_string() }),
                    );
                }
            };
            // Walk the document in FILE order (never through `Value`, which sorts keys by name).
            let is_array = bytes.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'[');
            let syntax_error = |error: serde_json::Error| {
                // Position only: never the message text.
                refuse(
                    "Input is not valid JSON",
                    json!({ "path": path, "line": error.line(), "column": error.column() }),
                )
            };
            if is_array {
                use serde::de::DeserializeSeed;
                let counted = std::cell::Cell::new(0usize);
                let mut deserializer = serde_json::Deserializer::from_slice(&bytes);
                let outcome = RootArraySeed {
                    shape: &shape,
                    limit: max_rows,
                    records: &counted,
                }
                .deserialize(&mut deserializer)
                .and_then(|()| deserializer.end());
                if let Err(error) = outcome {
                    return syntax_error(error);
                }
                records = counted.get();
            } else {
                let seen = std::cell::RefCell::new(Vec::new());
                let mut deserializer = serde_json::Deserializer::from_slice(&bytes);
                use serde::de::DeserializeSeed;
                let outcome = ShapeSeed {
                    shape: &shape,
                    seen: &seen,
                    path: "$".to_owned(),
                    depth: 0,
                }
                .deserialize(&mut deserializer)
                .and_then(|()| deserializer.end());
                if let Err(error) = outcome {
                    return syntax_error(error);
                }
                shape.borrow_mut().finish_record(seen.into_inner());
                records = 1;
            }
            hash =
                append_witness_record.then(|| format!("blake3:{}", blake3::hash(&bytes).to_hex()));
            total_bytes = size;
            truncated_lines = 0;
        }
    }

    if records == 0 {
        return refuse(
            "No valid records were available to inspect",
            json!({ "path": path, "invalid_lines": invalid_lines }),
        );
    }

    let shape = shape.into_inner();
    let fields: Vec<Value> = shape
        .fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let mut entry = json!({
                "index": index,
                "id": marker.mark(Kind::Path, &field.path),
                "depth": field.path.matches(['.', '[']).count().saturating_sub(field.path.matches("[]").count()),
                "present": field.present,
                "presence_rate": round_confidence(field.present as f64 / records as f64),
                "types": field.types,
            });
            if args.explicit {
                entry["path"] = json!(field.path);
            }
            entry
        })
        .collect();
    let mut result = json!({
        "file": path,
        "format": if kind == JsonPeekKind::Jsonl { "jsonl" } else { "json" },
        "dialect": dialect_json(if kind == JsonPeekKind::Jsonl { "jsonl" } else { "json" }, "none", None),
        "rows_requested": max_rows,
        "rows_observed": records,
        "fields_total": fields.len(),
        "fields_truncated": shape.truncated,
        "redaction": marker.mode(),
        "fields": fields,
    });
    if invalid_lines > 0 {
        result["invalid_lines"] = json!(invalid_lines);
        result["first_invalid_line"] = json!(first_invalid_line);
    }
    if truncated_lines > 0 {
        result["truncated_lines"] = json!(truncated_lines);
    }
    let result = apply_view(args, result);

    let witness_id = if append_witness_record {
        append_peek_witness(args, hash, total_bytes, &result)
    } else {
        None
    };
    emit_json(&json!({
        "version": ENVELOPE_VERSION,
        "outcome": "SUCCESS",
        "exit_code": 0,
        "subcommand": "peek",
        "result": result,
        "witness_id": witness_id
    }))?;
    Ok(0)
}

/// Most record candidates a brief view lists.
const BRIEF_MAX_CANDIDATES: usize = 5;

/// `--view brief`: the report without its per-row / per-column / per-field / per-path arrays,
/// so it stays small (well under ~500 tokens) however large the file is. Everything else
/// (format, dialect, counts, summary, suggestions, at most 5 record candidates) is kept, and
/// `omitted` names what was dropped. The view is applied before the witness record, so the
/// witness's output hash is that of what was actually printed.
fn apply_view(args: &PeekArgs, mut result: Value) -> Value {
    if args.view != PeekView::Brief {
        return result;
    }
    let mut omitted = Vec::new();
    if let Some(object) = result.as_object_mut() {
        for key in [
            "rows", "columns", "fields", "paths", "sheets", "headings", "tables", "tags",
            "classes", "outline",
        ] {
            if object.remove(key).is_some() {
                omitted.push(key);
            }
        }
        for list in ["record_candidates", "candidate_regions"] {
            if let Some(candidates) = object
                .get_mut("summary")
                .and_then(|summary| summary.get_mut(list))
                .and_then(Value::as_array_mut)
                && candidates.len() > BRIEF_MAX_CANDIDATES
            {
                candidates.truncate(BRIEF_MAX_CANDIDATES);
            }
        }
    }
    result["view"] = json!("brief");
    result["omitted"] = json!(omitted);
    result
}

/// Info-dictionary keys defined by the PDF specification: public vocabulary, so they are named
/// even by default. Any other key is a pseudonym `id` unless `--explicit`.
const STANDARD_INFO_KEYS: &[&str] = &[
    "Title",
    "Author",
    "Subject",
    "Keywords",
    "Creator",
    "Producer",
    "CreationDate",
    "ModDate",
    "Trapped",
];
const MAX_OUTLINE_ENTRIES: usize = 5000;
const MAX_OUTLINE_DEPTH: usize = 16;
const MAX_PDF_LISTED: usize = 200;

/// `fingerprint peek x.pdf`: structure only. Page count and page-size/rotation profile, PDF
/// version and whether it is encrypted, the Info dictionary's KEYS (never a value), and the
/// outline (bookmark) tree as entry count, depth and per-entry depth/children/title-length
/// bucket. No page content is read, no metadata value and no bookmark title is printed. A key
/// outside the PDF specification's Info vocabulary is an ordinal ref (or keyed marker) `id`,
/// named only with `--explicit`; each outline title is an ordinal ref `id`, never the title.
fn run_pdf(
    args: &PeekArgs,
    append_witness_record: bool,
    marker: &mut Marker<'_>,
) -> Result<u8, String> {
    use lopdf::{Dictionary, Document, Object, ObjectId};

    let path = args.file.display().to_string();
    let refuse = |message: &str, detail: Value| -> Result<u8, String> {
        emit_refusal("E_BAD_INPUT", message, detail)?;
        Ok(2)
    };
    let bytes = match fs::read(&args.file) {
        Ok(bytes) => bytes,
        Err(error) => {
            return refuse(
                "Could not read input file",
                json!({ "path": path, "error": error.kind().to_string() }),
            );
        }
    };
    if bytes.is_empty() {
        return refuse("Input file is empty", json!({ "path": path }));
    }
    let document = match Document::load_mem(&bytes) {
        Ok(document) => document,
        Err(error) => {
            return refuse(
                "Could not read pdf",
                json!({ "path": path, "error": error.to_string() }),
            );
        }
    };

    let dict_of = |object: &Object| -> Option<Dictionary> {
        match object {
            Object::Reference(id) => document.get_object(*id).ok()?.as_dict().ok().cloned(),
            other => other.as_dict().ok().cloned(),
        }
    };

    // Pages: size and rotation profile (MediaBox/Rotate may be inherited from the page tree).
    let pages = document.get_pages();
    let inherited = |start: ObjectId, key: &[u8]| -> Option<Object> {
        let mut current = start;
        for _ in 0..16 {
            let dict = document.get_object(current).ok()?.as_dict().ok()?;
            if let Ok(value) = dict.get(key) {
                return Some(value.clone());
            }
            current = dict.get(b"Parent").ok()?.as_reference().ok()?;
        }
        None
    };
    let mut sizes: BTreeMap<(i64, i64, i64), usize> = BTreeMap::new();
    for page_id in pages.values() {
        let rotate = inherited(*page_id, b"Rotate")
            .and_then(|object| object.as_i64().ok())
            .unwrap_or(0);
        let media = inherited(*page_id, b"MediaBox").and_then(|object| {
            let items = match object {
                Object::Reference(id) => document.get_object(id).ok()?.as_array().ok()?.clone(),
                other => other.as_array().ok()?.clone(),
            };
            let numbers: Vec<f32> = items
                .iter()
                .filter_map(|item| {
                    item.as_float()
                        .ok()
                        .or_else(|| item.as_i64().ok().map(|n| n as f32))
                })
                .collect();
            (numbers.len() == 4).then(|| (numbers[2] - numbers[0], numbers[3] - numbers[1]))
        });
        let (width, height) = media.unwrap_or((0.0, 0.0));
        *sizes
            .entry((
                (width * 10.0).round() as i64,
                (height * 10.0).round() as i64,
                rotate,
            ))
            .or_insert(0) += 1;
    }
    let mut size_list: Vec<_> = sizes.into_iter().collect();
    size_list.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let page_sizes: Vec<Value> = size_list
        .iter()
        .take(20)
        .map(|((width, height, rotate), count)| {
            json!({
                "width": *width as f64 / 10.0,
                "height": *height as f64 / 10.0,
                "rotate": rotate,
                "pages": count,
            })
        })
        .collect();

    // Info dictionary KEYS only.
    let info = document.trailer.get(b"Info").ok().and_then(dict_of);
    let mut standard_keys = Vec::new();
    let mut other_keys: Vec<String> = Vec::new();
    if let Some(info) = &info {
        for (name, _) in info.iter() {
            let key = String::from_utf8_lossy(name).into_owned();
            if STANDARD_INFO_KEYS.contains(&key.as_str()) {
                standard_keys.push(key);
            } else {
                other_keys.push(key);
            }
        }
    }
    // Standard keys are the PDF specification's public vocabulary (sorted). Other keys stay in
    // the Info dictionary's own (file) order: their refs must not follow name order.
    standard_keys.sort();
    let mut metadata = json!({
        "key_count": standard_keys.len() + other_keys.len(),
        "standard_keys": standard_keys,
        "other_key_ids": other_keys
            .iter()
            .map(|key| marker.mark(Kind::MetadataKey, key))
            .collect::<Vec<_>>(),
    });
    if args.explicit {
        metadata["other_keys"] = json!(other_keys);
    }

    // Outline tree: iterative walk of /Outlines with caps and a cycle guard.
    struct Entry {
        depth: usize,
        children: usize,
        title_length: usize,
        id: String,
    }
    let mut entries: Vec<Entry> = Vec::new();
    let mut truncated = false;
    let outlines_root = document
        .catalog()
        .ok()
        .and_then(|catalog| catalog.get(b"Outlines").ok())
        .and_then(|object| object.as_reference().ok());
    if let Some(root_id) = outlines_root
        && let Some(root) = document
            .get_object(root_id)
            .ok()
            .and_then(|object| object.as_dict().ok())
    {
        let mut seen: std::collections::BTreeSet<ObjectId> = std::collections::BTreeSet::new();
        // (item id, depth, index of the parent entry)
        let mut stack: Vec<(ObjectId, usize, Option<usize>)> = Vec::new();
        if let Ok(first) = root.get(b"First").and_then(Object::as_reference) {
            stack.push((first, 1, None));
        }
        while let Some((item_id, depth, parent)) = stack.pop() {
            if !seen.insert(item_id) {
                continue;
            }
            if entries.len() >= MAX_OUTLINE_ENTRIES || depth > MAX_OUTLINE_DEPTH {
                truncated = true;
                continue;
            }
            let Some(item) = document
                .get_object(item_id)
                .ok()
                .and_then(|o| o.as_dict().ok())
            else {
                continue;
            };
            let title = item
                .get(b"Title")
                .ok()
                .and_then(|object| match object {
                    Object::String(text, _) => Some(String::from_utf8_lossy(text).into_owned()),
                    _ => None,
                })
                .unwrap_or_default();
            let index = entries.len();
            entries.push(Entry {
                depth,
                children: 0,
                title_length: title.chars().count(),
                id: marker.mark(Kind::Outline, &title),
            });
            if let Some(parent) = parent {
                entries[parent].children += 1;
            }
            // Siblings after children on the stack so the walk is document order.
            if let Ok(next) = item.get(b"Next").and_then(Object::as_reference) {
                stack.push((next, depth, parent));
            }
            if let Ok(first) = item.get(b"First").and_then(Object::as_reference) {
                stack.push((first, depth + 1, Some(index)));
            }
        }
    }
    let outline_items: Vec<Value> = entries
        .iter()
        .take(MAX_PDF_LISTED)
        .enumerate()
        .map(|(index, entry)| {
            // The title is never printed (even with --explicit): only a ref, so equal titles
            // share one id inside this output.
            json!({
                "index": index,
                "id": entry.id,
                "depth": entry.depth,
                "children": entry.children,
                "title_length_bucket": length_bucket(entry.title_length),
            })
        })
        .collect();

    let mut result = json!({
        "file": path,
        "format": "pdf",
        "redaction": marker.mode(),
        "dialect": dialect_json("pdf", "none", None),
        "version": document.version,
        "encrypted": document.is_encrypted(),
        "page_count": pages.len(),
        "summary": {
            "page_sizes": page_sizes,
            "metadata_key_count": metadata["key_count"],
            "outline_entries": entries.len(),
            "outline_max_depth": entries.iter().map(|e| e.depth).max().unwrap_or(0),
            "outline_top_level": entries.iter().filter(|e| e.depth == 1).count(),
            "outline_truncated": truncated,
        },
        "metadata": metadata,
        "outline": outline_items,
    });
    if entries.len() > MAX_PDF_LISTED {
        result["outline_listed"] = json!(MAX_PDF_LISTED);
    }
    let result = apply_view(args, result);

    let witness_id = if append_witness_record {
        let hash = format!("blake3:{}", blake3::hash(&bytes).to_hex());
        append_peek_witness(args, Some(hash), bytes.len() as u64, &result)
    } else {
        None
    };
    emit_json(&json!({
        "version": ENVELOPE_VERSION,
        "outcome": "SUCCESS",
        "exit_code": 0,
        "subcommand": "peek",
        "result": result,
        "witness_id": witness_id
    }))?;
    Ok(0)
}

/// Largest HTML file parsed in memory.
const MAX_HTML_PEEK_BYTES: u64 = 256 * 1024 * 1024;
/// Headings and tables listed individually (the counts are always exact).
const MAX_HTML_ITEMS: usize = 200;

/// `fingerprint peek x.html`: the document's structural skeleton from its DOM: heading levels
/// and text-length buckets, per-table row/column shapes and a column-width profile, page-section
/// count and candidate regions (a heading and the tables that follow it before the next heading
/// of the same or a higher level). No text, no attribute value, no element or class name by
/// default; `--explicit` adds tag and class names, never any text. Each heading carries an
/// ordinal ref (or keyed marker) `id`.
fn run_html(
    args: &PeekArgs,
    append_witness_record: bool,
    marker: &mut Marker<'_>,
) -> Result<u8, String> {
    use scraper::{ElementRef, Html};

    let path = args.file.display().to_string();
    let refuse = |message: &str, detail: Value| -> Result<u8, String> {
        emit_refusal("E_BAD_INPUT", message, detail)?;
        Ok(2)
    };
    let size = match fs::metadata(&args.file) {
        Ok(metadata) => metadata.len(),
        Err(error) => {
            return refuse(
                "Could not read input file",
                json!({ "path": path, "error": error.kind().to_string() }),
            );
        }
    };
    if size == 0 {
        return refuse("Input file is empty", json!({ "path": path }));
    }
    if size > MAX_HTML_PEEK_BYTES {
        return refuse(
            "html file is over the in-memory limit",
            json!({ "path": path, "bytes": size, "limit": MAX_HTML_PEEK_BYTES }),
        );
    }
    let bytes = match fs::read(&args.file) {
        Ok(bytes) => bytes,
        Err(error) => {
            return refuse(
                "Could not read input file",
                json!({ "path": path, "error": error.kind().to_string() }),
            );
        }
    };
    let document = Html::parse_document(&String::from_utf8_lossy(&bytes));

    let text_of = |element: ElementRef<'_>| -> String {
        element
            .text()
            .collect::<Vec<_>>()
            .join(" ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };

    struct Heading {
        level: usize,
        length: usize,
        id: String,
        tables: usize,
    }
    let mut headings: Vec<Heading> = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    let mut table_items: Vec<Value> = Vec::new();
    let mut table_count = 0usize;
    let mut element_count = 0u64;
    let mut page_sections = 0usize;
    let mut tags: BTreeMap<String, usize> = BTreeMap::new();
    let mut classes: BTreeMap<String, usize> = BTreeMap::new();
    let mut modal_widths: Vec<usize> = Vec::new();

    for node in document.tree.root().descendants() {
        let Some(element) = ElementRef::wrap(node) else {
            continue;
        };
        element_count += 1;
        let name = element.value().name();
        if element.value().attr("data-page-number").is_some() {
            page_sections += 1;
        }
        if args.explicit {
            *tags.entry(name.to_owned()).or_insert(0) += 1;
            for class in element.value().classes() {
                *classes.entry(class.to_owned()).or_insert(0) += 1;
            }
        }
        if let [b'h', digit @ b'1'..=b'6'] = name.as_bytes() {
            let level = usize::from(digit - b'0');
            while open
                .last()
                .is_some_and(|last| headings[*last].level >= level)
            {
                open.pop();
            }
            let text = text_of(element);
            headings.push(Heading {
                level,
                length: text.chars().count(),
                id: marker.mark(Kind::Heading, &text),
                tables: 0,
            });
            open.push(headings.len() - 1);
        } else if name == "table" {
            for index in &open {
                headings[*index].tables += 1;
            }
            let this_table = element.id();
            let mut widths: BTreeMap<usize, usize> = BTreeMap::new();
            let mut widest = 0usize;
            let (mut rows, mut header_rows, mut leading_headers) = (0usize, 0usize, true);
            let (mut colspan_cells, mut rowspan_cells, mut full_width_rows) =
                (0usize, 0usize, 0usize);
            let mut cell_types = empty_type_counts();
            let tr = scraper::Selector::parse("tr").map_err(|e| format!("{e:?}"))?;
            for row in element.select(&tr) {
                // Rows of a nested table belong to that table.
                let owner = row
                    .ancestors()
                    .filter_map(ElementRef::wrap)
                    .find(|ancestor| ancestor.value().name() == "table");
                if owner.map(|owner| owner.id()) != Some(this_table) {
                    continue;
                }
                let cells: Vec<ElementRef<'_>> = row
                    .children()
                    .filter_map(ElementRef::wrap)
                    .filter(|cell| matches!(cell.value().name(), "td" | "th"))
                    .collect();
                rows += 1;
                let mut width = 0usize;
                let mut all_headers = !cells.is_empty();
                let mut single_span = 1usize;
                for cell in &cells {
                    let span = cell
                        .value()
                        .attr("colspan")
                        .and_then(|value| value.trim().parse::<usize>().ok())
                        .unwrap_or(1)
                        .max(1);
                    if span > 1 {
                        colspan_cells += 1;
                    }
                    if cell
                        .value()
                        .attr("rowspan")
                        .is_some_and(|value| value.trim() != "1")
                    {
                        rowspan_cells += 1;
                    }
                    if cell.value().name() != "th" {
                        all_headers = false;
                    }
                    single_span = span;
                    width += span;
                    *cell_types
                        .entry(classify_cell(&text_of(*cell)))
                        .or_insert(0) += 1;
                }
                if cells.len() == 1 && single_span >= 2 {
                    full_width_rows += 1;
                }
                if leading_headers && all_headers {
                    header_rows += 1;
                } else {
                    leading_headers = false;
                }
                if width > 0 {
                    *widths.entry(width).or_insert(0) += 1;
                    modal_widths.push(width);
                }
                widest = widest.max(width);
            }
            let widths_flat: Vec<usize> = widths
                .iter()
                .flat_map(|(width, count)| std::iter::repeat_n(*width, *count))
                .collect();
            let nested = element
                .select(&scraper::Selector::parse("table").map_err(|e| format!("{e:?}"))?)
                .count();
            if table_items.len() < MAX_HTML_ITEMS {
                table_items.push(json!({
                    "index": table_count,
                    "rows": rows,
                    "columns_max": widest,
                    "columns_modal": most_common_count(&widths_flat).map(|(width, _)| width),
                    "header_rows": header_rows,
                    "colspan_cells": colspan_cells,
                    "rowspan_cells": rowspan_cells,
                    "full_width_rows": full_width_rows,
                    "nested_tables": nested,
                    "row_width_histogram": widths
                        .iter()
                        .map(|(width, count)| (width.to_string(), *count))
                        .collect::<BTreeMap<String, usize>>(),
                    "cell_type_counts": cell_types,
                }));
            }
            table_count += 1;
        }
    }

    let mut by_level: BTreeMap<String, usize> = BTreeMap::new();
    for heading in &headings {
        *by_level.entry(heading.level.to_string()).or_insert(0) += 1;
    }
    let heading_items: Vec<Value> = headings
        .iter()
        .take(MAX_HTML_ITEMS)
        .enumerate()
        .map(|(index, heading)| {
            // The heading text is never printed (even with --explicit): only a ref, so equal
            // headings share one id inside this output.
            json!({
                "index": index,
                "id": heading.id,
                "level": heading.level,
                "text_length_bucket": length_bucket(heading.length),
                "tables_in_region": heading.tables,
            })
        })
        .collect();
    let candidate_regions: Vec<Value> = headings
        .iter()
        .enumerate()
        .filter(|(_, heading)| heading.tables > 0)
        .take(MAX_HTML_ITEMS)
        .map(|(index, heading)| {
            json!({ "heading_index": index, "level": heading.level, "tables": heading.tables })
        })
        .collect();

    let mut result = json!({
        "file": path,
        "format": "html",
        "redaction": marker.mode(),
        "dialect": dialect_json("html", "none", None),
        "element_count": element_count,
        "headings_total": headings.len(),
        "tables_total": table_count,
        "items_truncated": headings.len() > MAX_HTML_ITEMS || table_count > MAX_HTML_ITEMS,
        "summary": {
            "headings_by_level": by_level,
            "page_sections": page_sections,
            "modal_table_columns": most_common_count(&modal_widths).map(|(width, _)| width),
            "candidate_regions": candidate_regions,
        },
        "headings": heading_items,
        "tables": table_items,
    });
    if args.explicit {
        let mut tag_list: Vec<(&String, &usize)> = tags.iter().collect();
        tag_list.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        result["tags"] = json!(tag_list.into_iter().take(30).collect::<BTreeMap<_, _>>());
        let mut class_list: Vec<(&String, &usize)> = classes.iter().collect();
        class_list.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        result["classes"] = json!(class_list.into_iter().take(20).collect::<BTreeMap<_, _>>());
    }
    let result = apply_view(args, result);

    let witness_id = if append_witness_record {
        let hash = format!("blake3:{}", blake3::hash(&bytes).to_hex());
        append_peek_witness(args, Some(hash), size, &result)
    } else {
        None
    };
    emit_json(&json!({
        "version": ENVELOPE_VERSION,
        "outcome": "SUCCESS",
        "exit_code": 0,
        "subcommand": "peek",
        "result": result,
        "witness_id": witness_id
    }))?;
    Ok(0)
}

/// Zero-based column index to spreadsheet letters (0 -> A, 26 -> AA).
fn column_letters(index: usize) -> String {
    let mut n = index + 1;
    let mut letters = String::new();
    while n > 0 {
        let rem = (n - 1) % 26;
        letters.insert(0, (b'A' + rem as u8) as char);
        n = (n - 1) / 26;
    }
    letters
}

/// `fingerprint peek x.xml`: the element-path skeleton from the streaming index (one pass over
/// the whole file, O(distinct paths) memory): root, element and path counts, depth, and for
/// each element path its depth, element count, attribute-name count, whether it repeats within
/// its parent and whether it is a leaf. Repeating non-leaf paths are the record candidates
/// (`summary.record_candidates`). Elements are identified by an ordinal ref `id` in document order;
/// `--explicit` adds the paths and the root namespace URI. No text and no attribute value is
/// ever read into the report, and attribute NAMES appear only as a count.
fn run_xml(
    args: &PeekArgs,
    append_witness_record: bool,
    marker: &mut Marker<'_>,
) -> Result<u8, String> {
    use crate::document::XmlDocument;

    let path = args.file.display().to_string();
    let refuse = |message: &str, detail: Value| -> Result<u8, String> {
        emit_refusal("E_BAD_INPUT", message, detail)?;
        Ok(2)
    };
    let xml = match XmlDocument::open(&args.file) {
        Ok(xml) => xml,
        Err(error) => {
            // The error names the byte position, never content.
            return refuse(
                "Could not parse xml",
                json!({ "path": path, "error": error.replace(&path, "<file>") }),
            );
        }
    };

    let depth_of = |p: &str| p.matches('/').count();
    let has_children = |p: &str| {
        let prefix = format!("{p}/");
        xml.paths.keys().any(|other| other.starts_with(&prefix))
    };
    let parent_count = |p: &str| -> Option<u64> {
        p.rfind('/')
            .filter(|index| *index > 0)
            .and_then(|index| xml.paths.get(&p[..index]).copied())
    };
    struct Row {
        path: String,
        depth: usize,
        count: u64,
        repeating: bool,
        leaf: bool,
        attributes: usize,
        id: String,
    }
    // Refs are numbered in first-appearance (document) order, then rows are ordered by depth with
    // a stable sort, so neither the numbering nor the order ever looks at an element name.
    let mut rows: Vec<Row> = xml
        .path_order
        .iter()
        .map(|p| {
            let count = xml.paths.get(p).copied().unwrap_or(0);
            Row {
                path: p.clone(),
                depth: depth_of(p),
                count,
                repeating: parent_count(p).is_some_and(|parent| count > parent),
                leaf: !has_children(p),
                attributes: xml.attribute_names.get(p).map_or(0, |names| names.len()),
                id: marker.mark(Kind::Path, p),
            }
        })
        .collect();
    rows.sort_by_key(|row| row.depth);
    let max_depth = rows.iter().map(|row| row.depth).max().unwrap_or(0);
    let repeating_paths = rows.iter().filter(|row| row.repeating).count();
    let candidates: Vec<Value> = rows
        .iter()
        .filter(|row| row.repeating && !row.leaf)
        .map(|row| {
            let prefix = format!("{}/", row.path);
            let leaf_children = xml
                .paths
                .keys()
                .filter(|other| {
                    other
                        .strip_prefix(&prefix)
                        .is_some_and(|rest| !rest.contains('/') && !has_children(other))
                })
                .count();
            let mut entry = json!({
                "id": row.id,
                "count": row.count,
                "leaf_children": leaf_children,
            });
            if args.explicit {
                entry["path"] = json!(row.path);
            }
            entry
        })
        .collect();
    let truncated = rows.len() > MAX_JSON_FIELDS;
    let paths: Vec<Value> = rows
        .iter()
        .take(MAX_JSON_FIELDS)
        .enumerate()
        .map(|(index, row)| {
            let mut entry = json!({
                "index": index,
                "id": row.id,
                "depth": row.depth,
                "count": row.count,
                "repeating": row.repeating,
                "leaf": row.leaf,
                "attribute_names": row.attributes,
            });
            if args.explicit {
                entry["path"] = json!(row.path);
            }
            entry
        })
        .collect();

    let mut root = json!({
        "id": marker.mark(Kind::Path, &format!("/{}", xml.root)),
        "namespace_id": xml.root_namespace.as_deref().map(|ns| marker.mark(Kind::Namespace, ns)),
    });
    if args.explicit {
        root["name"] = json!(xml.root);
        root["namespace"] = json!(xml.root_namespace);
    }
    let result = json!({
        "file": path,
        "format": "xml",
        "redaction": marker.mode(),
        "dialect": dialect_json("xml", "none", None),
        "root": root,
        "element_count": xml.element_count,
        "paths_total": rows.len(),
        "paths_truncated": truncated,
        "max_depth": max_depth,
        "summary": {
            "repeating_paths": repeating_paths,
            "record_candidates": candidates,
        },
        "paths": paths,
    });
    let result = apply_view(args, result);

    let witness_id = if append_witness_record {
        match hash_file(&args.file) {
            Ok((hash, bytes)) => append_peek_witness(args, Some(hash), bytes, &result),
            Err(error) => {
                eprintln!("Warning: Failed to hash xml file for the witness record: {error}");
                None
            }
        }
    } else {
        None
    };
    emit_json(&json!({
        "version": ENVELOPE_VERSION,
        "outcome": "SUCCESS",
        "exit_code": 0,
        "subcommand": "peek",
        "result": result,
        "witness_id": witness_id
    }))?;
    Ok(0)
}

/// A Parquet column's type in `profile inspect`'s column vocabulary (`integer`, `number`,
/// `boolean`, `text`). Dates, timestamps and times are `text` because that is what the shared
/// tabular reader renders them as before inspect classifies them; the exact Parquet types stay
/// in `physical_type` and `logical_type`.
fn inspect_type(physical: &str, logical: Option<&str>) -> &'static str {
    match (physical, logical) {
        (_, Some("decimal" | "float16")) => "number",
        (_, Some("string" | "enum" | "json" | "uuid" | "date" | "time" | "timestamp")) => "text",
        ("boolean", _) => "boolean",
        ("int32" | "int64" | "int96", _) => "integer",
        ("float" | "double", _) => "number",
        ("byte_array" | "fixed_len_byte_array", _) => "text",
        _ => "text",
    }
}

/// `fingerprint peek x.parquet`: the footer's structure only (no column chunk is read): row and
/// row-group counts, per-column physical/logical type, repetition, compression and null count.
/// Columns are identified by an ordinal ref in column order (`id`; a keyed marker with
/// `FINGERPRINT_REDACTION_KEY`, which is what makes schemas comparable across files without
/// printing names); `--explicit` adds the names and the file key/value metadata keys. Metadata VALUES and statistics min/max are never emitted.
fn run_parquet(
    args: &PeekArgs,
    append_witness_record: bool,
    marker: &mut Marker<'_>,
) -> Result<u8, String> {
    use parquet::file::metadata::ParquetMetaDataReader;

    let path = args.file.display().to_string();
    let refuse = |message: &str, detail: Value| -> Result<u8, String> {
        emit_refusal("E_BAD_INPUT", message, detail)?;
        Ok(2)
    };
    let file = match fs::File::open(&args.file) {
        Ok(file) => file,
        Err(error) => {
            return refuse(
                "Could not read input file",
                json!({ "path": path, "error": error.kind().to_string() }),
            );
        }
    };
    let metadata = match ParquetMetaDataReader::new().parse_and_finish(&file) {
        Ok(metadata) => metadata,
        Err(error) => {
            return refuse(
                "Could not read parquet footer",
                json!({ "path": path, "error": error.to_string() }),
            );
        }
    };

    let file_meta = metadata.file_metadata();
    let schema = file_meta.schema_descr();
    let row_groups = metadata.row_groups();
    let mut physical_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut logical_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut columns = Vec::with_capacity(schema.num_columns());
    for (index, column) in schema.columns().iter().enumerate() {
        let physical = format!("{:?}", column.physical_type()).to_ascii_lowercase();
        let logical = column.logical_type_ref().map(|logical| {
            let debug = format!("{logical:?}");
            let end = debug
                .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .unwrap_or(debug.len());
            debug[..end].to_ascii_lowercase()
        });
        *physical_counts.entry(physical.clone()).or_insert(0) += 1;
        if let Some(logical) = &logical {
            *logical_counts.entry(logical.clone()).or_insert(0) += 1;
        }
        let mut compressions = std::collections::BTreeSet::new();
        let mut nulls: Option<u64> = Some(0);
        for group in row_groups {
            let chunk = group.column(index);
            compressions.insert(format!("{:?}", chunk.compression_codec()).to_ascii_lowercase());
            nulls = match (nulls, chunk.statistics().and_then(|s| s.null_count_opt())) {
                (Some(total), Some(count)) => Some(total + count),
                _ => None,
            };
        }
        let name = column.path().string();
        let mut entry = json!({
            "index": index,
            "id": marker.mark(Kind::Column, &name),
            "type": inspect_type(&physical, logical.as_deref()),
            "physical_type": physical,
            "logical_type": logical,
            "repetition": format!("{:?}", column.self_type().get_basic_info().repetition()).to_ascii_lowercase(),
            "max_definition_level": column.max_def_level(),
            "max_repetition_level": column.max_rep_level(),
            "compression": compressions,
            "null_count": nulls,
        });
        if args.explicit {
            entry["name"] = json!(name);
        }
        columns.push(entry);
    }

    let group_rows: Vec<i64> = row_groups.iter().map(|group| group.num_rows()).collect();
    let nested = schema
        .columns()
        .iter()
        .filter(|column| column.max_rep_level() > 0 || column.path().parts().len() > 1)
        .count();
    let mut result = json!({
        "file": path,
        "format": "parquet",
        "redaction": marker.mode(),
        "dialect": dialect_json("parquet", "none", None),
        // profile inspect's names for the footer's exact counts (`rows_total` and
        // `columns_total` are kept).
        "row_count": file_meta.num_rows(),
        "column_count": columns.len(),
        "rows_total": file_meta.num_rows(),
        "row_groups": {
            "count": row_groups.len(),
            "rows_min": group_rows.iter().min(),
            "rows_max": group_rows.iter().max(),
        },
        "columns_total": columns.len(),
        "summary": {
            "physical_types": physical_counts,
            "logical_types": logical_counts,
            "nested_columns": nested,
            "columns_with_unknown_null_count": columns.iter().filter(|c| c["null_count"].is_null()).count(),
        },
        "columns": columns,
        "key_value_metadata_count": file_meta.key_value_metadata().map_or(0, Vec::len),
        "created_by": file_meta.created_by(),
    });
    if args.explicit {
        result["key_value_metadata_keys"] = json!(
            file_meta
                .key_value_metadata()
                .map(|pairs| pairs
                    .iter()
                    .map(|pair| pair.key.clone())
                    .collect::<Vec<_>>())
                .unwrap_or_default()
        );
    }
    let result = apply_view(args, result);

    let witness_id = if append_witness_record {
        match hash_file(&args.file) {
            Ok((hash, bytes)) => append_peek_witness(args, Some(hash), bytes, &result),
            Err(error) => {
                eprintln!("Warning: Failed to hash parquet file for the witness record: {error}");
                None
            }
        }
    } else {
        None
    };
    emit_json(&json!({
        "version": ENVELOPE_VERSION,
        "outcome": "SUCCESS",
        "exit_code": 0,
        "subcommand": "peek",
        "result": result,
        "witness_id": witness_id
    }))?;
    Ok(0)
}

fn emit_refusal(code: &str, message: &str, detail: Value) -> Result<(), String> {
    emit_json(&json!({
        "version": ENVELOPE_VERSION,
        "outcome": "REFUSAL",
        "exit_code": 2,
        "subcommand": "peek",
        "refusal": {
            "code": code,
            "message": message,
            "detail": detail,
            "next_command": if code == crate::redaction::REFUSAL_CODE {
                "Set FINGERPRINT_REDACTION_KEY to at least 16 bytes, or unset it to use ordinal refs"
            } else {
                "Rerun fingerprint peek with a readable CSV, TSV, XLSX, Parquet, JSON, JSONL, XML, HTML or PDF file"
            }
        },
        "witness_id": null
    }))
}

fn emit_json(value: &Value) -> Result<(), String> {
    println!(
        "{}",
        serde_json::to_string_pretty(value)
            .map_err(|error| format!("failed to serialize peek output: {error}"))?
    );
    Ok(())
}

#[derive(Debug, Clone)]
struct DelimiterGuess {
    name: &'static str,
    byte: u8,
    confidence: f64,
}

fn guess_delimiter(guess_lines: &[Vec<u8>]) -> DelimiterGuess {
    let owned = guess_lines
        .iter()
        .map(|line| String::from_utf8_lossy(line).into_owned())
        .collect::<Vec<_>>();
    let lines = owned.iter().map(String::as_str).collect::<Vec<_>>();

    let candidates = [
        ("comma", b','),
        ("tab", b'\t'),
        ("semicolon", b';'),
        ("pipe", b'|'),
    ];
    let mut scored = candidates
        .into_iter()
        .map(|(name, byte)| {
            let counts = lines
                .iter()
                .map(|line| {
                    line.as_bytes()
                        .iter()
                        .filter(|candidate| **candidate == byte)
                        .count()
                })
                .collect::<Vec<_>>();
            let populated = counts.iter().filter(|count| **count > 0).count();
            let total = counts.iter().sum::<usize>();
            let consistency = most_common_count(&counts)
                .map(|(_, count)| count as f64 / counts.len().max(1) as f64)
                .unwrap_or(0.0);
            (name, byte, populated, total, consistency)
        })
        .collect::<Vec<_>>();

    scored.sort_by(|left, right| {
        right
            .2
            .cmp(&left.2)
            .then(right.3.cmp(&left.3))
            .then_with(|| right.4.total_cmp(&left.4))
    });

    let (name, byte, populated, total, consistency) = scored
        .first()
        .copied()
        .unwrap_or(("comma", b',', 0, 0, 0.0));
    let confidence = if lines.is_empty() || total == 0 {
        0.0
    } else {
        ((populated as f64 / lines.len() as f64) * 0.55 + consistency * 0.45).min(1.0)
    };

    DelimiterGuess {
        name,
        byte,
        confidence: round_confidence(confidence),
    }
}

fn inspect_rows(row_lines: &[Vec<u8>], delimiter: u8) -> Result<Vec<RawRowShape>, String> {
    let mut rows = Vec::new();

    for (index, raw) in row_lines.iter().enumerate() {
        // A line cut at MAX_LINE_BYTES may end inside a UTF-8 sequence; that is not invalid text.
        let line = match std::str::from_utf8(raw) {
            Ok(line) => line,
            Err(error) if error.error_len().is_none() && raw.len() >= MAX_LINE_BYTES => {
                std::str::from_utf8(&raw[..error.valid_up_to()])
                    .map_err(|_| "input is not valid UTF-8 text".to_owned())?
            }
            Err(_) => return Err("input is not valid UTF-8 text".to_owned()),
        };
        if line.trim().is_empty() {
            rows.push(RawRowShape {
                row_index: index + 1,
                column_count: 0,
                non_empty_count: 0,
                type_counts: empty_type_counts(),
                length_buckets: empty_length_buckets(),
                has_unit_tokens: false,
            });
            continue;
        }

        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .delimiter(delimiter)
            .from_reader(line.as_bytes());
        let record = reader
            .records()
            .next()
            .transpose()
            .map_err(|_| "CSV parser rejected row structure".to_owned())?
            .ok_or_else(|| "CSV parser produced no row".to_owned())?;

        let mut non_empty_count = 0usize;
        let mut type_counts = empty_type_counts();
        let mut length_buckets = empty_length_buckets();
        let mut has_unit_tokens = false;
        let mut column_count = record.len();

        if record.len() == 1 && record.get(0).is_some_and(|cell| cell.trim().is_empty()) {
            column_count = 0;
        }

        for cell in &record {
            let trimmed = cell.trim();
            if !trimmed.is_empty() {
                non_empty_count += 1;
            }
            let class = classify_cell(trimmed);
            *type_counts.entry(class).or_insert(0) += 1;
            *length_buckets
                .entry(length_bucket(trimmed.len()))
                .or_insert(0) += 1;
            if looks_like_unit_token(trimmed) {
                has_unit_tokens = true;
            }
        }

        rows.push(RawRowShape {
            row_index: index + 1,
            column_count,
            non_empty_count,
            type_counts,
            length_buckets,
            has_unit_tokens,
        });
    }

    Ok(rows)
}

fn empty_type_counts() -> BTreeMap<&'static str, usize> {
    [
        ("empty", 0),
        ("text", 0),
        ("numeric", 0),
        ("date", 0),
        ("boolean", 0),
        ("mixed", 0),
    ]
    .into_iter()
    .collect()
}

fn empty_length_buckets() -> BTreeMap<&'static str, usize> {
    [("0", 0), ("1-4", 0), ("5-16", 0), ("17-64", 0), ("65+", 0)]
        .into_iter()
        .collect()
}

fn classify_cell(value: &str) -> &'static str {
    if value.is_empty() {
        return "empty";
    }
    let lower = value.to_ascii_lowercase();
    if matches!(lower.as_str(), "true" | "false" | "yes" | "no") {
        return "boolean";
    }
    if is_dateish(value) {
        return "date";
    }
    if is_numericish(value) {
        return "numeric";
    }
    if value.chars().any(|ch| ch.is_ascii_digit()) && value.chars().any(|ch| ch.is_alphabetic()) {
        return "mixed";
    }
    "text"
}

fn is_numericish(value: &str) -> bool {
    let cleaned = value
        .trim_matches('%')
        .trim_matches('$')
        .replace([',', '_'], "");
    if cleaned.is_empty() {
        return false;
    }
    cleaned.parse::<f64>().is_ok()
}

fn is_dateish(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
    {
        return true;
    }
    if (8..=10).contains(&bytes.len()) && value.matches('/').count() == 2 {
        return value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'/');
    }
    false
}

fn length_bucket(length: usize) -> &'static str {
    match length {
        0 => "0",
        1..=4 => "1-4",
        5..=16 => "5-16",
        17..=64 => "17-64",
        _ => "65+",
    }
}

fn looks_like_unit_token(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "%" | "usd" | "eur" | "gbp" | "count" | "units" | "unit" | "date" | "$"
    ) || lower.contains("/sqft")
        || lower.contains("per ")
        || value.contains('%')
        || value.contains('$')
}

fn modal_positive_column_count(rows: &[RawRowShape]) -> Option<usize> {
    let counts = rows
        .iter()
        .filter_map(|row| (row.column_count > 0).then_some(row.column_count))
        .collect::<Vec<_>>();
    most_common_count(&counts).map(|(value, _)| value)
}

fn most_common_count(counts: &[usize]) -> Option<(usize, usize)> {
    let mut frequency: BTreeMap<usize, usize> = BTreeMap::new();
    for count in counts {
        *frequency.entry(*count).or_insert(0) += 1;
    }
    frequency
        .into_iter()
        .max_by(|left, right| left.1.cmp(&right.1).then_with(|| right.0.cmp(&left.0)))
}

fn row_to_json(row: &RawRowShape, modal_column_count: Option<usize>) -> Value {
    json!({
        "row_index": row.row_index,
        "column_count": row.column_count,
        "non_empty_count": row.non_empty_count,
        "shape_class": shape_class(row, modal_column_count),
        "cell_type_counts": row.type_counts,
        "cell_length_histogram": row.length_buckets
    })
}

fn shape_class(row: &RawRowShape, modal_column_count: Option<usize>) -> &'static str {
    if row.non_empty_count == 0 || row.column_count == 0 {
        return "blank";
    }
    if row.column_count == 1
        && row
            .length_buckets
            .get("65+")
            .is_some_and(|long_values| *long_values > 0)
    {
        return "paragraph";
    }
    if row.has_unit_tokens && row.non_empty_count >= 2 {
        return "dense-units";
    }
    if row.column_count <= 2 {
        return "sparse-label";
    }
    if row.non_empty_count.saturating_mul(2) <= row.column_count {
        return "sparse-multi";
    }

    let text_count = row.type_counts.get("text").copied().unwrap_or(0);
    let numeric_count = row.type_counts.get("numeric").copied().unwrap_or(0);
    let date_count = row.type_counts.get("date").copied().unwrap_or(0);
    let mixed_count = row.type_counts.get("mixed").copied().unwrap_or(0);
    let dense_modal = modal_column_count.is_some_and(|modal| modal == row.column_count);

    if dense_modal && numeric_count + date_count + mixed_count > 0 {
        "dense-data"
    } else if dense_modal && text_count > 0 {
        "dense-header-candidate"
    } else if dense_modal {
        "dense-data"
    } else {
        "irregular"
    }
}

fn summarize_rows(rows: &[RawRowShape], modal_column_count: Option<usize>) -> Value {
    let header_at_row = estimate_header_row(rows, modal_column_count);
    let preamble_rows = header_at_row.saturating_sub(1);
    let modal_rows = modal_column_count
        .map(|modal| rows.iter().filter(|row| row.column_count == modal).count())
        .unwrap_or(0);
    let data_starts_at = estimate_data_start(rows, modal_column_count, header_at_row);

    json!({
        "modal_column_count": modal_column_count,
        "rows_with_modal_column_count": modal_rows,
        "preamble_rows": preamble_rows,
        "header_at_row": header_at_row,
        // `profile inspect` calls the same concept `suggestions.header_row` (bd-jxd).
        "header_row": header_at_row,
        "data_starts_at": data_starts_at,
        "shape_classes": shape_class_counts(rows, modal_column_count)
    })
}

/// The dialect object `profile inspect` uses (`name` is the format, `delimiter` the character
/// as text with a tab written `\t`, or `none` for formats without one), so the two commands
/// share one name for the concept. `extra` carries peek-only facts (the guess confidence).
fn dialect_json(name: &str, delimiter: &str, extra: Option<(&str, Value)>) -> Value {
    let mut dialect = json!({ "name": name, "delimiter": delimiter });
    if let Some((key, value)) = extra {
        dialect[key] = value;
    }
    dialect
}

fn delimiter_text(byte: u8) -> String {
    match byte {
        b'\t' => "\\t".to_owned(),
        other => (other as char).to_string(),
    }
}

fn shape_class_counts(rows: &[RawRowShape], modal_column_count: Option<usize>) -> Value {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for row in rows {
        *counts
            .entry(shape_class(row, modal_column_count))
            .or_insert(0) += 1;
    }
    json!(counts)
}

fn estimate_header_row(rows: &[RawRowShape], modal_column_count: Option<usize>) -> usize {
    rows.iter()
        .find(|row| shape_class(row, modal_column_count) == "dense-header-candidate")
        .map(|row| row.row_index)
        .or_else(|| {
            rows.iter()
                .find(|row| modal_column_count.is_some_and(|modal| row.column_count == modal))
                .map(|row| row.row_index)
        })
        .unwrap_or(1)
}

fn estimate_data_start(
    rows: &[RawRowShape],
    modal_column_count: Option<usize>,
    header_at_row: usize,
) -> usize {
    rows.iter()
        .find(|row| {
            row.row_index > header_at_row
                && modal_column_count.is_some_and(|modal| row.column_count == modal)
                && shape_class(row, modal_column_count) == "dense-data"
        })
        .map(|row| row.row_index)
        .unwrap_or(header_at_row + 1)
}

fn suggestions(rows: &[RawRowShape], modal_column_count: Option<usize>) -> Value {
    let header_at_row = estimate_header_row(rows, modal_column_count);
    let data_starts_at = estimate_data_start(rows, modal_column_count, header_at_row);
    let unit_rows = rows
        .iter()
        .filter(|row| {
            row.row_index > header_at_row
                && row.row_index < data_starts_at
                && shape_class(row, modal_column_count) == "dense-units"
        })
        .map(|row| row.row_index)
        .collect::<Vec<_>>();

    let mode = if !unit_rows.is_empty() {
        "preamble_with_units"
    } else {
        "preamble_skip"
    };

    json!({
        "profile_pre_parse": {
            "mode": mode,
            "skip_rows": header_at_row.saturating_sub(1),
            "header_at_row": header_at_row,
            "unit_rows": unit_rows,
            "data_starts_at": data_starts_at
        },
        "confidence": confidence(rows, modal_column_count, header_at_row)
    })
}

fn confidence(
    rows: &[RawRowShape],
    modal_column_count: Option<usize>,
    header_at_row: usize,
) -> f64 {
    let Some(modal) = modal_column_count else {
        return 0.0;
    };
    let modal_rows = rows.iter().filter(|row| row.column_count == modal).count();
    let coverage = modal_rows as f64 / rows.len().max(1) as f64;
    let header_bonus = if header_at_row > 0 { 0.15 } else { 0.0 };
    round_confidence((coverage * 0.85 + header_bonus).min(1.0))
}

fn round_confidence(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn append_peek_witness(
    args: &PeekArgs,
    input_hash: Option<String>,
    input_bytes: u64,
    result: &Value,
) -> Option<String> {
    let output_bytes = match serde_json::to_vec(result) {
        Ok(output_bytes) => output_bytes,
        Err(error) => {
            eprintln!("Warning: Failed to record peek witness: {error}");
            return None;
        }
    };
    let record = match WitnessRecord::new(
        env!("CARGO_PKG_VERSION").to_owned(),
        crate::current_binary_hash(),
        vec![WitnessInput {
            path: args.file.display().to_string(),
            hash: input_hash,
            bytes: Some(input_bytes),
        }],
        {
            let mut params = json!({
                "subcommand": "peek",
                "rows": args.rows.max(1),
                "suggest": args.suggest,
                "file": args.file.display().to_string()
            });
            // Recorded only when given, so CSV witness params are unchanged.
            if let (Some(sheet), Some(object)) = (&args.sheet, params.as_object_mut()) {
                object.insert("sheet".to_owned(), json!(sheet));
            }
            params
        },
        "SUCCESS",
        0,
        format!("blake3:{}", blake3::hash(&output_bytes).to_hex()),
        chrono::Utc::now().to_rfc3339(),
    ) {
        Ok(record) => record,
        Err(error) => {
            eprintln!("Warning: Failed to record peek witness: {error}");
            return None;
        }
    };
    let witness_id = record.id.clone();
    let ledger = match ledger_path_for_append() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("Warning: Failed to prepare peek witness ledger: {error}");
            return None;
        }
    };
    if let Err(error) = append(&ledger, &record) {
        eprintln!("Warning: Failed to record peek witness: {error}");
        return None;
    }
    Some(witness_id)
}
