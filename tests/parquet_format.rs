//! Native `parquet` document format, footer only (bd-yq3, Parquet slice): DSL runtime,
//! --diagnose, `compile --check` validation, the footer-only proof, and the zero-retention
//! property of failure context (key and column names and types, never a metadata value).

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/parquet")
        .join(name)
}

struct Scratch(PathBuf);
impl Scratch {
    fn new(tag: &str) -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let unique = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("fp-pq-{tag}-{}-{unique}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

fn run_definition(
    yaml: &str,
    id: &str,
    file: &Path,
    diagnose: bool,
) -> (Option<i32>, Value, String) {
    let dir = Scratch::new("defs");
    fs::write(dir.0.join(format!("{id}.fp.yaml")), yaml).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_fingerprint"));
    command.env("FINGERPRINT_DEFINITIONS", &dir.0).args([
        "match",
        s(file),
        "--fp",
        id,
        "--no-witness",
    ]);
    if diagnose {
        command.arg("--diagnose");
    }
    let out = command.output().unwrap();
    let record: Value = String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .map(|line| serde_json::from_str(line).unwrap())
        .unwrap_or(Value::Null);
    (
        out.status.code(),
        record,
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn failed_details(record: &Value) -> Vec<String> {
    record["fingerprint"]["assertions"]
        .as_array()
        .map(|assertions| {
            assertions
                .iter()
                .filter(|a| a["passed"] == false)
                .map(|a| a["detail"].as_str().unwrap_or("").to_owned())
                .collect()
        })
        .unwrap_or_default()
}

const DELIVERY_OK: &str = r#"
fingerprint_id: cmbs-delivery.v2
format: parquet
assertions:
  - parquet_row_count:
      min: 30
      max: 30
  - parquet_row_groups:
      min: 3
      max: 3
  - parquet_column_exists: loan_id
  - parquet_column_exists: tags.list.element
  - parquet_column_type: { column: loan_id, parquet_type: string }
  - parquet_column_type: { column: balance, parquet_type: decimal }
  - parquet_column_type: { column: as_of, parquet_type: date }
  - parquet_column_type: { column: ts, parquet_type: timestamp }
  - parquet_column_type: { column: rate, parquet_type: double }
  - parquet_column_type: { column: active, parquet_type: boolean }
  - parquet_column_type: { column: seq, parquet_type: int64 }
  - parquet_kv_metadata: { key: delivery, pattern: "^cmbs\\.v2$" }
  - parquet_kv_metadata: { key: producer }
"#;

#[test]
fn parquet_fingerprint_matches_a_real_delivery() {
    let (code, record, stderr) = run_definition(
        DELIVERY_OK,
        "cmbs-delivery.v2",
        &fixture("delivery.parquet"),
        false,
    );
    assert_eq!(code, Some(0), "{stderr}\n{record}");
    assert_eq!(record["fingerprint"]["matched"], true, "{record}");
    assert_eq!(
        record["fingerprint"]["assertions"]
            .as_array()
            .unwrap()
            .len(),
        13
    );
}

#[test]
fn every_parquet_assertion_fails_when_its_condition_is_false() {
    let cases: [(&str, &str); 8] = [
        (
            "  - parquet_column_exists: nope\n",
            "parquet column 'nope' not found",
        ),
        (
            "  - parquet_column_type: { column: seq, parquet_type: int32 }\n",
            "parquet column 'seq' is int64, expected int32",
        ),
        (
            "  - parquet_column_type: { column: ghost, parquet_type: string }\n",
            "parquet column 'ghost' not found",
        ),
        (
            "  - parquet_row_count: { min: 31 }\n",
            "row count is 30, expected at least 31",
        ),
        (
            "  - parquet_row_groups: { max: 2 }\n",
            "row group count is 3, expected at most 2",
        ),
        (
            "  - parquet_kv_metadata: { key: absent }\n",
            "parquet key/value metadata key 'absent' not found",
        ),
        (
            "  - parquet_kv_metadata: { key: delivery, pattern: \"^cmbs\\\\.v9$\" }\n",
            "parquet key/value metadata 'delivery' does not match /^cmbs\\.v9$/",
        ),
        (
            "  - parquet_column_type: { column: balance, parquet_type: double }\n",
            "parquet column 'balance' is decimal, expected double",
        ),
    ];
    for (assertion, expected) in cases {
        let yaml = format!("fingerprint_id: neg.v1\nformat: parquet\nassertions:\n{assertion}");
        let (code, record, stderr) =
            run_definition(&yaml, "neg.v1", &fixture("delivery.parquet"), false);
        assert_eq!(code, Some(1), "{assertion}: {stderr}");
        assert_eq!(record["fingerprint"]["matched"], false, "{assertion}");
        assert_eq!(
            failed_details(&record),
            vec![expected.to_owned()],
            "{assertion}"
        );
    }
}

#[test]
fn footer_only_matching_never_reads_a_data_page() {
    // Every byte between the leading magic and the footer is zero: reading any column chunk
    // would fail. The footer assertions still match, so none was read.
    let yaml = "fingerprint_id: zeroed.v1\nformat: parquet\nassertions:\n  - parquet_row_count: { min: 3000, max: 3000 }\n  - parquet_row_groups: { min: 3, max: 3 }\n  - parquet_column_type: { column: balance, parquet_type: decimal }\n  - parquet_column_exists: tags.list.element\n";
    let (code, record, stderr) =
        run_definition(yaml, "zeroed.v1", &fixture("body_zeroed.parquet"), false);
    assert_eq!(code, Some(0), "{stderr}\n{record}");
    assert_eq!(record["fingerprint"]["matched"], true);

    // And the zeroed body really is unreadable data: the bytes are all zero.
    let bytes = fs::read(fixture("body_zeroed.parquet")).unwrap();
    let footer_len =
        u32::from_le_bytes(bytes[bytes.len() - 8..bytes.len() - 4].try_into().unwrap()) as usize;
    let footer_start = bytes.len() - 8 - footer_len;
    assert!(bytes[4..footer_start].iter().all(|b| *b == 0));
    assert!(
        footer_start > 100_000,
        "the zeroed body is large relative to the footer"
    );
}

#[test]
fn a_parquet_fingerprint_does_not_apply_to_other_formats_and_a_bad_footer_is_an_open_failure() {
    let (code, record, _) = run_definition(
        DELIVERY_OK,
        "cmbs-delivery.v2",
        &fixture("../json/loans.jsonl"),
        false,
    );
    assert_eq!(code, Some(1));
    assert!(record["fingerprint"].is_null(), "{record}");

    // A .parquet file that is not parquet fails to open, naming the file.
    let dir = Scratch::new("bad");
    let fake = dir.0.join("fake.parquet");
    fs::write(&fake, b"loan_id,balance\nL1,1\n").unwrap();
    let (code, record, stderr) = run_definition(DELIVERY_OK, "cmbs-delivery.v2", &fake, false);
    assert_eq!(code, Some(1));
    assert!(record["fingerprint"].is_null(), "{record}");
    assert!(stderr.contains("invalid parquet footer"), "{stderr}");
}

#[test]
fn diagnose_names_columns_and_keys_and_never_a_metadata_value() {
    let yaml = "fingerprint_id: diag.v1\nformat: parquet\nassertions:\n  - parquet_column_exists: loan_idd\n  - parquet_kv_metadata: { key: delivery, pattern: \"^nope$\" }\n";
    let (code, record, _) = run_definition(yaml, "diag.v1", &fixture("delivery.parquet"), true);
    assert_eq!(code, Some(1));
    let first = &record["fingerprint"]["assertions"][0]["context"];
    assert_eq!(first["nearest_match"], "loan_id");
    assert_eq!(first["row_count"], 30);
    assert_eq!(first["row_groups"], 3);
    assert_eq!(first["column_count"], 8);
    assert_eq!(
        first["metadata_keys"],
        // pyarrow adds ARROW:schema itself; the other three are the fixture's own.
        serde_json::json!(["ARROW:schema", "delivery", "producer", "secret_note"])
    );
    let rendered = record.to_string();
    for value in ["do-not-print-me", "fixture-gen", "cmbs.v2"] {
        assert!(
            !rendered.contains(value),
            "metadata value {value} leaked: {rendered}"
        );
    }
}

#[test]
fn compile_check_validates_parquet_definitions() {
    let dir = Scratch::new("check");
    let check = |yaml: &str| -> Output {
        let path = dir.0.join("def.fp.yaml");
        fs::write(&path, yaml).unwrap();
        Command::new(env!("CARGO_BIN_EXE_fingerprint"))
            .args(["compile", s(&path), "--check"])
            .output()
            .unwrap()
    };
    let ok = check(DELIVERY_OK);
    assert_eq!(
        ok.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&ok.stderr)
    );

    for (yaml, needle) in [
        (
            "fingerprint_id: x.v1\nformat: csv\nassertions:\n  - parquet_column_exists: a\n",
            "requires format 'parquet'",
        ),
        (
            "fingerprint_id: x.v1\nformat: json\nassertions:\n  - parquet_row_count: { min: 1 }\n",
            "requires format 'parquet'",
        ),
        (
            "fingerprint_id: x.v1\nformat: parquet\nassertions:\n  - parquet_row_count: {}\n",
            "at least one of 'min' or 'max'",
        ),
        (
            "fingerprint_id: x.v1\nformat: parquet\nassertions:\n  - parquet_row_groups: { min: 4, max: 2 }\n",
            "must be <=",
        ),
        (
            "fingerprint_id: x.v1\nformat: parquet\nassertions:\n  - parquet_kv_metadata: { key: k, pattern: \"(\" }\n",
            "not a valid regex",
        ),
        (
            "fingerprint_id: x.v1\nformat: parquet\nassertions:\n  - parquet_column_type: { column: a, parquet_type: \"\" }\n",
            "must not be empty",
        ),
        (
            "fingerprint_id: x.v1\nformat: parquet\nassertions:\n  - json_path_exists: \"$.a\"\n",
            "requires format 'json' or 'jsonl'",
        ),
    ] {
        let out = check(yaml);
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.status.code(), Some(2), "{needle}: {text}");
        assert!(text.contains(needle), "expected '{needle}' in: {text}");
    }
}
