use std::path::Path;

use serde::Serialize;

use crate::cli::args::{DoctorAction, DoctorArgs};

const DOCTOR_SCHEMA_VERSION: &str = "fingerprint.doctor.v1";
const DOCTOR_CONTRACT_VERSION: &str = "cmdrvl.read_only_doctor.v1";

pub fn run(args: &DoctorArgs) -> Result<u8, Box<dyn std::error::Error>> {
    if args.robot_triage {
        return robot_triage();
    }

    match &args.action {
        Some(DoctorAction::Health(health_args)) => health(health_args.json),
        Some(DoctorAction::Capabilities(capabilities_args)) => capabilities(capabilities_args.json),
        Some(DoctorAction::RobotDocs) => robot_docs(),
        None => human_triage(),
    }
}

fn health(json: bool) -> Result<u8, Box<dyn std::error::Error>> {
    let report = build_report();
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "fingerprint doctor {}: {} checks passed, {} findings",
            report.summary.status,
            report.summary.checks_passed,
            report.findings.len()
        );
    }
    Ok(report.exit_code)
}

fn human_triage() -> Result<u8, Box<dyn std::error::Error>> {
    let report = build_report();
    println!("FINGERPRINT DOCTOR");
    println!();
    println!("Status: {}", report.summary.status);
    println!("Checks passed: {}", report.summary.checks_passed);
    println!("Findings: {}", report.findings.len());
    if !report.findings.is_empty() {
        println!();
        for finding in &report.findings {
            println!("- {}: {}", finding.id, finding.summary);
            println!("  next: {}", finding.next_step);
        }
    }
    println!();
    println!("Next: fingerprint doctor capabilities --json");
    Ok(report.exit_code)
}

fn capabilities(json: bool) -> Result<u8, Box<dyn std::error::Error>> {
    let payload = build_capabilities();
    if json {
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!("fingerprint doctor capabilities");
        println!("schema_version: {}", payload.schema_version);
        println!("contract_version: {}", payload.contract_version);
        println!("read_only: {}", payload.read_only);
        println!("json: fingerprint doctor capabilities --json");
    }
    Ok(0)
}

fn robot_docs() -> Result<u8, Box<dyn std::error::Error>> {
    println!("# fingerprint doctor robot-docs");
    println!();
    println!(
        "fingerprint doctor is read-only in this release. It never repairs files, deletes files, runs network probes, writes witness ledgers, mutates fingerprint definitions, or writes run artifacts."
    );
    println!();
    println!("Commands:");
    println!("- fingerprint doctor health");
    println!("- fingerprint doctor health --json");
    println!("- fingerprint doctor capabilities --json");
    println!("- fingerprint doctor robot-docs");
    println!("- fingerprint doctor --robot-triage");
    println!();
    println!("Exit codes:");
    println!("- 0: healthy");
    println!("- 1: findings present");
    println!("- 2: command-line usage error from clap or doctor runtime error");
    println!();
    println!(
        "Repair policy: no doctor --fix surface exists yet. File follow-up work with detector, backup, inverse, fixture, and undo coverage before adding one."
    );
    Ok(0)
}

fn robot_triage() -> Result<u8, Box<dyn std::error::Error>> {
    let report = build_report();
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(report.exit_code)
}

