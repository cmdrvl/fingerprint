#![forbid(unsafe_code)]
#![recursion_limit = "512"]

pub mod cli;
pub mod compile;
pub mod document;
pub mod dsl;
pub mod infer;
pub mod output;
pub mod pipeline;
pub mod progress;
pub mod refusal;
pub mod registry;
pub mod witness;

// Public re-exports for compiled fingerprint crates.
pub use document::Document;
pub use registry::{Fingerprint, FingerprintResult};

use clap::{Parser, error::ErrorKind};

struct DiagnoseModeGuard;

impl DiagnoseModeGuard {
    fn new(enabled: bool) -> Self {
        crate::dsl::assertions::set_diagnose_mode(enabled);
        Self
    }
}

impl Drop for DiagnoseModeGuard {
    fn drop(&mut self) {
        crate::dsl::assertions::set_diagnose_mode(false);
    }
}

/// Run the fingerprint CLI. Returns an exit code (0, 1, or 2).
pub fn run() -> u8 {
    use cli::{Cli, Command};

    if let Some(display_mode) = detect_display_mode(std::env::args_os()) {
        return handle_display_mode(display_mode);
    }

    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let kind = error.kind();
            if kind == ErrorKind::DisplayHelp || kind == ErrorKind::DisplayVersion {
                print!("{error}");
                return 0;
            }

            eprint!("{error}");
            return 2;
        }
    };

    // Handle flags that cause immediate exit
    if cli.describe {
        return handle_describe();
    }
    if cli.list {
        return handle_list();
    }
    if cli.schema {
        return handle_schema();
    }

    match cli.command {
        Some(Command::Compile {
            yaml,
            out,
            check,
            schema,
        }) => {
            if schema {
                println!("{}", compile::schema::dsl_json_schema());
                0
            } else if let Some(yaml_path) = yaml.as_deref() {
                match handle_compile_command(yaml_path, out.as_deref(), check) {
                    Ok(()) => 0, // Success
                    Err(refusal) => {
                        output_compile_command_refusal(&refusal);
                        2 // Refusal
                    }
                }
            } else {
                eprintln!("Error: compile requires a YAML path or --schema");
                2
            }
        }
        Some(Command::Witness { action }) => handle_witness_command(action),
        Some(Command::Infer {
            dir,
            format,
            id,
            min_confidence,
            no_extract,
            out,
        }) => handle_infer_command(
            &dir,
            &format,
            &id,
            min_confidence,
            !no_extract,
            out.as_deref(),
            !cli.no_witness,
        ),
        Some(Command::InferSchema {
            doc,
            text_path,
            fields,
            id,
            out,
        }) => handle_infer_schema_command(
            &doc,
            text_path.as_deref(),
            &fields,
            id.as_deref(),
            out.as_deref(),
            !cli.no_witness,
        ),
        None => {
            // Default run mode (fingerprint processing)
            handle_run_mode(cli)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DisplayMode {
    Version,
    Describe,
    Schema,
    List,
}

fn detect_display_mode<I, T>(args: I) -> Option<DisplayMode>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString>,
{
    let args = args
        .into_iter()
        .skip(1)
        .map(Into::into)
        .collect::<Vec<std::ffi::OsString>>();

    for arg in args {
        if is_subcommand_token(&arg) {
            return None;
        }

        if arg == "--version" || arg == "-V" {
            return Some(DisplayMode::Version);
        }
        if arg == "--describe" {
            return Some(DisplayMode::Describe);
        }
        if arg == "--schema" {
            return Some(DisplayMode::Schema);
        }
        if arg == "--list" {
            return Some(DisplayMode::List);
        }
    }

    None
}

fn is_subcommand_token(arg: &std::ffi::OsStr) -> bool {
    matches!(
        arg.to_str(),
        Some("compile" | "witness" | "infer" | "infer-schema")
    )
}

fn handle_display_mode(mode: DisplayMode) -> u8 {
    match mode {
        DisplayMode::Version => {
            println!("fingerprint {}", env!("CARGO_PKG_VERSION"));
            0
        }
        DisplayMode::Describe => handle_describe(),
        DisplayMode::Schema => handle_schema(),
        DisplayMode::List => handle_list(),
    }
}

fn serialize_records_to_jsonl_bytes(records: &[serde_json::Value]) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    output::jsonl::write_jsonl(&mut output, records)?;
    Ok(output)
}

fn serialize_refusal_envelope_bytes<T: serde::Serialize>(refusal: &T) -> Result<Vec<u8>, String> {
    let mut output = serde_json::to_vec(refusal)
        .map_err(|error| format!("failed to serialize refusal envelope: {error}"))?;
    output.push(b'\n');
    Ok(output)
}

fn write_stdout_bytes(bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;

    let mut stdout = std::io::stdout();
    stdout
        .write_all(bytes)
        .map_err(|error| format!("failed to write JSONL output: {error}"))?;
    stdout
        .flush()
        .map_err(|error| format!("failed to flush JSONL output: {error}"))?;
    Ok(())
}

fn current_binary_hash() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|path| std::fs::read(path).ok())
        .map(|bytes| format!("blake3:{}", blake3::hash(&bytes).to_hex()))
        .unwrap_or_else(|| "blake3:unknown".to_owned())
}

