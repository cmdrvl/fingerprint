use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::NamedTempFile;

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn run_fingerprint(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fingerprint"))
        .args(args)
        .output()
        .expect("run fingerprint binary")
}

fn run_fingerprint_with_definitions(args: &[&str], definitions_dir: &Path) -> Output {
    let trust_file = NamedTempFile::new().expect("create trust file");
    fs::write(trust_file.path(), "trust:\n  - \"installed:*\"\n").expect("write trust file");
    Command::new(env!("CARGO_BIN_EXE_fingerprint"))
        .args(args)
        .env("FINGERPRINT_DEFINITIONS", definitions_dir)
        .env("FINGERPRINT_TRUST", trust_file.path())
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run fingerprint binary with definitions")
}

fn parse_jsonl(stdout: &[u8]) -> Vec<Value> {
    String::from_utf8(stdout.to_vec())
        .expect("stdout utf8")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("parse jsonl line"))
        .collect()
}

#[test]
fn infer_xlsx_writes_yaml_that_compile_check_accepts() {
    let tempdir = tempfile::tempdir().expect("create tempdir");
    let out_path = tempdir.path().join("inferred.fp.yaml");
    let dir = repo_path("tests/fixtures/files");

    let infer = run_fingerprint(&[
        "--no-witness",
        "infer",
        dir.to_str().expect("dir str"),
        "--format",
        "xlsx",
        "--id",
        "test-inferred.v1",
        "--out",
        out_path.to_str().expect("out str"),
    ]);
    assert_eq!(infer.status.code(), Some(0), "infer should succeed");

    let yaml = fs::read_to_string(&out_path).expect("read inferred yaml");
    assert!(
        yaml.contains("# confidence:"),
        "confidence comments expected"
    );

    let check = run_fingerprint(&["compile", out_path.to_str().expect("out str"), "--check"]);
    assert_eq!(
        check.status.code(),
        Some(0),
        "inferred yaml should pass compile --check"
    );
}

#[test]
fn infer_accepts_xls_alias_and_emits_xlsx_fingerprints() {
    let corpus = tempfile::tempdir().expect("create tempdir");
    fs::copy(
        repo_path("tests/fixtures/files/sample.xls"),
        corpus.path().join("sample.xls"),
    )
    .expect("copy xls fixture");

    let output = run_fingerprint(&[
        "--no-witness",
        "infer",
        corpus.path().to_str().expect("dir str"),
        "--format",
        "xls",
        "--id",
        "legacy-sheet.v1",
        "--no-extract",
    ]);

    assert_eq!(output.status.code(), Some(0));
    let yaml = String::from_utf8(output.stdout).expect("utf8");
    assert!(yaml.contains("\\.xls"), "expected xls-aware filename regex");
    assert!(
        !yaml.contains("\\.xlsx$"),
        "legacy-only corpus should not force xlsx-only regex"
    );
    let parsed: Value = serde_yaml::from_str(&yaml).expect("parse inferred yaml");
    assert_eq!(parsed["format"], "xlsx");
}

#[test]
fn infer_is_deterministic_for_same_inputs() {
    let dir = repo_path("tests/fixtures/files");
    let first = run_fingerprint(&[
        "--no-witness",
        "infer",
        dir.to_str().expect("dir str"),
        "--format",
        "xlsx",
        "--id",
        "test-inferred.v1",
    ]);
    let second = run_fingerprint(&[
        "--no-witness",
        "infer",
        dir.to_str().expect("dir str"),
        "--format",
        "xlsx",
        "--id",
        "test-inferred.v1",
    ]);

    assert_eq!(first.status.code(), Some(0));
    assert_eq!(second.status.code(), Some(0));
    assert_eq!(first.stdout, second.stdout);
}

#[test]
fn min_confidence_filters_low_support_assertions() {
    let tempdir = tempfile::tempdir().expect("create tempdir");
    let csv_a = tempdir.path().join("a.csv");
    let csv_b = tempdir.path().join("b.csv");
    fs::write(&csv_a, "name,city\nAda,London\n").expect("write csv a");
    fs::write(&csv_b, "name,state\nBob,CA\n").expect("write csv b");

    let strict = run_fingerprint(&[
        "--no-witness",
        "infer",
        tempdir.path().to_str().expect("tempdir str"),
        "--format",
        "csv",
        "--id",
        "csv-inferred.v1",
        "--min-confidence",
        "1.0",
    ]);
    let loose = run_fingerprint(&[
        "--no-witness",
        "infer",
        tempdir.path().to_str().expect("tempdir str"),
        "--format",
        "csv",
        "--id",
        "csv-inferred.v1",
        "--min-confidence",
        "0.5",
    ]);

    assert_eq!(strict.status.code(), Some(0));
    assert_eq!(loose.status.code(), Some(0));

    let strict_yaml = String::from_utf8(strict.stdout).expect("strict utf8");
    let loose_yaml = String::from_utf8(loose.stdout).expect("loose utf8");
    let strict_cell_eq = strict_yaml.matches("cell_eq:").count();
    let loose_cell_eq = loose_yaml.matches("cell_eq:").count();
    assert!(loose_cell_eq > strict_cell_eq);
}

#[test]
fn no_extract_omits_extract_and_content_hash_sections() {
    let dir = repo_path("tests/fixtures/files");
    let output = run_fingerprint(&[
        "--no-witness",
        "infer",
        dir.to_str().expect("dir str"),
        "--format",
        "xlsx",
        "--id",
        "test-inferred.v1",
        "--no-extract",
    ]);

    assert_eq!(output.status.code(), Some(0));
    let yaml = String::from_utf8(output.stdout).expect("utf8");
    assert!(!yaml.contains("\nextract:\n"));
    assert!(!yaml.contains("\ncontent_hash:\n"));
}

