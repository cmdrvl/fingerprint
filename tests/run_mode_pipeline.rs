use serde_json::{Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::{NamedTempFile, tempdir};

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn run_fingerprint(manifest_path: &Path, extra_args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fingerprint"));
    command.arg(manifest_path);
    command.args(extra_args);
    command.output().expect("run fingerprint binary")
}

fn run_fingerprint_with_definitions(
    manifest_path: &Path,
    extra_args: &[&str],
    definitions_dir: &Path,
) -> Output {
    let trust_file = NamedTempFile::new().expect("create trust file");
    std::fs::write(trust_file.path(), "trust:\n  - \"installed:*\"\n").expect("write trust file");
    let mut command = Command::new(env!("CARGO_BIN_EXE_fingerprint"));
    command.arg(manifest_path);
    command.args(extra_args);
    command.env("FINGERPRINT_DEFINITIONS", definitions_dir);
    command.env("FINGERPRINT_TRUST", trust_file.path());
    command.current_dir(env!("CARGO_MANIFEST_DIR"));
    command.output().expect("run fingerprint binary")
}

fn write_jsonl(records: &[Value]) -> NamedTempFile {
    let mut file = NamedTempFile::new().expect("create temp manifest");
    for record in records {
        serde_json::to_writer(&mut file, record).expect("serialize manifest record");
        file.write_all(b"\n").expect("write newline");
    }
    file.flush().expect("flush manifest file");
    file
}

fn parse_jsonl(stdout: &[u8]) -> Vec<Value> {
    let text = String::from_utf8(stdout.to_vec()).expect("stdout UTF-8");
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("parse JSON line"))
        .collect()
}

fn parse_witness_ledger(path: &Path) -> Vec<Value> {
    let contents = std::fs::read_to_string(path).expect("read witness ledger");
    contents
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("parse witness line"))
        .collect()
}

#[test]
fn run_mode_progress_flag_emits_structured_progress_events() {
    let csv_path = repo_path("tests/fixtures/files/sample.csv");
    let manifest = write_jsonl(&[json!({
        "version": "hash.v0",
        "path": csv_path.display().to_string(),
        "extension": ".csv",
        "bytes_hash": "blake3:csv",
        "tool_versions": { "hash": "0.1.0" }
    })]);

    let output = run_fingerprint(
        manifest.path(),
        &["--fp", "csv.v0", "--progress", "--no-witness"],
    );

    assert_eq!(output.status.code(), Some(0));
    let stderr_lines = parse_jsonl(&output.stderr);
    assert!(
        stderr_lines
            .iter()
            .any(|line| line["type"] == "progress" && line["tool"] == "fingerprint"),
        "progress mode should emit structured progress events to stderr"
    );
}

