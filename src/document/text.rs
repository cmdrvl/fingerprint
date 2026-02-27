use std::fs;
use std::path::{Path, PathBuf};

pub struct TextDocument {
    pub path: PathBuf,
    pub content: String,
    pub lines: Vec<String>,
}

impl TextDocument {
    /// Read plain text content and split into logical lines.
    pub fn open(path: &Path) -> Result<Self, String> {
        let content = fs::read_to_string(path)
            .map_err(|error| format!("failed to read text file '{}': {error}", path.display()))?;
        let lines = content.lines().map(str::to_owned).collect();
        Ok(Self {
            path: path.to_path_buf(),
            content,
            lines,
        })
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }
}

#[cfg(test)]
mod tests {
    use super::TextDocument;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn make_temp_file(contents: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().expect("create temp file");
        file.write_all(contents.as_bytes())
            .expect("write temp file contents");
        file.flush().expect("flush temp file");
        file
    }

    #[test]
    fn opens_file_and_splits_lines() {
        let file = make_temp_file("alpha\nbeta\ngamma");
        let document = TextDocument::open(file.path()).expect("open text document");

        assert_eq!(document.content(), "alpha\nbeta\ngamma");
        assert_eq!(
            document.lines(),
            &["alpha".to_string(), "beta".to_string(), "gamma".to_string()]
        );
    }

    #[test]
    fn reports_line_count() {
        let file = make_temp_file("one\ntwo\nthree");
        let document = TextDocument::open(file.path()).expect("open text document");

        assert_eq!(document.line_count(), 3);
    }

    #[test]
    fn opens_empty_file() {
        let file = make_temp_file("");
        let document = TextDocument::open(file.path()).expect("open text document");

        assert_eq!(document.content(), "");
        assert_eq!(document.lines(), &[] as &[String]);
        assert_eq!(document.line_count(), 0);
    }

    #[test]
    fn preserves_blank_lines_with_trailing_newlines() {
        let file = make_temp_file("first\n\nthird\n\n");
        let document = TextDocument::open(file.path()).expect("open text document");

        assert_eq!(document.content(), "first\n\nthird\n\n");
        assert_eq!(
            document.lines(),
            &[
                "first".to_string(),
                "".to_string(),
                "third".to_string(),
                "".to_string(),
            ]
        );
        assert_eq!(document.line_count(), 4);
    }
}
