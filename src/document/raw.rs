use crate::document::RawDocument;
use std::fs;
use std::path::Path;

impl RawDocument {
    /// Read raw bytes from a file.
    pub fn open(path: &Path) -> Result<Self, String> {
        let bytes = fs::read(path)
            .map_err(|error| format!("failed to read file '{}': {error}", path.display()))?;

        Ok(RawDocument {
            path: path.to_path_buf(),
            bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn make_temp_file(contents: &[u8]) -> NamedTempFile {
        let mut file = NamedTempFile::new().expect("create temp file");
        file.write_all(contents).expect("write temp file contents");
        file.flush().expect("flush temp file");
        file
    }

    #[test]
    fn opens_file_and_reads_bytes() {
        let content = b"Hello, world!";
        let file = make_temp_file(content);
        let document = RawDocument::open(file.path()).expect("open raw document");

        assert_eq!(document.bytes, content);
        assert_eq!(document.path, file.path());
    }

    #[test]
    fn opens_empty_file() {
        let file = make_temp_file(b"");
        let document = RawDocument::open(file.path()).expect("open raw document");

        assert_eq!(document.bytes, b"");
        assert_eq!(document.path, file.path());
    }

    #[test]
    fn opens_binary_file() {
        let content = b"\x00\x01\x02\x03\xFF\xFE\xFD";
        let file = make_temp_file(content);
        let document = RawDocument::open(file.path()).expect("open raw document");

        assert_eq!(document.bytes, content);
    }

    #[test]
    fn reports_error_for_nonexistent_file() {
        let result = RawDocument::open(Path::new("/nonexistent/file.bin"));
        let error = match result {
            Ok(_) => panic!("expected missing file error"),
            Err(error) => error,
        };
        assert!(error.contains("failed to read file"));
    }
}