#[test]
fn run_mode_matches_installed_html_fingerprint_and_extracts_content() {
    let definitions_dir = tempdir().expect("create definitions dir");
    let html_fp = r#"
fingerprint_id: html-schedule.v1
format: html
assertions:
  - heading_exists: "Rent Roll"
  - section_non_empty:
      heading: "(?i)income capitali[sz]ation"
  - table_columns:
      heading: "(?i)rent roll"
      patterns:
        - "(?i)tenant"
        - "(?i)sq\\.?\\s*ft|sf"
        - "(?i)rent"
  - text_regex:
      pattern: "\\d+\\.\\d+%"
extract:
  - name: rent_roll
    type: table
    anchor_heading: "(?i)rent roll"
    index: 0
  - name: income_cap
    type: section
    anchor_heading: "(?i)income capitali[sz]ation"
  - name: cap_rate
    type: text_match
    anchor: "(?i)cap rate"
    pattern: "\\d+\\.\\d+%"
    within_chars: 12
content_hash:
  algorithm: blake3
  over:
    - rent_roll
    - income_cap
    - cap_rate
"#
    .trim();
    std::fs::write(
        definitions_dir.path().join("html-schedule.fp.yaml"),
        html_fp,
    )
    .expect("write html fingerprint definition");

    let html_file = NamedTempFile::with_suffix(".html").expect("create html temp file");
    std::fs::write(
        html_file.path(),
        r#"
<html>
  <body>
    <h1>Rent Roll</h1>
    <table>
      <tr><th>Tenant Name</th><th>Sq. Ft.</th><th>Monthly Rent</th></tr>
      <tr><td>Acme</td><td>1200</td><td>$10</td></tr>
    </table>
    <h2>Income Capitalization</h2>
    <p>Cap rate is 5.25% for the current period.</p>
  </body>
</html>
"#,
    )
    .expect("write html fixture");

    let manifest = write_jsonl(&[json!({
        "version": "hash.v0",
        "path": html_file.path().display().to_string(),
        "extension": ".html",
        "bytes_hash": "blake3:html",
        "tool_versions": { "hash": "0.1.0" }
    })]);

    let output = run_fingerprint_with_definitions(
        manifest.path(),
        &["--fp", "html-schedule.v1", "--no-witness"],
        definitions_dir.path(),
    );

    assert_eq!(output.status.code(), Some(0));
    let lines = parse_jsonl(&output.stdout);
    assert_eq!(lines.len(), 1);
    assert_eq!(
        lines[0]["fingerprint"]["fingerprint_id"],
        "html-schedule.v1"
    );
    assert_eq!(lines[0]["fingerprint"]["matched"], true);
    assert_eq!(
        lines[0]["fingerprint"]["extracted"]["rent_roll"]["columns"],
        json!(["Tenant Name", "Sq. Ft.", "Monthly Rent"])
    );
    assert_eq!(
        lines[0]["fingerprint"]["extracted"]["income_cap"]["heading"],
        "Income Capitalization"
    );
    assert_eq!(
        lines[0]["fingerprint"]["extracted"]["cap_rate"]["matched"],
        "5.25%"
    );
    assert!(
        lines[0]["fingerprint"]["content_hash"]
            .as_str()
            .is_some_and(|hash| hash.starts_with("blake3:")),
        "matched html fingerprints should emit a content hash when configured"
    );
}

#[test]
fn run_mode_evaluates_csv_fingerprint_against_text_extension() {
    let definitions_dir = tempdir().expect("create definitions dir");
    let csv_fp = r#"
fingerprint_id: cmbs-setup.v1
format: csv
assertions:
  - filename_regex:
      pattern: "(?i).*_STUP\\.txt$"
  - sheet_exists: "csv"
  - sheet_min_rows:
      sheet: "Sheet1"
      min_rows: 2
"#
    .trim();
    std::fs::write(definitions_dir.path().join("cmbs-setup.fp.yaml"), csv_fp)
        .expect("write csv fingerprint definition");
    let text_fp = r#"
fingerprint_id: generic-text.v1
format: text
assertions:
  - text_contains: "loan_id"
"#
    .trim();
    std::fs::write(definitions_dir.path().join("generic-text.fp.yaml"), text_fp)
        .expect("write text fingerprint definition");

    let csv_file = NamedTempFile::with_suffix("_STUP.txt").expect("create csv text file");
    std::fs::write(csv_file.path(), "loan_id,balance,rate\nA-1,1000000,5.25\n")
        .expect("write csv text fixture");

    let manifest = write_jsonl(&[json!({
        "version": "hash.v0",
        "path": csv_file.path().display().to_string(),
        "extension": ".txt",
        "mime_guess": "text/plain",
        "bytes_hash": "blake3:csv-text",
        "tool_versions": { "hash": "0.1.0" }
    })]);

    let output = run_fingerprint_with_definitions(
        manifest.path(),
        &[
            "--fp",
            "cmbs-setup.v1",
            "--fp",
            "generic-text.v1",
            "--no-witness",
        ],
        definitions_dir.path(),
    );

    assert_eq!(output.status.code(), Some(0));
    let lines = parse_jsonl(&output.stdout);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["fingerprint"]["fingerprint_id"], "cmbs-setup.v1");
    assert_eq!(lines[0]["fingerprint"]["matched"], true);
}

