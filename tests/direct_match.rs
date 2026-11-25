//! `fingerprint match <PATH>... --fp <ID>...` (bd-h5o): the zero-pipe direct mode must give
//! the same per-file result as `vacuum | hashbytes | fingerprint`, deterministically, with
//! bounded `--view brief` and a `--view delta --since` loop.

use serde_json::{Value, json};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn fingerprint(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fingerprint"))
        .args(args)
        .arg("--no-witness")
        .output()
        .expect("run fingerprint")
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/test_files")
        .join(name)
}

/// A fresh scratch directory under the system temp dir (removed on drop).
struct Scratch(PathBuf);
impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("fp-match-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn lines(out: &Output) -> Vec<Value> {
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| serde_json::from_str(l).expect("jsonl line"))
        .collect()
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

/// What the piped pipeline produces for the same files: a hash.v0 manifest assembled here
/// (independently of src/direct.rs, hashing with the blake3 crate directly) fed on stdin.
fn piped(files: &[&Path], fps: &[&str]) -> Vec<Value> {
    let mut manifest = String::new();
    for file in files {
        let hash = blake3::hash(&fs::read(file).unwrap()).to_hex();
        let ext = file
            .extension()
            .map(|e| format!(".{}", e.to_str().unwrap().to_ascii_lowercase()));
        manifest.push_str(
            &json!({
                "version": "hash.v0", "path": s(file), "extension": ext,
                "bytes_hash": format!("blake3:{hash}"), "hash_algorithm": "blake3",
                "tool_versions": {"hash": "0.1.0"}
            })
            .to_string(),
        );
        manifest.push('\n');
    }
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_fingerprint"));
    for fp in fps {
        cmd.args(["--fp", fp]);
    }
    let mut child = cmd
        .arg("--no-witness")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(manifest.as_bytes())
        .unwrap();
    lines(&child.wait_with_output().unwrap())
}

/// The fields both routes must agree on for a file.
fn core(record: &Value) -> Value {
    json!({
        "path": record["path"], "bytes_hash": record["bytes_hash"],
        "hash_algorithm": record["hash_algorithm"], "fingerprint": record["fingerprint"],
        "version": record["version"],
    })
}

#[test]
fn match_equals_the_piped_pipeline_for_the_same_files() {
    let (csv, xlsx, bad, pdf) = (
        fixture("simple.csv"),
        fixture("sample.xlsx"),
        fixture("corrupt.xlsx"),
        fixture("report.pdf"),
    );
    let fps = ["csv.v0", "xlsx.v0"];
    let direct = fingerprint(&[
        "match",
        s(&csv),
        s(&xlsx),
        s(&bad),
        s(&pdf),
        "--fp",
        "csv.v0",
        "--fp",
        "xlsx.v0",
    ]);
    let direct_records = lines(&direct);
    assert_eq!(direct_records.len(), 4);

    // Walk order is by path, whatever the argument order.
    let mut sorted: Vec<&Path> = vec![&csv, &xlsx, &bad, &pdf];
    sorted.sort();
    let piped_records = piped(&sorted, &fps);
    let direct_core: Vec<Value> = direct_records.iter().map(core).collect();
    let piped_core: Vec<Value> = piped_records.iter().map(core).collect();
    assert_eq!(direct_core, piped_core);

    // Same outcome/exit as the pipe: something unmatched makes it PARTIAL (1).
    assert_eq!(direct.status.code(), Some(1));
    let all_matched = fingerprint(&["match", s(&csv), "--fp", "csv.v0"]);
    assert_eq!(all_matched.status.code(), Some(0));
}

