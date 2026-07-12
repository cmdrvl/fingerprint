use crate::document::html::{HtmlDocument, is_hard_page_break};
use crate::document::{Document, StructuredDocument};
use crate::dsl::parser::ExtractSection;
use calamine::{Reader, open_workbook_auto};
use regex::Regex;
use serde_json::Value;
use serde_json::json;
use std::collections::{BTreeSet, HashMap};
use std::path::Path;

type CellRef = (usize, usize);
type CellRange = (CellRef, CellRef);

/// Extract content sections from a matched document.
pub fn extract(
    doc: &Document,
    sections: &[ExtractSection],
) -> Result<HashMap<String, Value>, String> {
    let mut extracted = HashMap::new();

    for section in sections {
        let maybe_value = extract_one(doc, section)
            .map_err(|error| format!("extract section '{}': {error}", section.name))?;
        if let Some(value) = maybe_value {
            extracted.insert(section.name.clone(), value);
        }
    }

    Ok(extracted)
}

fn extract_one(doc: &Document, section: &ExtractSection) -> Result<Option<Value>, String> {
    match section.r#type.as_str() {
        "range" => extract_range(doc, section),
        "section" => extract_section(doc, section),
        "table" => extract_table(doc, section),
        "text_match" => extract_text_match(doc, section),
        "region" => extract_region(doc, section),
        other => Err(format!("unsupported extract type '{other}'")),
    }
}

fn extract_range(doc: &Document, section: &ExtractSection) -> Result<Option<Value>, String> {
    let sheet = section
        .sheet
        .as_deref()
        .ok_or_else(|| "range extract requires 'sheet'".to_owned())?;
    let range_str = section
        .range
        .as_deref()
        .ok_or_else(|| "range extract requires 'range'".to_owned())?;
    let (start, end) = parse_range_ref(range_str)?;

    match doc {
        Document::Csv(csv) => {
            if !csv_virtual_sheet_names(&csv.path)
                .iter()
                .any(|name| name.eq_ignore_ascii_case(sheet))
            {
                return Ok(None);
            }

            let rows = load_csv_rows(&csv.path)?;
            let row_count = count_non_empty_rows_in_range_csv(&rows, start, end);
            Ok(Some(json!({
                "range": range_str,
                "row_count": row_count,
            })))
        }
        Document::Xlsx(xlsx) => {
            let mut workbook = open_workbook_auto(&xlsx.path).map_err(|error| {
                format!("failed opening workbook '{}': {error}", xlsx.path.display())
            })?;
            let worksheet = match workbook.worksheet_range(sheet) {
                Ok(worksheet) => worksheet,
                Err(_) => return Ok(None),
            };
            let row_count = count_non_empty_rows_in_range_xlsx(&worksheet, start, end);
            Ok(Some(json!({
                "range": range_str,
                "row_count": row_count,
            })))
        }
        _ => Ok(None),
    }
}

fn extract_section(doc: &Document, section: &ExtractSection) -> Result<Option<Value>, String> {
    let pattern = section
        .anchor_heading
        .as_deref()
        .ok_or_else(|| "section extract requires 'anchor_heading'".to_owned())?;
    let heading_regex =
        Regex::new(pattern).map_err(|error| format!("invalid anchor_heading regex: {error}"))?;
    let content_doc = content_document(doc);

    let Some(content_doc) = content_doc else {
        return Ok(None);
    };

    let heading = content_doc
        .headings
        .iter()
        .find(|heading| heading_regex.is_match(&heading.text));
    let Some(heading) = heading else {
        return Ok(None);
    };

    let section = content_doc
        .sections
        .iter()
        .find(|candidate| candidate.heading.as_ref().map(|h| h.line) == Some(heading.line));
    let Some(section) = section else {
        return Ok(None);
    };

    Ok(Some(json!({
        "start_line": section.start_line,
        "end_line": section.end_line,
        "heading": heading.text,
    })))
}