#[test]
fn run_mode_html_specific_assertions_keep_content_hash_stable_and_null_on_no_match() {
    let definitions_dir = tempdir().expect("create definitions dir");
    let html_fp = r#"
fingerprint_id: pennant-html.v1
format: html
assertions:
  - page_section_count:
      min: 2
      max: 2
  - dominant_column_count:
      count: 5
      tolerance: 0
      sample_pages: 2
  - header_token_search:
      tokens:
        - "(?i)^industry$"
      min_matches: 1
  - full_width_row:
      pattern: "(?i)^(first lien debt investments|equity investments)$"
      min_cells: 5
extract:
  - name: schedule_table
    type: table
    anchor_heading: "(?i)schedule of investments"
    index: 0
content_hash:
  algorithm: blake3
  over:
    - schedule_table
"#
    .trim();
    std::fs::write(definitions_dir.path().join("pennant-html.fp.yaml"), html_fp)
        .expect("write html fingerprint definition");

    let pennant_path = repo_path("tests/fixtures/html/bdc_soi_pennant_like.html");
    let blackrock_path = repo_path("tests/fixtures/html/bdc_soi_blackrock_like.html");
    let manifest = write_jsonl(&[
        json!({
            "version": "hash.v0",
            "path": pennant_path.display().to_string(),
            "extension": ".html",
            "bytes_hash": "blake3:pennant-one",
            "tool_versions": { "hash": "0.1.0" }
        }),
        json!({
            "version": "hash.v0",
            "path": pennant_path.display().to_string(),
            "extension": ".html",
            "bytes_hash": "blake3:pennant-two",
            "tool_versions": { "hash": "0.1.0" }
        }),
        json!({
            "version": "hash.v0",
            "path": blackrock_path.display().to_string(),
            "extension": ".html",
            "bytes_hash": "blake3:blackrock",
            "tool_versions": { "hash": "0.1.0" }
        }),
    ]);

    let output = run_fingerprint_with_definitions(
        manifest.path(),
        &["--fp", "pennant-html.v1", "--no-witness"],
        definitions_dir.path(),
    );

    assert_eq!(output.status.code(), Some(1));
    let lines = parse_jsonl(&output.stdout);
    assert_eq!(lines.len(), 3);

    let first_hash = lines[0]["fingerprint"]["content_hash"]
        .as_str()
        .expect("first matched content hash");
    let second_hash = lines[1]["fingerprint"]["content_hash"]
        .as_str()
        .expect("second matched content hash");
    assert_eq!(lines[0]["fingerprint"]["matched"], true);
    assert_eq!(lines[1]["fingerprint"]["matched"], true);
    assert_eq!(
        first_hash, second_hash,
        "matched HTML records with the same extracted content should produce stable hashes"
    );

    assert_eq!(lines[2]["fingerprint"]["matched"], false);
    assert_eq!(
        lines[2]["fingerprint"]["content_hash"],
        Value::Null,
        "unmatched HTML records must keep content_hash null"
    );
}