fn default_run_jobs() -> usize {
    std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(1)
}

fn normalize_run_jobs(jobs: Option<usize>) -> usize {
    jobs.unwrap_or_else(default_run_jobs).max(1)
}

fn describe_run_input(input_path: Option<&std::path::Path>) -> witness::record::WitnessInput {
    match input_path {
        Some(path) => {
            let (hash, bytes) = match std::fs::read(path) {
                Ok(contents) => (
                    Some(format!("blake3:{}", blake3::hash(&contents).to_hex())),
                    Some(u64::try_from(contents.len()).unwrap_or(u64::MAX)),
                ),
                Err(_) => (None, None),
            };

            witness::record::WitnessInput {
                path: path.display().to_string(),
                hash,
                bytes,
            }
        }
        None => witness::record::WitnessInput {
            path: "stdin".to_owned(),
            hash: None,
            bytes: None,
        },
    }
}

fn append_run_mode_witness(
    cli: &cli::Cli,
    normalized_jobs: usize,
    outcome: cli::exit::Outcome,
    output_bytes: &[u8],
) {
    use progress::reporter::report_warning;
    use witness::ledger::{append, ledger_path};
    use witness::record::WitnessRecord;

    if cli.no_witness {
        return;
    }

    let ledger_path = ledger_path();
    let witness_record = WitnessRecord::new(
        env!("CARGO_PKG_VERSION").to_owned(),
        current_binary_hash(),
        vec![describe_run_input(cli.input.as_deref())],
        serde_json::json!({
            "fingerprints": cli.fingerprints,
            "jobs": normalized_jobs,
            "input": cli.input.as_ref().map(|path| path.display().to_string())
        }),
        match outcome {
            cli::exit::Outcome::AllMatched => "ALL_MATCHED",
            cli::exit::Outcome::Partial => "PARTIAL",
            cli::exit::Outcome::Refusal => "REFUSAL",
        }
        .to_owned(),
        outcome.exit_code(),
        format!("blake3:{}", blake3::hash(output_bytes).to_hex()),
        chrono::Utc::now().to_rfc3339(),
    );

    match witness_record {
        Ok(record) => {
            if let Err(error) = append(&ledger_path, &record) {
                if cli.progress {
                    report_warning(
                        &ledger_path.display().to_string(),
                        &format!("witness append failed: {error}"),
                    );
                } else {
                    eprintln!("Warning: Failed to record witness: {}", error);
                }
            }
        }
        Err(error) => {
            if cli.progress {
                report_warning(
                    &ledger_path.display().to_string(),
                    &format!("failed to build witness record: {error}"),
                );
            } else {
                eprintln!("Warning: Failed to build witness record: {}", error);
            }
        }
    }
}

fn emit_run_mode_refusal(cli: &cli::Cli, refusal: &refusal::codes::RefusalEnvelope) -> u8 {
    let normalized_jobs = normalize_run_jobs(cli.jobs);
    let output_bytes = match serialize_refusal_envelope_bytes(refusal) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("Error serializing refusal output: {}", error);
            return 2;
        }
    };

    if let Err(error) = write_stdout_bytes(&output_bytes) {
        eprintln!("Error writing refusal output: {}", error);
        return 2;
    }

    append_run_mode_witness(
        cli,
        normalized_jobs,
        cli::exit::Outcome::Refusal,
        &output_bytes,
    );
    2
}

enum CompileCommandRefusal {
    Compile(refusal::codes::CompileRefusalEnvelope),
    Run(refusal::codes::RefusalEnvelope),
}

fn output_compile_command_refusal(refusal: &CompileCommandRefusal) {
    match refusal {
        CompileCommandRefusal::Compile(refusal) => output_refusal_envelope(refusal),
        CompileCommandRefusal::Run(refusal) => output_refusal_envelope(refusal),
    }
}

fn extract_compile_line_number(error: &str) -> u64 {
    let Some((_, suffix)) = error.rsplit_once(" line ") else {
        return 1;
    };
    let digits: String = suffix
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect();
    digits.parse::<u64>().unwrap_or(1)
}

fn extract_compile_missing_field(error: &str) -> Option<String> {
    let marker = "missing field `";
    let (_, suffix) = error.split_once(marker)?;
    let (field, _) = suffix.split_once('`')?;
    Some(field.to_owned())
}