#[test]
fn infer_appends_witness_with_mode_and_corpus_size() {
    let tempdir = tempfile::tempdir().expect("create tempdir");
    let witness_path = tempdir.path().join("witness.jsonl");
    let dir = repo_path("tests/fixtures/files");

    let output = Command::new(env!("CARGO_BIN_EXE_fingerprint"))
        .args([
            "infer",
            dir.to_str().expect("dir str"),
            "--format",
            "xlsx",
            "--id",
            "test-inferred.v1",
        ])
        .env("EPISTEMIC_WITNESS", &witness_path)
        .output()
        .expect("run fingerprint binary");

    assert_eq!(output.status.code(), Some(0));
    let witness = fs::read_to_string(&witness_path).expect("read witness");
    let last_line = witness
        .lines()
        .last()
        .expect("witness should have at least one line");
    let record: Value = serde_json::from_str(last_line).expect("parse witness json");

    assert_eq!(record["tool"], "fingerprint");
    assert_eq!(record["params"]["mode"], "infer");
    assert_eq!(record["params"]["format"], "xlsx");
    assert!(
        record["params"]["corpus_size"]
            .as_u64()
            .expect("corpus size u64")
            >= 1
    );
}

#[test]
fn infer_html_is_deterministic_for_same_inputs() {
    let tempdir = tempfile::tempdir().expect("create tempdir");
    fs::copy(
        repo_path("tests/fixtures/html/bdc_soi_ares_like.html"),
        tempdir.path().join("ares.html"),
    )
    .expect("copy html fixture");

    let first = run_fingerprint(&[
        "--no-witness",
        "infer",
        tempdir.path().to_str().expect("dir str"),
        "--format",
        "html",
        "--id",
        "ares-inferred.v1",
    ]);
    let second = run_fingerprint(&[
        "--no-witness",
        "infer",
        tempdir.path().to_str().expect("dir str"),
        "--format",
        "html",
        "--id",
        "ares-inferred.v1",
    ]);

    assert_eq!(first.status.code(), Some(0));
    assert_eq!(second.status.code(), Some(0));
    assert_eq!(first.stdout, second.stdout);
}

#[test]
fn infer_html_output_bootstraps_positive_match_and_negative_rejection() {
    let corpus = tempfile::tempdir().expect("create corpus tempdir");
    fs::copy(
        repo_path("tests/fixtures/html/bdc_soi_ares_like.html"),
        corpus.path().join("ares.html"),
    )
    .expect("copy html fixture");

    let out_dir = tempfile::tempdir().expect("create out dir");
    let out_path = out_dir.path().join("ares-inferred.fp.yaml");
    let infer = run_fingerprint(&[
        "--no-witness",
        "infer",
        corpus.path().to_str().expect("dir str"),
        "--format",
        "html",
        "--id",
        "ares-inferred.v1",
        "--out",
        out_path.to_str().expect("out str"),
    ]);
    assert_eq!(infer.status.code(), Some(0), "html infer should succeed");

    let check = run_fingerprint(&["compile", out_path.to_str().expect("out str"), "--check"]);
    assert_eq!(
        check.status.code(),
        Some(0),
        "html inferred yaml should pass compile --check"
    );

    let definitions_dir = tempfile::tempdir().expect("create definitions dir");
    fs::copy(
        &out_path,
        definitions_dir.path().join("ares-inferred.fp.yaml"),
    )
    .expect("install inferred definition");

    let manifest_path = out_dir.path().join("manifest.jsonl");
    fs::write(
        &manifest_path,
        format!(
            concat!(
                "{{\"version\":\"hash.v0\",\"path\":\"{}\",\"extension\":\".html\",\"bytes_hash\":\"blake3:ares\",",
                "\"tool_versions\":{{\"hash\":\"0.1.0\"}}}}\n",
                "{{\"version\":\"hash.v0\",\"path\":\"{}\",\"extension\":\".html\",\"bytes_hash\":\"blake3:pennant\",",
                "\"tool_versions\":{{\"hash\":\"0.1.0\"}}}}\n"
            ),
            repo_path("tests/fixtures/html/bdc_soi_ares_like.html").display(),
            repo_path("tests/fixtures/html/bdc_soi_pennant_like.html").display(),
        ),
    )
    .expect("write manifest");

    let run = run_fingerprint_with_definitions(
        &[
            "--no-witness",
            "--fp",
            "ares-inferred.v1",
            manifest_path.to_str().expect("manifest str"),
        ],
        definitions_dir.path(),
    );
    assert_eq!(
        run.status.code(),
        Some(1),
        "positive match plus negative rejection should return PARTIAL\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let records = parse_jsonl(&run.stdout);
    assert_eq!(records.len(), 2);
    assert_eq!(records[0]["fingerprint"]["matched"], true);
    assert_eq!(
        records[0]["fingerprint"]["fingerprint_id"],
        "ares-inferred.v1"
    );
    assert_eq!(records[1]["fingerprint"]["matched"], false);
}

#[test]
fn infer_html_weak_corpus_fails_with_actionable_error() {
    let tempdir = tempfile::tempdir().expect("create tempdir");
    fs::copy(
        repo_path("tests/fixtures/html/minimal_empty_shell.html"),
        tempdir.path().join("empty.html"),
    )
    .expect("copy weak html fixture");

    let output = run_fingerprint(&[
        "--no-witness",
        "infer",
        tempdir.path().to_str().expect("dir str"),
        "--format",
        "html",
        "--id",
        "weak-html.v1",
    ]);

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).expect("stderr utf8");
    assert!(
        stderr.contains("html corpus did not expose stable structural signals"),
        "weak html corpus error should be actionable, got: {stderr}"
    );
}
