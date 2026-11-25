#![forbid(unsafe_code)]

//! `fingerprint materialize <FILE> --fp <ID> --extract <NAME> --out <PATH> [--format ...]`
//! (bd-uai): deterministically project a matched `extract:` section of a non-flat document
//! (an XLSX header-anchored range, an HTML region) into a clean CSV / JSONL / Parquet table plus a
//! provenance sidecar that pins what it came from.
//!
//! Fail closed: an unmatched document, an ambiguous child routing, an unknown extract, an
//! extract type that has no projection yet, an unrenderable cell, or an existing output file is
//! a typed refusal (exit 2) and writes nothing. Refusals name coordinates and counts, never cell
//! values.

pub mod project;
pub mod write;

use crate::cli::args::{MaterializeArgs, MaterializeFormat};
use crate::document::{Document, open_document_from_path};
use crate::dsl::assertions::evaluate_named_assertions_with_bindings;
use crate::dsl::parser::FingerprintDefinition;
use crate::registry::installed::scan_definitions;
use project::Projection;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use write::OutputFormat;

pub const SCHEMA: &str = "fingerprint.materialize.v0";

/// A typed refusal: stable code, one-line message, structured detail.
#[derive(Debug)]
pub struct Refusal {
    pub code: &'static str,
    pub message: String,
    pub detail: Value,
}

impl Refusal {
    fn new(code: &'static str, message: impl Into<String>, detail: Value) -> Self {
        Self {
            code,
            message: message.into(),
            detail,
        }
    }
}

/// Every refusal code `materialize` can emit.
pub const REFUSAL_CODES: &[&str] = &[
    "E_UNKNOWN_FP",
    "E_IO",
    "E_NOT_MATCHED",
    "E_AMBIGUOUS_ROUTING",
    "E_UNKNOWN_EXTRACT",
    "E_EXTRACT_UNSUPPORTED",
    "E_PROJECTION",
    "E_OUT_EXISTS",
];

fn hash(bytes: &[u8]) -> String {
    format!("blake3:{}", blake3::hash(bytes).to_hex())
}

fn output_format(args: &MaterializeArgs) -> OutputFormat {
    match args.format {
        Some(MaterializeFormat::Csv) => OutputFormat::Csv,
        Some(MaterializeFormat::Jsonl) => OutputFormat::Jsonl,
        Some(MaterializeFormat::Parquet) => OutputFormat::Parquet,
        None => match args
            .out
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("jsonl") => OutputFormat::Jsonl,
            Some("parquet") => OutputFormat::Parquet,
            _ => OutputFormat::Csv,
        },
    }
}

fn sidecar_path(out: &Path) -> PathBuf {
    let mut name = out.file_name().unwrap_or_default().to_os_string();
    name.push(".provenance.json");
    out.with_file_name(name)
}