fn compile_parse_refusal(error: String) -> refusal::codes::CompileRefusalEnvelope {
    use refusal::codes::{BadInputDetail, CompileRefusalCode, build_compile_envelope};

    let line = extract_compile_line_number(&error);
    if let Some(missing_field) = extract_compile_missing_field(&error) {
        return build_compile_envelope(
            CompileRefusalCode::MissingField,
            "Missing required field in fingerprint definition",
            BadInputDetail {
                line,
                error: Some(error),
                missing_field: Some(missing_field),
                version: None,
            },
            Some("Add the required field and rerun fingerprint compile".to_owned()),
        );
    }

    let code =
        if error.contains("no variant of enum Assertion") || error.contains("unknown variant") {
            CompileRefusalCode::UnknownAssertion
        } else {
            CompileRefusalCode::InvalidYaml
        };
    let message = match code {
        CompileRefusalCode::UnknownAssertion => "Unsupported assertion in fingerprint definition",
        CompileRefusalCode::InvalidYaml => "Failed to parse fingerprint definition",
        CompileRefusalCode::MissingField => unreachable!("handled above"),
    };
    let next_command = match code {
        CompileRefusalCode::UnknownAssertion => {
            Some("Check supported assertion types above".to_owned())
        }
        CompileRefusalCode::InvalidYaml => Some("Check YAML syntax and schema".to_owned()),
        CompileRefusalCode::MissingField => unreachable!("handled above"),
    };

    build_compile_envelope(
        code,
        message,
        BadInputDetail {
            line,
            error: Some(error),
            missing_field: None,
            version: None,
        },
        next_command,
    )
}

fn compile_validation_refusal(error: String) -> refusal::codes::CompileRefusalEnvelope {
    use refusal::codes::{BadInputDetail, CompileRefusalCode, build_compile_envelope};

    build_compile_envelope(
        CompileRefusalCode::InvalidYaml,
        "Fingerprint definition failed validation",
        BadInputDetail {
            line: 1,
            error: Some(error),
            missing_field: None,
            version: None,
        },
        Some("Fix the fingerprint definition and rerun fingerprint compile".to_owned()),
    )
}

/// Handle the compile subcommand.
#[allow(clippy::result_large_err)]
fn handle_compile_command(
    yaml_path: &std::path::Path,
    out_dir: Option<&std::path::Path>,
    check: bool,
) -> Result<(), CompileCommandRefusal> {
    use compile::crate_gen::generate_crate;
    use compile::validate::validate_definition;
    use dsl::parser::parse;
    use refusal::codes::{BadInputDetail, RefusalCode, RefusalDetail, build_envelope};

    let def = parse(yaml_path)
        .map_err(compile_parse_refusal)
        .map_err(CompileCommandRefusal::Compile)?;
    validate_definition(&def)
        .map_err(compile_validation_refusal)
        .map_err(CompileCommandRefusal::Compile)?;

    if check {
        println!("✓ {} is valid", yaml_path.display());
        return Ok(());
    }

    if let Some(out_dir) = out_dir {
        generate_crate(&def, out_dir)
            .map_err(|error| {
                build_envelope(
                    RefusalCode::BadInput,
                    "Failed to generate crate",
                    RefusalDetail::BadInput(BadInputDetail {
                        line: 0,
                        error: Some(error),
                        missing_field: None,
                        version: None,
                    }),
                    Some("Check output directory permissions".to_owned()),
                )
            })
            .map_err(CompileCommandRefusal::Run)?;

        println!("✓ Generated crate in {}", out_dir.display());
    } else {
        let rust_source = compile::codegen::generate_rust(&def)
            .map_err(|error| {
                build_envelope(
                    RefusalCode::BadInput,
                    "Failed to generate Rust source",
                    RefusalDetail::BadInput(BadInputDetail {
                        line: 0,
                        error: Some(error),
                        missing_field: None,
                        version: None,
                    }),
                    Some("Check fingerprint definition".to_owned()),
                )
            })
            .map_err(CompileCommandRefusal::Run)?;

        println!("{}", rust_source);
    }

    Ok(())
}

/// Handle --describe flag: print compiled operator.json and exit.
fn handle_describe() -> u8 {
    use std::io::Write;
    let mut stdout = std::io::stdout();
    if stdout
        .write_all(include_bytes!("../operator.json"))
        .is_err()
        || stdout.write_all(b"\n").is_err()
        || stdout.flush().is_err()
    {
        return 2;
    }
    0
}

/// Handle --schema flag: print JSON Schema and exit.
fn handle_schema() -> u8 {
    // TODO: Generate actual JSON schema for fingerprint input/output
    let schema = serde_json::json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "title": "Fingerprint JSONL Schema",
        "description": "Schema for fingerprint input and output JSONL records",
        "type": "object",
        "properties": {
            "version": { "type": "string" },
            "path": { "type": "string" },
            "bytes_hash": { "type": "string" },
            "fingerprint": { "type": ["object", "null"] },
            "_skipped": { "type": "boolean" },
            "_warnings": { "type": "array" }
        },
        "required": ["version", "path"]
    });

    if let Ok(json) = serde_json::to_string_pretty(&schema) {
        println!("{}", json);
        0
    } else {
        eprintln!("Error: Failed to serialize schema");
        2
    }
}

