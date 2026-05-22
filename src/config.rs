use serde_json::{Value, json};
use std::{
    env,
    ffi::OsString,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

const TOOL: &str = "fingerprint";

pub(crate) fn witness_path() -> PathBuf {
    witness_path_from_env(|key| env::var_os(key))
}

pub(crate) fn witness_path_for_append() -> Result<PathBuf, String> {
    witness_path_for_append_from_env(|key| env::var_os(key))
}

pub(crate) fn witness_path_for_query() -> Result<PathBuf, String> {
    ensure_witness_migrated_from_env(|key| env::var_os(key))?;
    Ok(witness_path())
}

pub(crate) fn trust_path() -> PathBuf {
    trust_path_from_env(|key| env::var_os(key))
}

pub(crate) fn trust_path_for_read() -> Result<PathBuf, String> {
    ensure_trust_migrated_from_env(|key| env::var_os(key))?;
    Ok(trust_path())
}

pub(crate) fn definitions_dir() -> PathBuf {
    definitions_dir_from_env(|key| env::var_os(key))
}

pub(crate) fn definitions_dir_for_read() -> Result<PathBuf, String> {
    ensure_definitions_migrated_from_env(|key| env::var_os(key))?;
    Ok(definitions_dir())
}

fn witness_path_from_env<F>(get_env: F) -> PathBuf
where
    F: Fn(&str) -> Option<OsString> + Copy,
{
    if let Some(path) = non_empty_env(get_env, "EPISTEMIC_WITNESS") {
        return PathBuf::from(path);
    }

    cmdrvl_root_from_env(get_env)
        .join("state")
        .join("witness")
        .join("witness.jsonl")
}

fn witness_path_for_append_from_env<F>(get_env: F) -> Result<PathBuf, String>
where
    F: Fn(&str) -> Option<OsString> + Copy,
{
    ensure_witness_migrated_from_env(get_env)?;
    let path = witness_path_from_env(get_env);
    if non_empty_env(get_env, "EPISTEMIC_WITNESS").is_none() {
        prepare_parent_from_env(get_env, &path)?;
    }
    Ok(path)
}

fn trust_path_from_env<F>(get_env: F) -> PathBuf
where
    F: Fn(&str) -> Option<OsString> + Copy,
{
    if let Some(path) = non_empty_env(get_env, "FINGERPRINT_TRUST") {
        return PathBuf::from(path);
    }

    cmdrvl_root_from_env(get_env)
        .join("config")
        .join("fingerprint")
        .join("trust.yaml")
}

fn definitions_dir_from_env<F>(get_env: F) -> PathBuf
where
    F: Fn(&str) -> Option<OsString> + Copy,
{
    if let Some(path) = non_empty_env(get_env, "FINGERPRINT_DEFINITIONS") {
        return PathBuf::from(path);
    }

    cmdrvl_root_from_env(get_env)
        .join("config")
        .join("fingerprint")
        .join("definitions")
}

fn ensure_witness_migrated_from_env<F>(get_env: F) -> Result<(), String>
where
    F: Fn(&str) -> Option<OsString> + Copy,
{
    if non_empty_env(get_env, "EPISTEMIC_WITNESS").is_some() {
        return Ok(());
    }

    migrate_file_from_env(
        get_env,
        "witness_ledger",
        &witness_path_from_env(get_env),
        legacy_witness_paths_from_env(get_env),
    )
}

fn ensure_trust_migrated_from_env<F>(get_env: F) -> Result<(), String>
where
    F: Fn(&str) -> Option<OsString> + Copy,
{
    if non_empty_env(get_env, "FINGERPRINT_TRUST").is_some() {
        return Ok(());
    }

    migrate_file_from_env(
        get_env,
        "fingerprint_trust_config",
        &trust_path_from_env(get_env),
        legacy_trust_paths_from_env(get_env),
    )
}

fn ensure_definitions_migrated_from_env<F>(get_env: F) -> Result<(), String>
where
    F: Fn(&str) -> Option<OsString> + Copy,
{
    if non_empty_env(get_env, "FINGERPRINT_DEFINITIONS").is_some() {
        return Ok(());
    }

    migrate_dir_from_env(
        get_env,
        "fingerprint_definitions",
        &definitions_dir_from_env(get_env),
        legacy_definition_dirs_from_env(get_env),
    )
}

fn migrate_file_from_env<F>(
    get_env: F,
    path_class: &str,
    canonical: &Path,
    legacy_paths: Vec<PathBuf>,
) -> Result<(), String>
where
    F: Fn(&str) -> Option<OsString> + Copy,
{
    let Some(legacy) = legacy_paths
        .into_iter()
        .find(|path| path != canonical && path.is_file())
    else {
        return Ok(());
    };

    let root = cmdrvl_root_from_env(get_env);
    let notice_path = root.join("notices").join("deprecated-paths.jsonl");
    let migration_path = root.join("migrations").join("applied.jsonl");

    if canonical.exists() {
        append_record_once(
            &notice_path,
            deprecation_record(
                path_class,
                &legacy,
                canonical,
                "legacy_path_present",
                "canonical_preferred",
            ),
        )?;
        return Ok(());
    }

    prepare_parent_from_env(get_env, canonical)?;
    fs::copy(&legacy, canonical).map_err(|error| {
        format!(
            "failed to copy legacy {path_class} '{}' to '{}': {error}",
            legacy.display(),
            canonical.display()
        )
    })?;
    preserve_permissions(&legacy, canonical)?;

    append_record_once(
        &migration_path,
        migration_record(path_class, &legacy, canonical, "copied_legacy_to_canonical"),
    )?;
    append_record_once(
        &notice_path,
        deprecation_record(
            path_class,
            &legacy,
            canonical,
            "legacy_path_migrated",
            "canonical_created",
        ),
    )?;

    Ok(())
}

fn migrate_dir_from_env<F>(
    get_env: F,
    path_class: &str,
    canonical: &Path,
    legacy_paths: Vec<PathBuf>,
) -> Result<(), String>
where
    F: Fn(&str) -> Option<OsString> + Copy,
{
    let Some(legacy) = legacy_paths
        .into_iter()
        .find(|path| path != canonical && path.is_dir())
    else {
        return Ok(());
    };

    let root = cmdrvl_root_from_env(get_env);
    let notice_path = root.join("notices").join("deprecated-paths.jsonl");
    let migration_path = root.join("migrations").join("applied.jsonl");

    if canonical.exists() {
        append_record_once(
            &notice_path,
            deprecation_record(
                path_class,
                &legacy,
                canonical,
                "legacy_path_present",
                "canonical_preferred",
            ),
        )?;
        return Ok(());
    }

    prepare_dir_from_env(get_env, canonical)?;
    copy_dir_recursive(&legacy, canonical).map_err(|error| {
        format!(
            "failed to copy legacy {path_class} '{}' to '{}': {error}",
            legacy.display(),
            canonical.display()
        )
    })?;

    append_record_once(
        &migration_path,
        migration_record(path_class, &legacy, canonical, "copied_legacy_to_canonical"),
    )?;
    append_record_once(
        &notice_path,
        deprecation_record(
            path_class,
            &legacy,
            canonical,
            "legacy_path_migrated",
            "canonical_created",
        ),
    )?;

    Ok(())
}

fn cmdrvl_root_from_env<F>(get_env: F) -> PathBuf
where
    F: Fn(&str) -> Option<OsString> + Copy,
{
    if let Some(home) =
        non_empty_env(get_env, "HOME").or_else(|| non_empty_env(get_env, "USERPROFILE"))
    {
        return PathBuf::from(home).join(".cmdrvl");
    }

    PathBuf::from(".cmdrvl")
}

fn non_empty_env<F>(get_env: F, key: &str) -> Option<OsString>
where
    F: Fn(&str) -> Option<OsString> + Copy,
{
    get_env(key).filter(|value| !value.is_empty())
}

fn legacy_witness_paths_from_env<F>(get_env: F) -> Vec<PathBuf>
where
    F: Fn(&str) -> Option<OsString> + Copy,
{
    let mut paths = Vec::new();

    if let Some(home) =
        non_empty_env(get_env, "HOME").or_else(|| non_empty_env(get_env, "USERPROFILE"))
    {
        paths.push(PathBuf::from(home).join(".epistemic").join("witness.jsonl"));
    }

    paths.push(PathBuf::from(".epistemic").join("witness.jsonl"));
    paths
}

fn legacy_trust_paths_from_env<F>(get_env: F) -> Vec<PathBuf>
where
    F: Fn(&str) -> Option<OsString> + Copy,
{
    let mut paths = Vec::new();

    if let Some(home) =
        non_empty_env(get_env, "HOME").or_else(|| non_empty_env(get_env, "USERPROFILE"))
    {
        let home = PathBuf::from(home);
        paths.push(home.join(".fingerprint").join("trust.yaml"));
        paths.push(home.join(".config").join("fingerprint").join("trust.yaml"));
        paths.push(home.join(".config").join("fingerprint").join("trust.yml"));
    }

    paths.push(PathBuf::from(".fingerprint").join("trust.yaml"));
    paths
}

fn legacy_definition_dirs_from_env<F>(get_env: F) -> Vec<PathBuf>
where
    F: Fn(&str) -> Option<OsString> + Copy,
{
    let mut paths = Vec::new();

    if let Some(home) =
        non_empty_env(get_env, "HOME").or_else(|| non_empty_env(get_env, "USERPROFILE"))
    {
        paths.push(PathBuf::from(home).join(".fingerprint").join("definitions"));
    }

    paths
}

fn prepare_parent_from_env<F>(get_env: F, path: &Path) -> Result<(), String>
where
    F: Fn(&str) -> Option<OsString> + Copy,
{
    let Some(parent) = path.parent() else {
        return Ok(());
    };

    prepare_dir_from_env(get_env, parent)
}

fn prepare_dir_from_env<F>(get_env: F, dir: &Path) -> Result<(), String>
where
    F: Fn(&str) -> Option<OsString> + Copy,
{
    let root = cmdrvl_root_from_env(get_env);
    fs::create_dir_all(&root)
        .map_err(|error| format!("failed to create cmdrvl root '{}': {error}", root.display()))?;
    harden_directory(&root)?;

    fs::create_dir_all(dir)
        .map_err(|error| format!("failed to create directory '{}': {error}", dir.display()))?;
    harden_directory(dir)
}

fn copy_dir_recursive(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::create_dir_all(destination)?;
    preserve_permissions_io(source, destination)?;

    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());

        if file_type.is_dir() {
            copy_dir_recursive(&source_path, &destination_path)?;
        } else if file_type.is_file() {
            fs::copy(&source_path, &destination_path)?;
            preserve_permissions_io(&source_path, &destination_path)?;
        }
    }

    Ok(())
}

