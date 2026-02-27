use crate::document::XlsxDocument;
use calamine::{Reader, open_workbook_auto};
use std::path::Path;

type CellRef = (usize, usize);
type CellRange = (CellRef, CellRef);

impl XlsxDocument {
    /// Open an XLSX file for lazy sheet access via calamine.
    pub fn open(path: &Path) -> Result<Self, String> {
        open_workbook_auto(path)
            .map_err(|error| format!("failed to open xlsx '{}': {error}", path.display()))?;
        Ok(Self {
            path: path.to_path_buf(),
        })
    }

    /// List sheet names in workbook order.
    pub fn sheet_names(&self) -> Result<Vec<String>, String> {
        let workbook = open_workbook_auto(&self.path)
            .map_err(|error| format!("failed to open xlsx '{}': {error}", self.path.display()))?;
        Ok(workbook.sheet_names().to_vec())
    }

    /// Read a single cell by A1-style address.
    pub fn read_cell(&self, sheet: &str, cell: &str) -> Result<Option<String>, String> {
        let position = parse_cell_ref(cell)?;
        let mut workbook = open_workbook_auto(&self.path)
            .map_err(|error| format!("failed to open xlsx '{}': {error}", self.path.display()))?;
        let worksheet = workbook
            .worksheet_range(sheet)
            .map_err(|error| format!("failed to read sheet '{sheet}': {error}"))?;

        Ok(worksheet
            .get_value((position.0 as u32, position.1 as u32))
            .map(|value| value.to_string())
            .filter(|value| !value.trim().is_empty()))
    }

    /// Read a rectangular range by A1 notation (e.g. "A1:C3").
    pub fn read_range(&self, sheet: &str, range: &str) -> Result<Vec<Vec<Option<String>>>, String> {
        let (start, end) = parse_range_ref(range)?;
        let mut workbook = open_workbook_auto(&self.path)
            .map_err(|error| format!("failed to open xlsx '{}': {error}", self.path.display()))?;
        let worksheet = workbook
            .worksheet_range(sheet)
            .map_err(|error| format!("failed to read sheet '{sheet}': {error}"))?;

        let mut rows = Vec::new();
        for row in start.0..=end.0 {
            let mut cells = Vec::new();
            for col in start.1..=end.1 {
                let value = worksheet
                    .get_value((row as u32, col as u32))
                    .map(|cell| cell.to_string())
                    .filter(|cell| !cell.trim().is_empty());
                cells.push(value);
            }
            rows.push(cells);
        }

        Ok(rows)
    }

    /// Count non-empty rows in a sheet.
    pub fn sheet_row_count(&self, sheet: &str) -> Result<usize, String> {
        let mut workbook = open_workbook_auto(&self.path)
            .map_err(|error| format!("failed to open xlsx '{}': {error}", self.path.display()))?;
        let worksheet = workbook
            .worksheet_range(sheet)
            .map_err(|error| format!("failed to read sheet '{sheet}': {error}"))?;

        Ok(worksheet
            .rows()
            .filter(|row| row.iter().any(|cell| !cell.to_string().trim().is_empty()))
            .count())
    }
}

fn parse_cell_ref(cell: &str) -> Result<CellRef, String> {
    let mut letters = String::new();
    let mut digits = String::new();

    for character in cell.chars() {
        if character.is_ascii_alphabetic() {
            if !digits.is_empty() {
                return Err(format!("invalid cell reference '{cell}'"));
            }
            letters.push(character);
        } else if character.is_ascii_digit() {
            digits.push(character);
        } else {
            return Err(format!("invalid cell reference '{cell}'"));
        }
    }

    if letters.is_empty() || digits.is_empty() {
        return Err(format!("invalid cell reference '{cell}'"));
    }

    let mut column: usize = 0;
    for character in letters.chars() {
        let upper = character.to_ascii_uppercase();
        if !upper.is_ascii_uppercase() {
            return Err(format!("invalid column reference in '{cell}'"));
        }
        column = column.saturating_mul(26) + (upper as usize - 'A' as usize + 1);
    }

    let row: usize = digits
        .parse()
        .map_err(|error| format!("invalid row in cell reference '{cell}': {error}"))?;
    if row == 0 {
        return Err(format!("row number must be >= 1 in '{cell}'"));
    }

    Ok((row - 1, column - 1))
}