/// Handle --list flag: list available fingerprints and exit.
fn handle_list() -> u8 {
    match build_registry() {
        Ok(registry) => {
            let fingerprints = registry.list();
            for fp in fingerprints {
                println!("{} ({})", fp.id, fp.format);
            }
            0
        }
        Err(refusal) => {
            output_refusal_envelope(&refusal);
            2
        }
    }
}

/// Handle witness subcommands.
fn handle_witness_command(action: cli::WitnessAction) -> u8 {
    use cli::WitnessAction;
    use serde_json::json;
    use witness::{ledger::ledger_path, query};

    let ledger_path = ledger_path();

    match action {
        WitnessAction::Query { filters, json } => match query::query(&ledger_path, &filters) {
            Ok(records) => {
                if json {
                    match serde_json::to_string(&records) {
                        Ok(json_output) => println!("{}", json_output),
                        Err(error) => {
                            eprintln!("Error serializing witness records: {}", error);
                            return 2;
                        }
                    }
                } else {
                    for record in &records {
                        match serde_json::to_string(record) {
                            Ok(record_json) => println!("{}", record_json),
                            Err(error) => {
                                eprintln!("Error serializing witness record: {}", error);
                                return 2;
                            }
                        }
                    }
                }

                if records.is_empty() {
                    if !json {
                        eprintln!(
                            "{}",
                            if filters.is_active() {
                                "No matching witness records found"
                            } else {
                                "No witness records found"
                            }
                        );
                    }
                    1
                } else {
                    0
                }
            }
            Err(error) => {
                eprintln!("Error querying witness: {}", error);
                2
            }
        },
        WitnessAction::Last { filters, json } => match query::last(&ledger_path, &filters) {
            Ok(Some(record)) => {
                match serde_json::to_string(&record) {
                    Ok(record_json) => println!("{}", record_json),
                    Err(error) => {
                        eprintln!("Error serializing witness record: {}", error);
                        return 2;
                    }
                }
                0
            }
            Ok(None) => {
                if json {
                    println!("null");
                } else {
                    eprintln!(
                        "{}",
                        if filters.is_active() {
                            "No matching witness records found"
                        } else {
                            "No witness records found"
                        }
                    );
                }
                1
            }
            Err(error) => {
                eprintln!("Error querying witness: {}", error);
                2
            }
        },
        WitnessAction::Count { filters, json } => match query::count(&ledger_path, &filters) {
            Ok(count) => {
                if json {
                    println!("{}", json!({ "count": count }));
                } else {
                    println!("{}", count);
                }
                if count == 0 { 1 } else { 0 }
            }
            Err(error) => {
                eprintln!("Error querying witness: {}", error);
                2
            }
        },
    }
}

/// Handle default run mode (fingerprint processing).
fn handle_run_mode(cli: cli::Cli) -> u8 {
    use cli::exit::Outcome;
    use pipeline::enricher::enrich_record_with_fingerprints;
    use pipeline::parallel::process_parallel_for_each;
    use progress::reporter::{ProgressEvent, report_progress};
    use std::time::Instant;

    // Validate fingerprint IDs provided
    if cli.fingerprints.is_empty() {
        eprintln!("Error: At least one --fp fingerprint ID is required");
        return 2;
    }

    // Build registry
    let registry = match build_registry() {
        Ok(reg) => reg,
        Err(refusal) => return emit_run_mode_refusal(&cli, &refusal),
    };

    // Validate fingerprint IDs exist
    for fp_id in &cli.fingerprints {
        if registry.get(fp_id).is_none() {
            let available: Vec<String> = registry.list().iter().map(|fp| fp.id.clone()).collect();
            let refusal = build_unknown_fp_refusal(fp_id, available);
            return emit_run_mode_refusal(&cli, &refusal);
        }
    }
    if let Err(refusal) = validate_orphan_children(&registry, &cli.fingerprints) {
        return emit_run_mode_refusal(&cli, &refusal);
    }

    // Read input records
    let records = match read_input_records(&cli.input) {
        Ok(recs) => recs,
        Err(error) => return emit_run_mode_refusal(&cli, &build_bad_input_refusal(error)),
    };

    let _diagnose_guard = DiagnoseModeGuard::new(cli.diagnose);
    let normalized_jobs = normalize_run_jobs(cli.jobs);

    // Process records through enrichment pipeline
    let total_records = u64::try_from(records.len()).unwrap_or(u64::MAX);
    let started_at = Instant::now();
    let mut enriched_records = Vec::with_capacity(records.len());
    let mut outcome = Outcome::AllMatched;
    let mut processed_records = 0u64;

    process_parallel_for_each(
        records,
        normalized_jobs,
        |record| enrich_record_with_fingerprints(&record, &registry, &cli.fingerprints),
        |_index, enriched| {
            if record_requires_partial_outcome(&enriched) {
                outcome = Outcome::Partial;
            }

            enriched_records.push(enriched);
            processed_records = processed_records.saturating_add(1);

            if cli.progress {
                let percent = if total_records == 0 {
                    None
                } else {
                    Some((processed_records as f64 / total_records as f64) * 100.0)
                };
                let elapsed_ms =
                    u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX);
                report_progress(&ProgressEvent {
                    event_type: "progress".to_owned(),
                    tool: "fingerprint".to_owned(),
                    processed: processed_records,
                    total: Some(total_records),
                    percent,
                    elapsed_ms,
                });
            }
        },
    );

    let output_bytes = match serialize_records_to_jsonl_bytes(&enriched_records) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("Error serializing output: {}", error);
            return 2;
        }
    };

    if let Err(error) = write_stdout_bytes(&output_bytes) {
        eprintln!("Error writing output: {}", error);
        return 2;
    }

    append_run_mode_witness(&cli, normalized_jobs, outcome, &output_bytes);

    outcome.exit_code()
}

