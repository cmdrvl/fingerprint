//! Native `json` / `jsonl` document formats (bd-yq3, JSON slice): DSL runtime, --diagnose,
//! `compile --check` validation, and the zero-retention property of failure context.

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/json")
        .join(name)
}

struct Scratch(PathBuf);
impl Scratch {
    fn new(tag: &str) -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let unique = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("fp-json-{tag}-{}-{unique}", std::process::id()));
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

/// Run `fingerprint match <file> --fp <id> [--diagnose]` with `yaml` installed as the only
/// definition. Returns (exit code, the one output record, stderr).
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

fn failed_details(record: &Value) -> Vec<(String, String)> {
    record["fingerprint"]["assertions"]
        .as_array()
        .map(|assertions| {
            assertions
                .iter()
                .filter(|a| a["passed"] == false)
                .map(|a| {
                    (
                        a["name"].as_str().unwrap_or("").to_owned(),
                        a["detail"].as_str().unwrap_or("").to_owned(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

const JSONL_OK: &str = r#"
fingerprint_id: loans-feed.v1
format: jsonl
assertions:
  - json_record_count:
      min: 3
      max: 10
  - json_path_exists: "$.loan_id"
  - json_path_type:
      path: "$.balance"
      json_type: number
  - json_path_type:
      path: "$.tags"
      json_type: array
  - json_keys_include:
      path: "$"
      keys: [loan_id, balance, status]
"#;

const JSON_OK: &str = r#"
fingerprint_id: delivery.v2
format: json
assertions:
  - json_path_exists: "$.filings[1].form"
  - json_array_len:
      path: "$.filings"
      min: 1
      max: 10
  - json_keys_include:
      path: "$.meta"
      keys: [generated, count]
  - json_path_type:
      path: "$.meta.count"
      json_type: integer
  - json_path_type:
      path: "$.meta.complete"
      json_type: boolean
  - json_record_count:
      min: 1
      max: 1
"#;

#[test]
fn jsonl_fingerprint_matches_a_real_feed() {
    let (code, record, stderr) =
        run_definition(JSONL_OK, "loans-feed.v1", &fixture("loans.jsonl"), false);
    assert_eq!(code, Some(0), "{stderr}\n{record}");
    assert_eq!(record["fingerprint"]["matched"], true, "{record}");
    assert_eq!(record["fingerprint"]["fingerprint_id"], "loans-feed.v1");
    assert_eq!(
        record["fingerprint"]["assertions"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
}

#[test]
fn json_fingerprint_matches_a_nested_delivery() {
    let (code, record, stderr) =
        run_definition(JSON_OK, "delivery.v2", &fixture("delivery.json"), false);
    assert_eq!(code, Some(0), "{stderr}\n{record}");
    assert_eq!(record["fingerprint"]["matched"], true, "{record}");
}

// ---- planted negatives: each assertion kind fails, with a specific detail -----------

#[test]
fn every_json_assertion_fails_when_its_condition_is_false() {
    let cases: [(&str, &str, &str); 6] = [
        (
            "  - json_path_exists: \"$.nope\"\n",
            "jsonl",
            "json path '$.nope' not found",
        ),
        (
            "  - json_path_type:\n      path: \"$.loan_id\"\n      json_type: number\n",
            "jsonl",
            "json path '$.loan_id' is string, expected number",
        ),
        (
            "  - json_array_len:\n      path: \"$.tags\"\n      min: 5\n",
            "jsonl",
            "array length at '$.tags' is 1, expected at least 5",
        ),
        (
            "  - json_keys_include:\n      path: \"$\"\n      keys: [loan_id, zzz, aaa]\n",
            "jsonl",
            "object at '$' is missing key(s): aaa, zzz",
        ),
        (
            "  - json_record_count:\n      max: 4\n",
            "jsonl",
            "record count is 5, expected at most 4",
        ),
        (
            "  - json_array_len:\n      path: \"$.loan_id\"\n      min: 1\n",
            "jsonl",
            "json path '$.loan_id' is string, expected array",
        ),
    ];
    for (assertion, format, expected_detail) in cases {
        let yaml = format!("fingerprint_id: neg.v1\nformat: {format}\nassertions:\n{assertion}");
        let (code, record, stderr) =
            run_definition(&yaml, "neg.v1", &fixture("loans.jsonl"), false);
        assert_eq!(code, Some(1), "{assertion}: {stderr}");
        assert_eq!(record["fingerprint"]["matched"], false, "{assertion}");
        let failed = failed_details(&record);
        assert_eq!(failed.len(), 1, "{assertion}: {record}");
        assert_eq!(failed[0].1, expected_detail, "{assertion}");
    }
}

#[test]
fn a_json_fingerprint_does_not_apply_to_a_jsonl_file_and_vice_versa() {
    let (code, record, _) = run_definition(JSON_OK, "delivery.v2", &fixture("loans.jsonl"), false);
    assert_eq!(code, Some(1));
    assert!(
        record["fingerprint"].is_null(),
        "format mismatch is not evaluated: {record}"
    );
    let (_, record, _) =
        run_definition(JSONL_OK, "loans-feed.v1", &fixture("delivery.json"), false);
    assert!(record["fingerprint"].is_null(), "{record}");
}

#[test]
fn a_malformed_jsonl_line_is_a_typed_open_failure_naming_the_line() {
    let (code, record, stderr) = run_definition(
        JSONL_OK,
        "loans-feed.v1",
        &fixture("loans_bad.jsonl"),
        false,
    );
    assert_eq!(code, Some(1));
    assert!(record["fingerprint"].is_null(), "{record}");
    assert!(
        stderr.contains("line 3"),
        "stderr should name the bad line: {stderr}"
    );
}

// ---- diagnose: structure only, never values -------------------------------------------

#[test]
fn diagnose_names_the_nearest_key_and_leaks_no_values() {
    let yaml = "fingerprint_id: diag.v1\nformat: json\nassertions:\n  - json_path_exists: \"$.meta.generatd\"\n";
    let (code, record, _) = run_definition(yaml, "diag.v1", &fixture("delivery.json"), true);
    assert_eq!(code, Some(1));
    let context = &record["fingerprint"]["assertions"][0]["context"];
    assert_eq!(context["resolved_prefix"], "$.meta");
    assert_eq!(context["found_type"], "object");
    assert_eq!(context["nearest_match"], "generated");
    assert_eq!(
        context["available_keys"],
        serde_json::json!(["complete", "count", "generated"])
    );
    let rendered = record.to_string();
    for value in ["2026-01-01", "N-PORT-P", "ABS-EE", "0001-26-000001"] {
        assert!(
            !rendered.contains(value),
            "value {value} leaked: {rendered}"
        );
    }
}

// ---- compile --check validation ----------------------------------------------------------

#[test]
fn compile_check_rejects_bad_json_definitions() {
    let dir = Scratch::new("check");
    let check = |yaml: &str| -> Output {
        let path = dir.0.join("def.fp.yaml");
        fs::write(&path, yaml).unwrap();
        Command::new(env!("CARGO_BIN_EXE_fingerprint"))
            .args(["compile", s(&path), "--check"])
            .output()
            .unwrap()
    };
    let ok = check(JSON_OK);
    assert_eq!(
        ok.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&ok.stderr)
    );

    for (yaml, needle) in [
        (
            "fingerprint_id: x.v1\nformat: csv\nassertions:\n  - json_path_exists: \"$.a\"\n",
            "requires format 'json' or 'jsonl'",
        ),
        (
            "fingerprint_id: x.v1\nformat: json\nassertions:\n  - json_path_exists: \"a.b\"\n",
            "must start with '$'",
        ),
        (
            "fingerprint_id: x.v1\nformat: json\nassertions:\n  - json_path_type:\n      path: \"$.a\"\n      json_type: float\n",
            "is not one of",
        ),
        (
            "fingerprint_id: x.v1\nformat: json\nassertions:\n  - json_array_len:\n      path: \"$.a\"\n",
            "at least one of 'min' or 'max'",
        ),
        (
            "fingerprint_id: x.v1\nformat: json\nassertions:\n  - json_array_len:\n      path: \"$.a\"\n      min: 5\n      max: 2\n",
            "must be <=",
        ),
        (
            "fingerprint_id: x.v1\nformat: json\nassertions:\n  - json_keys_include:\n      path: \"$\"\n      keys: []\n",
            "at least one non-empty key",
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

#[test]
fn large_jsonl_is_counted_exactly_without_loading_it() {
    let dir = Scratch::new("big");
    let path = dir.0.join("big.jsonl");
    let mut body = String::new();
    for i in 0..50_000 {
        body.push_str(&format!("{{\"id\":{i},\"v\":\"x\"}}\n"));
    }
    fs::write(&path, body).unwrap();
    let yaml = "fingerprint_id: big.v1\nformat: jsonl\nassertions:\n  - json_record_count:\n      min: 50000\n      max: 50000\n  - json_path_type:\n      path: \"$.id\"\n      json_type: integer\n";
    let (code, record, stderr) = run_definition(yaml, "big.v1", &path, false);
    assert_eq!(code, Some(0), "{stderr}\n{record}");
}