/// Run the materialization. `Ok` is the summary object printed on stdout.
pub fn materialize(args: &MaterializeArgs) -> Result<Value, Refusal> {
    let format = output_format(args);
    let sidecar = sidecar_path(&args.out);
    for path in [&args.out, &sidecar] {
        if path.exists() {
            return Err(Refusal::new(
                "E_OUT_EXISTS",
                format!(
                    "'{}' already exists; materialize never overwrites",
                    path.display()
                ),
                json!({ "path": path.display().to_string() }),
            ));
        }
    }

    let (definitions, invalid_definitions) = scan_definitions();
    let installed = definitions
        .iter()
        .find(|d| d.definition.fingerprint_id == args.fp)
        .ok_or_else(|| {
            Refusal::new(
                "E_UNKNOWN_FP",
                format!("no installed .fp.yaml definition has fingerprint_id '{}'", args.fp),
                json!({
                    "fingerprint_id": args.fp,
                    "available": definitions.iter().map(|d| d.definition.fingerprint_id.clone()).collect::<Vec<_>>(),
                    "skipped_invalid_definitions": invalid_definitions
                        .iter()
                        .map(|(path, error)| json!({ "path": path.display().to_string(), "error": error }))
                        .collect::<Vec<_>>(),
                }),
            )
        })?;
    let definition = &installed.definition;

    let source_bytes = std::fs::read(&args.file).map_err(|error| {
        Refusal::new(
            "E_IO",
            format!("cannot read '{}': {error}", args.file.display()),
            json!({ "path": args.file.display().to_string() }),
        )
    })?;
    let document = open_document_from_path(&args.file).map_err(|error| {
        Refusal::new(
            "E_IO",
            format!("cannot open '{}': {error}", args.file.display()),
            json!({ "path": args.file.display().to_string() }),
        )
    })?;

    // 1. The document must match the fingerprint.
    let (results, bindings) =
        evaluate_named_assertions_with_bindings(&definition.assertions, &document, false);
    if let Some(failed) = results.iter().find(|r| !r.passed) {
        return Err(Refusal::new(
            "E_NOT_MATCHED",
            format!(
                "'{}' does not match fingerprint '{}': assertion '{}' failed",
                args.file.display(),
                args.fp,
                failed.name
            ),
            json!({
                "fingerprint_id": args.fp,
                "failed_assertion": failed.name,
                "detail": failed.detail,
            }),
        ));
    }

    // 2. Child routing must not be ambiguous.
    let matching_children: Vec<String> = definitions
        .iter()
        .filter(|d| d.definition.parent.as_deref() == Some(args.fp.as_str()))
        .filter(|d| format_accepts(&d.definition.format, &document))
        .filter(|d| {
            evaluate_named_assertions_with_bindings(&d.definition.assertions, &document, false)
                .0
                .iter()
                .all(|r| r.passed)
        })
        .map(|d| d.definition.fingerprint_id.clone())
        .collect();
    if matching_children.len() > 1 {
        return Err(Refusal::new(
            "E_AMBIGUOUS_ROUTING",
            format!(
                "{} child fingerprints of '{}' match this document; refusing to guess",
                matching_children.len(),
                args.fp
            ),
            json!({ "fingerprint_id": args.fp, "matching_children": matching_children }),
        ));
    }

    // 3. The extract must exist and have a projection for this document.
    let section = definition
        .extract
        .iter()
        .find(|section| section.name == args.extract)
        .ok_or_else(|| {
            Refusal::new(
                "E_UNKNOWN_EXTRACT",
                format!("fingerprint '{}' has no extract named '{}'", args.fp, args.extract),
                json!({
                    "extract": args.extract,
                    "available": definition.extract.iter().map(|s| s.name.clone()).collect::<Vec<_>>(),
                }),
            )
        })?;

    let projection = project_section(&document, section, &bindings, &args.file)?;

    // 4. Render, hash, and write the table and its sidecar.
    let table_bytes = write::render(&projection, format).map_err(|error| {
        Refusal::new("E_PROJECTION", error, json!({ "format": format.as_str() }))
    })?;
    let csv = write::csv_bytes(&projection)
        .map_err(|error| Refusal::new("E_PROJECTION", error, json!({})))?;
    let content_hash = hash(&csv);

    let provenance = provenance_document(
        args,
        installed,
        definition,
        &source_bytes,
        &projection,
        format,
        &table_bytes,
        &content_hash,
    );
    let sidecar_bytes = {
        let mut bytes = serde_json::to_vec(&provenance).unwrap_or_default();
        bytes.push(b'\n');
        bytes
    };
    std::fs::write(&args.out, &table_bytes).map_err(|error| io_refusal(&args.out, error))?;
    std::fs::write(&sidecar, &sidecar_bytes).map_err(|error| io_refusal(&sidecar, error))?;

    Ok(json!({
        "version": SCHEMA,
        "outcome": "MATERIALIZED",
        "out": args.out.display().to_string(),
        "provenance": sidecar.display().to_string(),
        "format": format.as_str(),
        "rows": projection.rows.len(),
        "columns": projection.headers.len(),
        "content_hash": content_hash,
        "output_hash": hash(&table_bytes),
    }))
}

fn io_refusal(path: &Path, error: std::io::Error) -> Refusal {
    Refusal::new(
        "E_IO",
        format!("cannot write '{}': {error}", path.display()),
        json!({ "path": path.display().to_string() }),
    )
}

fn format_accepts(format: &str, document: &Document) -> bool {
    match document {
        Document::Xlsx(_) => format.eq_ignore_ascii_case("xlsx"),
        Document::Csv(_) => format.eq_ignore_ascii_case("csv"),
        Document::Html(_) => format.eq_ignore_ascii_case("html"),
        _ => false,
    }
}