fn build_report() -> DoctorReport {
    let capabilities = build_capabilities();
    let mut checks = vec![
        Check {
            id: "binary-metadata",
            status: CheckStatus::Pass,
            summary: format!("fingerprint {} is runnable", env!("CARGO_PKG_VERSION")),
        },
        Check {
            id: "operator-manifest",
            status: operator_manifest_status(),
            summary: "compiled operator manifest is readable".to_string(),
        },
        Check {
            id: "definition-registry-contract",
            status: CheckStatus::Pass,
            summary: "doctor commands do not load or mutate fingerprint definitions".to_string(),
        },
        Check {
            id: "witness-ledger-contract",
            status: CheckStatus::Pass,
            summary: "doctor commands do not append witness records".to_string(),
        },
    ];

    if let Some(check) = source_checkout_gitignore_check() {
        checks.push(check);
    }

    let findings: Vec<Finding> = checks
        .iter()
        .filter(|check| check.status == CheckStatus::Fail)
        .map(|check| Finding {
            id: check.id,
            severity: "warning",
            summary: check.summary.clone(),
            next_step: match check.id {
                "source-gitignore-doctor" => "add .doctor/ to .gitignore",
                "operator-manifest" => "rebuild fingerprint with a valid operator.json",
                _ => "inspect fingerprint doctor capabilities --json",
            },
        })
        .collect();

    let status = if findings.is_empty() {
        "healthy"
    } else {
        "findings_present"
    };
    let exit_code = if findings.is_empty() { 0 } else { 1 };
    let checks_passed = checks
        .iter()
        .filter(|check| check.status == CheckStatus::Pass)
        .count();

    DoctorReport {
        schema_version: DOCTOR_SCHEMA_VERSION,
        tool: "fingerprint",
        version: env!("CARGO_PKG_VERSION"),
        contract_version: DOCTOR_CONTRACT_VERSION,
        read_only: true,
        summary: Summary {
            status,
            checks_passed,
            checks_total: checks.len(),
            findings_count: findings.len(),
        },
        findings,
        checks,
        actions_planned: Vec::new(),
        recommended_command: if status == "healthy" {
            "fingerprint doctor health"
        } else {
            "fingerprint doctor --robot-triage"
        },
        capabilities_url: "command:fingerprint doctor capabilities --json",
        capabilities,
        exit_code,
    }
}

fn operator_manifest_status() -> CheckStatus {
    match serde_json::from_str::<serde_json::Value>(crate::OPERATOR_JSON) {
        Ok(value) if value.get("name").and_then(|name| name.as_str()) == Some("fingerprint") => {
            CheckStatus::Pass
        }
        _ => CheckStatus::Fail,
    }
}

fn source_checkout_gitignore_check() -> Option<Check> {
    let cwd = std::env::current_dir().ok()?;
    if !looks_like_fingerprint_source_checkout(&cwd) {
        return None;
    }

    let gitignore = cwd.join(".gitignore");
    let status = match std::fs::read_to_string(&gitignore) {
        Ok(contents) if contents.lines().any(|line| line.trim() == ".doctor/") => CheckStatus::Pass,
        _ => CheckStatus::Fail,
    };

    Some(Check {
        id: "source-gitignore-doctor",
        status,
        summary: ".doctor/ is ignored in this fingerprint checkout".to_string(),
    })
}

