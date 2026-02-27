use serde::Serialize;
use std::io::{self, Write};

/// Structured progress event emitted to stderr.
#[derive(Debug, Serialize)]
pub struct ProgressEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub tool: String,
    pub processed: u64,
    pub total: Option<u64>,
    pub percent: Option<f64>,
    pub elapsed_ms: u64,
}

/// Structured warning emitted to stderr for skipped files.
#[derive(Debug, Serialize)]
pub struct WarningEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub tool: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub message: String,
}

/// Report progress to stderr as JSONL.
pub fn report_progress(event: &ProgressEvent) {
    let mut stderr = io::stderr().lock();
    let _ = write_event_line(&mut stderr, event);
}

/// Report a skipped-file warning to stderr as JSONL.
pub fn report_warning(path: &str, message: &str) {
    report_warning_code(path, None, message);
}

/// Report a warning to stderr as JSONL with an optional warning code.
pub fn report_warning_code(path: &str, code: Option<&str>, message: &str) {
    let warning = WarningEvent {
        event_type: "warning".to_owned(),
        tool: "fingerprint".to_owned(),
        path: path.to_owned(),
        code: code.map(str::to_owned),
        message: message.to_owned(),
    };
    let mut stderr = io::stderr().lock();
    let _ = write_event_line(&mut stderr, &warning);
}

fn write_event_line<T: Serialize>(out: &mut dyn Write, event: &T) -> Result<(), String> {
    serde_json::to_writer(&mut *out, event)
        .map_err(|error| format!("failed to serialize progress event: {error}"))?;
    out.write_all(b"\n")
        .map_err(|error| format!("failed to write progress event newline: {error}"))?;
    out.flush()
        .map_err(|error| format!("failed to flush progress event output: {error}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        ProgressEvent, WarningEvent, report_warning, report_warning_code, write_event_line,
    };
    use serde_json::json;
    use std::io::{Cursor, Error, ErrorKind, Write};

    #[test]
    fn serializes_progress_event_to_plan_shape() {
        let event = ProgressEvent {
            event_type: "progress".to_owned(),
            tool: "fingerprint".to_owned(),
            processed: 500,
            total: Some(10_000),
            percent: Some(5.0),
            elapsed_ms: 3200,
        };

        assert_eq!(
            serde_json::to_value(event).expect("serialize progress event"),
            json!({
                "type": "progress",
                "tool": "fingerprint",
                "processed": 500,
                "total": 10000,
                "percent": 5.0,
                "elapsed_ms": 3200
            })
        );
    }

    #[test]
    fn serializes_warning_event_to_plan_shape() {
        let event = WarningEvent {
            event_type: "warning".to_owned(),
            tool: "fingerprint".to_owned(),
            path: "/data/corrupt.xlsx".to_owned(),
            code: None,
            message: "skipped: Invalid ZIP".to_owned(),
        };

        assert_eq!(
            serde_json::to_value(event).expect("serialize warning event"),
            json!({
                "type": "warning",
                "tool": "fingerprint",
                "path": "/data/corrupt.xlsx",
                "message": "skipped: Invalid ZIP"
            })
        );
    }

    #[test]
    fn serializes_warning_event_with_code() {
        let event = WarningEvent {
            event_type: "warning".to_owned(),
            tool: "fingerprint".to_owned(),
            path: "/data/corrupt.pdf".to_owned(),
            code: Some("W_SPARSE_TEXT".to_owned()),
            message: "text extraction appears sparse".to_owned(),
        };

        assert_eq!(
            serde_json::to_value(event).expect("serialize warning event"),
            json!({
                "type": "warning",
                "tool": "fingerprint",
                "path": "/data/corrupt.pdf",
                "code": "W_SPARSE_TEXT",
                "message": "text extraction appears sparse"
            })
        );
    }

    #[test]
    fn write_event_line_writes_json_with_newline() {
        let event = ProgressEvent {
            event_type: "progress".to_owned(),
            tool: "fingerprint".to_owned(),
            processed: 1,
            total: None,
            percent: None,
            elapsed_ms: 2,
        };
        let mut out = Cursor::new(Vec::new());

        write_event_line(&mut out, &event).expect("write progress event");

        let output = String::from_utf8(out.into_inner()).expect("valid UTF-8 output");
        assert_eq!(
            output,
            "{\"type\":\"progress\",\"tool\":\"fingerprint\",\"processed\":1,\"total\":null,\"percent\":null,\"elapsed_ms\":2}\n"
        );
    }

    #[test]
    fn write_event_line_surfaces_write_errors() {
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
        let event = ProgressEvent {
            event_type: "progress".to_owned(),
            tool: "fingerprint".to_owned(),
            processed: 1,
            total: None,
            percent: None,
            elapsed_ms: 2,
        };
        let error = write_event_line(&mut writer, &event).expect_err("write should fail");
        assert!(error.contains("failed to serialize progress event"));
    }

    #[test]
    fn report_warning_is_callable() {
        report_warning("/tmp/file", "skipped: parse error");
        report_warning_code("/tmp/file", Some("W_TEST"), "diagnostic warning");
    }
}
