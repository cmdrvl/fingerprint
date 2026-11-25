#![forbid(unsafe_code)]

//! Name redaction for `fingerprint peek` (bd-zwx, operator decision: Option 2).
//!
//! `peek` never prints a column, sheet, heading, element, field or metadata-key NAME unless
//! `--explicit` is given. Where it needs to say "the same name again" it prints a marker:
//!
//! - **Ordinal refs (default).** `redacted:ref:c1`, `c2`, ... numbered in the order the names
//!   first appear in the DATA (file order), never in name order. Equal names inside ONE output
//!   share a ref; each kind of name (column, sheet, heading, path, metadata key, outline title,
//!   namespace) has its own numbering. No hash of any name is computed, so nothing can be
//!   guessed from a dictionary. A ref reveals only the equality pattern inside this one output.
//!   Refs are not comparable across files. Output is byte-identical when the same file is peeked
//!   again with the same flags.
//! - **Keyed markers (opt-in).** When `FINGERPRINT_REDACTION_KEY` holds a key of at least 16
//!   bytes, a name becomes `redacted:hmac-sha256:<32 hex>`: the first 128 bits of
//!   HMAC-SHA256(key, `<kind>` NUL `<name>`), with `<kind>` one of `column`, `sheet`, `heading`,
//!   `path`, `metadata_key`, `outline`, `namespace`. Stable across files and runs for the key
//!   holder (that is the correlation feature); unguessable without the key. The kind prefix is
//!   domain separation: a column and a JSON path with the same spelling get different markers,
//!   so a key holder cannot correlate across kinds. A set-but-empty or too-short key is refused
//!   (`E_REDACTION_KEY_INVALID`), never silently downgraded to ordinal refs.
//!
//! What a key holder CAN learn: that two names (of the same kind) in different outputs are equal,
//! and, by hashing candidate names with the key, whether a candidate name is present. What a
//! key holder (and anyone without the key) cannot learn from a marker alone: the name. What
//! this is NOT: text lengths (bucketed), counts, depths, types and the format of the file are
//! not redacted, and a peek is not anonymous against someone who also holds the key and the
//! candidate names.

use std::collections::BTreeMap;
use std::ffi::OsString;

use hmac::{Hmac, Mac};
use sha2::Sha256;

pub const ENV_KEY: &str = "FINGERPRINT_REDACTION_KEY";
pub const MIN_KEY_BYTES: usize = 16;
pub const REF_PREFIX: &str = "redacted:ref:";
pub const HMAC_PREFIX: &str = "redacted:hmac-sha256:";
/// The typed refusal `peek` emits (exit 2) for an empty or too-short key. Listed in operator.json.
pub const REFUSAL_CODE: &str = "E_REDACTION_KEY_INVALID";

/// Why `FINGERPRINT_REDACTION_KEY` was refused. Never carries the key or its length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyError {
    Empty,
    TooShort,
}

impl KeyError {
    pub const fn reason(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::TooShort => "too_short",
        }
    }
}

/// An operator-held redaction key. `Debug` never prints the bytes.
#[derive(Clone)]
pub struct RedactionKey(Vec<u8>);

impl std::fmt::Debug for RedactionKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RedactionKey(<withheld>)")
    }
}

impl RedactionKey {
    /// `None` when the variable is unset (ordinal refs). Set-but-empty and too-short values are
    /// errors, so a misconfigured key cannot quietly fall back.
    pub fn from_env_value(value: Option<OsString>) -> Result<Option<Self>, KeyError> {
        let Some(value) = value else {
            return Ok(None);
        };
        #[cfg(unix)]
        let bytes = std::os::unix::ffi::OsStringExt::into_vec(value);
        #[cfg(not(unix))]
        let bytes = value.to_string_lossy().into_owned().into_bytes();
        if bytes.is_empty() {
            return Err(KeyError::Empty);
        }
        if bytes.len() < MIN_KEY_BYTES {
            return Err(KeyError::TooShort);
        }
        Ok(Some(Self(bytes)))
    }

    pub fn from_env() -> Result<Option<Self>, KeyError> {
        Self::from_env_value(std::env::var_os(ENV_KEY))
    }