fn parse_range_ref(range: &str) -> Result<CellRange, String> {
    let (left, right) = range
        .split_once(':')
        .ok_or_else(|| format!("invalid range reference '{range}'"))?;
    let start = parse_cell_ref(left)?;
    let end = parse_cell_ref(right)?;

    Ok((
        (start.0.min(end.0), start.1.min(end.1)),
        (start.0.max(end.0), start.1.max(end.1)),
    ))
}

#[cfg(test)]
mod tests {
    use super::XlsxDocument;
    use std::fs;
    use tempfile::NamedTempFile;

    // Minimal workbook with one sheet ("Sheet1") and values:
    // A1="Header", A2="Value", B2=42.
    const MINIMAL_XLSX_BASE64: &str = "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";

    fn decode_base64(input: &str) -> Vec<u8> {
        fn decode_char(character: u8) -> Option<u8> {
            match character {
                b'A'..=b'Z' => Some(character - b'A'),
                b'a'..=b'z' => Some(character - b'a' + 26),
                b'0'..=b'9' => Some(character - b'0' + 52),
                b'+' => Some(62),
                b'/' => Some(63),
                _ => None,
            }
        }

        let bytes = input.as_bytes();
        let mut output = Vec::new();
        for chunk in bytes.chunks(4) {
            if chunk.len() < 4 {
                break;
            }

            let pad = chunk.iter().rev().take_while(|byte| **byte == b'=').count();
            let a = decode_char(chunk[0]).expect("valid base64");
            let b = decode_char(chunk[1]).expect("valid base64");
            let c = if chunk[2] == b'=' {
                0
            } else {
                decode_char(chunk[2]).expect("valid base64")
            };
            let d = if chunk[3] == b'=' {
                0
            } else {
                decode_char(chunk[3]).expect("valid base64")
            };

            let triple = ((a as u32) << 18) | ((b as u32) << 12) | ((c as u32) << 6) | (d as u32);
            output.push(((triple >> 16) & 0xff) as u8);
            if pad < 2 {
                output.push(((triple >> 8) & 0xff) as u8);
            }
            if pad < 1 {
                output.push((triple & 0xff) as u8);
            }
        }

        output
    }

    fn write_minimal_xlsx() -> NamedTempFile {
        let file = NamedTempFile::with_suffix(".xlsx").expect("create xlsx temp file");
        let bytes = decode_base64(MINIMAL_XLSX_BASE64);
        fs::write(file.path(), bytes).expect("write xlsx bytes");
        file
    }

    #[test]
    fn open_rejects_missing_file() {
        let result = XlsxDocument::open(std::path::Path::new("/tmp/does-not-exist.xlsx"));
        assert!(result.is_err());
    }

    #[test]
    fn open_rejects_malformed_file() {
        let file = NamedTempFile::with_suffix(".xlsx").expect("create malformed xlsx temp file");
        fs::write(file.path(), "not an xlsx").expect("write malformed data");
        let result = XlsxDocument::open(file.path());
        assert!(result.is_err());
    }

    #[test]
    fn lists_sheet_names_and_reads_cells_and_ranges() {
        let file = write_minimal_xlsx();
        let doc = XlsxDocument::open(file.path()).expect("open xlsx");

        let names = doc.sheet_names().expect("list sheet names");
        assert_eq!(names, vec!["Sheet1".to_owned()]);

        let a1 = doc.read_cell("Sheet1", "A1").expect("read A1");
        let b1 = doc.read_cell("Sheet1", "B1").expect("read B1");
        let b2 = doc.read_cell("Sheet1", "B2").expect("read B2");
        assert_eq!(a1.as_deref(), Some("Header"));
        assert_eq!(b1, None);
        assert_eq!(b2.as_deref(), Some("42"));

        let range = doc.read_range("Sheet1", "A1:B2").expect("read range");
        assert_eq!(
            range,
            vec![
                vec![Some("Header".to_owned()), None],
                vec![Some("Value".to_owned()), Some("42".to_owned())],
            ]
        );

        let row_count = doc.sheet_row_count("Sheet1").expect("count sheet rows");
        assert_eq!(row_count, 2);
    }

    #[test]
    fn read_methods_validate_inputs() {
        let file = write_minimal_xlsx();
        let doc = XlsxDocument::open(file.path()).expect("open xlsx");

        assert!(doc.read_cell("Sheet1", "1A").is_err());
        assert!(doc.read_range("Sheet1", "A1-B2").is_err());
        assert!(doc.read_cell("Missing", "A1").is_err());
        assert!(doc.read_range("Missing", "A1:B2").is_err());
        assert!(doc.sheet_row_count("Missing").is_err());
    }
}
