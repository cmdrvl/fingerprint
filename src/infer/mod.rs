pub mod aggregator;
pub mod emitter;
pub mod frankensearch;
pub mod observer;
pub mod schema;
pub mod schema_infer;

use crate::document::open_document;
use crate::infer::aggregator::AggregatedProfile;
use crate::infer::frankensearch::{HybridSearcher, SearchDocument};
use crate::infer::observer::Observation;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// Infer a fingerprint profile from a directory corpus.
pub fn infer_from_dir(
    dir: &Path,
    format: &str,
    min_confidence: f64,
    include_extract: bool,
) -> Result<(AggregatedProfile, usize), String> {
    if !(0.0..=1.0).contains(&min_confidence) {
        return Err(format!(
            "--min-confidence must be within [0.0, 1.0], got {min_confidence}"
        ));
    }

    let normalized_format = normalize_format(format)?;
    let files = collect_files_for_format(dir, normalized_format)?;
    if files.is_empty() {
        return Err(format!(
            "no '{normalized_format}' files found in '{}'",
            dir.display()
        ));
    }

    let mut observations = Vec::with_capacity(files.len());
    let mut failed = 0usize;
    for path in &files {
        let document = match open_document(path, normalized_format) {
            Ok(document) => document,
            Err(_) => {
                failed += 1;
                continue;
            }
        };
        match observer::observe(&document) {
            Ok(observation) => observations.push(observation),
            Err(_) => failed += 1,
        }
    }

    if observations.is_empty() {
        return Err(format!(
            "no readable '{normalized_format}' files found in '{}' ({} matched extension, {} failed to parse)",
            dir.display(),
            files.len(),
            failed
        ));
    }

    let search_documents = observations_to_search_documents(&observations);
    let searcher = HybridSearcher::new(&search_documents)
        .map_err(|error| format!("failed building frankensearch infer index: {error}"))?;

    let inferred_id = format!("inferred-{normalized_format}.v1");
    let profile = aggregator::aggregate(
        &observations,
        normalized_format,
        &inferred_id,
        min_confidence,
        include_extract,
        Some(&searcher),
    )?;

    Ok((profile, observations.len()))
}

/// Render an inferred profile to `.fp.yaml`.
pub fn emit_profile(fingerprint_id: &str, profile: &AggregatedProfile) -> Result<String, String> {
    let mut rendered = profile.clone();
    rendered.fingerprint_id = fingerprint_id.to_owned();

    let mut output = Vec::new();
    emitter::emit_yaml(&rendered, &mut output)?;
    String::from_utf8(output)
        .map_err(|error| format!("failed encoding inferred yaml as UTF-8: {error}"))
}

fn normalize_format(format: &str) -> Result<&'static str, String> {
    match format.to_ascii_lowercase().as_str() {
        "xlsx" => Ok("xlsx"),
        "csv" => Ok("csv"),
        "pdf" => Ok("pdf"),
        other => Err(format!(
            "unsupported --format '{other}' (expected xlsx|csv|pdf)"
        )),
    }
}

fn collect_files_for_format(dir: &Path, format: &str) -> Result<Vec<PathBuf>, String> {
    if !dir.is_dir() {
        return Err(format!("'{}' is not a directory", dir.display()));
    }

    let mut files = Vec::new();
    collect_files_recursive(dir, format, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_files_recursive(
    dir: &Path,
    format: &str,
    files: &mut Vec<PathBuf>,
) -> Result<(), String> {
    for entry in std::fs::read_dir(dir)
        .map_err(|error| format!("failed reading directory '{}': {error}", dir.display()))?
    {
        let entry = entry.map_err(|error| format!("failed reading directory entry: {error}"))?;
        let path = entry.path();
        if path.is_dir() {
            collect_files_recursive(&path, format, files)?;
            continue;
        }
        let extension = path
            .extension()
            .and_then(OsStr::to_str)
            .unwrap_or("")
            .to_ascii_lowercase();
        if extension_matches_format(&extension, format) {
            files.push(path);
        }
    }
    Ok(())
}

fn extension_matches_format(extension: &str, format: &str) -> bool {
    match format {
        "xlsx" => extension.eq_ignore_ascii_case("xlsx") || extension.eq_ignore_ascii_case("xls"),
        "csv" => extension.eq_ignore_ascii_case("csv"),
        "pdf" => extension.eq_ignore_ascii_case("pdf"),
        _ => false,
    }
}

fn observations_to_search_documents(observations: &[Observation]) -> Vec<SearchDocument> {
    observations
        .iter()
        .enumerate()
        .map(|(index, observation)| SearchDocument {
            id: format!("obs-{index:06}"),
            title: Some(observation.filename.clone()),
            content: observation_search_text(observation),
        })
        .collect()
}

fn observation_search_text(observation: &Observation) -> String {
    let mut tokens = vec![
        observation.filename.clone(),
        observation.extension.clone(),
        observation.format.clone(),
    ];

    tokens.extend(observation.sheet_names.iter().cloned());
    tokens.extend(observation.csv_headers.iter().cloned());
    tokens.extend(observation.cell_values.values().cloned());
    tokens.extend(observation.pdf_metadata.values().cloned());

    if let Some(row_count) = observation.csv_row_count {
        tokens.push(format!("rows:{row_count}"));
    }
    if let Some(page_count) = observation.pdf_page_count {
        tokens.push(format!("pages:{page_count}"));
    }

    tokens.join(" ")
}

#[cfg(test)]
mod tests {
    use super::{emit_profile, infer_from_dir};
    use crate::dsl::FingerprintDefinition;
    use std::path::Path;

    fn fixture(relative: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
    }

    #[test]
    fn infers_xlsx_profile_from_fixture_dir() {
        let dir = fixture("tests/fixtures/files");
        let (profile, corpus_size) = infer_from_dir(&dir, "xlsx", 0.9, true).expect("infer");

        assert!(corpus_size >= 1);
        assert_eq!(profile.format, "xlsx");
        assert!(!profile.assertions.is_empty());
    }

    #[test]
    fn inferred_profile_emits_parseable_yaml() {
        let dir = fixture("tests/fixtures/files");
        let (profile, _) = infer_from_dir(&dir, "xlsx", 0.9, true).expect("infer");
        let yaml = emit_profile("test-inferred.v1", &profile).expect("emit yaml");
        let parsed: FingerprintDefinition = serde_yaml::from_str(&yaml).expect("parse yaml");

        assert_eq!(parsed.fingerprint_id, "test-inferred.v1");
        assert_eq!(parsed.format, "xlsx");
    }

    #[test]
    fn output_is_deterministic() {
        let dir = fixture("tests/fixtures/files");
        let (first_profile, _) = infer_from_dir(&dir, "xlsx", 0.9, true).expect("infer first");
        let (second_profile, _) = infer_from_dir(&dir, "xlsx", 0.9, true).expect("infer second");

        let first = emit_profile("test-inferred.v1", &first_profile).expect("emit first");
        let second = emit_profile("test-inferred.v1", &second_profile).expect("emit second");
        assert_eq!(first, second);
    }
}