fn preserve_permissions(source: &Path, destination: &Path) -> Result<(), String> {
    preserve_permissions_io(source, destination).map_err(|error| {
        format!(
            "failed to preserve permissions from '{}' to '{}': {error}",
            source.display(),
            destination.display()
        )
    })
}

fn preserve_permissions_io(source: &Path, destination: &Path) -> std::io::Result<()> {
    let permissions = fs::metadata(source)?.permissions();
    fs::set_permissions(destination, permissions)
}

fn migration_record(path_class: &str, source: &Path, destination: &Path, action: &str) -> Value {
    json!({
        "version": "cmdrvl.migration.v1",
        "tool": TOOL,
        "path_class": path_class,
        "source_path": source.display().to_string(),
        "destination_path": destination.display().to_string(),
        "action": action,
        "outcome": "ok",
        "secret_values_recorded": false
    })
}

fn deprecation_record(
    path_class: &str,
    source: &Path,
    destination: &Path,
    action: &str,
    outcome: &str,
) -> Value {
    json!({
        "version": "cmdrvl.deprecated_path_notice.v1",
        "tool": TOOL,
        "path_class": path_class,
        "source_path": source.display().to_string(),
        "destination_path": destination.display().to_string(),
        "action": action,
        "outcome": outcome,
        "secret_values_recorded": false
    })
}

