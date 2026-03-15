use crate::document::markdown::{Heading, Section, Table};
use scraper::node::Node;
use scraper::{ElementRef, Html};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct HtmlDocument {
    pub path: PathBuf,
    pub raw: String,
    pub normalized: String,
    pub headings: Vec<Heading>,
    pub sections: Vec<Section>,
    pub tables: Vec<Table>,
}

#[derive(Debug, Clone)]
struct HtmlBlock {
    page: Option<u32>,
    kind: HtmlBlockKind,
}

#[derive(Debug, Clone)]
enum HtmlBlockKind {
    Heading {
        level: u8,
        text: String,
    },
    Text {
        text: String,
    },
    Table {
        heading_ref: Option<String>,
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
}

#[derive(Debug, Clone)]
struct TableSeed {
    heading_ref: Option<String>,
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
}

#[derive(Debug, Clone)]
struct TableCellSeed {
    text: String,
    colspan: usize,
    rowspan: usize,
    is_header: bool,
}

#[derive(Debug, Clone)]
struct SpanSeed {
    text: String,
    remaining_rows: usize,
}

impl HtmlDocument {
    /// Read HTML content and parse structure with deterministic normalization.
    pub fn open(path: &Path) -> Result<Self, String> {
        let raw_bytes = fs::read(path)
            .map_err(|error| format!("failed to read html file '{}': {error}", path.display()))?;
        let raw = String::from_utf8_lossy(&raw_bytes).into_owned();
        Ok(parse_html_document(path, raw))
    }
}

fn parse_html_document(path: &Path, raw: String) -> HtmlDocument {
    let document = Html::parse_document(&raw);
    let mut blocks = Vec::new();
    let mut last_heading = None;

    collect_blocks(document.tree.root(), None, &mut last_heading, &mut blocks);

    let (normalized, headings, sections, tables) = materialize_blocks(&blocks);

    HtmlDocument {
        path: path.to_path_buf(),
        raw,
        normalized,
        headings,
        sections,
        tables,
    }
}

fn collect_blocks(
    node: ego_tree::NodeRef<'_, Node>,
    current_page: Option<u32>,
    last_heading: &mut Option<String>,
    blocks: &mut Vec<HtmlBlock>,
) {
    match node.value() {
        Node::Text(text) => {
            let normalized = normalize_text_fragment(text.text.as_ref());
            if !normalized.is_empty() {
                blocks.push(HtmlBlock {
                    page: current_page,
                    kind: HtmlBlockKind::Text { text: normalized },
                });
            }
        }
        Node::Element(_) => {
            let Some(element) = ElementRef::wrap(node) else {
                return;
            };
            let name = element.value().name();

            if should_ignore_element(name) {
                return;
            }

            let page = if name == "section" {
                parse_page_number(&element).or(current_page)
            } else {
                current_page
            };

            if let Some(level) = heading_level(name) {
                let text = normalize_text_fragment(&collect_heading_text(node));
                if !text.is_empty() {
                    *last_heading = Some(text.clone());
                    blocks.push(HtmlBlock {
                        page,
                        kind: HtmlBlockKind::Heading { level, text },
                    });
                }
                for child in node.children() {
                    if let Some(child_element) = ElementRef::wrap(child)
                        && (is_block_container(child_element.value().name())
                            || child_element.value().name() == "table")
                    {
                        collect_blocks(child, page, last_heading, blocks);
                    }
                }
                return;
            }

            if name == "table" {
                let table = parse_top_level_table(&element, last_heading.clone());
                blocks.push(HtmlBlock {
                    page,
                    kind: HtmlBlockKind::Table {
                        heading_ref: table.heading_ref,
                        headers: table.headers,
                        rows: table.rows,
                    },
                });
                return;
            }

            if should_capture_text_block(&element) {
                let text = normalize_text_fragment(&collect_text_with_breaks(node));
                if !text.is_empty() {
                    blocks.push(HtmlBlock {
                        page,
                        kind: HtmlBlockKind::Text { text },
                    });
                }
                return;
            }

            for child in node.children() {
                collect_blocks(child, page, last_heading, blocks);
            }
        }
        _ => {
            for child in node.children() {
                collect_blocks(child, current_page, last_heading, blocks);
            }
        }
    }
}

fn should_ignore_element(name: &str) -> bool {
    matches!(
        name,
        "script" | "style" | "head" | "meta" | "link" | "title" | "noscript"
    )
}

fn heading_level(name: &str) -> Option<u8> {
    match name {
        "h1" => Some(1),
        "h2" => Some(2),
        "h3" => Some(3),
        "h4" => Some(4),
        "h5" => Some(5),
        "h6" => Some(6),
        _ => None,
    }
}

fn parse_page_number(element: &ElementRef<'_>) -> Option<u32> {
    element
        .value()
        .attr("data-page-number")
        .and_then(|value| value.trim().parse::<u32>().ok())
}

fn should_capture_text_block(element: &ElementRef<'_>) -> bool {
    let name = element.value().name();
    if matches!(
        name,
        "html"
            | "body"
            | "table"
            | "thead"
            | "tbody"
            | "tfoot"
            | "tr"
            | "td"
            | "th"
            | "colgroup"
            | "col"
            | "caption"
    ) {
        return false;
    }

    if heading_level(name).is_some() {
        return false;
    }

    if element
        .children()
        .filter_map(ElementRef::wrap)
        .any(|child| is_block_container(child.value().name()))
    {
        return false;
    }

    !normalize_text_fragment(&element.text().collect::<Vec<_>>().join(" "))
        .trim()
        .is_empty()
}

fn is_block_container(name: &str) -> bool {
    matches!(
        name,
        "article"
            | "aside"
            | "blockquote"
            | "div"
            | "dl"
            | "dt"
            | "dd"
            | "fieldset"
            | "figcaption"
            | "figure"
            | "footer"
            | "form"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "header"
            | "hr"
            | "li"
            | "main"
            | "nav"
            | "ol"
            | "p"
            | "pre"
            | "section"
            | "table"
            | "ul"
    )
}

fn collect_text_with_breaks(node: ego_tree::NodeRef<'_, Node>) -> String {
    let mut output = String::new();
    collect_text_recursive(node, &mut output);
    output
}

fn collect_heading_text(node: ego_tree::NodeRef<'_, Node>) -> String {
    let mut output = String::new();
    collect_heading_text_recursive(node, &mut output);
    output
}

fn collect_text_recursive(node: ego_tree::NodeRef<'_, Node>, output: &mut String) {
    match node.value() {
        Node::Text(text) => output.push_str(text.text.as_ref()),
        Node::Element(_) => {
            let Some(element) = ElementRef::wrap(node) else {
                return;
            };
            let name = element.value().name();
            if should_ignore_element(name) {
                return;
            }
            if name == "br" {
                output.push('\n');
                return;
            }
            for child in node.children() {
                collect_text_recursive(child, output);
            }
            if matches!(name, "p" | "div" | "li") {
                output.push('\n');
            }
        }
        _ => {
            for child in node.children() {
                collect_text_recursive(child, output);
            }
        }
    }
}

fn collect_heading_text_recursive(node: ego_tree::NodeRef<'_, Node>, output: &mut String) {
    match node.value() {
        Node::Text(text) => output.push_str(text.text.as_ref()),
        Node::Element(_) => {
            let Some(element) = ElementRef::wrap(node) else {
                return;
            };
            let name = element.value().name();
            if should_ignore_element(name) {
                return;
            }
            if name == "br" {
                output.push('\n');
                return;
            }
            if is_block_container(name) && heading_level(name).is_none() {
                return;
            }
            for child in node.children() {
                collect_heading_text_recursive(child, output);
            }
        }
        _ => {
            for child in node.children() {
                collect_heading_text_recursive(child, output);
            }
        }
    }
}

fn normalize_text_fragment(text: &str) -> String {
    let replaced = text
        .replace('\u{00a0}', " ")
        .replace(['\u{2013}', '\u{2014}'], " ");
    let lines: Vec<String> = replaced
        .lines()
        .map(|line| collapse_whitespace(line).trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();
    lines.join("\n")
}

fn collapse_whitespace(text: &str) -> String {
    let mut result = String::new();
    let mut last_was_whitespace = false;

    for ch in text.chars() {
        if ch.is_whitespace() {
            if !last_was_whitespace {
                result.push(' ');
            }
            last_was_whitespace = true;
        } else {
            result.push(ch);
            last_was_whitespace = false;
        }
    }

    result
}

fn parse_top_level_table(element: &ElementRef<'_>, heading_ref: Option<String>) -> TableSeed {
    let row_seeds = collect_table_rows(*element);
    let (headers, rows) = expand_table_rows(&row_seeds);
    TableSeed {
        heading_ref,
        headers,
        rows,
    }
}

fn collect_table_rows(table: ElementRef<'_>) -> Vec<Vec<TableCellSeed>> {
    let mut rows = Vec::new();
    for child in table.children() {
        if let Some(element) = ElementRef::wrap(child) {
            match element.value().name() {
                "tr" => rows.push(parse_table_row(element)),
                "thead" | "tbody" | "tfoot" => {
                    for row_child in element.children() {
                        if let Some(row_element) = ElementRef::wrap(row_child)
                            && row_element.value().name() == "tr"
                        {
                            rows.push(parse_table_row(row_element));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    rows
}

fn parse_table_row(row: ElementRef<'_>) -> Vec<TableCellSeed> {
    let mut cells = Vec::new();
    for child in row.children() {
        if let Some(cell) = ElementRef::wrap(child) {
            let name = cell.value().name();
            if name == "td" || name == "th" {
                let text = normalize_text_fragment(&cell.text().collect::<Vec<_>>().join(" "));
                let colspan = parse_span(cell.value().attr("colspan"));
                let rowspan = parse_span(cell.value().attr("rowspan"));
                cells.push(TableCellSeed {
                    text,
                    colspan,
                    rowspan,
                    is_header: name == "th",
                });
            }
        }
    }
    cells
}

fn parse_span(raw: Option<&str>) -> usize {
    raw.and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(1)
}

fn expand_table_rows(row_seeds: &[Vec<TableCellSeed>]) -> (Vec<String>, Vec<Vec<String>>) {
    let mut carry: Vec<Option<SpanSeed>> = Vec::new();
    let mut expanded_rows: Vec<Vec<String>> = Vec::new();
    let mut expanded_flags: Vec<Vec<bool>> = Vec::new();
    let mut max_width = 0;

    for row in row_seeds {
        let mut cells = Vec::new();
        let mut flags = Vec::new();
        let mut column = 0;

        for seed in row {
            while column < carry.len() {
                if let Some(span) = carry[column].as_mut() {
                    cells.push(span.text.clone());
                    flags.push(false);
                    span.remaining_rows -= 1;
                    if span.remaining_rows == 0 {
                        carry[column] = None;
                    }
                    column += 1;
                } else {
                    break;
                }
            }

            for _ in 0..seed.colspan {
                cells.push(seed.text.clone());
                flags.push(seed.is_header);
                if carry.len() <= column {
                    carry.resize(column + 1, None);
                }
                if seed.rowspan > 1 {
                    carry[column] = Some(SpanSeed {
                        text: seed.text.clone(),
                        remaining_rows: seed.rowspan - 1,
                    });
                }
                column += 1;
            }
        }

        while column < carry.len() {
            if let Some(span) = carry[column].as_mut() {
                cells.push(span.text.clone());
                flags.push(false);
                span.remaining_rows -= 1;
                if span.remaining_rows == 0 {
                    carry[column] = None;
                }
            } else if column < max_width {
                cells.push(String::new());
                flags.push(false);
            } else {
                break;
            }
            column += 1;
        }

        max_width = max_width.max(cells.len());
        expanded_rows.push(cells);
        expanded_flags.push(flags);
    }

    for row in &mut expanded_rows {
        while row.len() < max_width {
            row.push(String::new());
        }
    }
    for flags in &mut expanded_flags {
        while flags.len() < max_width {
            flags.push(false);
        }
    }

    let header_index = expanded_rows
        .iter()
        .zip(&expanded_flags)
        .position(|(row, flags)| flags.iter().any(|flag| *flag) && !row_is_empty(row))
        .or_else(|| {
            expanded_rows
                .iter()
                .enumerate()
                .take(3)
                .find_map(|(index, row)| {
                    (!row_is_empty(row) && !row_is_separator(row)).then_some(index)
                })
        })
        .or_else(|| (!expanded_rows.is_empty()).then_some(0));

    let Some(header_index) = header_index else {
        return (Vec::new(), Vec::new());
    };

    let headers = expanded_rows[header_index].clone();
    let rows = expanded_rows
        .into_iter()
        .enumerate()
        .filter_map(|(index, row)| {
            (index > header_index && !row_is_empty(&row) && !row_is_separator(&row)).then_some(row)
        })
        .collect();

    (headers, rows)
}

fn row_is_empty(row: &[String]) -> bool {
    row.iter().all(|cell| cell.trim().is_empty())
}

fn row_is_separator(row: &[String]) -> bool {
    let mut saw_non_empty = false;
    for cell in row {
        let trimmed = cell.trim();
        if trimmed.is_empty() {
            continue;
        }
        saw_non_empty = true;
        if !trimmed
            .chars()
            .all(|ch| matches!(ch, '-' | '_' | '=' | ':' | '\u{2013}' | '\u{2014}'))
        {
            return false;
        }
    }
    saw_non_empty
}

fn materialize_blocks(blocks: &[HtmlBlock]) -> (String, Vec<Heading>, Vec<Section>, Vec<Table>) {
    let mut lines = Vec::new();
    let mut line_pages = Vec::new();
    let mut headings = Vec::new();
    let mut tables = Vec::new();
    let mut table_index = 0;

    for block in blocks {
        append_block_separator(&mut lines, &mut line_pages, block.page);
        match &block.kind {
            HtmlBlockKind::Heading { level, text } => {
                let line = format!("{} {}", "#".repeat(*level as usize), text);
                let line_number = lines.len() + 1;
                headings.push(Heading {
                    level: *level,
                    text: text.clone(),
                    line: line_number,
                });
                lines.push(line);
                line_pages.push(block.page);
            }
            HtmlBlockKind::Text { text } => {
                for text_line in text.lines() {
                    lines.push(text_line.to_string());
                    line_pages.push(block.page);
                }
            }
            HtmlBlockKind::Table {
                heading_ref,
                headers,
                rows,
            } => {
                let start_line = lines.len() + 1;
                for line in table_to_lines(headers, rows) {
                    lines.push(line);
                    line_pages.push(block.page);
                }
                let end_line = lines.len();
                tables.push(Table {
                    heading_ref: heading_ref.clone(),
                    index: table_index,
                    start_line,
                    end_line,
                    headers: headers.clone(),
                    rows: rows.clone(),
                });
                table_index += 1;
            }
        }
    }

    trim_trailing_blank_lines(&mut lines, &mut line_pages);
    let normalized = lines.join("\n");
    let sections = compute_sections_with_pages(&normalized, &headings, &line_pages);

    (normalized, headings, sections, tables)
}

fn append_block_separator(
    lines: &mut Vec<String>,
    line_pages: &mut Vec<Option<u32>>,
    page: Option<u32>,
) {
    if !lines.is_empty() && !lines.last().is_some_and(|line| line.is_empty()) {
        lines.push(String::new());
        line_pages.push(page);
    }
}

fn trim_trailing_blank_lines(lines: &mut Vec<String>, line_pages: &mut Vec<Option<u32>>) {
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
        line_pages.pop();
    }
}

fn table_to_lines(headers: &[String], rows: &[Vec<String>]) -> Vec<String> {
    let mut lines = Vec::new();
    if !headers.is_empty() {
        lines.push(headers.join(" | "));
    }
    for row in rows {
        lines.push(row.join(" | "));
    }
    lines
}

fn compute_sections_with_pages(
    content: &str,
    headings: &[Heading],
    line_pages: &[Option<u32>],
) -> Vec<Section> {
    let lines: Vec<&str> = content.lines().collect();
    let mut sections = Vec::new();

    if headings.is_empty() {
        sections.push(Section {
            heading: None,
            start_line: 1,
            end_line: lines.len(),
            content: lines.join("\n"),
            page: first_page(line_pages, 1, lines.len()),
        });
        return sections;
    }

    if !lines.is_empty() && headings[0].line > 1 {
        let end_line = headings[0].line - 1;
        sections.push(Section {
            heading: None,
            start_line: 1,
            end_line,
            content: lines[..end_line].join("\n"),
            page: first_page(line_pages, 1, end_line),
        });
    }

    for (index, heading) in headings.iter().enumerate() {
        let start_line = heading.line;
        let mut end_line = lines.len();
        for next_heading in &headings[index + 1..] {
            if next_heading.level <= heading.level {
                end_line = next_heading.line - 1;
                break;
            }
        }

        let content_slice = if start_line <= lines.len() {
            &lines[start_line - 1..end_line.min(lines.len())]
        } else {
            &[]
        };

        sections.push(Section {
            heading: Some(heading.clone()),
            start_line,
            end_line,
            content: content_slice.join("\n"),
            page: first_page(line_pages, start_line, end_line),
        });
    }

    sections
}

fn first_page(line_pages: &[Option<u32>], start_line: usize, end_line: usize) -> Option<u32> {
    if start_line == 0 || end_line == 0 || line_pages.is_empty() {
        return None;
    }
    let start_index = start_line.saturating_sub(1);
    let end_index = end_line.min(line_pages.len());
    line_pages[start_index..end_index]
        .iter()
        .copied()
        .flatten()
        .next()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn make_temp_html(contents: &str) -> NamedTempFile {
        let mut file = NamedTempFile::with_suffix(".html").expect("create temporary html fixture");
        file.write_all(contents.as_bytes())
            .expect("write temporary html fixture");
        file.flush().expect("flush temporary html fixture");
        file
    }

    #[test]
    fn opens_html_and_extracts_headings_sections_and_pages() {
        let file = make_temp_html(
            r#"
            <section data-page-number="1">
              <h1>Overview</h1>
              <p>Intro block</p>
              <h2>Details</h2>
              <p>Nested details</p>
            </section>
            <section data-page-number="2">
              <h1>Appendix</h1>
              <p>Second page text</p>
            </section>
        "#,
        );

        let document = HtmlDocument::open(file.path()).expect("open html document");

        assert_eq!(document.headings.len(), 3);
        assert_eq!(document.headings[0].text, "Overview");
        assert_eq!(document.headings[1].text, "Details");
        assert_eq!(document.headings[2].text, "Appendix");
        assert_eq!(document.sections.len(), 3);
        assert_eq!(document.sections[0].page, Some(1));
        assert_eq!(document.sections[1].page, Some(1));
        assert_eq!(document.sections[2].page, Some(2));
        assert!(document.normalized.contains("Intro block"));
        assert!(document.normalized.contains("Second page text"));
    }

    #[test]
    fn expands_colspan_and_rowspan_cells() {
        let file = make_temp_html(
            r#"
            <h1>Schedule</h1>
            <table>
              <tr><th>Col A</th><th>Col B</th><th>Col C</th></tr>
              <tr><td rowspan="2">Loan 1</td><td colspan="2">Value</td></tr>
              <tr><td>Second</td><td>Third</td></tr>
            </table>
        "#,
        );

        let document = HtmlDocument::open(file.path()).expect("open html document");
        let table = &document.tables[0];

        assert_eq!(table.headers, vec!["Col A", "Col B", "Col C"]);
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[0], vec!["Loan 1", "Value", "Value"]);
        assert_eq!(table.rows[1], vec!["Loan 1", "Second", "Third"]);
    }

    #[test]
    fn ignores_nested_tables_structurally_but_flattens_their_text() {
        let file = make_temp_html(
            r#"
            <h1>Outer</h1>
            <table>
              <tr><th>Name</th><th>Details</th></tr>
              <tr>
                <td>Alpha</td>
                <td>
                  <table>
                    <tr><td>Inner 1</td></tr>
                  </table>
                </td>
              </tr>
            </table>
        "#,
        );

        let document = HtmlDocument::open(file.path()).expect("open html document");

        assert_eq!(document.tables.len(), 1);
        assert_eq!(document.tables[0].rows[0], vec!["Alpha", "Inner 1"]);
        assert!(document.normalized.contains("Inner 1"));
    }

    #[test]
    fn scans_forward_for_header_row_when_th_is_absent() {
        let file = make_temp_html(
            r#"
            <table>
              <tr><td>&nbsp;</td><td>&nbsp;</td></tr>
              <tr><td>---</td><td>---</td></tr>
              <tr><td>Issuer</td><td>Fair Value</td></tr>
              <tr><td>Alpha</td><td>100</td></tr>
            </table>
        "#,
        );

        let document = HtmlDocument::open(file.path()).expect("open html document");
        let table = &document.tables[0];

        assert_eq!(table.headers, vec!["Issuer", "Fair Value"]);
        assert_eq!(
            table.rows,
            vec![vec!["Alpha".to_string(), "100".to_string()]]
        );
    }

    #[test]
    fn preserves_empty_cells_and_normalizes_whitespace_entities() {
        let file = make_temp_html(
            r#"
            <table>
              <tr><th>Issuer</th><th>Notes</th><th>Flag</th></tr>
              <tr>
                <td>Loan&nbsp;&ndash;&nbsp;A</td>
                <td>&nbsp;</td>
                <td>Ready&nbsp;&mdash;&nbsp;Now</td>
              </tr>
            </table>
        "#,
        );

        let document = HtmlDocument::open(file.path()).expect("open html document");
        let row = &document.tables[0].rows[0];

        assert_eq!(row[0], "Loan A");
        assert_eq!(row[1], "");
        assert_eq!(row[2], "Ready Now");
    }

    #[test]
    fn malformed_html_does_not_panic() {
        let file = make_temp_html(
            r#"
            <section data-page-number="3">
              <h1>Broken
              <div>Unclosed div
              <table>
                <tr><th>Col</th></tr>
                <tr><td>Value
            "#,
        );

        let document = HtmlDocument::open(file.path()).expect("open malformed html document");

        assert!(!document.headings.is_empty());
        assert_eq!(document.sections[0].page, Some(3));
        assert_eq!(document.tables.len(), 1);
        assert_eq!(document.tables[0].rows[0], vec!["Value"]);
    }
}