#[test]
fn run_mode_progress_flag_keeps_witness_failures_structured() {
    let csv_path = repo_path("tests/fixtures/files/sample.csv");
    let manifest = write_jsonl(&[json!({
        "version": "hash.v0",
        "path": csv_path.display().to_string(),
        "extension": ".csv",
        "bytes_hash": "blake3:csv",
        "tool_versions": { "hash": "0.1.0" }
    })]);
    let temp_dir = tempdir().expect("create tempdir");
    let blocker = temp_dir.path().join("blocked-parent");
    std::fs::write(&blocker, "not a directory").expect("create blocker file");
    let witness_path = blocker.join("witness.jsonl");

    let output = Command::new(env!("CARGO_BIN_EXE_fingerprint"))
        .arg(manifest.path())
        .args(["--fp", "csv.v0", "--progress"])
        .env("EPISTEMIC_WITNESS", &witness_path)
        .output()
        .expect("run fingerprint binary");

    assert_eq!(output.status.code(), Some(0));
    let stderr_lines = parse_jsonl(&output.stderr);
    assert!(
        stderr_lines.iter().any(|line| {
            line["type"] == "warning"
                && line["path"] == witness_path.display().to_string()
                && line["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("witness append failed"))
        }),
        "witness append failures should stay structured in progress mode"
    );
}

#[test]
fn run_mode_all_matched_exit_zero_and_preserves_order_with_jobs() {
    let csv_path = repo_path("tests/fixtures/files/sample.csv");
    let xlsx_path = repo_path("tests/fixtures/files/sample.xlsx");
    let pdf_path = repo_path("tests/fixtures/files/sample.pdf");
    let markdown_path = repo_path("tests/fixtures/files/sample.md");

    let records = vec![
        json!({
            "version": "hash.v0",
            "path": csv_path.display().to_string(),
            "extension": ".csv",
            "bytes_hash": "blake3:csv",
            "tool_versions": { "hash": "0.1.0" }
        }),
        json!({
            "version": "hash.v0",
            "path": xlsx_path.display().to_string(),
            "extension": ".xlsx",
            "bytes_hash": "blake3:xlsx",
            "tool_versions": { "hash": "0.1.0" }
        }),
        json!({
            "version": "hash.v0",
            "path": pdf_path.display().to_string(),
            "text_path": markdown_path.display().to_string(),
            "extension": ".pdf",
            "bytes_hash": "blake3:pdf",
            "tool_versions": { "hash": "0.1.0" }
        }),
    ];
    let manifest = write_jsonl(&records);

    let output = run_fingerprint(
        manifest.path(),
        &[
            "--fp",
            "csv.v0",
            "--fp",
            "xlsx.v0",
            "--fp",
            "pdf.v0",
            "--jobs",
            "4",
            "--no-witness",
        ],
    );

    assert_eq!(output.status.code(), Some(0));
    let lines = parse_jsonl(&output.stdout);
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0]["path"], records[0]["path"]);
    assert_eq!(lines[1]["path"], records[1]["path"]);
    assert_eq!(lines[2]["path"], records[2]["path"]);
    assert_eq!(lines[0]["fingerprint"]["matched"], true);
    assert_eq!(lines[1]["fingerprint"]["matched"], true);
    assert_eq!(lines[2]["fingerprint"]["matched"], true);
}

#[test]
fn run_mode_parse_failure_creates_new_skipped_and_exit_one() {
    let missing_markdown = repo_path("tests/fixtures/files/does-not-exist.md");
    let manifest = write_jsonl(&[json!({
        "version": "hash.v0",
        "path": missing_markdown.display().to_string(),
        "extension": ".md",
        "bytes_hash": "blake3:missing",
        "tool_versions": { "hash": "0.1.0" }
    })]);

    let output = run_fingerprint(manifest.path(), &["--fp", "markdown.v0", "--no-witness"]);

    assert_eq!(output.status.code(), Some(1));
    let lines = parse_jsonl(&output.stdout);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["_skipped"], true);
    assert_eq!(lines[0]["fingerprint"], Value::Null);
    assert_eq!(lines[0]["_warnings"][0]["code"], "E_PARSE");
}

