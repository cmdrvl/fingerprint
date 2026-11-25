//! Zero-ceremony direct mode: `fingerprint match <PATH>... --fp <ID>...` (bd-h5o).
//!
//! Replaces `vacuum <DIR> | hashbytes | fingerprint --fp <ID>` for 1-5 files or one data room.
//! It walks the paths itself, hashes each file with blake3 in-process, builds the same
//! `hash.v0` records the pipe would carry, and hands them to the unchanged run pipeline, so
//! the default `--view full` output is byte-for-byte the piped `fingerprint.v0` JSONL for the
//! same files.
//!
//! Walk rules (deterministic): files only, symlinks are not followed, hidden files are
//! included, records are sorted by `path` (bytewise) across all PATH arguments and duplicates
//! are dropped. An unreadable file becomes a `_skipped` record with a `fingerprint` warning
//! (which makes the outcome PARTIAL), exactly like an upstream tool skip.
//!
//! Views (`--view`): `full` (default, per-file JSONL), `brief` (one bounded summary object) and
//! `delta` (what changed since a previous `full` output, for `.fp.yaml` authoring loops).

use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Most failing-assertion rows and family rows a brief view will list. Keeps `--view brief`
/// near 500 tokens however many files there are.
pub const BRIEF_MAX_ROWS: usize = 5;
const BRIEF_MAX_FAMILIES: usize = 10;

/// Walk `paths` and build one `hash.v0` record per file, sorted by path.
pub fn build_records(paths: &[PathBuf]) -> Result<Vec<Value>, String> {
    if paths.is_empty() {
        return Err("at least one PATH is required".to_owned());
    }
    // (path as shown, root, relative path)
    let mut files: BTreeMap<String, (String, String)> = BTreeMap::new();
    for given in paths {
        let meta = fs::symlink_metadata(given)
            .map_err(|error| format!("cannot read '{}': {error}", given.display()))?;
        if meta.file_type().is_symlink() {
            return Err(format!(
                "'{}' is a symlink; symlinks are not followed (pass the real path)",
                given.display()
            ));
        }
        if meta.is_dir() {
            walk_dir(given, given, &mut files)?;
        } else {
            let parent = given
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            let name = given
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            files.insert(
                given.to_string_lossy().into_owned(),
                (parent.to_string_lossy().into_owned(), name),
            );
        }
    }
    Ok(files
        .into_iter()
        .map(|(path, (root, relative))| file_record(&path, &root, &relative))
        .collect())
}

fn walk_dir(
    root: &Path,
    dir: &Path,
    out: &mut BTreeMap<String, (String, String)>,
) -> Result<(), String> {
    let entries = fs::read_dir(dir)
        .map_err(|error| format!("cannot read directory '{}': {error}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("cannot read '{}': {error}", dir.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| format!("cannot stat '{}': {error}", path.display()))?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            walk_dir(root, &path, out)?;
        } else if file_type.is_file() {
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            out.insert(
                path.to_string_lossy().into_owned(),
                (root.to_string_lossy().into_owned(), relative),
            );
        }
    }
    Ok(())
}

fn file_record(path: &str, root: &str, relative: &str) -> Value {
    let extension = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{}", e.to_ascii_lowercase()));
    let metadata = fs::metadata(path).ok();
    let size = metadata.as_ref().map(|m| m.len());
    let mtime = metadata.as_ref().and_then(|m| m.modified().ok()).map(|t| {
        chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    });

    let mut record = Map::new();
    record.insert("version".to_owned(), json!("hash.v0"));
    record.insert("path".to_owned(), json!(path));
    record.insert("relative_path".to_owned(), json!(relative));
    record.insert("root".to_owned(), json!(root));
    record.insert("size".to_owned(), json!(size));
    record.insert("mtime".to_owned(), json!(mtime));
    record.insert("extension".to_owned(), json!(extension));
    record.insert("mime_guess".to_owned(), Value::Null);
    match hash_file(path) {
        Ok(hash) => {
            record.insert("bytes_hash".to_owned(), json!(format!("blake3:{hash}")));
            record.insert("hash_algorithm".to_owned(), json!("blake3"));
        }
        Err(error) => {
            record.insert("_skipped".to_owned(), json!(true));
            record.insert(
                "_warnings".to_owned(),
                json!([{ "tool": "fingerprint", "code": "E_IO", "message": error }]),
            );
            record.insert("bytes_hash".to_owned(), Value::Null);
            record.insert("hash_algorithm".to_owned(), Value::Null);
        }
    }
    Value::Object(record)
}

