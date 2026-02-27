use std::path::Path;
use std::{
    fs::File,
    io::{BufRead, BufReader, ErrorKind},
};

/// Query witness records matching filter criteria.
pub fn query(ledger_path: &Path) -> Result<Vec<serde_json::Value>, String> {
    read_records(ledger_path)
}

/// Return the last witness record from the ledger.
pub fn last(ledger_path: &Path) -> Result<Option<serde_json::Value>, String> {
    let mut records = read_records(ledger_path)?;
    Ok(records.pop())
}

/// Count witness records matching filter criteria.
pub fn count(ledger_path: &Path) -> Result<u64, String> {
    Ok(read_records(ledger_path)?.len() as u64)
}

fn read_records(ledger_path: &Path) -> Result<Vec<serde_json::Value>, String> {
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

        let value = serde_json::from_str::<serde_json::Value>(&line).map_err(|error| {
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
    use std::fs;

    #[test]
    fn query_returns_all_records() {
        let tempdir = tempfile::tempdir().expect("create temp dir");
        let ledger_path = tempdir.path().join("witness.jsonl");
        fs::write(
            &ledger_path,
            "{\"id\":\"1\",\"tool\":\"fingerprint\"}\n{\"id\":\"2\",\"tool\":\"fingerprint\"}\n",
        )
        .expect("write ledger");

        let records = query(&ledger_path).expect("query ledger");
        assert_eq!(records.len(), 2);
        assert_eq!(records[0]["id"], "1");
        assert_eq!(records[1]["id"], "2");
    }

    #[test]
    fn last_returns_most_recent_record() {
        let tempdir = tempfile::tempdir().expect("create temp dir");
        let ledger_path = tempdir.path().join("witness.jsonl");
        fs::write(
            &ledger_path,
            "{\"id\":\"1\",\"tool\":\"fingerprint\"}\n{\"id\":\"2\",\"tool\":\"fingerprint\"}\n",
        )
        .expect("write ledger");

        let record = last(&ledger_path)
            .expect("last record")
            .expect("record exists");
        assert_eq!(record["id"], "2");
    }

    #[test]
    fn count_returns_number_of_records() {
        let tempdir = tempfile::tempdir().expect("create temp dir");
        let ledger_path = tempdir.path().join("witness.jsonl");
        fs::write(
            &ledger_path,
            "{\"id\":\"1\",\"tool\":\"fingerprint\"}\n{\"id\":\"2\",\"tool\":\"fingerprint\"}\n",
        )
        .expect("write ledger");

        assert_eq!(count(&ledger_path).expect("count ledger"), 2);
    }

    #[test]
    fn missing_ledger_returns_empty_results() {
        let tempdir = tempfile::tempdir().expect("create temp dir");
        let ledger_path = tempdir.path().join("missing.jsonl");

        assert!(query(&ledger_path).expect("query missing file").is_empty());
        assert_eq!(last(&ledger_path).expect("last missing file"), None);
        assert_eq!(count(&ledger_path).expect("count missing file"), 0);
    }

    #[test]
    fn invalid_json_line_returns_error() {
        let tempdir = tempfile::tempdir().expect("create temp dir");
        let ledger_path = tempdir.path().join("witness.jsonl");
        fs::write(&ledger_path, "{\"id\":\"1\"}\nnot-json\n").expect("write ledger");

        let error = query(&ledger_path).expect_err("query should fail");
        assert!(error.contains("line 2"));
    }
}
