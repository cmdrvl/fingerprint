use std::path::Path;
use std::{
    fs::File,
    io::{BufRead, BufReader, ErrorKind},
};

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::cli::WitnessFilters;

/// Query witness records matching filter criteria.
pub fn query(ledger_path: &Path, filters: &WitnessFilters) -> Result<Vec<Value>, String> {
    let records = read_records(ledger_path)?;
    filter_records(records, filters)
}

/// Return the last witness record matching filter criteria.
pub fn last(ledger_path: &Path, filters: &WitnessFilters) -> Result<Option<Value>, String> {
    let mut records = query(ledger_path, filters)?;
    Ok(records.pop())
}

/// Count witness records matching filter criteria.
pub fn count(ledger_path: &Path, filters: &WitnessFilters) -> Result<u64, String> {
    Ok(query(ledger_path, filters)?.len() as u64)
}

fn filter_records(records: Vec<Value>, filters: &WitnessFilters) -> Result<Vec<Value>, String> {
    let since = parse_bound("since", filters.since.as_deref())?;
    let until = parse_bound("until", filters.until.as_deref())?;

    Ok(records
        .into_iter()
        .filter(|record| matches_tool(record, filters.tool.as_deref()))
        .filter(|record| matches_outcome(record, filters.outcome.as_deref()))
        .filter(|record| within_bounds(record, since.as_ref(), until.as_ref()))
        .filter(|record| matches_input_hash(record, filters.input_hash.as_deref()))
        .collect())
}

fn parse_bound(label: &str, value: Option<&str>) -> Result<Option<DateTime<Utc>>, String> {
    let Some(value) = value else {
        return Ok(None);
    };

    DateTime::parse_from_rfc3339(value)
        .map(|ts| Some(ts.with_timezone(&Utc)))
        .map_err(|error| format!("invalid --{label} timestamp '{value}': {error}"))
}

fn matches_tool(record: &Value, tool: Option<&str>) -> bool {
    match tool {
        Some(tool) => record.get("tool").and_then(Value::as_str) == Some(tool),
        None => true,
    }
}

fn matches_outcome(record: &Value, outcome: Option<&str>) -> bool {
    match outcome {
        Some(outcome) => record.get("outcome").and_then(Value::as_str) == Some(outcome),
        None => true,
    }
}

fn matches_input_hash(record: &Value, input_hash: Option<&str>) -> bool {
    let Some(input_hash) = input_hash else {
        return true;
    };

    record
        .get("inputs")
        .and_then(Value::as_array)
        .is_some_and(|inputs| {
            inputs.iter().any(|input| {
                input
                    .get("hash")
                    .and_then(Value::as_str)
                    .is_some_and(|hash| hash.contains(input_hash))
            })
        })
}

fn within_bounds(
    record: &Value,
    since: Option<&DateTime<Utc>>,
    until: Option<&DateTime<Utc>>,
) -> bool {
    if since.is_none() && until.is_none() {
        return true;
    }

    let Some(ts) = record_timestamp(record) else {
        return false;
    };

    if let Some(since) = since
        && ts < *since
    {
        return false;
    }

    if let Some(until) = until
        && ts > *until
    {
        return false;
    }

    true
}

fn record_timestamp(record: &Value) -> Option<DateTime<Utc>> {
    record
        .get("ts")
        .or_else(|| record.get("created_ts"))
        .and_then(Value::as_str)
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|ts| ts.with_timezone(&Utc))
}