    /// `redacted:hmac-sha256:<32 hex>` over `kind NUL name`.
    pub fn marker(&self, kind: Kind, name: &str) -> String {
        let mut mac =
            Hmac::<Sha256>::new_from_slice(&self.0).expect("HMAC accepts a key of any length");
        mac.update(kind.tag().as_bytes());
        mac.update(&[0]);
        mac.update(name.as_bytes());
        let digest = mac.finalize().into_bytes();
        let mut hex = String::with_capacity(32);
        for byte in &digest[..16] {
            hex.push_str(&format!("{byte:02x}"));
        }
        format!("{HMAC_PREFIX}{hex}")
    }
}

/// What kind of name is being marked: each kind is its own ordinal numbering and its own HMAC
/// domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Column,
    Sheet,
    Heading,
    Path,
    MetadataKey,
    Outline,
    Namespace,
}

impl Kind {
    /// HMAC domain separator.
    pub const fn tag(self) -> &'static str {
        match self {
            Self::Column => "column",
            Self::Sheet => "sheet",
            Self::Heading => "heading",
            Self::Path => "path",
            Self::MetadataKey => "metadata_key",
            Self::Outline => "outline",
            Self::Namespace => "namespace",
        }
    }

    const fn letter(self) -> char {
        match self {
            Self::Column => 'c',
            Self::Sheet => 's',
            Self::Heading => 'h',
            Self::Path => 'p',
            Self::MetadataKey => 'k',
            Self::Outline => 'o',
            Self::Namespace => 'n',
        }
    }
}

/// Turns names into markers for ONE output. Mark names in the order they appear in the data.
pub struct Marker<'a> {
    key: Option<&'a RedactionKey>,
    seen: BTreeMap<(Kind, String), usize>,
    counters: BTreeMap<Kind, usize>,
}

impl<'a> Marker<'a> {
    pub fn new(key: Option<&'a RedactionKey>) -> Self {
        Self {
            key,
            seen: BTreeMap::new(),
            counters: BTreeMap::new(),
        }
    }

    /// `"keyed"` or `"ordinal"`: recorded in the output and the witness (never the key).
    pub fn mode(&self) -> &'static str {
        if self.key.is_some() {
            "keyed"
        } else {
            "ordinal"
        }
    }

    pub fn mark(&mut self, kind: Kind, name: &str) -> String {
        if let Some(key) = self.key {
            return key.marker(kind, name);
        }
        let next = self.counters.entry(kind).or_insert(0);
        let ordinal = *self.seen.entry((kind, name.to_owned())).or_insert_with(|| {
            *next += 1;
            *next
        });
        format!("{REF_PREFIX}{}{ordinal}", kind.letter())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn ordinals_follow_first_appearance_not_name_order_and_equal_names_share() {
        let mut marker = Marker::new(None);
        assert_eq!(marker.mark(Kind::Column, "zebra"), "redacted:ref:c1");
        assert_eq!(marker.mark(Kind::Column, "apple"), "redacted:ref:c2");
        assert_eq!(marker.mark(Kind::Column, "zebra"), "redacted:ref:c1");
        // Separate numbering per kind.
        assert_eq!(marker.mark(Kind::Path, "zebra"), "redacted:ref:p1");
    }

    #[test]
    fn keys_are_validated_without_leaking_length_or_value() {
        let make = |v: &str| RedactionKey::from_env_value(Some(OsString::from(v)));
        assert_eq!(make("").unwrap_err(), KeyError::Empty);
        assert_eq!(make("0123456789abcde").unwrap_err(), KeyError::TooShort);
        assert!(make("0123456789abcdef").unwrap().is_some());
        assert!(RedactionKey::from_env_value(None).unwrap().is_none());
        let key = make("0123456789abcdef").unwrap().unwrap();
        assert!(!format!("{key:?}").contains("0123456789"));
    }

    #[test]
    fn keyed_markers_separate_kinds_and_change_with_the_key() {
        let a = RedactionKey::from_env_value(Some(OsString::from("0123456789abcdef")))
            .unwrap()
            .unwrap();
        let b = RedactionKey::from_env_value(Some(OsString::from("fedcba9876543210")))
            .unwrap()
            .unwrap();
        assert_ne!(a.marker(Kind::Column, "x"), a.marker(Kind::Path, "x"));
        assert_ne!(a.marker(Kind::Column, "x"), b.marker(Kind::Column, "x"));
        assert_eq!(a.marker(Kind::Column, "x"), a.marker(Kind::Column, "x"));
    }
}