fn project_section(
    document: &Document,
    section: &crate::dsl::parser::ExtractSection,
    bindings: &std::collections::HashMap<String, String>,
    file: &Path,
) -> Result<Projection, Refusal> {
    match (section.r#type.as_str(), document) {
        ("range", Document::Xlsx(xlsx)) => {
            let sheet_spec = section.sheet.as_deref().unwrap_or_default();
            let range = section.range.as_deref().unwrap_or_default();
            let sheet = match sheet_spec.strip_prefix('$') {
                Some(name) => bindings.get(name.trim()).cloned().ok_or_else(|| {
                    Refusal::new(
                        "E_PROJECTION",
                        format!(
                            "sheet binding '{sheet_spec}' was not established by the assertions"
                        ),
                        json!({ "binding": sheet_spec }),
                    )
                })?,
                None => sheet_spec.to_owned(),
            };
            project::xlsx_range(&xlsx.path, &sheet, range).map_err(|error| {
                Refusal::new(
                    "E_PROJECTION",
                    error,
                    json!({ "extract": section.name, "sheet": sheet, "range": range }),
                )
            })
        }
        ("region", Document::Html(html)) => {
            let extracted = crate::dsl::extract::extract(document, std::slice::from_ref(section))
                .map_err(|error| {
                Refusal::new("E_PROJECTION", error, json!({ "extract": section.name }))
            })?;
            let region = extracted.get(&section.name).ok_or_else(|| {
                Refusal::new(
                    "E_PROJECTION",
                    format!(
                        "region extract '{}' did not locate a region in this document",
                        section.name
                    ),
                    json!({ "extract": section.name }),
                )
            })?;
            project::html_region(html, region).map_err(|error| {
                Refusal::new("E_PROJECTION", error, json!({ "extract": section.name }))
            })
        }
        (kind, document) => Err(Refusal::new(
            "E_EXTRACT_UNSUPPORTED",
            format!(
                "extract type '{kind}' has no tabular projection for {} documents yet",
                document_kind(document)
            ),
            json!({
                "extract": section.name,
                "extract_type": kind,
                "document": document_kind(document),
                "file": file.display().to_string(),
                "supported": ["range on xlsx", "region on html"],
            }),
        )),
    }
}

fn document_kind(document: &Document) -> &'static str {
    match document {
        Document::Xlsx(_) => "xlsx",
        Document::Csv(_) => "csv",
        Document::Pdf(_) => "pdf",
        Document::Html(_) => "html",
        Document::Markdown(_) => "markdown",
        Document::Text(_) => "text",
        Document::Json(_) => "json",
        Document::Parquet(_) => "parquet",
        Document::Unknown(_) => "unknown",
    }
}

#[allow(clippy::too_many_arguments)]
fn provenance_document(
    args: &MaterializeArgs,
    installed: &crate::registry::installed::InstalledDefinition,
    definition: &FingerprintDefinition,
    source_bytes: &[u8],
    projection: &Projection,
    format: OutputFormat,
    table_bytes: &[u8],
    content_hash: &str,
) -> Value {
    json!({
        "version": SCHEMA,
        "tool": { "name": "fingerprint", "version": env!("CARGO_PKG_VERSION") },
        "source": {
            "path": args.file.display().to_string(),
            "bytes_hash": hash(source_bytes),
            "size": source_bytes.len(),
        },
        "fingerprint": {
            "id": definition.fingerprint_id,
            "format": definition.format,
            "source_path": installed.path.display().to_string(),
            "source_hash": installed.source_hash,
        },
        "extract": {
            "name": args.extract,
            "locator": projection.locator,
        },
        "content_hash": content_hash,
        "content_hash_over": "canonical CSV rendering of the projected table (header + rows)",
        "output": {
            "path": args.out.display().to_string(),
            "format": format.as_str(),
            "bytes_hash": hash(table_bytes),
            "rows": projection.rows.len(),
            "columns": projection.headers,
        },
        "skipped": projection.skipped,
    })
}

/// CLI entry: print the summary (or a refusal envelope) and return the exit code.
pub fn run(args: &MaterializeArgs) -> u8 {
    match materialize(args) {
        Ok(summary) => {
            println!("{summary}");
            0
        }
        Err(refusal) => {
            let envelope = json!({
                "version": SCHEMA,
                "outcome": "REFUSAL",
                "refusal": {
                    "code": refusal.code,
                    "message": refusal.message,
                    "detail": refusal.detail,
                },
            });
            println!("{envelope}");
            eprintln!("Error: {}", refusal.message);
            2
        }
    }
}
