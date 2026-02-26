use crate::registry::Fingerprint;

/// Discover installed fingerprint crates via cargo install paths.
pub fn discover_installed() -> Vec<Box<dyn Fingerprint>> {
    todo!()
}
