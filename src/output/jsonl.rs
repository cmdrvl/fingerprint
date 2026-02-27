use serde_json::Value;
use std::io::Write;

/// Write JSONL records to an output stream (one JSON object per line).
pub fn write_jsonl(out: &mut dyn Write, records: &[Value]) -> Result<(), String> {
    for record in records {
        serde_json::to_writer(&mut *out, record)
            .map_err(|error| format!("failed to serialize JSON record: {error}"))?;
        out.write_all(b"\n")
            .map_err(|error| format!("failed to write JSONL newline: {error}"))?;
    }

    out.flush()
        .map_err(|error| format!("failed to flush JSONL output: {error}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::write_jsonl;
    use serde_json::json;
    use std::io::{Cursor, Error, ErrorKind, Write};

    #[test]
    fn writes_empty_record_set() {
        let mut out = Cursor::new(Vec::new());
        write_jsonl(&mut out, &[]).expect("write empty records");
        assert!(out.into_inner().is_empty());
    }

    #[test]
    fn writes_multiple_records_as_jsonl() {
        let records = vec![json!({"path": "a.xlsx"}), json!({"path": "b.xlsx"})];
        let mut out = Cursor::new(Vec::new());

        write_jsonl(&mut out, &records).expect("write records");

        let output = String::from_utf8(out.into_inner()).expect("valid UTF-8 output");
        assert_eq!(output, "{\"path\":\"a.xlsx\"}\n{\"path\":\"b.xlsx\"}\n");
    }

    #[test]
    fn writes_refusal_envelope_as_single_json_object_line() {
        let records = vec![json!({
            "version": "fingerprint.v0",
            "outcome": "REFUSAL",
            "refusal": {
                "code": "E_UNKNOWN_FP",
                "message": "Fingerprint ID not found",
                "detail": { "fingerprint_id": "argus-model.v1", "available": ["csv.v0"] },
                "next_command": "cargo install fingerprint-argus"
            }
        })];
        let mut out = Cursor::new(Vec::new());

        write_jsonl(&mut out, &records).expect("write refusal");

        let output = String::from_utf8(out.into_inner()).expect("valid UTF-8 output");
        assert_eq!(
            output,
            "{\"outcome\":\"REFUSAL\",\"refusal\":{\"code\":\"E_UNKNOWN_FP\",\"detail\":{\"available\":[\"csv.v0\"],\"fingerprint_id\":\"argus-model.v1\"},\"message\":\"Fingerprint ID not found\",\"next_command\":\"cargo install fingerprint-argus\"},\"version\":\"fingerprint.v0\"}\n"
        );
    }

    #[test]
    fn surfaces_write_errors() {
        struct AlwaysFailWriter;

        impl Write for AlwaysFailWriter {
            fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
                Err(Error::new(ErrorKind::BrokenPipe, "write failed"))
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let mut writer = AlwaysFailWriter;
        let error = write_jsonl(&mut writer, &[json!({"path": "a.xlsx"})]).expect_err("fail");
        assert!(error.contains("failed to serialize JSON record"));
    }
}