/// Trust configuration file format (YAML).
#[derive(serde::Deserialize, Default)]
struct TrustConfig {
    #[serde(default)]
    trust: Vec<String>,
}

/// Load trust allowlist from config files.
///
/// Searches (in order, merging entries):
/// 1. `~/.fingerprint/trust.yaml` (user config)
/// 2. `.fingerprint/trust.yaml` (project config)
///
/// Override location with `FINGERPRINT_TRUST` environment variable.
fn load_trust_config() -> Vec<String> {
    let mut entries = Vec::new();

    if let Ok(path) = std::env::var("FINGERPRINT_TRUST") {
        load_trust_file(&std::path::PathBuf::from(path), &mut entries);
        return entries;
    }

    // User config: ~/.fingerprint/trust.yaml
    let home_config = std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join(".fingerprint")
        .join("trust.yaml");
    load_trust_file(&home_config, &mut entries);

    // Project config: .fingerprint/trust.yaml
    let project_config = std::path::PathBuf::from(".fingerprint").join("trust.yaml");
    load_trust_file(&project_config, &mut entries);

    entries
}

fn load_trust_file(path: &std::path::Path, entries: &mut Vec<String>) {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return;
    };
    let Ok(config) = serde_yaml::from_str::<TrustConfig>(&contents) else {
        eprintln!(
            "Warning: failed to parse trust config '{}', skipping",
            path.display()
        );
        return;
    };
    entries.extend(config.trust);
}

/// Build the fingerprint registry with builtin fingerprints.
#[allow(clippy::result_large_err)]
fn build_registry() -> Result<registry::FingerprintRegistry, refusal::codes::RefusalEnvelope> {
    use refusal::codes::{RefusalCode, RefusalDetail, build_envelope};
    use registry::core::RegistryValidationError;
    use registry::{FingerprintRegistry, builtin::register_builtins};

    let mut registry = FingerprintRegistry::new();

    // Register builtin fingerprints
    let builtins = register_builtins();
    for builtin in builtins {
        let info = registry::FingerprintInfo {
            id: builtin.id().to_owned(),
            crate_name: "fingerprint-core".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            source: "builtin:core".to_owned(),
            format: builtin.format().to_owned(),
            parent: builtin.parent().map(ToOwned::to_owned),
        };
        registry.register_with_info(builtin, info);
    }

    // Discover and register installed fingerprint definitions
    for (installed, info) in registry::installed::discover_installed() {
        registry.register_with_info(installed, info);
    }

    // Validate registry (check for duplicates and trust policy)
    let allowlist = load_trust_config();
    registry
        .validate(&allowlist)
        .map_err(|validation_error| match validation_error {
            RegistryValidationError::DuplicateFpId {
                fingerprint_id,
                providers,
            } => build_envelope(
                RefusalCode::DuplicateFpId,
                "Duplicate fingerprint ID detected",
                RefusalDetail::DuplicateFpId(refusal::codes::DuplicateFpIdDetail {
                    fingerprint_id,
                    providers,
                }),
                Some("Remove conflicting fingerprint crates".to_owned()),
            ),
            RegistryValidationError::UntrustedFp {
                fingerprint_id,
                provider,
                policy,
            } => {
                let next_command = format!(
                    "Add to ~/.fingerprint/trust.yaml:\ntrust:\n  - \"{}\"",
                    provider
                );
                build_envelope(
                    RefusalCode::UntrustedFp,
                    "Untrusted fingerprint provider",
                    RefusalDetail::UntrustedFp(refusal::codes::UntrustedFpDetail {
                        fingerprint_id,
                        provider,
                        policy,
                    }),
                    Some(next_command),
                )
            }
        })?;

    Ok(registry)
}