fn extract_table(doc: &Document, section: &ExtractSection) -> Result<Option<Value>, String> {
    let pattern = section
        .anchor_heading
        .as_deref()
        .ok_or_else(|| "table extract requires 'anchor_heading'".to_owned())?;
    let index = section.index.unwrap_or(0);
    let heading_regex =
        Regex::new(pattern).map_err(|error| format!("invalid anchor_heading regex: {error}"))?;
    let content_doc = content_document(doc);

    let Some(content_doc) = content_doc else {
        return Ok(None);
    };

    let heading = content_doc
        .headings
        .iter()
        .find(|heading| heading_regex.is_match(&heading.text));
    let Some(heading) = heading else {
        return Ok(None);
    };

    let tables: Vec<_> = content_doc
        .tables
        .iter()
        .filter(|table| table.heading_ref.as_deref() == Some(heading.text.as_str()))
        .collect();
    let Some(table) = tables.get(index) else {
        return Ok(None);
    };

    Ok(Some(json!({
        "start_line": table.start_line,
        "end_line": table.end_line,
        "columns": table.headers,
        "row_count": table.rows.len(),
    })))
}

fn extract_text_match(doc: &Document, section: &ExtractSection) -> Result<Option<Value>, String> {
    let anchor_pattern = section
        .anchor
        .as_deref()
        .ok_or_else(|| "text_match extract requires 'anchor'".to_owned())?;
    let pattern = section
        .pattern
        .as_deref()
        .ok_or_else(|| "text_match extract requires 'pattern'".to_owned())?;
    let within_chars = section
        .within_chars
        .ok_or_else(|| "text_match extract requires 'within_chars'".to_owned())?;

    let anchor_regex =
        Regex::new(anchor_pattern).map_err(|error| format!("invalid anchor regex: {error}"))?;
    let value_regex =
        Regex::new(pattern).map_err(|error| format!("invalid pattern regex: {error}"))?;
    let Some(text) = content_text(doc) else {
        return Ok(None);
    };

    let anchor_match = anchor_regex.find(text);
    let Some(anchor_match) = anchor_match else {
        return Ok(None);
    };

    let mut chosen = None;
    for value_match in value_regex.find_iter(text) {
        let distance = if value_match.start() >= anchor_match.end() {
            value_match.start().saturating_sub(anchor_match.end())
        } else {
            anchor_match.start().saturating_sub(value_match.end())
        };

        if distance <= within_chars as usize {
            chosen = Some(value_match);
            break;
        }
    }

    let Some(value_match) = chosen else {
        return Ok(None);
    };

    let line = text[..value_match.start()]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1;
    let line_start = text[..value_match.start()]
        .rfind('\n')
        .map_or(0, |position| position + 1);
    let char_offset = text[line_start..value_match.start()].chars().count();

    Ok(Some(json!({
        "line": line,
        "char_offset": char_offset,
        "matched": value_match.as_str(),
    })))
}

#[derive(Debug, Clone, Copy)]
struct LocatedElement {
    order: usize,
    byte_end: usize,
    line: usize,
}

