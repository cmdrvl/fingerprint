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
    use clap::Parser;
    use cli::{Cli, Command};

    // Parse CLI args (handles --version and --help via clap, then exits)
    let cli = Cli::parse();

    // Handle flags that cause immediate exit
    if cli.describe {
        return handle_describe();
    }
    if cli.schema {
        return handle_schema();
    }
    if cli.list {
        return handle_list();
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
                        output_refusal_envelope(&refusal);
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
            fields,
            id,
            out,
        }) => handle_infer_schema_command(
            &doc,
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

/// Handle the compile subcommand.
#[allow(clippy::result_large_err)]
fn handle_compile_command(
    yaml_path: &std::path::Path,
    out_dir: Option<&std::path::Path>,
    check: bool,
) -> Result<(), refusal::codes::RefusalEnvelope> {
    use compile::crate_gen::generate_crate;
    use dsl::parser::parse;
    use refusal::codes::{BadInputDetail, RefusalCode, RefusalDetail, build_envelope};

    // Parse the DSL definition
    let def = parse(yaml_path).map_err(|error| {
        build_envelope(
            RefusalCode::BadInput,
            "Failed to parse fingerprint definition",
            RefusalDetail::BadInput(BadInputDetail {
                line: 1, // TODO: Extract line number from parse error
                error: Some(error),
                missing_field: None,
                version: None,
            }),
            Some("Check YAML syntax and schema".to_owned()),
        )
    })?;

    if check {
        // Check mode: just validate and exit
        println!("✓ {} is valid", yaml_path.display());
        return Ok(());
    }

    // Generate crate
    if let Some(out_dir) = out_dir {
        generate_crate(&def, out_dir).map_err(|error| {
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
        })?;

        println!("✓ Generated crate in {}", out_dir.display());
    } else {
        // Output to stdout (just the Rust source)
        let rust_source = compile::codegen::generate_rust(&def).map_err(|error| {
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
        })?;

        println!("{}", rust_source);
    }

    Ok(())
}

/// Handle --describe flag: print operator.json and exit.
fn handle_describe() -> u8 {
    let operator = serde_json::json!({
        "name": "fingerprint",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Determine whether an artifact matches a known template",
        "author": "CMD+RVL",
        "repository": "https://github.com/SaltIO/fingerprint",
        "pipeline_role": "enricher",
        "input_format": "JSONL",
        "output_format": "JSONL",
        "stdin_support": true,
        "file_support": true
    });

    if let Ok(json) = serde_json::to_string_pretty(&operator) {
        println!("{}", json);
        0
    } else {
        eprintln!("Error: Failed to serialize operator metadata");
        2
    }
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
    use witness::{ledger::ledger_path, query};

    let ledger_path = ledger_path();

    match action {
        WitnessAction::Query => match query::query(&ledger_path) {
            Ok(records) => {
                for record in records {
                    if let Ok(json) = serde_json::to_string(&record) {
                        println!("{}", json);
                    }
                }
                0
            }
            Err(error) => {
                eprintln!("Error querying witness: {}", error);
                2
            }
        },
        WitnessAction::Last => match query::last(&ledger_path) {
            Ok(Some(record)) => {
                if let Ok(json) = serde_json::to_string(&record) {
                    println!("{}", json);
                }
                0
            }
            Ok(None) => {
                eprintln!("No witness records found");
                1
            }
            Err(error) => {
                eprintln!("Error querying witness: {}", error);
                2
            }
        },
        WitnessAction::Count => match query::count(&ledger_path) {
            Ok(count) => {
                println!("{}", count);
                0
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
    use output::jsonl::write_jsonl;
    use pipeline::enricher::enrich_record_with_fingerprints;
    use witness::ledger::{append, ledger_path};
    use witness::record::{WitnessInput, WitnessRecord};

    // Validate fingerprint IDs provided
    if cli.fingerprints.is_empty() {
        eprintln!("Error: At least one --fp fingerprint ID is required");
        return 2;
    }

    // Build registry
    let registry = match build_registry() {
        Ok(reg) => reg,
        Err(refusal) => {
            output_refusal_envelope(&refusal);
            return 2;
        }
    };

    // Validate fingerprint IDs exist
    for fp_id in &cli.fingerprints {
        if registry.get(fp_id).is_none() {
            let available: Vec<String> = registry.list().iter().map(|fp| fp.id.clone()).collect();
            let refusal = build_unknown_fp_refusal(fp_id, available);
            output_refusal_envelope(&refusal);
            return 2;
        }
    }
    if let Err(refusal) = validate_orphan_children(&registry, &cli.fingerprints) {
        output_refusal_envelope(&refusal);
        return 2;
    }

    // Read input records
    let records = match read_input_records(&cli.input) {
        Ok(recs) => recs,
        Err(error) => {
            output_refusal_envelope(&build_bad_input_refusal(error));
            return 2;
        }
    };

    let _diagnose_guard = DiagnoseModeGuard::new(cli.diagnose);

    // Process records through enrichment pipeline
    let enriched_records: Vec<serde_json::Value> = records
        .into_iter()
        .map(|record| enrich_record_with_fingerprints(&record, &registry, &cli.fingerprints))
        .collect();

    // Determine outcome for exit code
    let mut outcome = Outcome::AllMatched;
    for record in &enriched_records {
        if record_requires_partial_outcome(record) {
            outcome = Outcome::Partial;
        }
    }

    // Write output
    let mut stdout = std::io::stdout();
    if let Err(error) = write_jsonl(&mut stdout, &enriched_records) {
        eprintln!("Error writing output: {}", error);
        return 2;
    }

    // Append witness record (unless --no-witness)
    if !cli.no_witness {
        let outcome_text = match outcome {
            Outcome::AllMatched => "ALL_MATCHED",
            Outcome::Partial => "PARTIAL",
            Outcome::Refusal => "REFUSAL",
        };
        let witness_input_path = cli
            .input
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "stdin".to_owned());
        let witness_record = WitnessRecord::new(
            env!("CARGO_PKG_VERSION").to_owned(),
            "blake3:unknown".to_owned(),
            vec![WitnessInput {
                path: witness_input_path,
                hash: None,
                bytes: None,
            }],
            serde_json::json!({
                "fingerprints": cli.fingerprints,
                "input": cli.input.as_ref().map(|path| path.display().to_string())
            }),
            outcome_text.to_owned(),
            outcome.exit_code(),
            "blake3:unknown".to_owned(),
            None,
            chrono::Utc::now().to_rfc3339(),
        );

        match witness_record {
            Ok(record) => {
                let ledger_path = ledger_path();
                if let Err(error) = append(&ledger_path, &record) {
                    eprintln!("Warning: Failed to record witness: {}", error);
                }
            }
            Err(error) => {
                eprintln!("Warning: Failed to build witness record: {}", error);
            }
        }
    }

    outcome.exit_code()
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

    // TODO: Discover and register installed fingerprint crates

    // Validate registry (check for duplicates and trust policy)
    let allowlist: Vec<String> = vec![]; // TODO: Load from config
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
            } => build_envelope(
                RefusalCode::UntrustedFp,
                "Untrusted fingerprint provider",
                RefusalDetail::UntrustedFp(refusal::codes::UntrustedFpDetail {
                    fingerprint_id,
                    provider,
                    policy,
                }),
                Some("Add provider to allowlist or use --trust-all".to_owned()),
            ),
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
            children.iter().any(|child| {
                !child
                    .get("matched")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false)
            })
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
fn output_refusal_envelope(refusal: &refusal::codes::RefusalEnvelope) {
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
            None,
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
    fields_path: &std::path::Path,
    id: Option<&str>,
    out_path: Option<&std::path::Path>,
    append_witness_record: bool,
) -> u8 {
    use crate::document::open_document_from_path;
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

    let document = match open_document_from_path(doc_path) {
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
        let witness = WitnessRecord::new(
            env!("CARGO_PKG_VERSION").to_owned(),
            "blake3:unknown".to_owned(),
            vec![
                WitnessInput {
                    path: doc_path.display().to_string(),
                    hash: None,
                    bytes: None,
                },
                WitnessInput {
                    path: fields_path.display().to_string(),
                    hash: None,
                    bytes: None,
                },
            ],
            serde_json::json!({
                "mode": "infer-schema",
                "id": fingerprint_id,
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
            None,
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
    fn record_partial_outcome_detects_failed_child() {
        let record = json!({
            "fingerprint": {
                "matched": true,
                "children": [
                    { "fingerprint_id": "parent.v1/child-a.v1", "matched": true },
                    { "fingerprint_id": "parent.v1/child-b.v1", "matched": false }
                ]
            }
        });

        assert!(record_requires_partial_outcome(&record));
    }

    #[test]
    fn record_partial_outcome_keeps_all_matched_when_children_match() {
        let record = json!({
            "fingerprint": {
                "matched": true,
                "children": [
                    { "fingerprint_id": "parent.v1/child-a.v1", "matched": true },
                    { "fingerprint_id": "parent.v1/child-b.v1", "matched": true }
                ]
            }
        });

        assert!(!record_requires_partial_outcome(&record));
    }
}