fn append_record_once(path: &Path, record: Value) -> Result<(), String> {
    if record_already_exists(path, &record)? {
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "failed to create migration record directory '{}': {error}",
                parent.display()
            )
        })?;
        harden_directory(parent)?;
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| {
            format!(
                "failed to open migration record '{}': {error}",
                path.display()
            )
        })?;
    writeln!(file, "{record}").map_err(|error| {
        format!(
            "failed to append migration record '{}': {error}",
            path.display()
        )
    })
}

fn record_already_exists(path: &Path, record: &Value) -> Result<bool, String> {
    let Ok(contents) = fs::read_to_string(path) else {
        return Ok(false);
    };

    Ok(contents.lines().any(|line| {
        let Ok(existing) = serde_json::from_str::<Value>(line) else {
            return false;
        };

        existing.get("tool") == record.get("tool")
            && existing.get("path_class") == record.get("path_class")
            && existing.get("source_path") == record.get("source_path")
            && existing.get("destination_path") == record.get("destination_path")
            && existing.get("action") == record.get("action")
    }))
}

#[cfg(unix)]
fn harden_directory(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("failed to harden directory '{}': {error}", path.display()))
}

#[cfg(not(unix))]
fn harden_directory(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        definitions_dir_from_env, ensure_definitions_migrated_from_env,
        ensure_trust_migrated_from_env, trust_path_from_env, witness_path_for_append_from_env,
        witness_path_from_env,
    };
    use std::{
        ffi::OsString,
        fs,
        path::{Path, PathBuf},
    };
    use tempfile::TempDir;

    fn env_for_home(home: &Path) -> impl Fn(&str) -> Option<OsString> + Copy + '_ {
        |key| match key {
            "HOME" => Some(home.as_os_str().to_owned()),
            "USERPROFILE" => None,
            "EPISTEMIC_WITNESS" => None,
            "FINGERPRINT_TRUST" => None,
            "FINGERPRINT_DEFINITIONS" => None,
            _ => None,
        }
    }

    #[test]
    fn witness_defaults_to_cmdrvl_root() {
        let path = witness_path_from_env(|key| match key {
            "HOME" => Some(OsString::from("/tmp/home")),
            _ => None,
        });

        assert_eq!(
            path,
            PathBuf::from("/tmp/home/.cmdrvl/state/witness/witness.jsonl")
        );
    }

    #[test]
    fn explicit_witness_override_wins() {
        let path = witness_path_from_env(|key| match key {
            "EPISTEMIC_WITNESS" => Some(OsString::from("/tmp/custom.jsonl")),
            "HOME" => Some(OsString::from("/tmp/home")),
            _ => None,
        });

        assert_eq!(path, PathBuf::from("/tmp/custom.jsonl"));
    }

    #[test]
    fn trust_and_definitions_default_to_cmdrvl_config() {
        let trust = trust_path_from_env(|key| match key {
            "HOME" => Some(OsString::from("/tmp/home")),
            _ => None,
        });
        let definitions = definitions_dir_from_env(|key| match key {
            "HOME" => Some(OsString::from("/tmp/home")),
            _ => None,
        });

        assert_eq!(
            trust,
            PathBuf::from("/tmp/home/.cmdrvl/config/fingerprint/trust.yaml")
        );
        assert_eq!(
            definitions,
            PathBuf::from("/tmp/home/.cmdrvl/config/fingerprint/definitions")
        );
    }

    #[test]
    fn witness_append_migrates_legacy_ledger() {
        let temp = TempDir::new().expect("temp dir");
        let home = temp.path().to_path_buf();
        let legacy = home.join(".epistemic").join("witness.jsonl");
        fs::create_dir_all(legacy.parent().expect("legacy parent")).expect("legacy parent");
        fs::write(&legacy, "{\"id\":\"old\"}\n").expect("legacy ledger");

        let canonical = witness_path_for_append_from_env(env_for_home(&home))
            .expect("witness migration should succeed");

        assert_eq!(canonical, home.join(".cmdrvl/state/witness/witness.jsonl"));
        assert_eq!(
            fs::read_to_string(&canonical).expect("canonical ledger"),
            "{\"id\":\"old\"}\n"
        );
        assert!(home.join(".cmdrvl/migrations/applied.jsonl").exists());
        assert!(home.join(".cmdrvl/notices/deprecated-paths.jsonl").exists());
    }

    #[test]
    fn trust_migration_prefers_existing_canonical_without_overwrite() {
        let temp = TempDir::new().expect("temp dir");
        let home = temp.path().to_path_buf();
        let legacy = home.join(".fingerprint").join("trust.yaml");
        let canonical = home.join(".cmdrvl/config/fingerprint/trust.yaml");
        fs::create_dir_all(legacy.parent().expect("legacy parent")).expect("legacy parent");
        fs::create_dir_all(canonical.parent().expect("canonical parent"))
            .expect("canonical parent");
        fs::write(&legacy, "trust:\n  - legacy\n").expect("legacy trust");
        fs::write(&canonical, "trust:\n  - canonical\n").expect("canonical trust");

        ensure_trust_migrated_from_env(env_for_home(&home))
            .expect("trust migration should succeed");

        assert_eq!(
            fs::read_to_string(&canonical).expect("canonical trust"),
            "trust:\n  - canonical\n"
        );
        let notice = fs::read_to_string(home.join(".cmdrvl/notices/deprecated-paths.jsonl"))
            .expect("notice");
        assert!(notice.contains("canonical_preferred"));
    }

    #[test]
    fn definitions_directory_migrates_recursively() {
        let temp = TempDir::new().expect("temp dir");
        let home = temp.path().to_path_buf();
        let legacy = home.join(".fingerprint").join("definitions");
        fs::create_dir_all(legacy.join("nested")).expect("legacy definitions");
        fs::write(legacy.join("sample.fp.yaml"), "fingerprint_id: sample\n")
            .expect("legacy definition");
        fs::write(
            legacy.join("nested").join("child.fp.yaml"),
            "fingerprint_id: child\n",
        )
        .expect("nested definition");

        ensure_definitions_migrated_from_env(env_for_home(&home))
            .expect("definitions migration should succeed");

        let canonical = home.join(".cmdrvl/config/fingerprint/definitions");
        assert!(canonical.join("sample.fp.yaml").exists());
        assert!(canonical.join("nested").join("child.fp.yaml").exists());
        let migration =
            fs::read_to_string(home.join(".cmdrvl/migrations/applied.jsonl")).expect("migration");
        assert!(migration.contains("fingerprint_definitions"));
    }
}
