#![forbid(unsafe_code)]

//! Parquet documents for structural fingerprinting (bd-yq3, Parquet slice).
//!
//! Only the file footer is read: the parquet crate fetches the footer (and nothing else, since
//! page indexes are not requested) through the file's `ChunkReader`, so no column chunk is
//! decompressed or even touched. Cost is O(footer), independent of row count, which is what
//! makes sub-millisecond checks on large deliveries possible. `tests/parquet_format.rs`
//! proves it by zeroing every data page of a file and still matching.
//!
//! The crate is built with default features off (no arrow, no codecs).

use parquet::file::metadata::ParquetMetaDataReader;
use std::collections::BTreeMap;
use std::fs::File;
use std::path::{Path, PathBuf};

/// One leaf column of the schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParquetColumn {
    /// Dotted path of the leaf (`loan_id`, `tags.list.element`).
    pub path: String,
    /// Logical type when the file declares one (`string`, `decimal`, `date`, `timestamp`,
    /// `time`, `uuid`, ...), otherwise the physical type (`int32`, `int64`, `double`, ...).
    /// Integer annotations report the physical type, which is what a consumer reads.
    pub type_name: String,
    /// `required`, `optional` or `repeated`.
    pub repetition: String,
}

#[derive(Debug)]
pub struct ParquetDocument {
    pub path: PathBuf,
    pub columns: Vec<ParquetColumn>,
    pub num_rows: u64,
    pub num_row_groups: u64,
    /// File-level key/value metadata (key -> value; a key without a value maps to "").
    pub key_values: BTreeMap<String, String>,
}

impl ParquetDocument {
    pub fn open(path: &Path) -> Result<Self, String> {
        let file = File::open(path)
            .map_err(|error| format!("failed opening '{}': {error}", path.display()))?;
        let metadata = ParquetMetaDataReader::new()
            .parse_and_finish(&file)
            .map_err(|error| format!("invalid parquet footer in '{}': {error}", path.display()))?;
        let file_meta = metadata.file_metadata();
        let columns = file_meta
            .schema_descr()
            .columns()
            .iter()
            .map(|column| {
                let logical = column
                    .logical_type_ref()
                    .map(|logical| logical_name(&format!("{logical:?}")))
                    .filter(|name| name != "integer" && name != "unknown");
                let type_name = logical.unwrap_or_else(|| {
                    format!("{:?}", column.physical_type()).to_ascii_lowercase()
                });
                let repetition = format!("{:?}", column.self_type().get_basic_info().repetition())
                    .to_ascii_lowercase();
                ParquetColumn {
                    path: column.path().string(),
                    type_name,
                    repetition,
                }
            })
            .collect();
        let key_values = file_meta
            .key_value_metadata()
            .map(|pairs| {
                pairs
                    .iter()
                    .map(|pair| (pair.key.clone(), pair.value.clone().unwrap_or_default()))
                    .collect()
            })
            .unwrap_or_default();
        Ok(Self {
            path: path.to_path_buf(),
            columns,
            num_rows: u64::try_from(file_meta.num_rows()).unwrap_or(0),
            num_row_groups: metadata.num_row_groups() as u64,
            key_values,
        })
    }

    pub fn column(&self, path: &str) -> Option<&ParquetColumn> {
        self.columns.iter().find(|column| column.path == path)
    }
}

/// `Decimal(DecimalType { .. })` -> `decimal`, `String` -> `string`.
fn logical_name(debug: &str) -> String {
    let end = debug
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .unwrap_or(debug.len());
    debug[..end].to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_names_drop_the_payload() {
        assert_eq!(logical_name("String"), "string");
        assert_eq!(
            logical_name("Decimal(DecimalType { scale: 2, precision: 18 })"),
            "decimal"
        );
        assert_eq!(logical_name("Timestamp(TimestampType { .. })"), "timestamp");
    }

    #[test]
    fn a_non_parquet_file_is_a_footer_error_naming_the_file() {
        let path =
            std::env::temp_dir().join(format!("fp-not-parquet-{}.parquet", std::process::id()));
        std::fs::write(&path, b"loan_id,balance\nL1,1\n").unwrap();
        let error = ParquetDocument::open(&path).unwrap_err();
        let _ = std::fs::remove_file(&path);
        assert!(error.contains("invalid parquet footer"), "{error}");
    }
}