fn looks_like_fingerprint_source_checkout(path: &Path) -> bool {
    let cargo_toml = path.join("Cargo.toml");
    let operator_json = path.join("operator.json");
    match std::fs::read_to_string(cargo_toml) {
        Ok(contents) => {
            contents
                .lines()
                .any(|line| line.trim() == r#"name = "fingerprint""#)
                && operator_json.exists()
        }
        Err(_) => false,
    }
}

fn build_capabilities() -> DoctorCapabilities {
    DoctorCapabilities {
        schema_version: "fingerprint.doctor.capabilities.v1",
        tool: "fingerprint",
        version: env!("CARGO_PKG_VERSION"),
        contract_version: DOCTOR_CONTRACT_VERSION,
        read_only: true,
        online_default: false,
        commands: vec![
            CommandCapability {
                command: "fingerprint doctor health",
                output: "one-line text",
                mutates: false,
            },
            CommandCapability {
                command: "fingerprint doctor health --json",
                output: "json",
                mutates: false,
            },
            CommandCapability {
                command: "fingerprint doctor capabilities --json",
                output: "json",
                mutates: false,
            },
            CommandCapability {
                command: "fingerprint doctor robot-docs",
                output: "markdown",
                mutates: false,
            },
            CommandCapability {
                command: "fingerprint doctor --robot-triage",
                output: "json",
                mutates: false,
            },
        ],
        detectors: vec![
            DetectorCapability {
                id: "binary-metadata",
                description: "Confirms the fingerprint binary can report its compiled version.",
                online_required: false,
            },
            DetectorCapability {
                id: "operator-manifest",
                description: "Confirms the compiled operator manifest is present and names fingerprint.",
                online_required: false,
            },
            DetectorCapability {
                id: "source-gitignore-doctor",
                description: "When run from the fingerprint source checkout, confirms .doctor/ is ignored.",
                online_required: false,
            },
            DetectorCapability {
                id: "definition-registry-contract",
                description: "Confirms this doctor release does not load or mutate fingerprint definitions.",
                online_required: false,
            },
            DetectorCapability {
                id: "witness-ledger-contract",
                description: "Confirms this doctor release does not append witness records.",
                online_required: false,
            },
        ],
        fixers: Vec::new(),
        exit_codes: vec![
            ExitCodeCapability {
                code: 0,
                meaning: "healthy or display command succeeded",
            },
            ExitCodeCapability {
                code: 1,
                meaning: "doctor findings present or fingerprint partial outcome",
            },
            ExitCodeCapability {
                code: 2,
                meaning: "command-line usage error, doctor runtime error, or fingerprint refusal",
            },
        ],
        env_vars: vec![
            EnvVarCapability {
                name: "FINGERPRINT_DEFINITIONS",
                description: "Overrides installed definition discovery for run mode; doctor commands do not read or write it.",
            },
            EnvVarCapability {
                name: "FINGERPRINT_TRUST",
                description: "Overrides trust policy for installed definitions; doctor commands do not read or write it.",
            },
            EnvVarCapability {
                name: "EPISTEMIC_WITNESS",
                description: "Overrides the witness ledger path for run/infer commands; doctor commands do not write it.",
            },
            EnvVarCapability {
                name: "HOME",
                description: "Used by run-mode registry and witness fallbacks; doctor commands do not write home-scoped files.",
            },
        ],
        data_paths: vec![
            DataPathCapability {
                path: ".doctor/",
                purpose: "reserved and gitignored for future doctor run artifacts",
                mutates_in_this_release: false,
            },
            DataPathCapability {
                path: "$FINGERPRINT_DEFINITIONS",
                purpose: "run-mode installed definition directory; not touched by doctor commands",
                mutates_in_this_release: false,
            },
            DataPathCapability {
                path: "~/.cmdrvl/config/fingerprint/trust.yaml",
                purpose: "run-mode installed definition trust config; not touched by doctor commands",
                mutates_in_this_release: false,
            },
            DataPathCapability {
                path: "~/.cmdrvl/config/fingerprint/definitions/",
                purpose: "run-mode installed definition directory; not touched by doctor commands",
                mutates_in_this_release: false,
            },
            DataPathCapability {
                path: "~/.cmdrvl/state/witness/witness.jsonl",
                purpose: "run/infer witness ledger; not touched by doctor commands",
                mutates_in_this_release: false,
            },
        ],
    }
}

#[derive(Debug, Serialize)]
struct DoctorReport {
    schema_version: &'static str,
    tool: &'static str,
    version: &'static str,
    contract_version: &'static str,
    read_only: bool,
    summary: Summary,
    findings: Vec<Finding>,
    checks: Vec<Check>,
    actions_planned: Vec<String>,
    recommended_command: &'static str,
    capabilities_url: &'static str,
    capabilities: DoctorCapabilities,
    #[serde(skip)]
    exit_code: u8,
}

#[derive(Debug, Serialize)]
struct Summary {
    status: &'static str,
    checks_passed: usize,
    checks_total: usize,
    findings_count: usize,
}

#[derive(Debug, Serialize)]
struct Finding {
    id: &'static str,
    severity: &'static str,
    summary: String,
    next_step: &'static str,
}

#[derive(Debug, Serialize)]
struct Check {
    id: &'static str,
    status: CheckStatus,
    summary: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum CheckStatus {
    Pass,
    Fail,
}

#[derive(Debug, Serialize)]
struct DoctorCapabilities {
    schema_version: &'static str,
    tool: &'static str,
    version: &'static str,
    contract_version: &'static str,
    read_only: bool,
    online_default: bool,
    commands: Vec<CommandCapability>,
    detectors: Vec<DetectorCapability>,
    fixers: Vec<String>,
    exit_codes: Vec<ExitCodeCapability>,
    env_vars: Vec<EnvVarCapability>,
    data_paths: Vec<DataPathCapability>,
}

#[derive(Debug, Serialize)]
struct CommandCapability {
    command: &'static str,
    output: &'static str,
    mutates: bool,
}

#[derive(Debug, Serialize)]
struct DetectorCapability {
    id: &'static str,
    description: &'static str,
    online_required: bool,
}

#[derive(Debug, Serialize)]
struct ExitCodeCapability {
    code: u8,
    meaning: &'static str,
}

#[derive(Debug, Serialize)]
struct EnvVarCapability {
    name: &'static str,
    description: &'static str,
}

#[derive(Debug, Serialize)]
struct DataPathCapability {
    path: &'static str,
    purpose: &'static str,
    mutates_in_this_release: bool,
}