#[test]
fn directory_walk_is_sorted_recursive_deterministic_and_argument_order_independent() {
    let dir = Scratch::new("walk");
    fs::create_dir_all(dir.path("sub")).unwrap();
    for (name, body) in [
        ("b.csv", "x,y\n1,2\n"),
        (".hidden.csv", "x,y\n1,2\n"),
        ("sub/a.csv", "x,y\n3,4\n"),
    ] {
        fs::write(dir.path(name), body).unwrap();
    }
    let args = ["match", s(&dir.0), "--fp", "csv.v0"];
    let one = fingerprint(&args);
    let two = fingerprint(&args);
    assert_eq!(one.stdout, two.stdout, "byte-identical across runs");
    let relative: Vec<String> = lines(&one)
        .iter()
        .map(|r| r["relative_path"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(relative, [".hidden.csv", "b.csv", "sub/a.csv"]);

    // Two files named in the opposite order, plus the directory that contains them: no
    // duplicates, same order.
    let reversed = fingerprint(&[
        "match",
        s(&dir.path("sub/a.csv")),
        s(&dir.path("b.csv")),
        "--fp",
        "csv.v0",
    ]);
    let paths: Vec<String> = lines(&reversed)
        .iter()
        .map(|r| r["relative_path"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(paths, ["b.csv", "a.csv"]);
}

#[test]
fn an_unreadable_file_is_a_skipped_record_and_a_partial_outcome() {
    use std::os::unix::fs::PermissionsExt;
    let dir = Scratch::new("skip");
    let locked = dir.path("locked.csv");
    fs::write(&locked, "x\n1\n").unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    let out = fingerprint(&["match", s(&locked), "--fp", "csv.v0"]);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o644)).unwrap();
    let records = lines(&out);
    // Running as root can read a 0o000 file; only assert the skip when it really was unreadable.
    if records[0]["_skipped"] == json!(true) {
        assert_eq!(records[0]["bytes_hash"], Value::Null);
        assert_eq!(records[0]["_warnings"][0]["tool"], "fingerprint");
        assert_eq!(out.status.code(), Some(1));
    }
}

// ---- views -----------------------------------------------------------------------

#[test]
fn brief_view_is_one_bounded_object_with_exact_counts() {
    let dir = Scratch::new("brief");
    for i in 0..120 {
        if i % 3 == 0 {
            // A workbook: csv.v0 does not apply, so it is unmatched.
            fs::copy(
                fixture("sample.xlsx"),
                dir.path(&format!("book{i:03}.xlsx")),
            )
            .unwrap();
        } else {
            fs::write(
                dir.path(&format!("data{i:03}.csv")),
                format!("x,y\n{i},2\n"),
            )
            .unwrap();
        }
    }
    let out = fingerprint(&["match", s(&dir.0), "--fp", "csv.v0", "--view", "brief"]);
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert_eq!(text.lines().count(), 1, "one object");
    assert!(
        text.len() < 1500,
        "bounded (~500 tokens): {} bytes",
        text.len()
    );
    let brief: Value = serde_json::from_str(text.trim()).unwrap();
    assert_eq!(brief["version"], "fingerprint.brief.v0");
    assert_eq!(brief["files"], 120);
    assert_eq!(brief["matched"], 80);
    assert_eq!(brief["unmatched"], 40);
    assert_eq!(brief["by_family"]["csv.v0"], 80);
    assert_eq!(out.status.code(), Some(1), "exit stays the run outcome");

    // refs.output_hash is the hash of the full view of the same run.
    let full = fingerprint(&["match", s(&dir.0), "--fp", "csv.v0"]);
    assert_eq!(
        brief["refs"]["output_hash"],
        json!(format!("blake3:{}", blake3::hash(&full.stdout).to_hex()))
    );
}

#[test]
fn delta_view_reports_newly_matched_and_regressed_files() {
    let dir = Scratch::new("delta");
    // book.xlsx starts valid (matches xlsx.v0); other.xlsx starts corrupt (does not).
    fs::copy(fixture("sample.xlsx"), dir.path("book.xlsx")).unwrap();
    fs::copy(fixture("corrupt.xlsx"), dir.path("other.xlsx")).unwrap();
    fs::write(dir.path("stable.csv"), "x\n1\n").unwrap();
    let args = ["match", s(&dir.0), "--fp", "xlsx.v0", "--fp", "csv.v0"];
    let first = fingerprint(&args);
    let previous = dir
        .path("../")
        .join(format!("fp-match-delta-prev-{}.jsonl", std::process::id()));
    fs::write(&previous, &first.stdout).unwrap();

    // Iterate: book.xlsx breaks (regression), other.xlsx is fixed (newly matched), a new file
    // appears.
    fs::copy(fixture("corrupt.xlsx"), dir.path("book.xlsx")).unwrap();
    fs::copy(fixture("sample.xlsx"), dir.path("other.xlsx")).unwrap();
    fs::write(dir.path("added.csv"), "x\n1\n").unwrap();

    let out = fingerprint(&[
        "match",
        s(&dir.0),
        "--fp",
        "xlsx.v0",
        "--fp",
        "csv.v0",
        "--view",
        "delta",
        "--since",
        s(&previous),
    ]);
    let _ = fs::remove_file(&previous);
    let delta: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(delta["version"], "fingerprint.delta.v0");
    assert_eq!(delta["regressed"]["count"], 1, "{delta}");
    assert!(
        delta["regressed"]["paths"][0]
            .as_str()
            .unwrap()
            .ends_with("book.xlsx")
    );
    assert_eq!(delta["newly_matched"]["count"], 1, "{delta}");
    assert!(
        delta["newly_matched"]["paths"][0]
            .as_str()
            .unwrap()
            .ends_with("other.xlsx")
    );
    assert_eq!(delta["new_files"]["count"], 1);
    assert_eq!(delta["removed_files"]["count"], 0);
    assert_eq!(delta["content_changed"]["count"], 2);
}

// ---- refusals and compatibility ------------------------------------------------------

#[test]
fn bad_invocations_exit_2_with_a_message() {
    let csv = fixture("simple.csv");
    for args in [
        vec!["match", s(&csv), "--fp", "csv.v0", "--view", "delta"], // delta needs --since
        vec!["match", s(&csv), "--fp", "csv.v0", "--since", "x.jsonl"], // since needs delta
        vec!["match", "/no/such/path-fp-test", "--fp", "csv.v0"],
        vec!["match", s(&csv)],          // --fp is required
        vec!["match", "--fp", "csv.v0"], // PATH is required
    ] {
        let out = fingerprint(&args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(!out.stderr.is_empty(), "{args:?}");
    }
    // Unknown fingerprint id refuses like the piped mode.
    let out = fingerprint(&["match", s(&csv), "--fp", "nope.v9"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn delta_rejects_a_since_file_that_is_not_full_view_output() {
    let dir = Scratch::new("badsince");
    let bogus = dir.path("bogus.jsonl");
    fs::write(&bogus, "{\"hello\":1}\n").unwrap();
    let out = fingerprint(&[
        "match",
        s(&fixture("simple.csv")),
        "--fp",
        "csv.v0",
        "--view",
        "delta",
        "--since",
        s(&bogus),
    ]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--view full"));
}
