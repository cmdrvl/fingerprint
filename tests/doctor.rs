use serde_json::Value;
use std::path::Path;
use std::process::{Command, Output};

fn run_fingerprint(args: impl IntoIterator<Item = impl AsRef<std::ffi::OsStr>>) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fingerprint"))
        .args(args)
        .output()
        .expect("run fingerprint")
}

fn run_with_witness(
    args: impl IntoIterator<Item = impl AsRef<std::ffi::OsStr>>,
    witness_path: &Path,
) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fingerprint"))
        .args(args)
        .env("EPISTEMIC_WITNESS", witness_path)
        .output()
        .expect("run fingerprint with witness")
}

fn doctor_report(args: &[&str], mode: &str) -> Value {
    let output = run_fingerprint(args);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).expect("doctor stdout is JSON");
    assert_eq!(report["schema_version"], "cmdrvl.doctor.v1");
    assert_eq!(report["mode"], mode);
    assert_eq!(report["tool"], "fingerprint");
    assert_eq!(report["tool_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(report["healthy"], true);
    assert!(report["findings"].is_array());
    report
}

#[test]
fn doctor_health_uses_family_schema() {
    doctor_report(&["doctor", "--json"], "health");
}

#[test]
fn legacy_doctor_health_alias_remains_supported() {
    doctor_report(&["doctor", "health", "--json"], "health");
}

#[test]
fn doctor_capabilities_embeds_the_exact_operator_contract() {
    let doctor = doctor_report(&["doctor", "capabilities", "--json"], "capabilities");
    let capabilities = run_fingerprint(["capabilities", "--json"]);
    let contract: Value = serde_json::from_slice(&capabilities.stdout).unwrap();
    assert_eq!(doctor["contract"], contract);
    assert_eq!(contract["schema_version"], "operator.v1");
}

#[test]
fn doctor_triage_has_next_actions_and_legacy_alias_works() {
    let triage = doctor_report(&["doctor", "triage", "--json"], "triage");
    assert!(triage["next_actions"].as_array().is_some_and(|actions| {
        actions
            .iter()
            .any(|action| action == "fingerprint capabilities --json")
    }));
    let legacy = doctor_report(&["doctor", "--robot-triage"], "triage");
    assert_eq!(legacy["next_actions"], triage["next_actions"]);
}

#[test]
fn top_level_capabilities_matches_describe() {
    let describe = run_fingerprint(["--describe"]);
    let capabilities = run_fingerprint(["capabilities", "--json"]);
    let described: Value = serde_json::from_slice(&describe.stdout).unwrap();
    let advertised: Value = serde_json::from_slice(&capabilities.stdout).unwrap();
    assert_eq!(described, advertised);
}

#[test]
fn doctor_is_read_only_and_does_not_write_witness() {
    let dir = tempfile::tempdir().unwrap();
    let witness = dir.path().join("witness.jsonl");
    let output = run_with_witness(["doctor", "--json"], &witness);
    assert_eq!(output.status.code(), Some(0));
    assert!(!witness.exists());
}

#[test]
fn help_routes_exit_success() {
    for args in [
        ["--help"].as_slice(),
        ["doctor", "--help"].as_slice(),
        ["doctor", "health", "--help"].as_slice(),
        ["doctor", "capabilities", "--help"].as_slice(),
        ["capabilities", "--help"].as_slice(),
        ["robot-docs", "--help"].as_slice(),
    ] {
        let output = run_fingerprint(args);
        assert_eq!(output.status.code(), Some(0), "help route failed: {args:?}");
        assert!(output.stderr.is_empty(), "help route wrote stderr: {args:?}");
        assert!(!output.stdout.is_empty(), "help route was empty: {args:?}");
    }
}

#[test]
fn doctor_health_is_read_only_and_successful() {
    let output = run_fingerprint(["doctor", "health"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    assert!(String::from_utf8_lossy(&output.stdout).contains("fingerprint doctor healthy"));
}

#[test]
fn no_color_and_non_tty_help_are_ansi_free() {
    for no_color in [true, false] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_fingerprint"));
        command.arg("--help");
        if no_color {
            command.env("NO_COLOR", "1").env("TERM", "dumb");
        } else {
            command.env_remove("NO_COLOR").env_remove("TERM");
        }
        let output = command.output().unwrap();
        assert!(!output.stdout.contains(&0x1b), "help stdout contained ANSI");
        assert!(!output.stderr.contains(&0x1b), "help stderr contained ANSI");
    }
}

#[test]
fn typo_recovery_keeps_edit_distance_one_hints() {
    // These are Clap's built-in suggestions. A bare top-level word is the
    // positional INPUT, so fingerprint infr intentionally has no subcommand hint.
    for (args, expected) in [
        (["--jbos", "1"].as_slice(), "--jobs"),
        (["--jsn"].as_slice(), "--json"),
        (["doctor", "healt"].as_slice(), "health"),
    ] {
        let output = run_fingerprint(args);
        assert_eq!(output.status.code(), Some(2));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("similar"));
        assert!(
            stderr.contains(expected),
            "expected suggestion {expected:?} for {args:?}, got: {stderr}"
        );
    }
}

#[test]
fn doctor_fix_is_unavailable() {
    let output = run_fingerprint(["doctor", "--fix"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("fingerprint doctor --fix is unavailable")
    );
}
