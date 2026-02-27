use crate::document::CsvDocument;
use std::path::Path;

impl CsvDocument {
    /// Open a CSV file for header + streaming record access.
    pub fn open(path: &Path) -> Result<Self, String> {
        let mut reader = csv::Reader::from_path(path)
            .map_err(|error| format!("failed to open CSV '{}': {error}", path.display()))?;
        let _headers = reader
            .headers()
            .map_err(|error| format!("failed to read CSV headers '{}': {error}", path.display()))?;

        Ok(Self {
            path: path.to_path_buf(),
        })
    }

    /// Return CSV headers in source order.
    pub fn headers(&self) -> Result<Vec<String>, String> {
        let mut reader = csv::Reader::from_path(&self.path)
            .map_err(|error| format!("failed to open CSV '{}': {error}", self.path.display()))?;
        let headers = reader
            .headers()
            .map_err(|error| {
                format!(
                    "failed to read CSV headers '{}': {error}",
                    self.path.display()
                )
            })?
            .iter()
            .map(str::to_owned)
            .collect();
        Ok(headers)
    }

    /// Return all CSV rows (excluding header row).
    pub fn rows(&self) -> Result<Vec<Vec<String>>, String> {
        let mut reader = csv::Reader::from_path(&self.path)
            .map_err(|error| format!("failed to open CSV '{}': {error}", self.path.display()))?;
        let mut rows = Vec::new();

        for record in reader.records() {
            let record = record.map_err(|error| {
                format!(
                    "failed to read CSV record from '{}': {error}",
                    self.path.display()
                )
            })?;
            rows.push(record.iter().map(str::to_owned).collect());
        }

        Ok(rows)
    }

    /// Return a single cell value by row index and header name.
    pub fn cell_by_column(
        &self,
        row_index: usize,
        column_name: &str,
    ) -> Result<Option<String>, String> {
        let headers = self.headers()?;
        let Some(column_index) = headers.iter().position(|header| header == column_name) else {
            return Err(format!(
                "column '{}' not found in CSV '{}'",
                column_name,
                self.path.display()
            ));
        };

        let mut reader = csv::Reader::from_path(&self.path)
            .map_err(|error| format!("failed to open CSV '{}': {error}", self.path.display()))?;
        for (index, record) in reader.records().enumerate() {
            let record = record.map_err(|error| {
                format!(
                    "failed to read CSV record from '{}': {error}",
                    self.path.display()
                )
            })?;
            if index == row_index {
                return Ok(record.get(column_index).map(str::to_owned));
            }
        }

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::CsvDocument;
    use std::fs;
    use tempfile::NamedTempFile;

    fn write_csv(contents: &str) -> NamedTempFile {
        let file = NamedTempFile::with_suffix(".csv").expect("create csv temp file");
        fs::write(file.path(), contents).expect("write csv fixture");
        file
    }

    #[test]
    fn opens_csv_and_reads_headers() {
        let file = write_csv("name,city\nAda,London\n");
        let doc = CsvDocument::open(file.path()).expect("open CSV");
        let headers = doc.headers().expect("read headers");
        assert_eq!(headers, vec!["name", "city"]);
    }

    #[test]
    fn reads_rows_without_header() {
        let file = write_csv("name,city\nAda,London\nBob,Paris\n");
        let doc = CsvDocument::open(file.path()).expect("open CSV");
        let rows = doc.rows().expect("read rows");

        assert_eq!(
            rows,
            vec![
                vec!["Ada".to_owned(), "London".to_owned()],
                vec!["Bob".to_owned(), "Paris".to_owned()]
            ]
        );
    }

    #[test]
    fn accesses_cell_by_header_name() {
        let file = write_csv("name,city\nAda,London\nBob,Paris\n");
        let doc = CsvDocument::open(file.path()).expect("open CSV");

        let city = doc
            .cell_by_column(1, "city")
            .expect("read cell")
            .expect("row exists");
        assert_eq!(city, "Paris");
    }

    #[test]
    fn returns_none_for_out_of_range_row() {
        let file = write_csv("name,city\nAda,London\n");
        let doc = CsvDocument::open(file.path()).expect("open CSV");
        let value = doc.cell_by_column(3, "city").expect("query cell");
        assert_eq!(value, None);
    }

    #[test]
    fn errors_for_missing_column_name() {
        let file = write_csv("name,city\nAda,London\n");
        let doc = CsvDocument::open(file.path()).expect("open CSV");
        let error = doc
            .cell_by_column(0, "country")
            .expect_err("missing column should fail");
        assert!(error.contains("column 'country' not found"));
    }
}
