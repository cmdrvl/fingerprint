use crate::registry::Fingerprint;

/// Discover installed fingerprint crates via cargo install paths.
pub fn discover_installed() -> Vec<Box<dyn Fingerprint>> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::discover_installed;

    #[test]
    fn discover_installed_is_empty_stub_for_v0_1() {
        assert!(discover_installed().is_empty());
    }
}
