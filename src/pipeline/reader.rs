use serde_json::{Map, Value};
use std::fmt;
use std::io::BufRead;

const SUPPORTED_UPSTREAM_VERSIONS: &[&str] = &["hash.v0"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BadInputKind {
    InvalidJson {
        error: String,
    },
    RecordNotObject,
    MissingField {
        field: String,
    },
    InvalidFieldType {
        field: String,
        expected: &'static str,
    },
    UnknownVersion {
        version: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReaderError {
    ReadFailure { error: String },
    BadInput { line: u64, kind: BadInputKind },
}

impl fmt::Display for BadInputKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidJson { error } => write!(f, "invalid JSON: {error}"),
            Self::RecordNotObject => f.write_str("record must be a JSON object"),
            Self::MissingField { field } => write!(f, "missing required field '{field}'"),
            Self::InvalidFieldType { field, expected } => {
                write!(f, "field '{field}' must be a {expected}")
            }
            Self::UnknownVersion { version } => {
                write!(f, "unrecognized upstream version '{version}'")
            }
        }
    }
}

impl fmt::Display for ReaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadFailure { error } => write!(f, "failed reading input: {error}"),
            Self::BadInput { line, kind } => write!(f, "line {line}: {kind}"),
        }
    }
}

impl std::error::Error for ReaderError {}

/// Read JSONL records from an input source, validating structure and version.
pub fn read_records(input: &mut dyn BufRead) -> Result<Vec<Value>, ReaderError> {
    let mut records = Vec::new();
    let mut line_number: u64 = 0;
    let mut line = String::new();

    loop {
        line.clear();
        let bytes_read = input
            .read_line(&mut line)
            .map_err(|error| ReaderError::ReadFailure {
                error: error.to_string(),
            })?;
        if bytes_read == 0 {
            break;
        }
        line_number += 1;

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let value: Value =
            serde_json::from_str(trimmed).map_err(|error| ReaderError::BadInput {
                line: line_number,
                kind: BadInputKind::InvalidJson {
                    error: error.to_string(),
                },
            })?;
        validate_record(&value, line_number)?;
        records.push(value);
    }

    Ok(records)
}

fn validate_record(record: &Value, line_number: u64) -> Result<(), ReaderError> {
    let object = record_object(record, line_number)?;
    validate_version(object, line_number)?;
    validate_bytes_hash(object, line_number)?;
    let _ = record_text_path(record).map_err(|kind| ReaderError::BadInput {
        line: line_number,
        kind,
    })?;
    Ok(())
}

fn record_object(record: &Value, line_number: u64) -> Result<&Map<String, Value>, ReaderError> {
    record.as_object().ok_or(ReaderError::BadInput {
        line: line_number,
        kind: BadInputKind::RecordNotObject,
    })
}

fn validate_version(object: &Map<String, Value>, line_number: u64) -> Result<(), ReaderError> {
    match object.get("version") {
        Some(Value::String(version)) => {
            if SUPPORTED_UPSTREAM_VERSIONS.contains(&version.as_str()) {
                Ok(())
            } else {
                Err(ReaderError::BadInput {
                    line: line_number,
                    kind: BadInputKind::UnknownVersion {
                        version: version.clone(),
                    },
                })
            }
        }
        Some(_) => Err(ReaderError::BadInput {
            line: line_number,
            kind: BadInputKind::InvalidFieldType {
                field: "version".to_owned(),
                expected: "string",
            },
        }),
        None => Err(ReaderError::BadInput {
            line: line_number,
            kind: BadInputKind::MissingField {
                field: "version".to_owned(),
            },
        }),
    }
}

fn validate_bytes_hash(object: &Map<String, Value>, line_number: u64) -> Result<(), ReaderError> {
    let skipped = match object.get("_skipped") {
        Some(Value::Bool(value)) => *value,
        Some(_) => {
            return Err(ReaderError::BadInput {
                line: line_number,
                kind: BadInputKind::InvalidFieldType {
                    field: "_skipped".to_owned(),
                    expected: "boolean",
                },
            });
        }
        None => false,
    };

    if skipped {
        return Ok(());
    }

    match object.get("bytes_hash") {
        Some(Value::String(_)) => Ok(()),
        Some(Value::Null) | None => Err(ReaderError::BadInput {
            line: line_number,
            kind: BadInputKind::MissingField {
                field: "bytes_hash".to_owned(),
            },
        }),
        Some(_) => Err(ReaderError::BadInput {
            line: line_number,
            kind: BadInputKind::InvalidFieldType {
                field: "bytes_hash".to_owned(),
                expected: "string",
            },
        }),
    }
}