#[test]
fn run_mode_builtin_xlsx_skips_unreadable_workbooks() {
    let corrupt_xlsx = repo_path("tests/fixtures/files/corrupt.xlsx");
    let manifest = write_jsonl(&[json!({
        "version": "hash.v0",
        "path": corrupt_xlsx.display().to_string(),
        "extension": ".xlsx",
        "bytes_hash": "blake3:corrupt",
        "tool_versions": { "hash": "0.1.0" }
    })]);

    let output = run_fingerprint(manifest.path(), &["--fp", "xlsx.v0", "--no-witness"]);

    assert_eq!(output.status.code(), Some(1));
    let lines = parse_jsonl(&output.stdout);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["_skipped"], true);
    assert_eq!(lines[0]["fingerprint"], Value::Null);
    assert_eq!(lines[0]["_warnings"][0]["code"], "E_PARSE");
}

#[test]
fn run_mode_xlsx_builtin_matches_legacy_xls_inputs() {
    let legacy_xls = repo_path("tests/fixtures/files/sample.xls");
    let manifest = write_jsonl(&[json!({
        "version": "hash.v0",
        "path": legacy_xls.display().to_string(),
        "extension": ".xls",
        "bytes_hash": "blake3:legacy-xls",
        "tool_versions": { "hash": "0.1.0" }
    })]);

    let output = run_fingerprint(manifest.path(), &["--fp", "xlsx.v0", "--no-witness"]);

    assert_eq!(output.status.code(), Some(0));
    let lines = parse_jsonl(&output.stdout);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["fingerprint"]["matched"], true);
}

#[test]
fn run_mode_diagnose_surfaces_attempt_history_and_nearest_match_context() {
    let definitions_dir = tempdir().expect("create definitions tempdir");
    std::fs::write(
        definitions_dir.path().join("near-miss.fp.yaml"),
        r#"
fingerprint_id: near-miss.v1
format: markdown
assertions:
  - name: rent_roll_detail_heading
    heading_regex:
      pattern: "(?i)rent roll detail"
"#,
    )
    .expect("write near-miss definition");
    std::fs::write(
        definitions_dir.path().join("winner.fp.yaml"),
        r#"
fingerprint_id: winner.v1
format: markdown
assertions:
  - name: rent_roll_summary_heading
    heading_regex:
      pattern: "(?i)rent roll summary"
"#,
    )
    .expect("write winner definition");
    std::fs::write(
        definitions_dir.path().join("later.fp.yaml"),
        r#"
fingerprint_id: later.v1
format: markdown
assertions:
  - name: property_description_heading
    heading_regex:
      pattern: "(?i)property description"
"#,
    )
    .expect("write later definition");

    let markdown_path = repo_path("tests/fixtures/test_files/cbre_appraisal.md");
    let manifest = write_jsonl(&[json!({
        "version": "hash.v0",
        "path": markdown_path.display().to_string(),
        "extension": ".md",
        "bytes_hash": "blake3:cbre",
        "tool_versions": { "hash": "0.1.0" }
    })]);

    let output = run_fingerprint_with_definitions(
        manifest.path(),
        &[
            "--diagnose",
            "--no-witness",
            "--fp",
            "near-miss.v1",
            "--fp",
            "winner.v1",
            "--fp",
            "later.v1",
        ],
        definitions_dir.path(),
    );

    assert_eq!(output.status.code(), Some(0));
    let lines = parse_jsonl(&output.stdout);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["fingerprint"]["fingerprint_id"], "winner.v1");
    assert_eq!(
        lines[0]["fingerprint"]["diagnostics"]["attempts"][0]["fingerprint_id"],
        "near-miss.v1"
    );
    assert_eq!(
        lines[0]["fingerprint"]["diagnostics"]["attempts"][0]["first_failed_assertion"]["name"],
        "rent_roll_detail_heading"
    );
    assert_eq!(
        lines[0]["fingerprint"]["diagnostics"]["attempts"][0]["first_failed_assertion"]["context"]
            ["nearest_match"],
        "RENT ROLL SUMMARY"
    );
    assert_eq!(
        lines[0]["fingerprint"]["diagnostics"]["attempts"][1]["fingerprint_id"],
        "winner.v1"
    );
    assert_eq!(
        lines[0]["fingerprint"]["diagnostics"]["short_circuited_fingerprint_ids"],
        json!(["later.v1"])
    );
    assert_eq!(
        lines[0]["fingerprint"]["diagnostics"]["all_candidates_failed"],
        false
    );
}

