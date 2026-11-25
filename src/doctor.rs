use serde_json::{Value, json};
use std::path::Path;

use crate::cli::args::{DoctorAction, DoctorArgs, RobotDocsAction};

const DOCTOR_SCHEMA_VERSION: &str = "cmdrvl.doctor.v1";
const OPERATOR_JSON: &str = crate::OPERATOR_JSON;

pub fn run(args: &DoctorArgs, json_output: bool) -> Result<u8, Box<dyn std::error::Error>> {
    if args.fix {
        return Ok(emit_fix_unavailable());
    }
    if args.robot_triage {
        return emit_robot_triage();
    }

    let json_output = json_output || args.json;
    match &args.action {
        Some(DoctorAction::Health(health_args)) => emit_health(health_args.json || json_output),
        Some(DoctorAction::Capabilities(capabilities_args)) => {
            emit_doctor_capabilities(capabilities_args.json || json_output)
        }
        Some(DoctorAction::Triage(triage_args)) => emit_triage(triage_args.json || json_output),
        Some(DoctorAction::RobotDocs) => emit_robot_docs(None),
        None => emit_health(json_output),
    }
}

/// The top-level capabilities document is the same contract emitted by
/// `--describe`.
pub fn emit_capabilities(json: bool) -> Result<u8, Box<dyn std::error::Error>> {
    if json {
        print_operator_json();
    } else {
        println!("fingerprint capabilities");
        println!("operator contract: fingerprint --describe");
        println!("doctor: fingerprint doctor --json");
        println!("triage: fingerprint doctor triage --json");
        println!("robot docs: fingerprint robot-docs guide");
    }
    Ok(0)
}

pub fn emit_robot_docs(action: Option<&RobotDocsAction>) -> Result<u8, Box<dyn std::error::Error>> {
    match action {
        Some(RobotDocsAction::Guide) | None => {}
    }
    println!("# fingerprint robot-docs guide");
    println!();
    println!("Use `fingerprint --robot-triage` for a single read-only triage report.");
    println!("Use `fingerprint capabilities --json` for the operator.v1 contract.");
    println!(
        "Use `fingerprint doctor --json` for health, `fingerprint doctor capabilities --json` for the embedded contract, and `fingerprint doctor triage --json` for next actions."
    );
    println!(
        "Legacy aliases remain supported: `fingerprint doctor health --json` and `fingerprint doctor --robot-triage`."
    );
    println!(
        "Canonical generated-definition output is `--out`; legacy `--output` remains an alias where supported."
    );
    println!(
        "Composition: vacuum --json <ROOT>... | hashbytes | fingerprint --fp <ID> | lock --dataset-id <DATASET> > dataset.lock.json"
    );
    println!("Seal: pack seal dataset.lock.json --out evidence/<DATASET>/");
    println!("NO_COLOR=1 and non-TTY output are ANSI-free.");
    println!(
        "Repair policy: `fingerprint doctor --fix` is unavailable and exits 2 without stdout."
    );
    Ok(0)
}

pub fn emit_robot_triage() -> Result<u8, Box<dyn std::error::Error>> {
    emit_triage(true)
}

pub fn emit_fix_unavailable() -> u8 {
    eprintln!("fingerprint doctor --fix is unavailable; doctor is read-only in this release.");
    eprintln!("Try --robot-triage: fingerprint --robot-triage");
    eprintln!("Try capabilities --json: fingerprint capabilities --json");
    eprintln!("Try doctor capabilities --json: fingerprint doctor capabilities --json");
    2
}

fn emit_health(json_output: bool) -> Result<u8, Box<dyn std::error::Error>> {
    let report = doctor_report("health");
    if json_output {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "fingerprint doctor {}",
            if report["healthy"].as_bool() == Some(true) {
                "healthy"
            } else {
                "unhealthy"
            }
        );
    }
    Ok(doctor_exit_code(&report))
}

fn emit_doctor_capabilities(json_output: bool) -> Result<u8, Box<dyn std::error::Error>> {
    let report = doctor_report("capabilities");
    if json_output {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("fingerprint doctor capabilities");
        println!("contract: fingerprint capabilities --json");
        println!("healthy: {}", report["healthy"]);
    }
    Ok(doctor_exit_code(&report))
}

fn emit_triage(json_output: bool) -> Result<u8, Box<dyn std::error::Error>> {
    let report = doctor_report("triage");
    if json_output {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "fingerprint doctor {}",
            if report["healthy"].as_bool() == Some(true) {
                "healthy"
            } else {
                "unhealthy"
            }
        );
        println!("next: fingerprint capabilities --json");
    }
    Ok(doctor_exit_code(&report))
}