/// Extract optional text_path from an input record.
pub fn record_text_path(record: &Value) -> Result<Option<&str>, BadInputKind> {
    let object = record.as_object().ok_or(BadInputKind::RecordNotObject)?;

    match object.get("text_path") {
        Some(Value::String(path)) => Ok(Some(path.as_str())),
        Some(Value::Null) | None => Ok(None),
        Some(_) => Err(BadInputKind::InvalidFieldType {
            field: "text_path".to_owned(),
            expected: "string",
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn reads_jsonl_records_and_parses_text_path() {
        let input = r#"{"version":"hash.v0","path":"a.pdf","bytes_hash":"blake3:1","text_path":"a.md"}
{"version":"hash.v0","path":"b.pdf","bytes_hash":"blake3:2"}
"#;
        let mut cursor = Cursor::new(input.as_bytes());
        let records = read_records(&mut cursor).expect("read records");

        assert_eq!(records.len(), 2);
        assert_eq!(
            record_text_path(&records[0]).expect("extract text_path"),
            Some("a.md")
        );
        assert_eq!(
            record_text_path(&records[1]).expect("extract text_path"),
            None
        );
    }

    #[test]
    fn rejects_non_object_records() {
        let input = "\"not-an-object\"\n";
        let mut cursor = Cursor::new(input.as_bytes());
        let error = read_records(&mut cursor).expect_err("record should fail");

        assert_eq!(
            error,
            ReaderError::BadInput {
                line: 1,
                kind: BadInputKind::RecordNotObject
            }
        );
    }

    #[test]
    fn rejects_non_string_text_path() {
        let input =
            r#"{"version":"hash.v0","path":"a.pdf","bytes_hash":"blake3:1","text_path":42}"#;
        let mut cursor = Cursor::new(input.as_bytes());
        let error = read_records(&mut cursor).expect_err("record should fail");

        assert_eq!(
            error,
            ReaderError::BadInput {
                line: 1,
                kind: BadInputKind::InvalidFieldType {
                    field: "text_path".to_owned(),
                    expected: "string"
                }
            }
        );
    }

    #[test]
    fn rejects_missing_bytes_hash_on_non_skipped_record() {
        let input = r#"{"version":"hash.v0","path":"a.pdf"}"#;
        let mut cursor = Cursor::new(input.as_bytes());
        let error = read_records(&mut cursor).expect_err("record should fail");

        assert_eq!(
            error,
            ReaderError::BadInput {
                line: 1,
                kind: BadInputKind::MissingField {
                    field: "bytes_hash".to_owned()
                }
            }
        );
    }

    #[test]
    fn allows_skipped_records_without_bytes_hash() {
        let input = r#"{"version":"hash.v0","path":"a.pdf","_skipped":true}"#;
        let mut cursor = Cursor::new(input.as_bytes());
        let records = read_records(&mut cursor).expect("read records");

        assert_eq!(records.len(), 1);
    }

    #[test]
    fn rejects_unknown_upstream_version() {
        let input = r#"{"version":"unknown.v3","path":"a.pdf","bytes_hash":"blake3:1"}"#;
        let mut cursor = Cursor::new(input.as_bytes());
        let error = read_records(&mut cursor).expect_err("record should fail");

        assert_eq!(
            error,
            ReaderError::BadInput {
                line: 1,
                kind: BadInputKind::UnknownVersion {
                    version: "unknown.v3".to_owned()
                }
            }
        );
    }

    #[test]
    fn rejects_invalid_json() {
        let input = r#"{"version":"hash.v0","path":"a.pdf","bytes_hash":"blake3:1""#;
        let mut cursor = Cursor::new(input.as_bytes());
        let error = read_records(&mut cursor).expect_err("record should fail");

        assert!(matches!(
            error,
            ReaderError::BadInput {
                line: 1,
                kind: BadInputKind::InvalidJson { .. }
            }
        ));
    }

    #[test]
    fn skips_blank_lines() {
        let input = r#"

{"version":"hash.v0","path":"a.pdf","bytes_hash":"blake3:1","text_path":"a.md"}

"#;
        let mut cursor = Cursor::new(input.as_bytes());
        let records = read_records(&mut cursor).expect("read records");

        assert_eq!(records.len(), 1);
    }
}