/// Build a refusal envelope for unknown fingerprint ID.
fn build_unknown_fp_refusal(
    fp_id: &str,
    available: Vec<String>,
) -> refusal::codes::RefusalEnvelope {
    use refusal::codes::{RefusalCode, RefusalDetail, build_envelope};

    build_envelope(
        RefusalCode::UnknownFp,
        "Fingerprint ID not found",
        RefusalDetail::UnknownFp(refusal::codes::UnknownFpDetail {
            fingerprint_id: fp_id.to_owned(),
            available,
        }),
        Some("fingerprint --list".to_owned()),
    )
}

#[allow(clippy::result_large_err)]
fn validate_orphan_children(
    registry: &registry::FingerprintRegistry,
    requested_fingerprints: &[String],
) -> Result<(), refusal::codes::RefusalEnvelope> {
    use refusal::codes::{OrphanChildDetail, RefusalCode, RefusalDetail, build_envelope};
    use std::collections::BTreeSet;

    let loaded: BTreeSet<&str> = requested_fingerprints.iter().map(String::as_str).collect();

    for child_id in requested_fingerprints {
        let Some(parent_id) = registry
            .info_for(child_id)
            .and_then(|info| info.parent.as_deref())
        else {
            continue;
        };

        if loaded.contains(parent_id) {
            continue;
        }

        return Err(build_envelope(
            RefusalCode::OrphanChild,
            "Child fingerprint references unloaded parent",
            RefusalDetail::OrphanChild(OrphanChildDetail {
                child_id: child_id.clone(),
                parent_id: parent_id.to_owned(),
                loaded: requested_fingerprints.to_vec(),
            }),
            Some(format!("fingerprint --fp {parent_id} --fp {child_id}")),
        ));
    }

    Ok(())
}

fn record_requires_partial_outcome(record: &serde_json::Value) -> bool {
    if record
        .get("_skipped")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        return record
            .get("_warnings")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|warnings| {
                warnings.iter().any(|warning| {
                    warning
                        .get("tool")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|tool| tool == "fingerprint")
                })
            });
    }

    let Some(fingerprint) = record.get("fingerprint") else {
        return true;
    };
    if fingerprint.is_null()
        || !fingerprint
            .get("matched")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
    {
        return true;
    }

    fingerprint
        .get("children")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|children| {
            let matched_children = children
                .iter()
                .filter(|child| {
                    child
                        .get("matched")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false)
                })
                .count();
            matched_children != 1
        })
}

fn build_bad_input_refusal(error: impl Into<String>) -> refusal::codes::RefusalEnvelope {
    use refusal::codes::{BadInputDetail, RefusalCode, RefusalDetail, build_envelope};

    build_envelope(
        RefusalCode::BadInput,
        "Invalid input stream",
        RefusalDetail::BadInput(BadInputDetail {
            line: 0,
            error: Some(error.into()),
            missing_field: None,
            version: None,
        }),
        Some("Fix JSONL input and rerun fingerprint".to_owned()),
    )
}

/// Output a refusal envelope to stdout.
fn output_refusal_envelope<T: serde::Serialize>(refusal: &T) {
    if let Ok(json) = serde_json::to_string(refusal) {
        println!("{}", json);
    }
}

/// Read input records from file or stdin.
fn read_input_records(
    input_path: &Option<std::path::PathBuf>,
) -> Result<Vec<serde_json::Value>, String> {
    use pipeline::reader::read_records;
    use std::fs::File;
    use std::io::{self, BufReader};

    match input_path {
        Some(path) => {
            let file = File::open(path)
                .map_err(|e| format!("Failed to open input file '{}': {}", path.display(), e))?;
            let mut reader = BufReader::new(file);
            read_records(&mut reader).map_err(|e| format!("Failed to read input records: {}", e))
        }
        None => {
            let stdin = io::stdin();
            let mut reader = stdin.lock();
            read_records(&mut reader)
                .map_err(|e| format!("Failed to read input records from stdin: {}", e))
        }
    }
}

