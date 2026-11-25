#![forbid(unsafe_code)]

//! Deterministic writers for a [`Projection`]: CSV, JSONL and Parquet.
//!
//! Same projection in, same bytes out, every run: CSV is comma-separated with `\n` line ends and
//! quotes only where needed; JSONL has one object per row with keys in column order; Parquet has
//! one optional UTF-8 string column per header, uncompressed, no dictionary, no statistics and a
//! fixed `created_by`, so the file has no timestamp or run-specific byte.
//!
//! An empty source cell is `""` in CSV and null in JSONL and Parquet (CSV cannot tell them
//! apart; downstream readers render a null as empty text).

use crate::materialize::project::Projection;
use parquet::basic::{Compression, ConvertedType, Repetition, Type as PhysicalType};
use parquet::data_type::{ByteArray, ByteArrayType};
use parquet::file::properties::{EnabledStatistics, WriterProperties, WriterVersion};
use parquet::file::writer::SerializedFileWriter;
use parquet::schema::types::Type;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Csv,
    Jsonl,
    Parquet,
}

impl OutputFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Jsonl => "jsonl",
            Self::Parquet => "parquet",
        }
    }
}

/// Header plus rows as canonical CSV bytes. This is also what `content_hash` is computed over,
/// whatever the output format, so one projection has one content hash.
pub fn csv_bytes(projection: &Projection) -> Result<Vec<u8>, String> {
    let mut writer = csv::WriterBuilder::new()
        .terminator(csv::Terminator::Any(b'\n'))
        .from_writer(Vec::new());
    writer
        .write_record(&projection.headers)
        .map_err(|error| format!("csv write failed: {error}"))?;
    for row in &projection.rows {
        writer
            .write_record(row)
            .map_err(|error| format!("csv write failed: {error}"))?;
    }
    writer
        .into_inner()
        .map_err(|error| format!("csv write failed: {error}"))
}

pub fn jsonl_bytes(projection: &Projection) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let keys: Vec<String> = projection
        .headers
        .iter()
        .map(|header| serde_json::to_string(header).map_err(|error| error.to_string()))
        .collect::<Result<_, _>>()?;
    for row in &projection.rows {
        out.push(b'{');
        for (index, (key, cell)) in keys.iter().zip(row).enumerate() {
            if index > 0 {
                out.push(b',');
            }
            out.extend_from_slice(key.as_bytes());
            out.push(b':');
            if cell.is_empty() {
                out.extend_from_slice(b"null");
            } else {
                out.extend_from_slice(
                    serde_json::to_string(cell)
                        .map_err(|error| error.to_string())?
                        .as_bytes(),
                );
            }
        }
        out.extend_from_slice(b"}\n");
    }
    Ok(out)
}

pub fn parquet_bytes(projection: &Projection) -> Result<Vec<u8>, String> {
    let fields = projection
        .headers
        .iter()
        .map(|header| {
            Type::primitive_type_builder(header, PhysicalType::BYTE_ARRAY)
                .with_repetition(Repetition::OPTIONAL)
                .with_converted_type(ConvertedType::UTF8)
                .build()
                .map(Arc::new)
                .map_err(|error| format!("parquet schema failed for column '{header}': {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let schema = Arc::new(
        Type::group_type_builder("projection")
            .with_fields(fields)
            .build()
            .map_err(|error| format!("parquet schema failed: {error}"))?,
    );
    let properties = Arc::new(
        WriterProperties::builder()
            .set_writer_version(WriterVersion::PARQUET_1_0)
            .set_compression(Compression::UNCOMPRESSED)
            .set_dictionary_enabled(false)
            .set_statistics_enabled(EnabledStatistics::None)
            .set_created_by("fingerprint materialize".to_owned())
            .build(),
    );

    let mut buffer = Vec::new();
    {
        let mut writer = SerializedFileWriter::new(&mut buffer, schema, properties)
            .map_err(|error| format!("parquet write failed: {error}"))?;
        // An empty projection (header only) has no row group.
        if !projection.rows.is_empty() {
            let mut group = writer
                .next_row_group()
                .map_err(|error| format!("parquet write failed: {error}"))?;
            let mut column_index = 0usize;
            while let Some(mut column) = group
                .next_column()
                .map_err(|error| format!("parquet write failed: {error}"))?
            {
                let mut values = Vec::new();
                let mut definition = Vec::with_capacity(projection.rows.len());
                for row in &projection.rows {
                    let cell = &row[column_index];
                    if cell.is_empty() {
                        definition.push(0i16);
                    } else {
                        definition.push(1i16);
                        values.push(ByteArray::from(cell.as_bytes().to_vec()));
                    }
                }
                column
                    .typed::<ByteArrayType>()
                    .write_batch(&values, Some(&definition), None)
                    .map_err(|error| format!("parquet write failed: {error}"))?;
                column
                    .close()
                    .map_err(|error| format!("parquet write failed: {error}"))?;
                column_index += 1;
            }
            group
                .close()
                .map_err(|error| format!("parquet write failed: {error}"))?;
        }
        writer
            .close()
            .map_err(|error| format!("parquet write failed: {error}"))?;
    }
    Ok(buffer)
}

pub fn render(projection: &Projection, format: OutputFormat) -> Result<Vec<u8>, String> {
    match format {
        OutputFormat::Csv => csv_bytes(projection),
        OutputFormat::Jsonl => jsonl_bytes(projection),
        OutputFormat::Parquet => parquet_bytes(projection),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> Projection {
        Projection {
            headers: vec!["id".into(), "note".into()],
            rows: vec![
                vec!["L1".into(), "has, comma".into()],
                vec!["L2".into(), String::new()],
            ],
            locator: json!({}),
            skipped: json!({}),
        }
    }

    #[test]
    fn csv_quotes_only_where_needed_and_ends_lines_with_lf() {
        let bytes = csv_bytes(&sample()).unwrap();
        assert_eq!(
            String::from_utf8(bytes).unwrap(),
            "id,note\nL1,\"has, comma\"\nL2,\n"
        );
    }

    #[test]
    fn jsonl_has_column_order_and_null_for_empty() {
        let bytes = jsonl_bytes(&sample()).unwrap();
        assert_eq!(
            String::from_utf8(bytes).unwrap(),
            "{\"id\":\"L1\",\"note\":\"has, comma\"}\n{\"id\":\"L2\",\"note\":null}\n"
        );
    }

    #[test]
    fn parquet_is_byte_deterministic_and_has_the_magic() {
        let one = parquet_bytes(&sample()).unwrap();
        let two = parquet_bytes(&sample()).unwrap();
        assert_eq!(one, two);
        assert_eq!(&one[..4], b"PAR1");
        assert_eq!(&one[one.len() - 4..], b"PAR1");
    }
}