fn hash_file(path: &str) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

// ---- views ---------------------------------------------------------------------------

/// How one enriched record landed.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Status {
    Skipped,
    Matched(String),
    Ambiguous(String),
    Unmatched,
}

fn status(record: &Value) -> Status {
    if record
        .get("_skipped")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Status::Skipped;
    }
    let Some(fp) = record.get("fingerprint").filter(|f| !f.is_null()) else {
        return Status::Unmatched;
    };
    if !fp.get("matched").and_then(Value::as_bool).unwrap_or(false) {
        return Status::Unmatched;
    }
    let id = fp
        .get("fingerprint_id")
        .and_then(Value::as_str)
        .unwrap_or("?")
        .to_owned();
    let ambiguous = fp
        .get("child_routing")
        .and_then(|r| r.get("status"))
        .and_then(Value::as_str)
        == Some("ambiguous");
    if ambiguous {
        Status::Ambiguous(id)
    } else {
        Status::Matched(id)
    }
}

/// The first failed assertion of an unmatched record: (assertion name, nearest_match).
fn first_failure(record: &Value) -> (String, Option<String>) {
    let failed = record
        .get("fingerprint")
        .and_then(|f| f.get("assertions"))
        .and_then(Value::as_array)
        .and_then(|a| {
            a.iter()
                .find(|assertion| assertion.get("passed").and_then(Value::as_bool) == Some(false))
        });
    match failed {
        Some(assertion) => (
            assertion
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("?")
                .to_owned(),
            assertion
                .get("failure_context")
                .and_then(|c| c.get("nearest_match"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
        ),
        None => ("no_applicable_fingerprint".to_owned(), None),
    }
}

/// `--view brief`: one bounded summary object over all enriched records.
///
/// `refs.output_hash` is the blake3 of the full per-file output for the same run, so a brief
/// can be tied back to (and diffed against) the `--view full` bytes.
pub fn brief(records: &[Value], full_output_hash: &str) -> Value {
    let (mut matched, mut unmatched, mut ambiguous, mut skipped) = (0u64, 0u64, 0u64, 0u64);
    let mut families: BTreeMap<String, u64> = BTreeMap::new();
    // (assertion, nearest_match) -> count
    let mut failures: BTreeMap<(String, Option<String>), u64> = BTreeMap::new();
    for record in records {
        match status(record) {
            Status::Skipped => skipped += 1,
            Status::Matched(id) => {
                matched += 1;
                *families.entry(id).or_default() += 1;
            }
            Status::Ambiguous(id) => {
                ambiguous += 1;
                *families.entry(id).or_default() += 1;
            }
            Status::Unmatched => {
                unmatched += 1;
                *failures.entry(first_failure(record)).or_default() += 1;
            }
        }
    }

    let mut family_rows: Vec<(String, u64)> = families.into_iter().collect();
    family_rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let family_total = family_rows.len();
    let other: u64 = family_rows
        .iter()
        .skip(BRIEF_MAX_FAMILIES)
        .map(|(_, count)| count)
        .sum();
    let mut by_family = Map::new();
    for (id, count) in family_rows.into_iter().take(BRIEF_MAX_FAMILIES) {
        by_family.insert(id, json!(count));
    }
    if family_total > BRIEF_MAX_FAMILIES {
        by_family.insert("(other)".to_owned(), json!(other));
    }

    let mut failure_rows: Vec<((String, Option<String>), u64)> = failures.into_iter().collect();
    failure_rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let failure_kinds = failure_rows.len();
    let top_failures: Vec<Value> = failure_rows
        .into_iter()
        .take(BRIEF_MAX_ROWS)
        .map(|((assertion, nearest), count)| {
            json!({
                "assertion": clip(&assertion),
                "count": count,
                "nearest_match": nearest.as_deref().map(clip),
            })
        })
        .collect();

    json!({
        "version": "fingerprint.brief.v0",
        "files": records.len(),
        "matched": matched,
        "unmatched": unmatched,
        "ambiguous": ambiguous,
        "skipped": skipped,
        "by_family": by_family,
        "top_failures": top_failures,
        "failure_kinds_not_shown": failure_kinds.saturating_sub(BRIEF_MAX_ROWS),
        "refs": {
            "output_hash": full_output_hash,
            "tool_version": env!("CARGO_PKG_VERSION"),
        },
    })
}

/// Keep free text in a brief short; the bound has to hold for hostile assertion names too.
fn clip(text: &str) -> String {
    const MAX: usize = 80;
    if text.chars().count() <= MAX {
        text.to_owned()
    } else {
        let mut clipped: String = text.chars().take(MAX - 1).collect();
        clipped.push('…');
        clipped
    }
}

/// Parse a previous `--view full` output (JSONL, one enriched record per line).
pub fn parse_previous(bytes: &[u8]) -> Result<Vec<Value>, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "previous output is not UTF-8".to_owned())?;
    let mut records = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let record: Value = serde_json::from_str(line)
            .map_err(|error| format!("previous output line {}: {error}", index + 1))?;
        if record.get("path").and_then(Value::as_str).is_none() {
            return Err(format!(
                "previous output line {}: not a fingerprint.v0 record (no path); pass a `--view full` output",
                index + 1
            ));
        }
        records.push(record);
    }
    Ok(records)
}

