use fingerprint::dsl::FingerprintDefinition;
use fingerprint::dsl::assertions::Assertion;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::tempdir;

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn run_infer(dir: &Path, args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fingerprint"));
    command.arg("--no-witness").arg("infer").arg(dir).args(args);
    command.output().expect("run fingerprint infer")
}

fn parse_yaml(stdout: &[u8]) -> FingerprintDefinition {
    let text = String::from_utf8(stdout.to_vec()).expect("utf8 yaml");
    serde_yaml::from_str(&text).expect("parse emitted yaml")
}

#[test]
fn infer_subcommand_emits_parseable_yaml_and_respects_no_extract() {
    let dir = repo_path("tests/fixtures/files");
    let output = run_infer(
        &dir,
        &[
            "--format",
            "xlsx",
            "--id",
            "test-inferred.v1",
            "--min-confidence",
            "0.9",
            "--no-extract",
        ],
    );

    assert_eq!(output.status.code(), Some(0));
    let rendered = String::from_utf8(output.stdout.clone()).expect("utf8 output");
    assert!(rendered.contains("# confidence:"));

    let parsed = parse_yaml(&output.stdout);
    assert_eq!(parsed.fingerprint_id, "test-inferred.v1");
    assert_eq!(parsed.format, "xlsx");
    assert!(parsed.extract.is_empty());
    assert!(parsed.content_hash.is_none());
}

#[test]
fn infer_subcommand_min_confidence_filters_partial_csv_headers() {
    let corpus = tempdir().expect("create temp corpus");
    fs::write(corpus.path().join("a.csv"), "name,city\nAlice,Seattle\n").expect("write a.csv");
    fs::write(corpus.path().join("b.csv"), "name,state\nBob,WA\n").expect("write b.csv");

    let output = run_infer(
        corpus.path(),
        &[
            "--format",
            "csv",
            "--id",
            "csv-inferred.v1",
            "--min-confidence",
            "1.0",
            "--no-extract",
        ],
    );

    assert_eq!(output.status.code(), Some(0));
    let parsed = parse_yaml(&output.stdout);
    let mut found_a1_name = false;
    let mut found_b1 = false;

    for named in parsed.assertions {
        match named.assertion {
            Assertion::CellEq { cell, value, .. } if cell == "A1" && value == "name" => {
                found_a1_name = true;
            }
            Assertion::CellEq { cell, .. } if cell == "B1" => {
                found_b1 = true;
            }
            _ => {}
        }
    }

    assert!(found_a1_name, "expected common A1 header assertion");
    assert!(
        !found_b1,
        "B1 header should be filtered at min-confidence 1.0"
    );
}