fn read_records(ledger_path: &Path) -> Result<Vec<Value>, String> {
    let file = match File::open(ledger_path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(format!(
                "failed to open witness ledger '{}': {error}",
                ledger_path.display()
            ));
        }
    };

    let reader = BufReader::new(file);
    let mut records = Vec::new();

    for (line_index, line_result) in reader.lines().enumerate() {
        let line_number = line_index + 1;
        let line = line_result.map_err(|error| {
            format!(
                "failed to read witness ledger '{}' at line {}: {error}",
                ledger_path.display(),
                line_number
            )
        })?;

        if line.trim().is_empty() {
            continue;
        }

        let value = serde_json::from_str::<Value>(&line).map_err(|error| {
            format!(
                "invalid witness JSON at '{}' line {}: {error}",
                ledger_path.display(),
                line_number
            )
        })?;
        records.push(value);
    }

    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::{count, last, query};
    use crate::cli::WitnessFilters;
    use serde_json::json;
    use std::fs;

    fn write_ledger(contents: &str) -> tempfile::NamedTempFile {
        let ledger = tempfile::NamedTempFile::new().expect("create temp ledger");
        fs::write(ledger.path(), contents).expect("write ledger");
        ledger
    }

    fn record_line(
        id: &str,
        tool: &str,
        outcome: &str,
        input_hash: Option<&str>,
        ts: &str,
    ) -> String {
        json!({
            "id": id,
            "tool": tool,
            "version": "0.1.0",
            "binary_hash": "blake3:binary",
            "inputs": [{
                "path": "stdin",
                "hash": input_hash,
                "bytes": 10
            }],
            "params": { "fingerprints": ["csv.v0"] },
            "outcome": outcome,
            "exit_code": if outcome == "PARTIAL" { 1 } else { 0 },
            "output_hash": format!("blake3:out:{id}"),
            "prev": null,
            "ts": ts
        })
        .to_string()
    }

    #[test]
    fn query_returns_all_records_without_filters() {
        let ledger = write_ledger(&format!(
            "{}\n{}\n",
            record_line(
                "1",
                "fingerprint",
                "ALL_MATCHED",
                Some("blake3:in-1"),
                "2026-01-01T00:00:00Z"
            ),
            record_line(
                "2",
                "fingerprint",
                "PARTIAL",
                Some("blake3:in-2"),
                "2026-01-01T01:00:00Z"
            )
        ));

        let records = query(ledger.path(), &WitnessFilters::default()).expect("query ledger");
        assert_eq!(records.len(), 2);
        assert_eq!(records[0]["id"], "1");
        assert_eq!(records[1]["id"], "2");
    }

    #[test]
    fn query_filters_by_tool_outcome_and_input_hash() {
        let ledger = write_ledger(&format!(
            "{}\n{}\n{}\n",
            record_line(
                "1",
                "fingerprint",
                "ALL_MATCHED",
                Some("blake3:keep"),
                "2026-01-01T00:00:00Z"
            ),
            record_line(
                "2",
                "hash",
                "ALL_MATCHED",
                Some("blake3:keep"),
                "2026-01-01T01:00:00Z"
            ),
            record_line(
                "3",
                "fingerprint",
                "PARTIAL",
                Some("blake3:drop"),
                "2026-01-01T02:00:00Z"
            )
        ));
        let filters = WitnessFilters {
            tool: Some("fingerprint".to_owned()),
            since: None,
            until: None,
            outcome: Some("ALL_MATCHED".to_owned()),
            input_hash: Some("keep".to_owned()),
        };

        let records = query(ledger.path(), &filters).expect("filtered query");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["id"], "1");
    }

    #[test]
    fn query_filters_by_time_bounds_and_legacy_created_ts() {
        let ledger = write_ledger(&format!(
            "{}\n{}\n",
            json!({
                "id": "1",
                "tool": "fingerprint",
                "outcome": "ALL_MATCHED",
                "inputs": [],
                "created_ts": "2026-01-01T00:00:00Z"
            }),
            record_line(
                "2",
                "fingerprint",
                "ALL_MATCHED",
                Some("blake3:in-2"),
                "2026-01-02T00:00:00Z"
            )
        ));
        let filters = WitnessFilters {
            tool: None,
            since: Some("2026-01-01T12:00:00Z".to_owned()),
            until: Some("2026-01-02T12:00:00Z".to_owned()),
            outcome: None,
            input_hash: None,
        };

        let records = query(ledger.path(), &filters).expect("time-bounded query");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["id"], "2");
    }

    #[test]
    fn last_returns_last_matching_record() {
        let ledger = write_ledger(&format!(
            "{}\n{}\n{}\n",
            record_line(
                "1",
                "fingerprint",
                "ALL_MATCHED",
                Some("blake3:in-1"),
                "2026-01-01T00:00:00Z"
            ),
            record_line(
                "2",
                "hash",
                "ALL_HASHED",
                Some("blake3:in-2"),
                "2026-01-01T01:00:00Z"
            ),
            record_line(
                "3",
                "fingerprint",
                "PARTIAL",
                Some("blake3:in-3"),
                "2026-01-01T02:00:00Z"
            )
        ));
        let filters = WitnessFilters {
            tool: Some("fingerprint".to_owned()),
            since: None,
            until: None,
            outcome: None,
            input_hash: None,
        };

        let record = last(ledger.path(), &filters)
            .expect("last record")
            .expect("record exists");
        assert_eq!(record["id"], "3");
    }

    #[test]
    fn count_returns_number_of_matching_records() {
        let ledger = write_ledger(&format!(
            "{}\n{}\n",
            record_line(
                "1",
                "fingerprint",
                "ALL_MATCHED",
                Some("blake3:shared"),
                "2026-01-01T00:00:00Z"
            ),
            record_line(
                "2",
                "fingerprint",
                "PARTIAL",
                Some("blake3:shared"),
                "2026-01-01T01:00:00Z"
            )
        ));
        let filters = WitnessFilters {
            tool: Some("fingerprint".to_owned()),
            since: None,
            until: None,
            outcome: None,
            input_hash: Some("shared".to_owned()),
        };

        assert_eq!(count(ledger.path(), &filters).expect("count ledger"), 2);
    }

    #[test]
    fn missing_ledger_returns_empty_results() {
        let tempdir = tempfile::tempdir().expect("create temp dir");
        let ledger_path = tempdir.path().join("missing.jsonl");

        assert!(
            query(&ledger_path, &WitnessFilters::default())
                .expect("query missing file")
                .is_empty()
        );
        assert_eq!(
            last(&ledger_path, &WitnessFilters::default()).expect("last missing file"),
            None
        );
        assert_eq!(
            count(&ledger_path, &WitnessFilters::default()).expect("count missing file"),
            0
        );
    }

    #[test]
    fn invalid_json_line_returns_error() {
        let ledger = write_ledger("{\"id\":\"1\"}\nnot-json\n");

        let error =
            query(ledger.path(), &WitnessFilters::default()).expect_err("query should fail");
        assert!(error.contains("line 2"));
    }

    #[test]
    fn invalid_timestamp_filter_returns_error() {
        let ledger = write_ledger(&record_line(
            "1",
            "fingerprint",
            "ALL_MATCHED",
            Some("blake3:in-1"),
            "2026-01-01T00:00:00Z",
        ));
        let filters = WitnessFilters {
            tool: None,
            since: Some("not-a-timestamp".to_owned()),
            until: None,
            outcome: None,
            input_hash: None,
        };

        let error = query(ledger.path(), &filters).expect_err("invalid filter should fail");
        assert!(error.contains("--since"));
    }
}