fn extract_region(doc: &Document, section: &ExtractSection) -> Result<Option<Value>, String> {
    let Document::Html(html) = doc else {
        return Ok(None);
    };

    let anchor_selector = section
        .anchor_selector
        .as_deref()
        .ok_or_else(|| "region extract requires 'anchor_selector'".to_owned())?;
    let stop_selector = section
        .stop_selector
        .as_deref()
        .ok_or_else(|| "region extract requires 'stop_selector'".to_owned())?;

    let anchor_nodes = html.select_nodes(anchor_selector)?;
    let Some(anchor) = anchor_nodes.first().copied() else {
        return Ok(None);
    };
    let Some(anchor) = locate_element(html, anchor, 0) else {
        return Ok(None);
    };

    let continue_past = section
        .continue_past
        .iter()
        .map(|pattern| {
            Regex::new(pattern)
                .map_err(|error| format!("invalid continue_past regex '{pattern}': {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let stop_nodes = html.select_nodes(stop_selector)?;
    let stop = stop_nodes
        .into_iter()
        .filter_map(|node| {
            let order = html.element_order_index(&node)?;
            (order > anchor.order).then_some((order, node))
        })
        .find_map(|(_, node)| {
            let text = selector_node_text(node);
            if continue_past
                .iter()
                .any(|pattern| pattern.is_match(text.as_str()))
            {
                return None;
            }
            locate_element(html, node, anchor.byte_end)
        });

    let end_line = stop
        .map(|stop| stop.line)
        .unwrap_or_else(|| html.normalized.lines().count() + 1);
    let end_line = end_line.max(anchor.line + 1);
    let stop_order = stop.map(|stop| stop.order);

    let table_indices = html
        .tables
        .iter()
        .filter(|table| table.start_line >= anchor.line && table.start_line < end_line)
        .map(|table| table.index)
        .collect::<Vec<_>>();
    let page_span = region_page_span(html, anchor.line, end_line, anchor.order, stop_order)?;

    Ok(Some(json!({
        "start_line": anchor.line,
        "end_line": end_line,
        "table_indices": table_indices,
        "page_span": page_span,
    })))
}

fn locate_element(
    html: &HtmlDocument,
    element: scraper::ElementRef<'_>,
    search_start: usize,
) -> Option<LocatedElement> {
    let text = selector_node_text(element);
    if text.is_empty() {
        return None;
    }
    let bounded_start = search_start.min(html.normalized.len());
    let relative_start = html.normalized[bounded_start..].find(text.as_str())?;
    let byte_start = bounded_start + relative_start;
    let byte_end = byte_start + text.len();
    let line = html.normalized[..byte_start]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1;
    let order = html.element_order_index(&element)?;

    Some(LocatedElement {
        order,
        byte_end,
        line,
    })
}

fn selector_node_text(node: scraper::ElementRef<'_>) -> String {
    node.text()
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn region_page_span(
    html: &HtmlDocument,
    start_line: usize,
    end_line: usize,
    start_order: usize,
    stop_order: Option<usize>,
) -> Result<[usize; 2], String> {
    let mut data_pages = BTreeSet::new();
    for section in &html.sections {
        if ranges_overlap(
            start_line,
            end_line,
            section.start_line,
            section.end_line + 1,
        ) && let Some(page) = section.page
        {
            data_pages.insert(page as usize);
        }
    }
    for table in &html.tables {
        if table.start_line >= start_line
            && table.start_line < end_line
            && let Some(page) = table.page
        {
            data_pages.insert(page as usize);
        }
    }
    if let (Some(first), Some(last)) = (data_pages.first(), data_pages.last()) {
        return Ok([*first, *last]);
    }

    let hard_break_orders = html
        .select_nodes(r#"[style*="break"]"#)?
        .into_iter()
        .filter(|node| node.value().attr("style").is_some_and(is_hard_page_break))
        .filter_map(|node| html.element_order_index(&node))
        .collect::<Vec<_>>();
    let before_start = hard_break_orders
        .iter()
        .filter(|order| **order < start_order)
        .count();
    let span_end_order = stop_order.unwrap_or(usize::MAX);
    let within_span = hard_break_orders
        .iter()
        .filter(|order| **order >= start_order && **order < span_end_order)
        .count();
    let first_page = before_start + 1;
    let last_page = first_page + within_span;

    Ok([first_page, last_page])
}

fn ranges_overlap(start_a: usize, end_a: usize, start_b: usize, end_b: usize) -> bool {
    start_a < end_b && start_b < end_a
}

fn content_document(doc: &Document) -> Option<StructuredDocument<'_>> {
    match doc {
        Document::Html(html) => Some(StructuredDocument::from_html(html)),
        Document::Markdown(markdown) => Some(StructuredDocument::from_markdown(markdown)),
        // PDF content assertions ride the frozen markdown normalizer. Richer
        // PDF structure is a separate authoring decision (bd-yw9), not a place
        // to add new heuristic normalization here.
        Document::Pdf(pdf) => pdf.text.as_ref().map(StructuredDocument::from_markdown),
        _ => None,
    }
}

fn content_text(doc: &Document) -> Option<&str> {
    match doc {
        Document::Html(html) => Some(&html.normalized),
        Document::Markdown(markdown) => Some(&markdown.normalized),
        Document::Pdf(pdf) => pdf
            .text
            .as_ref()
            .map(|markdown| markdown.normalized.as_str()),
        Document::Text(text) => Some(text.content()),
        _ => None,
    }
}

fn load_csv_rows(path: &Path) -> Result<Vec<Vec<String>>, String> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .from_path(path)
        .map_err(|error| format!("failed opening csv '{}': {error}", path.display()))?;
    let mut rows = Vec::new();

    for (index, record) in reader.records().enumerate() {
        let record = record.map_err(|error| {
            format!(
                "failed reading csv '{}' row {}: {error}",
                path.display(),
                index + 1
            )
        })?;
        rows.push(record.iter().map(ToOwned::to_owned).collect());
    }

    Ok(rows)
}

fn count_non_empty_rows_in_range_csv(rows: &[Vec<String>], start: CellRef, end: CellRef) -> usize {
    (start.0..=end.0)
        .filter(|row_index| {
            (start.1..=end.1).any(|col_index| {
                rows.get(*row_index)
                    .and_then(|row| row.get(col_index))
                    .is_some_and(|value| !value.trim().is_empty())
            })
        })
        .count()
}

fn count_non_empty_rows_in_range_xlsx(
    worksheet: &calamine::Range<calamine::Data>,
    start: CellRef,
    end: CellRef,
) -> usize {
    (start.0..=end.0)
        .filter(|row_index| {
            (start.1..=end.1).any(|col_index| {
                worksheet
                    .get_value((*row_index as u32, col_index as u32))
                    .is_some_and(|cell| !cell.to_string().trim().is_empty())
            })
        })
        .count()
}

fn csv_virtual_sheet_names(path: &Path) -> Vec<String> {
    let mut names = vec!["Sheet1".to_owned(), "csv".to_owned()];
    if let Some(stem) = path.file_stem().and_then(|value| value.to_str()) {
        names.push(stem.to_owned());
    }
    names
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
    use super::*;
    use crate::document::{CsvDocument, HtmlDocument, MarkdownDocument, PdfDocument};
    use std::fs;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn csv_document(contents: &str) -> Document {
        let file = NamedTempFile::with_suffix(".csv").expect("create csv temp file");
        fs::write(file.path(), contents).expect("write csv fixture");
        let (_persisted_file, path) = file.keep().expect("persist csv fixture");
        Document::Csv(CsvDocument { path })
    }

    fn markdown_document(contents: &str) -> Document {
        let mut file = NamedTempFile::with_suffix(".md").expect("create markdown temp file");
        file.write_all(contents.as_bytes())
            .expect("write markdown fixture");
        file.flush().expect("flush markdown fixture");
        let markdown = MarkdownDocument::open(file.path()).expect("open markdown fixture");
        Document::Markdown(markdown)
    }

    fn html_document(contents: &str) -> Document {
        let mut file = NamedTempFile::with_suffix(".html").expect("create html temp file");
        file.write_all(contents.as_bytes())
            .expect("write html fixture");
        file.flush().expect("flush html fixture");
        let html = HtmlDocument::open(file.path()).expect("open html fixture");
        Document::Html(html)
    }

    #[test]
    fn extracts_range_from_csv() {
        let doc = csv_document("a,b,c\nx,y,z\n1,2,3\n");
        let sections = vec![ExtractSection {
            name: "rent_roll_range".to_owned(),
            r#type: "range".to_owned(),
            anchor_heading: None,
            index: None,
            anchor: None,
            pattern: None,
            within_chars: None,
            sheet: Some("Sheet1".to_owned()),
            range: Some("A1:C3".to_owned()),
            ..Default::default()
        }];

        let extracted = extract(&doc, &sections).expect("extract range");
        assert_eq!(
            extracted.get("rent_roll_range"),
            Some(&json!({
                "range": "A1:C3",
                "row_count": 3,
            }))
        );
    }

    #[test]
    fn extracts_section_table_and_text_match_from_markdown() {
        let doc = markdown_document(
            "# Rent Roll\n\n| Tenant | SF |\n| --- | --- |\n| Acme | 1200 |\n\n## Income Capitalization\n\nAs of June 15, 2024 the cap rate is 6.25%.\n",
        );
        let sections = vec![
            ExtractSection {
                name: "rent_roll_table".to_owned(),
                r#type: "table".to_owned(),
                anchor_heading: Some("(?i)rent roll".to_owned()),
                index: Some(0),
                anchor: None,
                pattern: None,
                within_chars: None,
                sheet: None,
                range: None,
                ..Default::default()
            },
            ExtractSection {
                name: "income_cap_section".to_owned(),
                r#type: "section".to_owned(),
                anchor_heading: Some("(?i)income capitali[sz]ation".to_owned()),
                index: None,
                anchor: None,
                pattern: None,
                within_chars: None,
                sheet: None,
                range: None,
                ..Default::default()
            },
            ExtractSection {
                name: "as_of_date".to_owned(),
                r#type: "text_match".to_owned(),
                anchor_heading: None,
                index: None,
                anchor: Some("(?i)as of".to_owned()),
                pattern: Some(r"\w+ \d{1,2},? \d{4}".to_owned()),
                within_chars: Some(100),
                sheet: None,
                range: None,
                ..Default::default()
            },
        ];

        let extracted = extract(&doc, &sections).expect("extract markdown sections");

        let table = extracted
            .get("rent_roll_table")
            .expect("table extract present");
        assert_eq!(table["columns"], json!(["Tenant", "SF"]));
        assert_eq!(table["row_count"], json!(1));

        let section = extracted
            .get("income_cap_section")
            .expect("section extract present");
        assert_eq!(section["heading"], json!("Income Capitalization"));

        let text_match = extracted
            .get("as_of_date")
            .expect("text match extract present");
        assert_eq!(text_match["matched"], json!("June 15, 2024"));
        assert_eq!(text_match["line"], json!(9));
    }

    #[test]
    fn skips_unresolved_targets_without_failing() {
        let doc = markdown_document("# Property Description\n\nBody");
        let sections = vec![ExtractSection {
            name: "missing_table".to_owned(),
            r#type: "table".to_owned(),
            anchor_heading: Some("(?i)rent roll".to_owned()),
            index: Some(0),
            anchor: None,
            pattern: None,
            within_chars: None,
            sheet: None,
            range: None,
            ..Default::default()
        }];

        let extracted = extract(&doc, &sections).expect("missing target should be non-fatal");
        assert!(extracted.is_empty());
    }

    #[test]
    fn extracts_from_pdf_text_markdown_when_available() {
        let mut pdf = NamedTempFile::with_suffix(".pdf").expect("create pdf temp file");
        pdf.write_all(b"%PDF-1.4\n")
            .expect("write pdf placeholder content");
        pdf.flush().expect("flush pdf");

        let mut markdown = NamedTempFile::with_suffix(".md").expect("create markdown temp file");
        markdown
            .write_all(b"# Income Capitalization\n\nCap rate is 5.10%.")
            .expect("write markdown content");
        markdown.flush().expect("flush markdown");

        let pdf_doc = PdfDocument::open(pdf.path(), Some(markdown.path())).expect("open pdf doc");
        let doc = Document::Pdf(pdf_doc);
        let sections = vec![ExtractSection {
            name: "income_cap_section".to_owned(),
            r#type: "section".to_owned(),
            anchor_heading: Some("(?i)income capitali[sz]ation".to_owned()),
            index: None,
            anchor: None,
            pattern: None,
            within_chars: None,
            sheet: None,
            range: None,
            ..Default::default()
        }];

        let extracted = extract(&doc, &sections).expect("extract section from pdf text");
        assert!(extracted.contains_key("income_cap_section"));
    }

    #[test]
    fn extracts_section_table_and_text_match_from_html() {
        let doc = html_document(
            r#"
<html>
  <body>
    <h1>Rent Roll</h1>
    <table>
      <tr><th>Tenant</th><th>SF</th></tr>
      <tr><td>Acme</td><td>1200</td></tr>
    </table>
    <h2>Income Capitalization</h2>
    <p>As of June 15, 2024 the cap rate is 6.25%.</p>
  </body>
</html>
"#,
        );
        let sections = vec![
            ExtractSection {
                name: "rent_roll_table".to_owned(),
                r#type: "table".to_owned(),
                anchor_heading: Some("(?i)rent roll".to_owned()),
                index: Some(0),
                anchor: None,
                pattern: None,
                within_chars: None,
                sheet: None,
                range: None,
                ..Default::default()
            },
            ExtractSection {
                name: "income_cap_section".to_owned(),
                r#type: "section".to_owned(),
                anchor_heading: Some("(?i)income capitali[sz]ation".to_owned()),
                index: None,
                anchor: None,
                pattern: None,
                within_chars: None,
                sheet: None,
                range: None,
                ..Default::default()
            },
            ExtractSection {
                name: "as_of_date".to_owned(),
                r#type: "text_match".to_owned(),
                anchor_heading: None,
                index: None,
                anchor: Some("(?i)as of".to_owned()),
                pattern: Some(r"\w+ \d{1,2},? \d{4}".to_owned()),
                within_chars: Some(100),
                sheet: None,
                range: None,
                ..Default::default()
            },
        ];

        let extracted = extract(&doc, &sections).expect("extract html sections");

        let table = extracted
            .get("rent_roll_table")
            .expect("table extract present");
        assert_eq!(table["columns"], json!(["Tenant", "SF"]));
        assert_eq!(table["row_count"], json!(1));

        let section = extracted
            .get("income_cap_section")
            .expect("section extract present");
        assert_eq!(section["heading"], json!("Income Capitalization"));

        let text_match = extracted
            .get("as_of_date")
            .expect("text match extract present");
        assert_eq!(text_match["matched"], json!("June 15, 2024"));
    }
}