#[test]
fn run_mode_witness_records_exact_output_hash_and_append_receipts() {
    let csv_path = repo_path("tests/fixtures/files/sample.csv");
    let manifest = write_jsonl(&[json!({
        "version": "hash.v0",
        "path": csv_path.display().to_string(),
        "extension": ".csv",
        "bytes_hash": "blake3:csv",
        "tool_versions": { "hash": "0.1.0" }
    })]);
    let witness_dir = tempdir().expect("create witness tempdir");
    let witness_path = witness_dir.path().join("witness.jsonl");

    let first = Command::new(env!("CARGO_BIN_EXE_fingerprint"))
        .arg(manifest.path())
        .args(["--fp", "csv.v0", "--jobs", "4"])
        .env("EPISTEMIC_WITNESS", &witness_path)
        .output()
        .expect("run fingerprint binary");
    let second = Command::new(env!("CARGO_BIN_EXE_fingerprint"))
        .arg(manifest.path())
        .args(["--fp", "csv.v0", "--jobs", "4"])
        .env("EPISTEMIC_WITNESS", &witness_path)
        .output()
        .expect("run fingerprint binary");

    assert_eq!(first.status.code(), Some(0));
    assert_eq!(second.status.code(), Some(0));

    let witness_rows = parse_witness_ledger(&witness_path);
    assert_eq!(witness_rows.len(), 2);

    let manifest_bytes = std::fs::read(manifest.path()).expect("read manifest bytes");
    let expected_input_hash = format!("blake3:{}", blake3::hash(&manifest_bytes).to_hex());
    let expected_first_output_hash = format!("blake3:{}", blake3::hash(&first.stdout).to_hex());
    let expected_second_output_hash = format!("blake3:{}", blake3::hash(&second.stdout).to_hex());

    assert_eq!(
        witness_rows[0]["inputs"][0]["path"],
        manifest.path().display().to_string()
    );
    assert_eq!(witness_rows[0]["inputs"][0]["hash"], expected_input_hash);
    assert_eq!(
        witness_rows[0]["inputs"][0]["bytes"],
        u64::try_from(manifest_bytes.len()).expect("manifest length fits u64")
    );
    assert_eq!(witness_rows[0]["params"]["jobs"], 4);
    assert_eq!(witness_rows[0]["output_hash"], expected_first_output_hash);
    assert!(
        witness_rows[0]["binary_hash"]
            .as_str()
            .is_some_and(|value| value.starts_with("blake3:"))
    );
    assert_ne!(witness_rows[1]["id"], witness_rows[0]["id"]);
    assert_eq!(witness_rows[1]["params"]["jobs"], 4);
    assert_eq!(witness_rows[1]["output_hash"], expected_second_output_hash);
}

#[test]
fn run_mode_refusal_appends_witness_record() {
    let mut manifest = NamedTempFile::new().expect("create malformed manifest");
    writeln!(
        manifest,
        "{{\"version\":\"hash.v0\",\"path\":\"{}\",\"extension\":\".csv\",\"bytes_hash\":\"blake3:csv\"}}",
        repo_path("tests/fixtures/files/sample.csv").display()
    )
    .expect("write first manifest line");
    writeln!(manifest, "{{\"version\":\"hash.v0\",\"path\":").expect("write malformed line");
    manifest.flush().expect("flush malformed manifest");

    let witness_dir = tempdir().expect("create witness tempdir");
    let witness_path = witness_dir.path().join("witness.jsonl");
    let output = Command::new(env!("CARGO_BIN_EXE_fingerprint"))
        .arg(manifest.path())
        .args(["--fp", "csv.v0"])
        .env("EPISTEMIC_WITNESS", &witness_path)
        .output()
        .expect("run fingerprint binary");

    assert_eq!(output.status.code(), Some(2));

    let witness_rows = parse_witness_ledger(&witness_path);
    assert_eq!(witness_rows.len(), 1);
    assert_eq!(witness_rows[0]["outcome"], "REFUSAL");
    assert_eq!(witness_rows[0]["exit_code"], 2);
    assert_eq!(
        witness_rows[0]["output_hash"],
        format!("blake3:{}", blake3::hash(&output.stdout).to_hex())
    );
    assert_eq!(
        witness_rows[0]["inputs"][0]["path"],
        manifest.path().display().to_string()
    );
}

