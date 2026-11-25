#![forbid(unsafe_code)]

//! JSON and JSONL documents for structural fingerprinting (bd-yq3, JSON slice).
//!
//! - `.jsonl` / `.ndjson`: streamed line by line. Every non-empty line must parse as one JSON
//!   value; memory is O(longest line). Only the first record is kept (for path assertions) plus
//!   the record count.
//! - `.json`: parsed whole, up to [`MAX_JSON_BYTES`]; a larger file is refused at open with a
//!   typed message instead of being half-read. (A single JSON document cannot be streamed to a
//!   random-access path check; JSONL is the streaming shape.)
//!
//! Paths are a small, deterministic subset of JSONPath: `$`, `.key`, `[index]`. No wildcards,
//! filters or recursive descent, so a path means exactly one place.

use serde_json::Value;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// Largest `.json` file read into memory.
pub const MAX_JSON_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonKind {
    Json,
    Jsonl,
}

impl JsonKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Jsonl => "jsonl",
        }
    }
}

#[derive(Debug)]
pub struct JsonDocument {
    pub path: PathBuf,
    pub kind: JsonKind,
    /// `.json`: the whole document. `.jsonl`: the first record (`Null` when the file has none).
    pub root: Value,
    /// `.jsonl`: number of records. `.json`: array length when the root is an array, else 1.
    pub record_count: u64,
}

impl JsonDocument {
    pub fn open(path: &Path, kind: JsonKind) -> Result<Self, String> {
        match kind {
            JsonKind::Json => Self::open_json(path),
            JsonKind::Jsonl => Self::open_jsonl(path),
        }
    }

    fn open_json(path: &Path) -> Result<Self, String> {
        let size = std::fs::metadata(path)
            .map_err(|error| format!("failed reading '{}': {error}", path.display()))?
            .len();
        if size > MAX_JSON_BYTES {
            return Err(format!(
                "json file '{}' is {size} bytes, over the {MAX_JSON_BYTES} byte in-memory limit; use jsonl for large inputs",
                path.display()
            ));
        }
        let file = File::open(path)
            .map_err(|error| format!("failed opening '{}': {error}", path.display()))?;
        let root: Value = serde_json::from_reader(BufReader::new(file))
            .map_err(|error| format!("invalid json '{}': {error}", path.display()))?;
        let record_count = root.as_array().map_or(1, |items| items.len() as u64);
        Ok(Self {
            path: path.to_path_buf(),
            kind: JsonKind::Json,
            root,
            record_count,
        })
    }

    fn open_jsonl(path: &Path) -> Result<Self, String> {
        let file = File::open(path)
            .map_err(|error| format!("failed opening '{}': {error}", path.display()))?;
        let mut reader = BufReader::new(file);
        let mut line = String::new();
        let mut first: Option<Value> = None;
        let mut count = 0u64;
        let mut line_number = 0u64;
        loop {
            line.clear();
            let read = reader
                .read_line(&mut line)
                .map_err(|error| format!("failed reading '{}': {error}", path.display()))?;
            if read == 0 {
                break;
            }
            line_number += 1;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let value: Value = serde_json::from_str(trimmed).map_err(|error| {
                format!(
                    "invalid jsonl '{}' at line {line_number}: {error}",
                    path.display()
                )
            })?;
            if first.is_none() {
                first = Some(value);
            }
            count += 1;
        }
        Ok(Self {
            path: path.to_path_buf(),
            kind: JsonKind::Jsonl,
            root: first.unwrap_or(Value::Null),
            record_count: count,
        })
    }
}

/// One step of a parsed path.
#[derive(Debug, PartialEq, Eq)]
enum Step<'a> {
    Key(&'a str),
    Index(usize),
}

fn parse_path(path: &str) -> Result<Vec<Step<'_>>, String> {
    let rest = path
        .strip_prefix('$')
        .ok_or_else(|| format!("json path '{path}' must start with '$'"))?;
    let mut steps = Vec::new();
    let mut remaining = rest;
    while !remaining.is_empty() {
        if let Some(after) = remaining.strip_prefix('.') {
            let end = after.find(['.', '[']).unwrap_or(after.len());
            let key = &after[..end];
            if key.is_empty() {
                return Err(format!("json path '{path}' has an empty key"));
            }
            steps.push(Step::Key(key));
            remaining = &after[end..];
        } else if let Some(after) = remaining.strip_prefix('[') {
            let close = after
                .find(']')
                .ok_or_else(|| format!("json path '{path}' has an unclosed '['"))?;
            let index: usize = after[..close].parse().map_err(|_| {
                format!(
                    "json path '{path}': '[{}]' is not a non-negative array index",
                    &after[..close]
                )
            })?;
            steps.push(Step::Index(index));
            remaining = &after[close + 1..];
        } else {
            return Err(format!(
                "json path '{path}' is invalid near '{remaining}' (use $.key[0].key)"
            ));
        }
    }
    Ok(steps)
}

/// The value at `path`, `Ok(None)` when the path is well-formed but absent, `Err` when the path
/// itself is malformed.
pub fn resolve_path<'a>(root: &'a Value, path: &str) -> Result<Option<&'a Value>, String> {
    let mut current = root;
    for step in parse_path(path)? {
        let next = match step {
            Step::Key(key) => current.get(key),
            Step::Index(index) => current.get(index),
        };
        match next {
            Some(value) => current = value,
            None => return Ok(None),
        }
    }
    Ok(Some(current))
}

/// JSON type name of a value, in the vocabulary the DSL uses.
pub fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(number) if number.is_i64() || number.is_u64() => "integer",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Whether `value` satisfies the DSL type `expected` (`integer` is also a `number`).
pub fn has_type(value: &Value, expected: &str) -> bool {
    let actual = type_name(value);
    actual == expected || (expected == "number" && actual == "integer")
}

pub const JSON_TYPES: &[&str] = &[
    "string", "number", "integer", "boolean", "array", "object", "null",
];

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn paths_resolve_keys_and_indexes() {
        let doc = json!({"a": {"b": [10, {"c": "x"}]}});
        assert_eq!(resolve_path(&doc, "$").unwrap(), Some(&doc));
        assert_eq!(resolve_path(&doc, "$.a.b[0]").unwrap(), Some(&json!(10)));
        assert_eq!(resolve_path(&doc, "$.a.b[1].c").unwrap(), Some(&json!("x")));
        assert_eq!(resolve_path(&doc, "$.a.z").unwrap(), None);
        assert_eq!(resolve_path(&doc, "$.a.b[9]").unwrap(), None);
    }

    #[test]
    fn malformed_paths_are_errors_not_misses() {
        let doc = json!({});
        for bad in ["a.b", "$.", "$..a", "$[x]", "$[1", "$a"] {
            assert!(resolve_path(&doc, bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn integer_is_a_number_but_not_the_other_way() {
        assert!(has_type(&json!(3), "number"));
        assert!(has_type(&json!(3), "integer"));
        assert!(has_type(&json!(3.5), "number"));
        assert!(!has_type(&json!(3.5), "integer"));
        assert!(!has_type(&json!("3"), "number"));
    }
}
