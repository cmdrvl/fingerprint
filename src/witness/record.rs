use blake3::Hasher;
use serde::Serialize;
use serde_json::Value;

/// Witness record following the witness.v0 schema.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WitnessRecord {
    pub id: String,
    pub tool: String,
    pub version: String,
    pub binary_hash: String,
    pub inputs: Vec<WitnessInput>,
    pub params: Value,
    pub outcome: String,
    pub exit_code: u8,
    pub output_hash: String,
    pub prev: Option<String>,
    pub ts: String,
}

/// An input source referenced in a witness record.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WitnessInput {
    pub path: String,
    pub hash: Option<String>,
    pub bytes: Option<u64>,
}

impl WitnessRecord {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        version: impl Into<String>,
        binary_hash: impl Into<String>,
        inputs: Vec<WitnessInput>,
        params: Value,
        outcome: impl Into<String>,
        exit_code: u8,
        output_hash: impl Into<String>,
        prev: Option<String>,
        ts: impl Into<String>,
    ) -> Result<Self, String> {
        let tool = "fingerprint".to_owned();
        let version = version.into();
        let binary_hash = binary_hash.into();
        let outcome = outcome.into();
        let output_hash = output_hash.into();
        let ts = ts.into();

        let id = compute_record_id(
            &tool,
            &version,
            &binary_hash,
            &inputs,
            &params,
            &outcome,
            exit_code,
            &output_hash,
            prev.as_deref(),
            &ts,
        )?;

        Ok(Self {
            id,
            tool,
            version,
            binary_hash,
            inputs,
            params,
            outcome,
            exit_code,
            output_hash,
            prev,
            ts,
        })
    }

    pub fn to_jsonl(&self) -> Result<String, String> {
        let json = serde_json::to_string(self)
            .map_err(|error| format!("failed to serialize witness record: {error}"))?;
        Ok(format!("{json}\n"))
    }
}

#[derive(Debug, Serialize)]
struct WitnessRecordIdPayload<'a> {
    tool: &'a str,
    version: &'a str,
    binary_hash: &'a str,
    inputs: &'a [WitnessInput],
    params: &'a Value,
    outcome: &'a str,
    exit_code: u8,
    output_hash: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    prev: Option<&'a str>,
    ts: &'a str,
}

#[allow(clippy::too_many_arguments)]
fn compute_record_id(
    tool: &str,
    version: &str,
    binary_hash: &str,
    inputs: &[WitnessInput],
    params: &Value,
    outcome: &str,
    exit_code: u8,
    output_hash: &str,
    prev: Option<&str>,
    ts: &str,
) -> Result<String, String> {
    let payload = WitnessRecordIdPayload {
        tool,
        version,
        binary_hash,
        inputs,
        params,
        outcome,
        exit_code,
        output_hash,
        prev,
        ts,
    };
    let encoded = serde_json::to_vec(&payload)
        .map_err(|error| format!("failed to encode witness record ID payload: {error}"))?;

    let mut hasher = Hasher::new();
    hasher.update(&encoded);
    Ok(format!("blake3:{}", hasher.finalize().to_hex()))
}

#[cfg(test)]
mod tests {
    use super::{WitnessInput, WitnessRecord};
    use serde_json::json;

    fn sample_record() -> WitnessRecord {
        WitnessRecord::new(
            "0.1.0",
            "blake3:binary",
            vec![WitnessInput {
                path: "stdin".to_owned(),
                hash: None,
                bytes: None,
            }],
            json!({ "fingerprints": ["argus-model.v1"], "jobs": 4 }),
            "ALL_MATCHED",
            0,
            "blake3:output",
            Some("blake3:prev".to_owned()),
            "2026-02-24T10:00:00Z",
        )
        .expect("construct witness record")
    }

    #[test]
    fn builds_record_with_blake3_id() {
        let record = sample_record();
        assert!(record.id.starts_with("blake3:"));
        assert!(record.id.len() > "blake3:".len());
        assert_eq!(record.tool, "fingerprint");
    }

    #[test]
    fn id_is_deterministic_for_same_payload() {
        let record_a = sample_record();
        let record_b = sample_record();
        assert_eq!(record_a.id, record_b.id);
    }

    #[test]
    fn id_changes_when_payload_changes() {
        let record_a = sample_record();
        let mut inputs = vec![WitnessInput {
            path: "stdin".to_owned(),
            hash: None,
            bytes: None,
        }];
        inputs.push(WitnessInput {
            path: "/tmp/input.jsonl".to_owned(),
            hash: Some("blake3:source".to_owned()),
            bytes: Some(10),
        });
        let record_b = WitnessRecord::new(
            "0.1.0",
            "blake3:binary",
            inputs,
            json!({ "fingerprints": ["argus-model.v1"], "jobs": 4 }),
            "ALL_MATCHED",
            0,
            "blake3:output",
            Some("blake3:prev".to_owned()),
            "2026-02-24T10:00:00Z",
        )
        .expect("construct witness record");

        assert_ne!(record_a.id, record_b.id);
    }

    #[test]
    fn serializes_to_jsonl_with_schema_fields() {
        let record = sample_record();
        let line = record.to_jsonl().expect("serialize JSONL");
        assert!(line.ends_with('\n'));

        let value: serde_json::Value =
            serde_json::from_str(line.trim_end()).expect("parse JSON output");
        assert_eq!(value["tool"], "fingerprint");
        assert_eq!(value["version"], "0.1.0");
        assert_eq!(value["binary_hash"], "blake3:binary");
        assert_eq!(value["outcome"], "ALL_MATCHED");
        assert_eq!(value["exit_code"], 0);
        assert_eq!(value["output_hash"], "blake3:output");
        assert_eq!(value["prev"], "blake3:prev");
        assert_eq!(value["ts"], "2026-02-24T10:00:00Z");
        assert_eq!(
            value["inputs"],
            json!([{ "path": "stdin", "hash": null, "bytes": null }])
        );
        assert_eq!(
            value["params"],
            json!({ "fingerprints": ["argus-model.v1"], "jobs": 4 })
        );
    }
}