/// Handle the infer subcommand.
fn handle_infer_command(
    dir: &std::path::Path,
    format: &str,
    id: &str,
    min_confidence: f64,
    include_extract: bool,
    out_path: Option<&std::path::Path>,
    append_witness_record: bool,
) -> u8 {
    use crate::infer;
    use std::fs;
    use witness::ledger::{append, ledger_path};
    use witness::record::{WitnessInput, WitnessRecord};

    // Infer profile from directory
    let (profile, corpus_size) =
        match infer::infer_from_dir(dir, format, min_confidence, include_extract) {
            Ok(result) => result,
            Err(error) => {
                eprintln!("Error: {error}");
                return 2;
            }
        };

    // Emit YAML
    let yaml = match infer::emit_profile(id, &profile) {
        Ok(yaml) => yaml,
        Err(error) => {
            eprintln!("Error: failed to render inferred fingerprint: {error}");
            return 2;
        }
    };

    // Write output
    if let Some(path) = out_path {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
            && let Err(error) = fs::create_dir_all(parent)
        {
            eprintln!(
                "Error: failed to create output directory '{}': {error}",
                parent.display()
            );
            return 2;
        }
        if let Err(error) = fs::write(path, &yaml) {
            eprintln!(
                "Error: failed writing output file '{}': {error}",
                path.display()
            );
            return 2;
        }
    } else {
        print!("{yaml}");
    }

    if append_witness_record {
        let output_hash = format!("blake3:{}", blake3::hash(yaml.as_bytes()).to_hex());
        let witness = WitnessRecord::new(
            env!("CARGO_PKG_VERSION").to_owned(),
            "blake3:unknown".to_owned(),
            vec![WitnessInput {
                path: dir.display().to_string(),
                hash: None,
                bytes: None,
            }],
            serde_json::json!({
                "mode": "infer",
                "format": format,
                "id": id,
                "corpus_size": corpus_size,
                "min_confidence": min_confidence,
                "include_extract": include_extract
            }),
            "INFERRED".to_owned(),
            0,
            output_hash,
            chrono::Utc::now().to_rfc3339(),
        );

        match witness {
            Ok(record) => {
                let ledger = ledger_path();
                if let Err(error) = append(&ledger, &record) {
                    eprintln!("Warning: Failed to record witness: {error}");
                }
            }
            Err(error) => {
                eprintln!("Warning: Failed to build witness record: {error}");
            }
        }
    }

    0
}

/// Handle the infer-schema subcommand.
fn handle_infer_schema_command(
    doc_path: &std::path::Path,
    text_path: Option<&std::path::Path>,
    fields_path: &std::path::Path,
    id: Option<&str>,
    out_path: Option<&std::path::Path>,
    append_witness_record: bool,
) -> u8 {
    use crate::document::open_document_from_path_with_text_path;
    use crate::infer::schema;
    use std::fs;
    use witness::ledger::{append, ledger_path};
    use witness::record::{WitnessInput, WitnessRecord};

    let fields = match schema::parse_fields_file(fields_path) {
        Ok(fields) => fields,
        Err(error) => {
            eprintln!("Error: {error}");
            return 2;
        }
    };

    let document = match open_document_from_path_with_text_path(doc_path, text_path) {
        Ok(document) => document,
        Err(error) => {
            eprintln!(
                "Error: failed opening document '{}': {error}",
                doc_path.display()
            );
            return 2;
        }
    };

    let fingerprint_id = id
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| default_infer_schema_id(doc_path));

    let result = match schema::infer_schema(&document, &fields, &fingerprint_id) {
        Ok(result) => result,
        Err(error) => {
            eprintln!("Error: {error}");
            return 2;
        }
    };

    let yaml = match serde_yaml::to_string(&result.definition) {
        Ok(yaml) => yaml,
        Err(error) => {
            eprintln!("Error: failed rendering inferred schema YAML: {error}");
            return 2;
        }
    };

    if let Some(path) = out_path {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
            && let Err(error) = fs::create_dir_all(parent)
        {
            eprintln!(
                "Error: failed to create output directory '{}': {error}",
                parent.display()
            );
            return 2;
        }
        if let Err(error) = fs::write(path, &yaml) {
            eprintln!(
                "Error: failed writing output file '{}': {error}",
                path.display()
            );
            return 2;
        }
    } else {
        print!("{yaml}");
    }

    if append_witness_record {
        let output_hash = format!("blake3:{}", blake3::hash(yaml.as_bytes()).to_hex());
        let mut inputs = vec![WitnessInput {
            path: doc_path.display().to_string(),
            hash: None,
            bytes: None,
        }];
        if let Some(path) = text_path {
            inputs.push(WitnessInput {
                path: path.display().to_string(),
                hash: None,
                bytes: None,
            });
        }
        inputs.push(WitnessInput {
            path: fields_path.display().to_string(),
            hash: None,
            bytes: None,
        });
        let witness = WitnessRecord::new(
            env!("CARGO_PKG_VERSION").to_owned(),
            "blake3:unknown".to_owned(),
            inputs,
            serde_json::json!({
                "mode": "infer-schema",
                "id": fingerprint_id,
                "text_path": text_path.map(|path| path.display().to_string()),
                "located_fields": result.located_fields,
                "missing_fields": result.missing_fields,
            }),
            "INFERRED".to_owned(),
            if result.missing_fields.is_empty() {
                0
            } else {
                1
            },
            output_hash,
            chrono::Utc::now().to_rfc3339(),
        );

        match witness {
            Ok(record) => {
                let ledger = ledger_path();
                if let Err(error) = append(&ledger, &record) {
                    eprintln!("Warning: Failed to record witness: {error}");
                }
            }
            Err(error) => {
                eprintln!("Warning: Failed to build witness record: {error}");
            }
        }
    }

    if result.missing_fields.is_empty() {
        0
    } else {
        1
    }
}

