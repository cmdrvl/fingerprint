use std::path::Path;
use std::process::{Command, Output};

fn run_fingerprint(args: impl IntoIterator<Item = impl AsRef<std::ffi::OsStr>>) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fingerprint"))
        .args(args)
        .output()
        .expect("run fingerprint binary")
}

fn run_fingerprint_with_witness(
    args: impl IntoIterator<Item = impl AsRef<std::ffi::OsStr>>,
    witness_path: &Path,
) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fingerprint"))
        .args(args)
        .env("EPISTEMIC_WITNESS", witness_path)
        .output()
        .expect("run fingerprint binary with witness path")
}

#[test]
fn help_routes_exit_success() {
    for args in [
        &["--help"][..],
        &["doctor", "--help"][..],
        &["doctor", "health", "--help"][..],
        &["doctor", "capabilities", "--help"][..],
        &["capabilities", "--help"][..],
        &["robot-docs", "--help"][..],
    ] {
        let result = run_fingerprint(args);
        assert_eq!(result.status.code(), Some(0), "help route: {args:?}");
        assert!(
            result.stderr.is_empty(),
            "help should not write stderr: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(!result.stdout.is_empty(), "help should print stdout");
    }
}

#[test]
fn doctor_health_is_read_only_and_successful() {
    let result = run_fingerprint(["doctor", "health"]);

    assert_eq!(result.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&result.stdout).contains("fingerprint doctor healthy"));
    assert!(
        result.stderr.is_empty(),
        "doctor health should not write stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn doctor_health_json_is_read_only_and_successful() {
    let result = run_fingerprint(["doctor", "health", "--json"]);

    assert_eq!(result.status.code(), Some(0));
    assert!(
        result.stderr.is_empty(),
        "doctor health json should not write stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let value: serde_json::Value =
        serde_json::from_slice(&result.stdout).expect("health should be JSON");

    assert_eq!(value["schema_version"], "fingerprint.doctor.v1");
    assert_eq!(value["tool"], "fingerprint");
    assert_eq!(value["summary"]["status"], "healthy");
    assert_eq!(value["read_only"], true);
}

#[test]
fn doctor_capabilities_json_declares_read_only_contract() {
    let result = run_fingerprint(["doctor", "capabilities", "--json"]);

    assert_eq!(result.status.code(), Some(0));
    assert!(
        result.stderr.is_empty(),
        "doctor capabilities should not write stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let value: serde_json::Value =
        serde_json::from_slice(&result.stdout).expect("capabilities should be JSON");

    assert_eq!(
        value["schema_version"],
        "fingerprint.doctor.capabilities.v1"
    );
    assert_eq!(value["tool"], "fingerprint");
    assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(value["read_only"], true);
    assert_eq!(value["fix_mode"]["available"], false);
    assert_eq!(
        value["agent_surfaces"]["robot_triage"]["argv"][1],
        "--robot-triage"
    );
    assert_eq!(
        value["fixers"]
            .as_array()
            .expect("fixers should be an array")
            .len(),
        0
    );

    let commands = value["commands"]
        .as_array()
        .expect("commands should be an array");
    for expected in [
        "fingerprint --robot-triage",
        "fingerprint capabilities --json",
        "fingerprint robot-docs guide",
        "fingerprint doctor health",
        "fingerprint doctor health --json",
        "fingerprint doctor capabilities --json",
        "fingerprint doctor robot-docs",
        "fingerprint doctor --robot-triage",
        "fingerprint doctor --fix",
    ] {
        assert!(
            commands
                .iter()
                .any(|command| command["command"].as_str() == Some(expected)),
            "missing command capability {expected}"
        );
    }
}

#[test]
fn top_level_agent_surfaces_are_read_only_and_machine_discoverable() {
    let capabilities = run_fingerprint(["capabilities", "--json"]);

    assert_eq!(capabilities.status.code(), Some(0));
    assert!(
        capabilities.stderr.is_empty(),
        "top-level capabilities should not write stderr: {}",
        String::from_utf8_lossy(&capabilities.stderr)
    );
    let value: serde_json::Value =
        serde_json::from_slice(&capabilities.stdout).expect("capabilities should be JSON");

    assert_eq!(
        value["schema_version"],
        "fingerprint.doctor.capabilities.v1"
    );
    assert_eq!(
        value["agent_surfaces"]["capabilities"]["argv"][1],
        "capabilities"
    );
    assert_eq!(value["side_effects"]["reads_input_manifest"], false);
    assert_eq!(value["side_effects"]["writes_witness_ledger"], false);

    let triage = run_fingerprint(["--robot-triage"]);
    assert_eq!(triage.status.code(), Some(0));
    assert!(
        triage.stderr.is_empty(),
        "top-level robot triage should not write stderr: {}",
        String::from_utf8_lossy(&triage.stderr)
    );
    let triage_value: serde_json::Value =
        serde_json::from_slice(&triage.stdout).expect("triage should be JSON");
    assert_eq!(triage_value["schema_version"], "fingerprint.doctor.v1");
    assert_eq!(
        triage_value["capabilities"]["agent_surfaces"]["robot_docs"]["argv"][2],
        "guide"
    );

    let docs = run_fingerprint(["robot-docs", "guide"]);
    assert_eq!(docs.status.code(), Some(0));
    assert!(
        docs.stderr.is_empty(),
        "top-level robot docs should not write stderr: {}",
        String::from_utf8_lossy(&docs.stderr)
    );
    let stdout = String::from_utf8(docs.stdout).expect("robot docs utf8");
    assert!(stdout.contains("fingerprint robot-docs guide"));
    assert!(stdout.contains("fingerprint capabilities --json"));
}

#[test]
fn describe_includes_doctor_surface() {
    let result = run_fingerprint(["--describe"]);

    assert_eq!(result.status.code(), Some(0));
    assert!(
        result.stderr.is_empty(),
        "describe should not write stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let value: serde_json::Value =
        serde_json::from_slice(&result.stdout).expect("describe should be JSON");
    let subcommands = value["subcommands"]
        .as_array()
        .expect("subcommands should be an array");
    let doctor = subcommands
        .iter()
        .find(|command| command["name"].as_str() == Some("doctor"))
        .expect("operator.json should describe doctor");

    assert_eq!(doctor["current_runtime_behavior"]["read_only"], true);
    assert_eq!(
        doctor["current_runtime_behavior"]["fix_mode"],
        "not_available"
    );
    assert_eq!(doctor["current_runtime_behavior"]["writes_witness"], false);
    assert_eq!(
        doctor["current_runtime_behavior"]["writes_definitions"],
        false
    );
}

#[test]
fn doctor_robot_docs_names_agent_surface() {
    let result = run_fingerprint(["doctor", "robot-docs"]);

    assert_eq!(result.status.code(), Some(0));
    assert!(
        result.stderr.is_empty(),
        "robot-docs should not write stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let stdout = String::from_utf8(result.stdout).expect("robot docs utf8");
    assert!(stdout.contains("fingerprint --robot-triage"));
    assert!(stdout.contains("fingerprint capabilities --json"));
    assert!(stdout.contains("fingerprint robot-docs guide"));
    assert!(stdout.contains("fingerprint doctor health"));
    assert!(stdout.contains("fingerprint doctor health --json"));
    assert!(stdout.contains("fingerprint doctor capabilities --json"));
    assert!(stdout.contains("fingerprint doctor --fix is unavailable"));
}

#[test]
fn doctor_robot_triage_is_single_call_json() {
    let result = run_fingerprint(["doctor", "--robot-triage"]);

    assert_eq!(result.status.code(), Some(0));
    assert!(
        result.stderr.is_empty(),
        "robot triage should not write stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let value: serde_json::Value =
        serde_json::from_slice(&result.stdout).expect("robot triage should be JSON");

    assert_eq!(value["schema_version"], "fingerprint.doctor.v1");
    assert_eq!(value["summary"]["status"], "healthy");
    assert_eq!(value["read_only"], true);
    assert_eq!(
        value["actions_planned"]
            .as_array()
            .expect("actions array")
            .len(),
        0
    );
    assert_eq!(
        value["capabilities_url"],
        "command:fingerprint capabilities --json"
    );
}

#[test]
fn doctor_fix_surface_reports_unavailable_without_mutating() {
    let result = run_fingerprint(["doctor", "--fix"]);

    assert_eq!(result.status.code(), Some(2));
    assert!(
        result.stdout.is_empty(),
        "doctor --fix should not emit stdout"
    );
    let stderr = String::from_utf8(result.stderr).expect("stderr utf8");
    assert!(stderr.contains("fingerprint doctor --fix is unavailable"));
    assert!(stderr.contains("fingerprint --robot-triage"));
    assert!(stderr.contains("fingerprint capabilities --json"));
}

#[test]
fn doctor_does_not_write_witness_ledger() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let witness_path = dir.path().join("witness.jsonl");

    let result = run_fingerprint_with_witness(["doctor", "health"], &witness_path);

    assert_eq!(result.status.code(), Some(0));
    assert!(
        !witness_path.exists(),
        "doctor commands must not create witness ledger"
    );
}

#[test]
fn doctor_runtime_artifacts_are_gitignored() {
    let gitignore = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/.gitignore"))
        .expect(".gitignore should be readable");

    assert!(gitignore.lines().any(|line| line.trim() == ".doctor/"));
}
