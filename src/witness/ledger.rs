use crate::witness::record::WitnessRecord;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

/// Append a witness record to the ledger file.
pub fn append(ledger_path: &Path, record: &WitnessRecord) -> Result<(), String> {
    if let Some(parent) = ledger_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "failed to create witness directory '{}': {error}",
                parent.display()
            )
        })?;
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(ledger_path)
        .map_err(|error| {
            format!(
                "failed to open witness ledger '{}': {error}",
                ledger_path.display()
            )
        })?;

    let line = record.to_jsonl()?;
    file.write_all(line.as_bytes()).map_err(|error| {
        format!(
            "failed to append witness record to '{}': {error}",
            ledger_path.display()
        )
    })?;
    file.flush().map_err(|error| {
        format!(
            "failed to flush witness ledger '{}': {error}",
            ledger_path.display()
        )
    })?;

    Ok(())
}

/// Resolve the witness ledger path from `$EPISTEMIC_WITNESS` or default.
pub fn ledger_path() -> std::path::PathBuf {
    ledger_path_from_env(|key| std::env::var(key).ok())
}

fn ledger_path_from_env<F>(get_env: F) -> std::path::PathBuf
where
    F: Fn(&str) -> Option<String>,
{
    if let Some(path) = get_env("EPISTEMIC_WITNESS")
        && !path.trim().is_empty()
    {
        return path.into();
    }

    if let Some(home) = get_env("HOME")
        && !home.trim().is_empty()
    {
        return std::path::PathBuf::from(home)
            .join(".epistemic")
            .join("witness.jsonl");
    }

    std::path::PathBuf::from(".epistemic/witness.jsonl")
}

#[cfg(test)]
mod tests {
    use super::{append, ledger_path_from_env};
    use crate::witness::record::{WitnessInput, WitnessRecord};
    use serde_json::json;
    use std::fs;

    fn sample_record() -> WitnessRecord {
        WitnessRecord::new(
            "0.1.0",
            "blake3:binary",
            vec![WitnessInput {
                path: "stdin".to_owned(),
                hash: None,
                bytes: None,
            }],
            json!({ "fingerprints": ["csv.v0"], "jobs": 1 }),
            "ALL_MATCHED",
            0,
            "blake3:output",
            None,
            "2026-02-24T10:00:00Z",
        )
        .expect("build witness record")
    }

    #[test]
    fn ledger_path_prefers_epistemic_witness_env_var() {
        let path = ledger_path_from_env(|key| match key {
            "EPISTEMIC_WITNESS" => Some("/tmp/custom-witness.jsonl".to_owned()),
            "HOME" => Some("/tmp/home".to_owned()),
            _ => None,
        });

        assert_eq!(path, std::path::PathBuf::from("/tmp/custom-witness.jsonl"));
    }

    #[test]
    fn ledger_path_defaults_to_home_epistemic_path() {
        let path = ledger_path_from_env(|key| match key {
            "EPISTEMIC_WITNESS" => None,
            "HOME" => Some("/tmp/home".to_owned()),
            _ => None,
        });

        assert_eq!(
            path,
            std::path::PathBuf::from("/tmp/home/.epistemic/witness.jsonl")
        );
    }

    #[test]
    fn append_creates_parent_dirs_and_writes_jsonl() {
        let tempdir = tempfile::tempdir().expect("create temp dir");
        let ledger_path = tempdir.path().join("nested").join("witness.jsonl");

        append(&ledger_path, &sample_record()).expect("append first record");
        append(&ledger_path, &sample_record()).expect("append second record");

        let content = fs::read_to_string(&ledger_path).expect("read witness ledger");
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);

        for line in lines {
            let value: serde_json::Value = serde_json::from_str(line).expect("parse JSONL line");
            assert_eq!(value["tool"], "fingerprint");
            assert_eq!(value["outcome"], "ALL_MATCHED");
        }
    }
}
