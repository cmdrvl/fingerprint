use crate::document::{Document, dispatch::open_document_with_text_path};
use crate::dsl::assertions::diagnose_mode;
use crate::progress::reporter::{report_warning, report_warning_code};
use crate::refusal::codes::{BadInputDetail, RefusalCode, RefusalDetail, build_envelope};
use crate::registry::{AssertionResult, FingerprintInfo, FingerprintRegistry, FingerprintResult};
use serde_json::{Map, Value, json};
use std::path::Path;

/// Warning structure for `_warnings` array in JSONL records.
#[derive(Debug, Clone, serde::Serialize)]
struct Warning {
    tool: String,
    code: String,
    message: String,
    detail: Value,
}

impl Warning {
    fn new(code: impl Into<String>, message: impl Into<String>, detail: Value) -> Self {
        Self {
            tool: "fingerprint".to_owned(),
            code: code.into(),
            message: message.into(),
            detail,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
struct FingerprintDiagnostics {
    attempts: Vec<DiagnosticAttempt>,
    all_candidates_failed: bool,
    short_circuited_fingerprint_ids: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DiagnosticAttempt {
    fingerprint_id: String,
    matched: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    first_failed_assertion: Option<FailedAssertionDiagnostic>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct FailedAssertionDiagnostic {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<Value>,
}

/// Enrich a single JSONL record with fingerprint results.
pub fn enrich_record(record: &Value, registry: &FingerprintRegistry) -> Value {
    let fingerprint_ids: Vec<String> = registry
        .iter()
        .map(|fingerprint| fingerprint.id().to_owned())
        .collect();
    enrich_record_with_fingerprints(record, registry, &fingerprint_ids)
}

/// Enrich a single JSONL record with fingerprint results using explicit fingerprint IDs.
pub fn enrich_record_with_fingerprints(
    record: &Value,
    registry: &FingerprintRegistry,
    fingerprint_ids: &[String],
) -> Value {
    if !record.is_object() {
        return create_bad_input_refusal(
            0,
            "Record is not a JSON object",
            None,
            Some("record must be a JSON object".to_owned()),
        );
    }

    let upstream_skipped = is_skipped_record(record);

    let path_str = record.get("path").and_then(Value::as_str).unwrap_or("");
    let text_path = record
        .get("text_path")
        .and_then(Value::as_str)
        .map(Path::new);
    let extension = extract_extension(record, path_str);

    let mut enriched = record.clone();
    let enriched_obj = match enriched.as_object_mut() {
        Some(obj) => obj,
        None => {
            return create_bad_input_refusal(
                0,
                "Record is not a JSON object",
                None,
                Some("record must be a JSON object".to_owned()),
            );
        }
    };

    enriched_obj.insert(
        "version".to_owned(),
        Value::String("fingerprint.v0".to_owned()),
    );
    update_tool_versions(enriched_obj);

    if upstream_skipped {
        return handle_skipped_passthrough(enriched_obj);
    }

    if let Err(missing_field) = validate_required_fields(record) {
        return create_bad_input_refusal(
            0,
            "Missing required input field",
            Some(missing_field),
            None,
        );
    }

    let document = match open_document_with_text_path(Path::new(path_str), &extension, text_path) {
        Ok(document) => document,
        Err(error) => {
            let warning = Warning::new(
                "E_PARSE",
                format!("Cannot parse {}: {}", extension.to_uppercase(), error),
                json!({
                    "path": path_str,
                    "error": error
                }),
            );
            report_warning(path_str, &format!("skipped: {}", error));
            return create_skipped_record_with_warning(enriched_obj, warning);
        }
    };

    maybe_emit_sparse_text_warning(path_str, &document);

    let fingerprint_value =
        evaluate_fingerprints(&document, registry, fingerprint_ids).unwrap_or(Value::Null);
    enriched_obj.insert("fingerprint".to_owned(), fingerprint_value);

    Value::Object(enriched_obj.clone())
}

fn extract_extension(record: &Value, path_str: &str) -> String {
    if let Some(extension) = record.get("extension").and_then(Value::as_str) {
        return extension
            .trim_start_matches('.')
            .to_ascii_lowercase()
            .to_string();
    }

    Path::new(path_str)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .to_string()
}

fn evaluate_fingerprints(
    document: &Document,
    registry: &FingerprintRegistry,
    fingerprint_ids: &[String],
) -> Option<Value> {
    let diagnose = diagnose_mode();
    let mut last_attempt: Option<Value> = None;
    let mut attempts = Vec::new();

    for (index, fingerprint_id) in fingerprint_ids.iter().enumerate() {
        let Some(fingerprint_info) = registry.info_for(fingerprint_id) else {
            continue;
        };
        if fingerprint_info.parent.is_some() {
            continue;
        }

        let Some(fingerprint) = registry.get(fingerprint_id) else {
            continue;
        };
        if !format_matches(fingerprint.format(), document) {
            continue;
        }

        let result = fingerprint.fingerprint(document);
        if diagnose {
            attempts.push(build_diagnostic_attempt(fingerprint.id(), &result));
        }
        let mut payload =
            build_fingerprint_payload(fingerprint.id(), Some(fingerprint_info), &result);

        if result.matched {
            if diagnose {
                attach_run_diagnostics(
                    &mut payload,
                    &attempts,
                    false,
                    remaining_root_candidate_ids(document, registry, fingerprint_ids, index + 1),
                );
            }
            let children = evaluate_children(document, registry, fingerprint_ids, fingerprint.id());
            if !children.is_empty()
                && let Some(parent_payload) = payload.as_object_mut()
            {
                parent_payload.insert("children".to_owned(), Value::Array(children));
            }
            return Some(payload);
        }
        last_attempt = Some(payload);
    }

    if diagnose && let Some(payload) = last_attempt.as_mut() {
        attach_run_diagnostics(payload, &attempts, true, Vec::new());
    }

    last_attempt
}

fn evaluate_children(
    document: &Document,
    registry: &FingerprintRegistry,
    fingerprint_ids: &[String],
    parent_id: &str,
) -> Vec<Value> {
    let mut children = Vec::new();

    for child_id in fingerprint_ids {
        let Some(child_info) = registry.info_for(child_id) else {
            continue;
        };
        if child_info.parent.as_deref() != Some(parent_id) {
            continue;
        }

        let Some(child_fingerprint) = registry.get(child_id) else {
            continue;
        };
        if !format_matches(child_fingerprint.format(), document) {
            continue;
        }

        let child_result = child_fingerprint.fingerprint(document);
        let child_payload =
            build_fingerprint_payload(child_fingerprint.id(), Some(child_info), &child_result);
        children.push(child_payload);
    }

    children
}

fn format_matches(fingerprint_format: &str, document: &Document) -> bool {
    match document {
        Document::Xlsx(_) => fingerprint_format.eq_ignore_ascii_case("xlsx"),
        Document::Csv(_) => fingerprint_format.eq_ignore_ascii_case("csv"),
        Document::Pdf(_) => fingerprint_format.eq_ignore_ascii_case("pdf"),
        Document::Markdown(_) => {
            fingerprint_format.eq_ignore_ascii_case("markdown")
                || fingerprint_format.eq_ignore_ascii_case("md")
        }
        Document::Text(_) => fingerprint_format.eq_ignore_ascii_case("text"),
        Document::Unknown(_) => false,
    }
}

fn build_fingerprint_payload(
    fingerprint_id: &str,
    info: Option<&FingerprintInfo>,
    result: &FingerprintResult,
) -> Value {
    let fingerprint_crate = info
        .map(|meta| meta.crate_name.as_str())
        .unwrap_or("unknown");
    let fingerprint_version = info.map(|meta| meta.version.as_str()).unwrap_or("0.0.0");
    let source_hint = info.map(|meta| meta.source.as_str()).unwrap_or("builtin");
    let fingerprint_source = if source_hint.starts_with("builtin") {
        "rust".to_owned()
    } else if source_hint.starts_with("dsl") {
        "dsl".to_owned()
    } else {
        source_hint.to_owned()
    };

    json!({
        "fingerprint_id": fingerprint_id,
        "fingerprint_crate": fingerprint_crate,
        "fingerprint_version": fingerprint_version,
        "fingerprint_source": fingerprint_source,
        "matched": result.matched,
        "reason": result.reason,
        "assertions": result.assertions,
        "extracted": result.extracted,
        "content_hash": result.content_hash,
    })
}

fn build_diagnostic_attempt(fingerprint_id: &str, result: &FingerprintResult) -> DiagnosticAttempt {
    DiagnosticAttempt {
        fingerprint_id: fingerprint_id.to_owned(),
        matched: result.matched,
        reason: result.reason.clone(),
        first_failed_assertion: first_failed_assertion(&result.assertions),
    }
}

fn first_failed_assertion(assertions: &[AssertionResult]) -> Option<FailedAssertionDiagnostic> {
    assertions
        .iter()
        .find(|assertion| !assertion.passed)
        .map(|assertion| FailedAssertionDiagnostic {
            name: assertion.name.clone(),
            detail: assertion.detail.clone(),
            context: assertion.context.clone(),
        })
}

fn attach_run_diagnostics(
    payload: &mut Value,
    attempts: &[DiagnosticAttempt],
    all_candidates_failed: bool,
    short_circuited_fingerprint_ids: Vec<String>,
) {
    let has_failed_attempts = attempts.iter().any(|attempt| !attempt.matched);
    if !has_failed_attempts && short_circuited_fingerprint_ids.is_empty() {
        return;
    }

    let diagnostics = serde_json::to_value(FingerprintDiagnostics {
        attempts: attempts.to_vec(),
        all_candidates_failed,
        short_circuited_fingerprint_ids,
    })
    .expect("fingerprint diagnostics should serialize");

    if let Some(payload_obj) = payload.as_object_mut() {
        payload_obj.insert("diagnostics".to_owned(), diagnostics);
    }
}

fn remaining_root_candidate_ids(
    document: &Document,
    registry: &FingerprintRegistry,
    fingerprint_ids: &[String],
    start_index: usize,
) -> Vec<String> {
    fingerprint_ids
        .iter()
        .skip(start_index)
        .filter_map(|fingerprint_id| {
            let info = registry.info_for(fingerprint_id)?;
            if info.parent.is_some() {
                return None;
            }

            let fingerprint = registry.get(fingerprint_id)?;
            format_matches(fingerprint.format(), document).then(|| fingerprint.id().to_owned())
        })
        .collect()
}

fn maybe_emit_sparse_text_warning(path: &str, document: &Document) {
    let Document::Pdf(pdf) = document else {
        return;
    };

    let Some(text_document) = pdf.text.as_ref() else {
        return;
    };

    let Ok(page_count) = pdf.page_count() else {
        return;
    };

    let text_chars = text_document.normalized.chars().count();
    if let Some(message) = sparse_text_warning_message(page_count, text_chars) {
        report_warning_code(path, Some("W_SPARSE_TEXT"), &message);
    }
}

fn sparse_text_warning_message(page_count: u64, text_chars: usize) -> Option<String> {
    if page_count > 10 && text_chars < 100 {
        Some(format!(
            "text_path has {text_chars} chars but PDF has {page_count} pages — possible scanned PDF or extraction failure"
        ))
    } else {
        None
    }
}

/// Check if record has `_skipped: true`.
fn is_skipped_record(record: &Value) -> bool {
    record
        .get("_skipped")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

/// Handle passthrough of upstream `_skipped` records.
fn handle_skipped_passthrough(enriched_obj: &mut Map<String, Value>) -> Value {
    enriched_obj.insert("fingerprint".to_owned(), Value::Null);
    Value::Object(enriched_obj.clone())
}

/// Validate that non-skipped records have required fields.
fn validate_required_fields(record: &Value) -> Result<(), &'static str> {
    if !record.get("bytes_hash").is_some_and(Value::is_string) {
        return Err("bytes_hash");
    }

    if !record.get("path").is_some_and(Value::is_string) {
        return Err("path");
    }

    Ok(())
}

/// Update `tool_versions` to include fingerprint version.
fn update_tool_versions(enriched_obj: &mut Map<String, Value>) {
    let mut tool_versions = enriched_obj
        .get("tool_versions")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    tool_versions.insert(
        "fingerprint".to_owned(),
        Value::String(env!("CARGO_PKG_VERSION").to_owned()),
    );
    enriched_obj.insert("tool_versions".to_owned(), Value::Object(tool_versions));
}

/// Create a `_skipped` record with warning.
fn create_skipped_record_with_warning(
    enriched_obj: &mut Map<String, Value>,
    warning: Warning,
) -> Value {
    enriched_obj.insert("_skipped".to_owned(), Value::Bool(true));
    enriched_obj.insert("fingerprint".to_owned(), Value::Null);

    let mut warnings = enriched_obj
        .get("_warnings")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    warnings.push(serde_json::to_value(&warning).expect("warning serialization should never fail"));
    enriched_obj.insert("_warnings".to_owned(), Value::Array(warnings));

    Value::Object(enriched_obj.clone())
}

fn create_bad_input_refusal(
    line: u64,
    message: &str,
    missing_field: Option<&str>,
    error: Option<String>,
) -> Value {
    let refusal = build_envelope(
        RefusalCode::BadInput,
        message,
        RefusalDetail::BadInput(BadInputDetail {
            line,
            error,
            missing_field: missing_field.map(str::to_owned),
            version: None,
        }),
        Some("Run `hash` first and provide valid JSONL input".to_owned()),
    );
    serde_json::to_value(refusal).expect("refusal serialization should never fail")
}

#[cfg(test)]
mod tests {
    use super::{enrich_record, enrich_record_with_fingerprints, sparse_text_warning_message};
    use crate::document::Document;
    use crate::dsl::assertions::set_diagnose_mode;
    use crate::registry::{
        AssertionResult, Fingerprint, FingerprintInfo, FingerprintRegistry, FingerprintResult,
    };
    use serde_json::{Value, json};
    use std::collections::HashMap;
    use std::fs;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tempfile::NamedTempFile;

    struct TestFingerprint {
        id: &'static str,
        format: &'static str,
        parent: Option<&'static str>,
        matched: bool,
        calls: Option<Arc<AtomicUsize>>,
        failure_context: Option<Value>,
    }

    impl Fingerprint for TestFingerprint {
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
            if let Some(counter) = &self.calls {
                counter.fetch_add(1, Ordering::Relaxed);
            }

            FingerprintResult {
                matched: self.matched,
                reason: if self.matched {
                    None
                } else {
                    Some("no match".to_owned())
                },
                assertions: vec![AssertionResult {
                    name: "format_match".to_owned(),
                    passed: self.matched,
                    detail: (!self.matched).then_some("assertion failed".to_owned()),
                    context: (!self.matched)
                        .then(|| self.failure_context.clone())
                        .flatten(),
                }],
                extracted: self
                    .matched
                    .then_some(HashMap::from([("sample".to_owned(), json!("value"))])),
                content_hash: self.matched.then_some("blake3:sample-content".to_owned()),
            }
        }
    }

    fn registry_with_fingerprints(
        fingerprints: Vec<(TestFingerprint, FingerprintInfo)>,
    ) -> FingerprintRegistry {
        let mut registry = FingerprintRegistry::new();
        for (fingerprint, info) in fingerprints {
            registry.register_with_info(Box::new(fingerprint), info);
        }
        registry
    }

    struct DiagnoseModeReset;

    impl DiagnoseModeReset {
        fn enable() -> Self {
            set_diagnose_mode(true);
            Self
        }
    }

    impl Drop for DiagnoseModeReset {
        fn drop(&mut self) {
            set_diagnose_mode(false);
        }
    }

    #[test]
    fn upstream_skipped_records_passthrough_with_fingerprint_null() {
        let registry = FingerprintRegistry::new();
        let input = json!({
            "version": "hash.v0",
            "path": "/tmp/input.csv",
            "_skipped": true,
            "tool_versions": { "hash": "0.1.0" }
        });

        let output = enrich_record(&input, &registry);
        assert_eq!(output["version"], "fingerprint.v0");
        assert_eq!(output["fingerprint"], Value::Null);
        assert_eq!(output["_skipped"], true);
        assert_eq!(
            output["tool_versions"]["fingerprint"],
            env!("CARGO_PKG_VERSION")
        );
    }

    #[test]
    fn missing_bytes_hash_returns_refusal_envelope() {
        let registry = FingerprintRegistry::new();
        let input = json!({
            "version": "hash.v0",
            "path": "/tmp/input.csv",
            "tool_versions": { "hash": "0.1.0" }
        });

        let output = enrich_record(&input, &registry);
        assert_eq!(output["outcome"], "REFUSAL");
        assert_eq!(output["refusal"]["code"], "E_BAD_INPUT");
        assert_eq!(output["refusal"]["detail"]["missing_field"], "bytes_hash");
    }

    #[test]
    fn parse_failures_create_skipped_warning() {
        let registry = FingerprintRegistry::new();
        let input = json!({
            "version": "hash.v0",
            "path": "/definitely/missing.txt",
            "extension": ".txt",
            "bytes_hash": "blake3:abc"
        });

        let output = enrich_record(&input, &registry);
        assert_eq!(output["_skipped"], true);
        assert_eq!(output["fingerprint"], Value::Null);
        assert_eq!(output["_warnings"][0]["code"], "E_PARSE");
    }

    #[test]
    fn first_matching_fingerprint_wins() {
        let temp_file = NamedTempFile::with_suffix(".txt").expect("create text temp file");
        fs::write(temp_file.path(), "hello world").expect("write text file");
        let path = temp_file.path().display().to_string();

        let registry = registry_with_fingerprints(vec![
            (
                TestFingerprint {
                    id: "first.v0",
                    format: "text",
                    parent: None,
                    matched: true,
                    calls: None,
                    failure_context: None,
                },
                FingerprintInfo {
                    id: "first.v0".to_owned(),
                    crate_name: "fingerprint-first".to_owned(),
                    version: "0.1.0".to_owned(),
                    source: "builtin:first".to_owned(),
                    format: "text".to_owned(),
                    parent: None,
                },
            ),
            (
                TestFingerprint {
                    id: "second.v0",
                    format: "text",
                    parent: None,
                    matched: true,
                    calls: None,
                    failure_context: None,
                },
                FingerprintInfo {
                    id: "second.v0".to_owned(),
                    crate_name: "fingerprint-second".to_owned(),
                    version: "0.1.0".to_owned(),
                    source: "builtin:second".to_owned(),
                    format: "text".to_owned(),
                    parent: None,
                },
            ),
        ]);

        let input = json!({
            "version": "hash.v0",
            "path": path,
            "extension": ".txt",
            "bytes_hash": "blake3:abc",
            "tool_versions": { "hash": "0.1.0" }
        });
        let output = enrich_record(&input, &registry);

        assert_eq!(output["fingerprint"]["fingerprint_id"], "first.v0");
        assert_eq!(output["fingerprint"]["matched"], true);
    }

    #[test]
    fn when_none_match_last_attempt_is_reported() {
        let temp_file = NamedTempFile::with_suffix(".txt").expect("create text temp file");
        fs::write(temp_file.path(), "hello world").expect("write text file");
        let path = temp_file.path().display().to_string();

        let registry = registry_with_fingerprints(vec![
            (
                TestFingerprint {
                    id: "first.v0",
                    format: "text",
                    parent: None,
                    matched: false,
                    calls: None,
                    failure_context: None,
                },
                FingerprintInfo {
                    id: "first.v0".to_owned(),
                    crate_name: "fingerprint-first".to_owned(),
                    version: "0.1.0".to_owned(),
                    source: "dsl:first".to_owned(),
                    format: "text".to_owned(),
                    parent: None,
                },
            ),
            (
                TestFingerprint {
                    id: "last.v0",
                    format: "text",
                    parent: None,
                    matched: false,
                    calls: None,
                    failure_context: None,
                },
                FingerprintInfo {
                    id: "last.v0".to_owned(),
                    crate_name: "fingerprint-last".to_owned(),
                    version: "0.2.0".to_owned(),
                    source: "dsl:last".to_owned(),
                    format: "text".to_owned(),
                    parent: None,
                },
            ),
        ]);

        let input = json!({
            "version": "hash.v0",
            "path": path,
            "extension": ".txt",
            "bytes_hash": "blake3:abc",
            "tool_versions": { "hash": "0.1.0" }
        });
        let output = enrich_record(&input, &registry);

        assert_eq!(output["fingerprint"]["fingerprint_id"], "last.v0");
        assert_eq!(output["fingerprint"]["matched"], false);
    }

    #[test]
    fn parent_match_evaluates_children_and_attaches_results() {
        let temp_file = NamedTempFile::with_suffix(".txt").expect("create text temp file");
        fs::write(temp_file.path(), "hello world").expect("write text file");
        let path = temp_file.path().display().to_string();

        let parent_calls = Arc::new(AtomicUsize::new(0));
        let child_a_calls = Arc::new(AtomicUsize::new(0));
        let child_b_calls = Arc::new(AtomicUsize::new(0));

        let registry = registry_with_fingerprints(vec![
            (
                TestFingerprint {
                    id: "parent.v1",
                    format: "text",
                    parent: None,
                    matched: true,
                    calls: Some(parent_calls.clone()),
                    failure_context: None,
                },
                FingerprintInfo {
                    id: "parent.v1".to_owned(),
                    crate_name: "fingerprint-parent".to_owned(),
                    version: "1.0.0".to_owned(),
                    source: "dsl:parent".to_owned(),
                    format: "text".to_owned(),
                    parent: None,
                },
            ),
            (
                TestFingerprint {
                    id: "parent.v1/child-a.v1",
                    format: "text",
                    parent: Some("parent.v1"),
                    matched: true,
                    calls: Some(child_a_calls.clone()),
                    failure_context: None,
                },
                FingerprintInfo {
                    id: "parent.v1/child-a.v1".to_owned(),
                    crate_name: "fingerprint-child-a".to_owned(),
                    version: "1.0.0".to_owned(),
                    source: "dsl:child-a".to_owned(),
                    format: "text".to_owned(),
                    parent: Some("parent.v1".to_owned()),
                },
            ),
            (
                TestFingerprint {
                    id: "parent.v1/child-b.v1",
                    format: "text",
                    parent: Some("parent.v1"),
                    matched: false,
                    calls: Some(child_b_calls.clone()),
                    failure_context: None,
                },
                FingerprintInfo {
                    id: "parent.v1/child-b.v1".to_owned(),
                    crate_name: "fingerprint-child-b".to_owned(),
                    version: "1.0.0".to_owned(),
                    source: "dsl:child-b".to_owned(),
                    format: "text".to_owned(),
                    parent: Some("parent.v1".to_owned()),
                },
            ),
        ]);

        let input = json!({
            "version": "hash.v0",
            "path": path,
            "extension": ".txt",
            "bytes_hash": "blake3:abc",
            "tool_versions": { "hash": "0.1.0" }
        });
        let selected = vec![
            "parent.v1".to_owned(),
            "parent.v1/child-a.v1".to_owned(),
            "parent.v1/child-b.v1".to_owned(),
        ];
        let output = enrich_record_with_fingerprints(&input, &registry, &selected);

        assert_eq!(output["fingerprint"]["fingerprint_id"], "parent.v1");
        assert_eq!(output["fingerprint"]["matched"], true);
        let children = output["fingerprint"]["children"]
            .as_array()
            .expect("children array");
        assert_eq!(children.len(), 2);
        assert_eq!(children[0]["fingerprint_id"], "parent.v1/child-a.v1");
        assert_eq!(children[0]["matched"], true);
        assert_eq!(children[1]["fingerprint_id"], "parent.v1/child-b.v1");
        assert_eq!(children[1]["matched"], false);

        assert_eq!(parent_calls.load(Ordering::Relaxed), 1);
        assert_eq!(child_a_calls.load(Ordering::Relaxed), 1);
        assert_eq!(child_b_calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn parent_no_match_skips_child_evaluation() {
        let temp_file = NamedTempFile::with_suffix(".txt").expect("create text temp file");
        fs::write(temp_file.path(), "hello world").expect("write text file");
        let path = temp_file.path().display().to_string();

        let child_calls = Arc::new(AtomicUsize::new(0));
        let registry = registry_with_fingerprints(vec![
            (
                TestFingerprint {
                    id: "parent.v1",
                    format: "text",
                    parent: None,
                    matched: false,
                    calls: None,
                    failure_context: None,
                },
                FingerprintInfo {
                    id: "parent.v1".to_owned(),
                    crate_name: "fingerprint-parent".to_owned(),
                    version: "1.0.0".to_owned(),
                    source: "dsl:parent".to_owned(),
                    format: "text".to_owned(),
                    parent: None,
                },
            ),
            (
                TestFingerprint {
                    id: "parent.v1/child-a.v1",
                    format: "text",
                    parent: Some("parent.v1"),
                    matched: true,
                    calls: Some(child_calls.clone()),
                    failure_context: None,
                },
                FingerprintInfo {
                    id: "parent.v1/child-a.v1".to_owned(),
                    crate_name: "fingerprint-child-a".to_owned(),
                    version: "1.0.0".to_owned(),
                    source: "dsl:child-a".to_owned(),
                    format: "text".to_owned(),
                    parent: Some("parent.v1".to_owned()),
                },
            ),
        ]);

        let input = json!({
            "version": "hash.v0",
            "path": path,
            "extension": ".txt",
            "bytes_hash": "blake3:abc",
            "tool_versions": { "hash": "0.1.0" }
        });
        let selected = vec!["parent.v1".to_owned(), "parent.v1/child-a.v1".to_owned()];
        let output = enrich_record_with_fingerprints(&input, &registry, &selected);

        assert_eq!(output["fingerprint"]["fingerprint_id"], "parent.v1");
        assert_eq!(output["fingerprint"]["matched"], false);
        assert!(output["fingerprint"].get("children").is_none());
        assert_eq!(child_calls.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn diagnose_run_mode_records_attempt_history_and_short_circuit() {
        let _diagnose = DiagnoseModeReset::enable();
        let temp_file = NamedTempFile::with_suffix(".txt").expect("create text temp file");
        fs::write(temp_file.path(), "hello world").expect("write text file");
        let path = temp_file.path().display().to_string();

        let registry = registry_with_fingerprints(vec![
            (
                TestFingerprint {
                    id: "near-miss.v0",
                    format: "text",
                    parent: None,
                    matched: false,
                    calls: None,
                    failure_context: Some(json!({ "nearest_match": "hello world" })),
                },
                FingerprintInfo {
                    id: "near-miss.v0".to_owned(),
                    crate_name: "fingerprint-near-miss".to_owned(),
                    version: "0.1.0".to_owned(),
                    source: "dsl:near-miss".to_owned(),
                    format: "text".to_owned(),
                    parent: None,
                },
            ),
            (
                TestFingerprint {
                    id: "winner.v0",
                    format: "text",
                    parent: None,
                    matched: true,
                    calls: None,
                    failure_context: None,
                },
                FingerprintInfo {
                    id: "winner.v0".to_owned(),
                    crate_name: "fingerprint-winner".to_owned(),
                    version: "0.1.0".to_owned(),
                    source: "dsl:winner".to_owned(),
                    format: "text".to_owned(),
                    parent: None,
                },
            ),
            (
                TestFingerprint {
                    id: "later.v0",
                    format: "text",
                    parent: None,
                    matched: true,
                    calls: None,
                    failure_context: None,
                },
                FingerprintInfo {
                    id: "later.v0".to_owned(),
                    crate_name: "fingerprint-later".to_owned(),
                    version: "0.1.0".to_owned(),
                    source: "dsl:later".to_owned(),
                    format: "text".to_owned(),
                    parent: None,
                },
            ),
        ]);

        let input = json!({
            "version": "hash.v0",
            "path": path,
            "extension": ".txt",
            "bytes_hash": "blake3:abc",
            "tool_versions": { "hash": "0.1.0" }
        });
        let output = enrich_record_with_fingerprints(
            &input,
            &registry,
            &[
                "near-miss.v0".to_owned(),
                "winner.v0".to_owned(),
                "later.v0".to_owned(),
            ],
        );

        assert_eq!(output["fingerprint"]["fingerprint_id"], "winner.v0");
        assert_eq!(output["fingerprint"]["matched"], true);
        assert_eq!(
            output["fingerprint"]["diagnostics"]["short_circuited_fingerprint_ids"],
            json!(["later.v0"])
        );
        assert_eq!(
            output["fingerprint"]["diagnostics"]["all_candidates_failed"],
            false
        );
        assert_eq!(
            output["fingerprint"]["diagnostics"]["attempts"][0]["fingerprint_id"],
            "near-miss.v0"
        );
        assert_eq!(
            output["fingerprint"]["diagnostics"]["attempts"][0]["first_failed_assertion"]["name"],
            "format_match"
        );
        assert_eq!(
            output["fingerprint"]["diagnostics"]["attempts"][0]["first_failed_assertion"]["context"]
                ["nearest_match"],
            "hello world"
        );
        assert_eq!(
            output["fingerprint"]["diagnostics"]["attempts"][1]["fingerprint_id"],
            "winner.v0"
        );
    }

    #[test]
    fn diagnose_run_mode_marks_when_all_candidates_fail() {
        let _diagnose = DiagnoseModeReset::enable();
        let temp_file = NamedTempFile::with_suffix(".txt").expect("create text temp file");
        fs::write(temp_file.path(), "hello world").expect("write text file");
        let path = temp_file.path().display().to_string();

        let registry = registry_with_fingerprints(vec![
            (
                TestFingerprint {
                    id: "first.v0",
                    format: "text",
                    parent: None,
                    matched: false,
                    calls: None,
                    failure_context: Some(json!({ "nearest_match": "hello world" })),
                },
                FingerprintInfo {
                    id: "first.v0".to_owned(),
                    crate_name: "fingerprint-first".to_owned(),
                    version: "0.1.0".to_owned(),
                    source: "dsl:first".to_owned(),
                    format: "text".to_owned(),
                    parent: None,
                },
            ),
            (
                TestFingerprint {
                    id: "last.v0",
                    format: "text",
                    parent: None,
                    matched: false,
                    calls: None,
                    failure_context: None,
                },
                FingerprintInfo {
                    id: "last.v0".to_owned(),
                    crate_name: "fingerprint-last".to_owned(),
                    version: "0.2.0".to_owned(),
                    source: "dsl:last".to_owned(),
                    format: "text".to_owned(),
                    parent: None,
                },
            ),
        ]);

        let input = json!({
            "version": "hash.v0",
            "path": path,
            "extension": ".txt",
            "bytes_hash": "blake3:abc",
            "tool_versions": { "hash": "0.1.0" }
        });
        let output = enrich_record_with_fingerprints(
            &input,
            &registry,
            &["first.v0".to_owned(), "last.v0".to_owned()],
        );

        assert_eq!(output["fingerprint"]["fingerprint_id"], "last.v0");
        assert_eq!(output["fingerprint"]["matched"], false);
        assert_eq!(
            output["fingerprint"]["diagnostics"]["all_candidates_failed"],
            true
        );
        assert_eq!(
            output["fingerprint"]["diagnostics"]["attempts"][0]["fingerprint_id"],
            "first.v0"
        );
        assert_eq!(
            output["fingerprint"]["diagnostics"]["attempts"][1]["fingerprint_id"],
            "last.v0"
        );
        assert_eq!(
            output["fingerprint"]["diagnostics"]["short_circuited_fingerprint_ids"],
            json!([])
        );
    }

    #[test]
    fn sparse_text_warning_rule_only_triggers_for_large_pdf_and_short_text() {
        let warning = sparse_text_warning_message(287, 47).expect("warning should trigger");
        assert!(warning.contains("47 chars"));
        assert!(warning.contains("287 pages"));

        assert_eq!(sparse_text_warning_message(8, 47), None);
        assert_eq!(sparse_text_warning_message(287, 180), None);
    }
}
