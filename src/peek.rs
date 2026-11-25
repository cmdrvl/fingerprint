use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader};

use serde_json::{Value, json};

use crate::cli::args::PeekArgs;
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
const NOT_APPLICABLE_EXTENSIONS: &[&str] = &[
    "xlsx", "xlsm", "xls", "parquet", "pdf", "zip", "gz", "docx", "pptx", "png", "jpg", "jpeg",
];

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
    let max_rows = args.rows.max(1);
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
    let mut result = json!({
        "file": args.file.display().to_string(),
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

    let witness_id = if append_witness_record {
        append_peek_witness(args, &input, &result)
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
            "next_command": "Rerun fingerprint peek with a readable CSV-like text file"
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
        "data_starts_at": data_starts_at,
        "shape_classes": shape_class_counts(rows, modal_column_count)
    })
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

fn append_peek_witness(args: &PeekArgs, input: &PeekInput, result: &Value) -> Option<String> {
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
            hash: input.blake3.clone(),
            bytes: Some(input.total_bytes),
        }],
        json!({
            "subcommand": "peek",
            "rows": args.rows.max(1),
            "suggest": args.suggest,
            "file": args.file.display().to_string()
        }),
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
