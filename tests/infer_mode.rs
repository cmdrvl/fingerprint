use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn run_fingerprint(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fingerprint"))
        .args(args)
        .output()
        .expect("run fingerprint binary")
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