fn default_infer_schema_id(doc_path: &std::path::Path) -> String {
    let stem = doc_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("inferred-schema");
    let mut normalized = String::new();
    let mut previous_dash = false;

    for character in stem.chars().map(|character| character.to_ascii_lowercase()) {
        if character.is_ascii_alphanumeric() {
            normalized.push(character);
            previous_dash = false;
        } else if !previous_dash {
            normalized.push('-');
            previous_dash = true;
        }
    }

    let normalized = normalized.trim_matches('-');
    if normalized.is_empty() {
        "inferred-schema.v1".to_owned()
    } else {
        format!("{normalized}.v1")
    }
}

#[cfg(test)]
mod tests {
    use super::{record_requires_partial_outcome, validate_orphan_children};
    use crate::document::Document;
    use crate::registry::{
        AssertionResult, Fingerprint, FingerprintInfo, FingerprintRegistry, FingerprintResult,
    };
    use serde_json::json;

    struct DummyFingerprint {
        id: &'static str,
        format: &'static str,
        parent: Option<&'static str>,
    }

    impl Fingerprint for DummyFingerprint {
        fn id(&self) -> &str {
            self.id
        }

        fn format(&self) -> &str {
            self.format
        }

        fn parent(&self) -> Option<&str> {
            self.parent
        }

        fn fingerprint(&self, _doc: &Document) -> FingerprintResult {
            FingerprintResult {
                matched: true,
                reason: None,
                assertions: vec![AssertionResult {
                    name: "dummy".to_owned(),
                    passed: true,
                    detail: None,
                    context: None,
                }],
                extracted: None,
                content_hash: None,
            }
        }
    }

    fn registry_with_parent_and_child() -> FingerprintRegistry {
        let mut registry = FingerprintRegistry::new();
        registry.register_with_info(
            Box::new(DummyFingerprint {
                id: "parent.v1",
                format: "text",
                parent: None,
            }),
            FingerprintInfo {
                id: "parent.v1".to_owned(),
                crate_name: "fingerprint-parent".to_owned(),
                version: "1.0.0".to_owned(),
                source: "dsl:parent".to_owned(),
                format: "text".to_owned(),
                parent: None,
            },
        );
        registry.register_with_info(
            Box::new(DummyFingerprint {
                id: "parent.v1/child-a.v1",
                format: "text",
                parent: Some("parent.v1"),
            }),
            FingerprintInfo {
                id: "parent.v1/child-a.v1".to_owned(),
                crate_name: "fingerprint-child".to_owned(),
                version: "1.0.0".to_owned(),
                source: "dsl:child".to_owned(),
                format: "text".to_owned(),
                parent: Some("parent.v1".to_owned()),
            },
        );
        registry
    }

    #[test]
    fn orphan_child_validation_rejects_child_without_loaded_parent() {
        let registry = registry_with_parent_and_child();
        let requested = vec!["parent.v1/child-a.v1".to_owned()];

        let refusal =
            validate_orphan_children(&registry, &requested).expect_err("orphan child refusal");
        assert_eq!(
            refusal.refusal.code,
            crate::refusal::codes::RefusalCode::OrphanChild
        );
        assert_eq!(refusal.refusal.detail["child_id"], "parent.v1/child-a.v1");
        assert_eq!(refusal.refusal.detail["parent_id"], "parent.v1");
        assert_eq!(refusal.refusal.detail["loaded"], json!(requested));
    }

    #[test]
    fn orphan_child_validation_accepts_loaded_parent_child_pair() {
        let registry = registry_with_parent_and_child();
        let requested = vec!["parent.v1".to_owned(), "parent.v1/child-a.v1".to_owned()];

        validate_orphan_children(&registry, &requested).expect("valid parent-child selection");
    }

    #[test]
    fn record_partial_outcome_accepts_single_matched_child_with_unmatched_siblings() {
        let record = json!({
            "fingerprint": {
                "matched": true,
                "children": [
                    { "fingerprint_id": "parent.v1/child-a.v1", "matched": true },
                    { "fingerprint_id": "parent.v1/child-b.v1", "matched": false }
                ]
            }
        });

        assert!(!record_requires_partial_outcome(&record));
    }

    #[test]
    fn record_partial_outcome_detects_zero_matched_children() {
        let record = json!({
            "fingerprint": {
                "matched": true,
                "children": [
                    { "fingerprint_id": "parent.v1/child-a.v1", "matched": false },
                    { "fingerprint_id": "parent.v1/child-b.v1", "matched": false }
                ]
            }
        });

        assert!(record_requires_partial_outcome(&record));
    }

    #[test]
    fn record_partial_outcome_detects_ambiguous_child_match() {
        let record = json!({
            "fingerprint": {
                "matched": true,
                "children": [
                    { "fingerprint_id": "parent.v1/child-a.v1", "matched": true },
                    { "fingerprint_id": "parent.v1/child-b.v1", "matched": true }
                ]
            }
        });

        assert!(record_requires_partial_outcome(&record));
    }
}
