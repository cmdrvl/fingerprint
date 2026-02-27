use crate::document::PdfDocument;
use lopdf::Object;
use std::path::Path;

impl PdfDocument {
    /// Open a PDF file for structural access and optional text_path markdown.
    pub fn open(path: &Path, text_path: Option<&Path>) -> Result<Self, String> {
        let text = text_path
            .map(crate::document::MarkdownDocument::open)
            .transpose()?;
        Ok(Self {
            path: path.to_path_buf(),
            text,
        })
    }

    /// Return PDF page count using structural access.
    pub fn page_count(&self) -> Result<u64, String> {
        let document = lopdf::Document::load(&self.path)
            .map_err(|error| format!("failed reading pdf '{}': {error}", self.path.display()))?;
        Ok(document.get_pages().len() as u64)
    }

    /// Return metadata key/value pairs from trailer Info dictionary.
    pub fn metadata(&self) -> Result<Vec<(String, String)>, String> {
        let document = lopdf::Document::load(&self.path)
            .map_err(|error| format!("failed reading pdf '{}': {error}", self.path.display()))?;
        let info_object = document
            .trailer
            .get(b"Info")
            .map_err(|error| format!("missing Info dictionary in trailer: {error}"))?;

        let info_dictionary = match info_object {
            Object::Reference(object_id) => document
                .get_object(*object_id)
                .map_err(|error| format!("unable to resolve Info dictionary reference: {error}"))?,
            object => object,
        };

        let dictionary = info_dictionary
            .as_dict()
            .map_err(|error| format!("Info object is not a dictionary: {error}"))?;

        let mut metadata = Vec::new();
        for (name, object) in dictionary {
            let key = String::from_utf8_lossy(name).to_string();
            let value = pdf_object_as_string(&document, object)?;
            metadata.push((key, value));
        }
        metadata.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(metadata)
    }

    /// Return a metadata value by key (case-insensitive).
    pub fn metadata_value(&self, key: &str) -> Result<Option<String>, String> {
        let metadata = self.metadata()?;
        Ok(metadata
            .iter()
            .find_map(|(candidate, value)| candidate.eq_ignore_ascii_case(key).then_some(value))
            .cloned())
    }
}

fn pdf_object_as_string(document: &lopdf::Document, object: &Object) -> Result<String, String> {
    match object {
        Object::String(bytes, _) => Ok(String::from_utf8_lossy(bytes).to_string()),
        Object::Name(bytes) => Ok(String::from_utf8_lossy(bytes).to_string()),
        Object::Integer(value) => Ok(value.to_string()),
        Object::Real(value) => Ok(value.to_string()),
        Object::Boolean(value) => Ok(value.to_string()),
        Object::Reference(object_id) => {
            let resolved = document
                .get_object(*object_id)
                .map_err(|error| format!("unable to resolve metadata reference: {error}"))?;
            pdf_object_as_string(document, resolved)
        }
        _ => Ok(format!("{object:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn make_temp_file_with_suffix(contents: &str, suffix: &str) -> NamedTempFile {
        let mut file = NamedTempFile::with_suffix(suffix).expect("create temp file with suffix");
        file.write_all(contents.as_bytes())
            .expect("write temp file contents");
        file.flush().expect("flush temp file");
        file
    }

    #[test]
    fn opens_pdf_without_text_path() {
        let pdf = make_temp_file_with_suffix("%PDF-1.4\n", ".pdf");
        let document = PdfDocument::open(pdf.path(), None).expect("open pdf document");

        assert_eq!(document.path, pdf.path());
        assert!(document.text.is_none());
    }

    #[test]
    fn opens_pdf_with_markdown_text_path() {
        let pdf = make_temp_file_with_suffix("%PDF-1.4\n", ".pdf");
        let markdown = make_temp_file_with_suffix("# Heading\n\nBody", ".md");
        let document =
            PdfDocument::open(pdf.path(), Some(markdown.path())).expect("open pdf document");

        assert_eq!(document.path, pdf.path());
        let text = document.text.expect("text markdown should be loaded");
        assert_eq!(text.path, markdown.path());
        assert_eq!(text.headings.len(), 1);
        assert_eq!(text.headings[0].text, "Heading");
    }

    fn write_minimal_pdf_with_metadata() -> NamedTempFile {
        let file = NamedTempFile::with_suffix(".pdf").expect("create pdf temp file");
        let mut document = lopdf::Document::with_version("1.5");

        let pages_id = document.new_object_id();
        let page_id = document.new_object_id();
        let content_id = document.add_object(lopdf::Stream::new(
            lopdf::Dictionary::new(),
            b"BT ET".to_vec(),
        ));

        document.objects.insert(
            page_id,
            Object::Dictionary(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Contents" => content_id,
                "MediaBox" => vec![0.into(), 0.into(), 300.into(), 300.into()],
            }),
        );
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_id.into()],
                "Count" => 1,
            }),
        );

        let info_id = document.add_object(dictionary! {
            "Producer" => Object::string_literal("fingerprint-test"),
            "Title" => Object::string_literal("Test PDF"),
        });
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        document.trailer.set("Root", catalog_id);
        document.trailer.set("Info", info_id);
        document.compress();
        document
            .save(file.path())
            .expect("write minimal pdf with metadata");

        file
    }

    #[test]
    fn page_count_and_metadata_queries_work() {
        let pdf = write_minimal_pdf_with_metadata();
        let document = PdfDocument::open(pdf.path(), None).expect("open pdf document");

        let page_count = document.page_count().expect("read page count");
        assert_eq!(page_count, 1);

        let metadata = document.metadata().expect("read metadata");
        assert!(
            metadata
                .iter()
                .any(|(k, v)| k == "Producer" && v == "fingerprint-test")
        );
        assert!(
            metadata
                .iter()
                .any(|(k, v)| k == "Title" && v == "Test PDF")
        );

        let producer = document
            .metadata_value("producer")
            .expect("read metadata value");
        assert_eq!(producer.as_deref(), Some("fingerprint-test"));
    }

    #[test]
    fn metadata_access_fails_for_non_pdf_bytes() {
        let file = make_temp_file_with_suffix("not-a-pdf", ".pdf");
        let document = PdfDocument::open(file.path(), None).expect("open pdf wrapper");

        assert!(document.page_count().is_err());
        assert!(document.metadata().is_err());
    }
}