/// `--view delta --since <previous full output>`: what changed per file path.
///
/// `newly_matched`: unmatched (or absent-but-present-before as unmatched) before, matched now.
/// `regressed`: matched before, not matched now. `changed_family`: matched both times by a
/// different fingerprint. `content_changed`: same path, different `bytes_hash`.
/// Bounded: at most [`BRIEF_MAX_ROWS`] paths are listed per bucket, the counts are exact.
pub fn delta(current: &[Value], previous: &[Value], full_output_hash: &str) -> Value {
    let key = |record: &Value| record["path"].as_str().unwrap_or("").to_owned();
    let before: BTreeMap<String, &Value> = previous.iter().map(|r| (key(r), r)).collect();
    let now: BTreeMap<String, &Value> = current.iter().map(|r| (key(r), r)).collect();

    let mut newly_matched = Vec::new();
    let mut regressed = Vec::new();
    let mut changed_family = Vec::new();
    let mut content_changed = Vec::new();
    let mut new_files = Vec::new();
    let mut unchanged = 0u64;
    for (path, record) in &now {
        let Some(old) = before.get(path) else {
            new_files.push(path.clone());
            continue;
        };
        let (old_status, new_status) = (status(old), status(record));
        let matched_id = |s: &Status| match s {
            Status::Matched(id) | Status::Ambiguous(id) => Some(id.clone()),
            _ => None,
        };
        let (old_id, new_id) = (matched_id(&old_status), matched_id(&new_status));
        let mut changed = false;
        match (&old_id, &new_id) {
            (None, Some(_)) => {
                newly_matched.push(path.clone());
                changed = true;
            }
            (Some(_), None) => {
                regressed.push(path.clone());
                changed = true;
            }
            (Some(a), Some(b)) if a != b => {
                changed_family.push(path.clone());
                changed = true;
            }
            _ => {}
        }
        if old.get("bytes_hash") != record.get("bytes_hash") {
            content_changed.push(path.clone());
            changed = true;
        }
        if !changed {
            unchanged += 1;
        }
    }
    let removed: Vec<String> = before
        .keys()
        .filter(|path| !now.contains_key(*path))
        .cloned()
        .collect();

    let bucket = |paths: Vec<String>| -> Value {
        json!({
            "count": paths.len(),
            "paths": paths.iter().take(BRIEF_MAX_ROWS).map(|p| clip(p)).collect::<Vec<_>>(),
        })
    };
    json!({
        "version": "fingerprint.delta.v0",
        "files": current.len(),
        "previous_files": previous.len(),
        "unchanged": unchanged,
        "newly_matched": bucket(newly_matched),
        "regressed": bucket(regressed),
        "changed_family": bucket(changed_family),
        "content_changed": bucket(content_changed),
        "new_files": bucket(new_files),
        "removed_files": bucket(removed),
        "refs": {
            "output_hash": full_output_hash,
            "tool_version": env!("CARGO_PKG_VERSION"),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(path: &str, matched: Option<&str>, hash: &str) -> Value {
        json!({
            "version": "fingerprint.v0",
            "path": path,
            "bytes_hash": hash,
            "fingerprint": matched.map(|id| json!({"matched": true, "fingerprint_id": id})),
        })
    }

    #[test]
    fn delta_buckets_are_exact() {
        let previous = vec![
            rec("a", Some("csv.v0"), "h1"),
            rec("b", None, "h2"),
            rec("c", Some("csv.v0"), "h3"),
            rec("gone", None, "h9"),
        ];
        let current = vec![
            rec("a", Some("csv.v0"), "h1"),
            rec("b", Some("csv.v0"), "h2"),
            rec("c", None, "h3x"),
            rec("fresh", None, "h8"),
        ];
        let d = delta(&current, &previous, "blake3:x");
        assert_eq!(d["unchanged"], 1);
        assert_eq!(d["newly_matched"]["paths"], json!(["b"]));
        assert_eq!(d["regressed"]["paths"], json!(["c"]));
        assert_eq!(d["content_changed"]["paths"], json!(["c"]));
        assert_eq!(d["new_files"]["paths"], json!(["fresh"]));
        assert_eq!(d["removed_files"]["paths"], json!(["gone"]));
    }

    #[test]
    fn brief_caps_rows_and_keeps_exact_counts() {
        let mut records = Vec::new();
        for i in 0..200 {
            records.push(rec(&format!("f{i}"), Some(&format!("fam{i}.v1")), "h"));
        }
        let b = brief(&records, "blake3:x");
        assert_eq!(b["matched"], 200);
        assert_eq!(
            b["by_family"].as_object().unwrap().len(),
            BRIEF_MAX_FAMILIES + 1
        );
        assert_eq!(b["by_family"]["(other)"], 190);
        assert!(serde_json::to_string(&b).unwrap().len() < 2000);
    }

    #[test]
    fn walk_is_sorted_and_skips_nothing_hidden() {
        let dir = std::env::temp_dir().join(format!("fp-direct-{}", std::process::id()));
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("b.csv"), "x\n1\n").unwrap();
        fs::write(dir.join(".hidden.csv"), "x\n1\n").unwrap();
        fs::write(dir.join("sub/a.csv"), "x\n1\n").unwrap();
        let records = build_records(std::slice::from_ref(&dir)).unwrap();
        let rel: Vec<&str> = records
            .iter()
            .map(|r| r["relative_path"].as_str().unwrap())
            .collect();
        assert_eq!(rel, vec![".hidden.csv", "b.csv", "sub/a.csv"]);
        assert!(records.iter().all(|r| r["version"] == "hash.v0"));
        let _ = fs::remove_dir_all(&dir);
    }
}