fn doctor_report(mode: &str) -> Value {
    let findings = health_findings();
    let healthy = !findings
        .iter()
        .any(|finding| finding["severity"].as_str() == Some("error"));
    let mut report = json!({
        "schema_version": DOCTOR_SCHEMA_VERSION,
        "mode": mode,
        "tool": "fingerprint",
        "tool_version": env!("CARGO_PKG_VERSION"),
        "healthy": healthy,
        "findings": findings,
    });

    match mode {
        "health" => {}
        "capabilities" => report["contract"] = operator_contract(),
        "triage" => report["next_actions"] = json!(["fingerprint capabilities --json"]),
        _ => unreachable!("doctor mode is fixed by the caller"),
    }
    report
}

fn health_findings() -> Vec<Value> {
    let mut checks = vec![
        ("binary-metadata", true, "fingerprint binary is runnable"),
        (
            "operator-manifest",
            operator_manifest_is_valid(),
            "compiled operator manifest is valid and names fingerprint",
        ),
        (
            "definition-registry-contract",
            true,
            "doctor does not load or mutate fingerprint definitions",
        ),
        (
            "witness-ledger-contract",
            true,
            "doctor does not append witness records",
        ),
    ];
    // Reports only whether a usable key is configured: never its value or its length.
    let (redaction_ok, redaction_message) = match crate::redaction::RedactionKey::from_env() {
        Ok(Some(_)) => (true, "peek redaction key: configured (keyed markers)"),
        Ok(None) => (true, "peek redaction key: not configured (ordinal refs)"),
        Err(_) => (
            false,
            "peek redaction key: set but unusable, so peek refuses (E_REDACTION_KEY_INVALID)",
        ),
    };
    checks.push(("redaction-key", redaction_ok, redaction_message));
    if let Some(path_check) = source_checkout_gitignore_check() {
        checks.push(path_check);
    }

    checks
        .into_iter()
        .map(|(id, ok, message)| {
            let mut finding = json!({
                "id": format!("fingerprint.{id}"),
                "severity": if ok { "info" } else { "error" },
                "message": message,
            });
            if !ok {
                finding["fix"] = json!("inspect fingerprint doctor capabilities --json");
            }
            finding
        })
        .collect()
}

fn operator_contract() -> Value {
    serde_json::from_str(OPERATOR_JSON).expect("operator.json must be valid JSON")
}

fn operator_manifest_is_valid() -> bool {
    let Ok(value) = serde_json::from_str::<Value>(OPERATOR_JSON) else {
        return false;
    };
    let version = value
        .get("tool_version")
        .or_else(|| value.get("version"))
        .and_then(Value::as_str);
    value["schema_version"] == "operator.v1"
        && value["name"] == "fingerprint"
        && version == Some(env!("CARGO_PKG_VERSION"))
        && ["capabilities", "robot-docs", "doctor"]
            .iter()
            .all(|name| has_command(&value, name))
}

fn has_command(value: &Value, name: &str) -> bool {
    value
        .get("commands")
        .and_then(Value::as_array)
        .is_some_and(|commands| {
            commands
                .iter()
                .any(|command| command.get("name").and_then(Value::as_str) == Some(name))
        })
}

fn source_checkout_gitignore_check() -> Option<(&'static str, bool, &'static str)> {
    let cwd = std::env::current_dir().ok()?;
    if !looks_like_fingerprint_source_checkout(&cwd) {
        return None;
    }
    let gitignore = cwd.join(".gitignore");
    let ignored = std::fs::read_to_string(gitignore)
        .map(|contents| contents.lines().any(|line| line.trim() == ".doctor/"))
        .unwrap_or(false);
    Some((
        "source-gitignore-doctor",
        ignored,
        ".doctor/ is ignored in this checkout",
    ))
}

fn looks_like_fingerprint_source_checkout(path: &Path) -> bool {
    let cargo_toml = path.join("Cargo.toml");
    let operator_json = path.join("operator.json");
    std::fs::read_to_string(cargo_toml)
        .map(|contents| {
            contents
                .lines()
                .any(|line| line.trim() == r#"name = "fingerprint""#)
                && operator_json.exists()
        })
        .unwrap_or(false)
}

fn doctor_exit_code(report: &Value) -> u8 {
    if report["healthy"].as_bool() == Some(true) {
        0
    } else {
        1
    }
}

fn print_operator_json() {
    print!("{OPERATOR_JSON}");
    if !OPERATOR_JSON.ends_with('\n') {
        println!();
    }
}
