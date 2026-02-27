use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::NamedTempFile;

fn fixture(path: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

fn run_fingerprint(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fingerprint"))
        .args(args)
        .output()
        .expect("run fingerprint binary")
}

fn create_manifest_with_content(content: &str) -> NamedTempFile {
    let file = NamedTempFile::new().expect("create temp manifest");
    fs::write(file.path(), content).expect("write manifest content");
    file
}

#[test]
fn run_mode_output_is_deterministic_for_same_input() {
    let manifest_content = format!(
        r#"{{"version":"hash.v0","path":"{}","extension":".csv","bytes_hash":"sha256:test","tool_versions":{{"hash":"0.1.0"}}}}"#,
        fixture("tests/fixtures/files/sample.csv").display()
    );
    let manifest = create_manifest_with_content(&manifest_content);

    let first = run_fingerprint(&[
        "--no-witness",
        "--fp",
        "csv.v0",
        manifest.path().to_str().expect("manifest path"),
    ]);
    let second = run_fingerprint(&[
        "--no-witness",
        "--fp",
        "csv.v0",
        manifest.path().to_str().expect("manifest path"),
    ]);

    assert_eq!(first.status.code(), Some(0));
    assert_eq!(second.status.code(), Some(0));
    assert_eq!(first.stdout, second.stdout);
}

#[test]
fn infer_mode_output_is_deterministic_for_same_input() {
    let dir = fixture("tests/fixtures/files");

    let first = run_fingerprint(&[
        "--no-witness",
        "infer",
        dir.to_str().expect("dir path"),
        "--format",
        "xlsx",
        "--id",
        "determinism-test.v1",
    ]);
    let second = run_fingerprint(&[
        "--no-witness",
        "infer",
        dir.to_str().expect("dir path"),
        "--format",
        "xlsx",
        "--id",
        "determinism-test.v1",
    ]);

    assert_eq!(first.status.code(), Some(0));
    assert_eq!(second.status.code(), Some(0));
    assert_eq!(first.stdout, second.stdout);
}

#[test]
fn compile_mode_output_is_deterministic_for_same_dsl() {
    let dsl = r#"
fingerprint_id: determinism-test.v1
format: csv
assertions:
  - filename_regex:
      pattern: "(?i).*\\.csv$"
"#
    .trim();

    let yaml = NamedTempFile::with_suffix(".fp.yaml").expect("create temp yaml");
    fs::write(yaml.path(), dsl).expect("write dsl");

    let first = run_fingerprint(&["compile", yaml.path().to_str().expect("yaml path")]);
    let second = run_fingerprint(&["compile", yaml.path().to_str().expect("yaml path")]);

    assert_eq!(first.status.code(), Some(0));
    assert_eq!(second.status.code(), Some(0));
    assert_eq!(first.stdout, second.stdout);
}