#[test]
fn run_mode_upstream_skipped_passthrough_keeps_fingerprint_null() {
    let csv_path = repo_path("tests/fixtures/files/sample.csv");
    let manifest = write_jsonl(&[json!({
        "version": "hash.v0",
        "path": csv_path.display().to_string(),
        "_skipped": true,
        "tool_versions": { "hash": "0.1.0" }
    })]);

    let output = run_fingerprint(manifest.path(), &["--fp", "csv.v0", "--no-witness"]);

    assert_eq!(output.status.code(), Some(0));
    let lines = parse_jsonl(&output.stdout);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["_skipped"], true);
    assert_eq!(lines[0]["fingerprint"], Value::Null);
    assert_eq!(lines[0]["tool_versions"]["hash"], "0.1.0");
    assert_eq!(
        lines[0]["tool_versions"]["fingerprint"],
        env!("CARGO_PKG_VERSION")
    );
    assert!(lines[0].get("_warnings").is_none());
}

#[test]
fn run_mode_refusal_unknown_fingerprint_has_envelope_shape() {
    let csv_path = repo_path("tests/fixtures/files/sample.csv");
    let manifest = write_jsonl(&[json!({
        "version": "hash.v0",
        "path": csv_path.display().to_string(),
        "extension": ".csv",
        "bytes_hash": "blake3:csv",
        "tool_versions": { "hash": "0.1.0" }
    })]);

    let output = run_fingerprint(
        manifest.path(),
        &["--fp", "does-not-exist.v9", "--no-witness"],
    );

    assert_eq!(output.status.code(), Some(2));
    let lines = parse_jsonl(&output.stdout);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["version"], "fingerprint.v0");
    assert_eq!(lines[0]["outcome"], "REFUSAL");
    assert_eq!(lines[0]["refusal"]["code"], "E_UNKNOWN_FP");
    assert_eq!(
        lines[0]["refusal"]["detail"]["fingerprint_id"],
        "does-not-exist.v9"
    );
}

#[test]
fn run_mode_refusal_bad_input_has_envelope_shape() {
    let mut manifest = NamedTempFile::new().expect("create malformed manifest");
    writeln!(
        manifest,
        "{{\"version\":\"hash.v0\",\"path\":\"{}\",\"extension\":\".csv\",\"bytes_hash\":\"blake3:csv\"}}",
        repo_path("tests/fixtures/files/sample.csv").display()
    )
    .expect("write first manifest line");
    writeln!(manifest, "{{\"version\":\"hash.v0\",\"path\":").expect("write malformed line");
    manifest.flush().expect("flush malformed manifest");

    let output = run_fingerprint(manifest.path(), &["--fp", "csv.v0", "--no-witness"]);

    assert_eq!(output.status.code(), Some(2));
    let lines = parse_jsonl(&output.stdout);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["version"], "fingerprint.v0");
    assert_eq!(lines[0]["outcome"], "REFUSAL");
    assert_eq!(lines[0]["refusal"]["code"], "E_BAD_INPUT");
    assert!(
        lines[0]["refusal"]["detail"]["error"]
            .as_str()
            .expect("bad input detail error")
            .contains("invalid JSON")
    );
}
