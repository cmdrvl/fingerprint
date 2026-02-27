use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct MarkdownDocument {
    pub path: PathBuf,
    pub raw: String,
    pub normalized: String,
    pub headings: Vec<Heading>,
    pub sections: Vec<Section>,
    pub tables: Vec<Table>,
}

#[derive(Debug, Clone)]
pub struct Heading {
    pub level: u8,
    pub text: String,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct Section {
    pub heading: Option<Heading>,
    pub start_line: usize,
    pub end_line: usize,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct Table {
    pub heading_ref: Option<String>,
    pub index: usize,
    pub start_line: usize,
    pub end_line: usize,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl MarkdownDocument {
    /// Read markdown content and parse structure with normalization.
    pub fn open(path: &Path) -> Result<Self, String> {
        let raw = fs::read_to_string(path).map_err(|error| {
            format!("failed to read markdown file '{}': {error}", path.display())
        })?;

        let normalized = normalize_markdown(&raw);
        let headings = parse_headings(&normalized);
        let sections = compute_sections(&normalized, &headings);
        let tables = parse_tables(&normalized, &headings);

        Ok(Self {
            path: path.to_path_buf(),
            raw,
            normalized,
            headings,
            sections,
            tables,
        })
    }
}

fn normalize_markdown(content: &str) -> String {
    let mut result = content.to_string();

    // Apply normalization passes
    result = convert_setext_to_atx(&result);
    result = convert_bold_as_heading(&result);
    result = normalize_whitespace(&result);
    result = normalize_table_pipes(&result);

    result
}

fn convert_setext_to_atx(content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut result_lines = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        if i + 1 < lines.len() {
            let current_line = lines[i].trim();
            let next_line = lines[i + 1];

            // Check if next line is setext underline (=== or ---)
            if !current_line.is_empty()
                && (next_line.chars().all(|c| c == '=') || next_line.chars().all(|c| c == '-'))
                && next_line.len() >= current_line.len()
            {
                // Convert to ATX heading
                let level = if next_line.chars().all(|c| c == '=') {
                    1
                } else {
                    2
                };
                let prefix = "#".repeat(level);
                result_lines.push(format!("{} {}", prefix, current_line));
                i += 2; // Skip both current and underline
                continue;
            }
        }

        result_lines.push(lines[i].to_string());
        i += 1;
    }

    result_lines.join("\n")
}

fn convert_bold_as_heading(content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut result_lines = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();

        // Check if line is solely **Bold Text** pattern with surrounding blank lines
        if trimmed.starts_with("**") && trimmed.ends_with("**") && trimmed.len() > 4 {
            let text = &trimmed[2..trimmed.len() - 2].trim();
            let prev_blank = i == 0 || lines[i - 1].trim().is_empty();
            let next_blank = i == lines.len() - 1 || lines[i + 1].trim().is_empty();

            if prev_blank && next_blank && !text.is_empty() {
                // Convert to ATX heading (assume H2)
                result_lines.push(format!("## {}", text));
                continue;
            }
        }

        result_lines.push(line.to_string());
    }

    result_lines.join("\n")
}

fn normalize_whitespace(content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut result_lines = Vec::new();
    let mut prev_was_blank = false;

    for line in lines {
        let trimmed_line = line.trim_end();
        let is_blank = trimmed_line.is_empty();

        // Collapse consecutive blank lines to one
        if is_blank && prev_was_blank {
            continue;
        }

        result_lines.push(trimmed_line.to_string());
        prev_was_blank = is_blank;
    }

    result_lines.join("\n")
}

fn normalize_table_pipes(content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut result_lines = Vec::new();

    for line in lines {
        if line.contains('|') && line.trim().starts_with('|') && line.trim().ends_with('|') {
            // Normalize pipe spacing in table rows
            let cells: Vec<&str> = line.split('|').collect();
            let normalized_cells: Vec<String> =
                cells.iter().map(|cell| cell.trim().to_string()).collect();
            result_lines.push(normalized_cells.join(" | "));
        } else {
            result_lines.push(line.to_string());
        }
    }

    result_lines.join("\n")
}

fn parse_headings(content: &str) -> Vec<Heading> {
    let mut headings = Vec::new();

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            let mut level = 0;
            let mut chars = trimmed.chars();

            // Count leading '#' characters
            while let Some('#') = chars.next() {
                level += 1;
                if level > 6 {
                    break;
                }
            }

            if level > 0 && level <= 6 {
                let text = trimmed[level..].trim().to_string();
                headings.push(Heading {
                    level: level as u8,
                    text,
                    line: line_num + 1, // 1-indexed
                });
            }
        }
    }

    headings
}

fn compute_sections(content: &str, headings: &[Heading]) -> Vec<Section> {
    let lines: Vec<&str> = content.lines().collect();
    let mut sections = Vec::new();

    if headings.is_empty() {
        // No headings - entire content is preamble
        let content_str = lines.join("\n");
        sections.push(Section {
            heading: None,
            start_line: 1,
            end_line: lines.len(),
            content: content_str,
        });
        return sections;
    }

    // Preamble section (content before first heading)
    if !lines.is_empty() && headings[0].line > 1 {
        let preamble_end = headings[0].line - 1;
        let preamble_content = lines[..preamble_end].join("\n");
        sections.push(Section {
            heading: None,
            start_line: 1,
            end_line: preamble_end,
            content: preamble_content,
        });
    }

    // Sections for each heading
    for (i, heading) in headings.iter().enumerate() {
        let start_line = heading.line;

        // Find end: next heading at equal or lesser depth
        let mut end_line = lines.len();
        for next_heading in &headings[i + 1..] {
            if next_heading.level <= heading.level {
                end_line = next_heading.line - 1;
                break;
            }
        }

        let section_lines = if start_line <= lines.len() {
            &lines[start_line - 1..end_line.min(lines.len())]
        } else {
            &[]
        };

        sections.push(Section {
            heading: Some(heading.clone()),
            start_line,
            end_line,
            content: section_lines.join("\n"),
        });
    }

    sections
}

fn parse_tables(content: &str, headings: &[Heading]) -> Vec<Table> {
    let lines: Vec<&str> = content.lines().collect();
    let mut tables = Vec::new();
    let mut table_index = 0;
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim();

        // Look for table start (line with pipes)
        if line.contains('|') && line.starts_with('|') && line.ends_with('|') {
            let table_start = i;
            let mut table_end = i;
            let mut header_row = Vec::new();
            let mut data_rows = Vec::new();

            // Parse header row
            let header_cells: Vec<&str> = line.split('|').collect();
            for cell in &header_cells[1..header_cells.len() - 1] {
                // Skip empty first/last
                header_row.push(cell.trim().to_string());
            }

            // Look ahead for separator row and data rows
            let mut j = i + 1;
            while j < lines.len() {
                let next_line = lines[j].trim();

                if next_line.contains('|') && next_line.starts_with('|') && next_line.ends_with('|')
                {
                    // Skip separator row (contains dashes)
                    if next_line.contains('-') && j == i + 1 {
                        j += 1;
                        continue;
                    }

                    // Parse data row
                    let data_cells: Vec<&str> = next_line.split('|').collect();
                    let mut row = Vec::new();
                    for cell in &data_cells[1..data_cells.len() - 1] {
                        // Skip empty first/last
                        row.push(cell.trim().to_string());
                    }
                    data_rows.push(row);
                    table_end = j;
                    j += 1;
                } else {
                    break;
                }
            }

            // Find the nearest preceding heading
            let heading_ref = headings
                .iter()
                .rev()
                .find(|h| h.line < table_start + 1)
                .map(|h| h.text.clone());

            tables.push(Table {
                heading_ref,
                index: table_index,
                start_line: table_start + 1,
                end_line: table_end + 1,
                headers: header_row,
                rows: data_rows,
            });

            table_index += 1;
            i = table_end + 1;
        } else {
            i += 1;
        }
    }

    tables
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn opens_markdown_file() {
        let content = "# Title\n\nSome content.";
        let file = make_temp_file(content);
        let doc = MarkdownDocument::open(file.path()).expect("open markdown document");

        assert_eq!(doc.raw, content);
        assert_eq!(doc.headings.len(), 1);
        assert_eq!(doc.headings[0].text, "Title");
        assert_eq!(doc.headings[0].level, 1);
    }

    #[test]
    fn converts_setext_to_atx() {
        let content = "Title\n=====\n\nSubtitle\n--------";
        let normalized = convert_setext_to_atx(content);
        assert!(normalized.contains("# Title"));
        assert!(normalized.contains("## Subtitle"));
    }

    #[test]
    fn converts_bold_as_heading() {
        let content = "\n**Bold Heading**\n\nContent here";
        let normalized = convert_bold_as_heading(content);
        assert!(normalized.contains("## Bold Heading"));
    }

    #[test]
    fn normalizes_whitespace() {
        let content = "Line 1  \n\n\n\nLine 2   ";
        let normalized = normalize_whitespace(content);
        assert_eq!(normalized, "Line 1\n\nLine 2");
    }

    #[test]
    fn normalizes_table_pipes() {
        let content = "|  Col1  |Col2|   Col3   |";
        let normalized = normalize_table_pipes(content);
        assert_eq!(normalized, " | Col1 | Col2 | Col3 | ");
    }

    #[test]
    fn parses_headings() {
        let content = "# H1\n## H2\n### H3";
        let headings = parse_headings(content);

        assert_eq!(headings.len(), 3);
        assert_eq!(headings[0].level, 1);
        assert_eq!(headings[0].text, "H1");
        assert_eq!(headings[1].level, 2);
        assert_eq!(headings[1].text, "H2");
        assert_eq!(headings[2].level, 3);
        assert_eq!(headings[2].text, "H3");
    }

    #[test]
    fn computes_sections_with_preamble() {
        let content = "Preamble\n\n# Section 1\nContent 1\n\n## Section 2\nContent 2";
        let headings = parse_headings(content);
        let sections = compute_sections(content, &headings);

        assert_eq!(sections.len(), 3);
        assert!(sections[0].heading.is_none()); // Preamble
        assert_eq!(sections[1].heading.as_ref().unwrap().text, "Section 1");
        assert_eq!(sections[2].heading.as_ref().unwrap().text, "Section 2");
    }

    #[test]
    fn section_boundaries_equal_or_lesser_depth() {
        let content = "# Main\nContent\n## Sub1\nSub content\n## Sub2\nMore content\n# Next Main\nNext content";
        let headings = parse_headings(content);
        let sections = compute_sections(content, &headings);

        // Main section should include both Sub1 and Sub2 until Next Main
        let main_section = sections
            .iter()
            .find(|s| s.heading.as_ref().is_some_and(|h| h.text == "Main"))
            .unwrap();
        assert!(main_section.content.contains("Sub1"));
        assert!(main_section.content.contains("Sub2"));
        assert!(!main_section.content.contains("Next content"));
    }

    #[test]
    fn parses_tables() {
        let content =
            "# Data\n\n| Col1 | Col2 |\n|------|------|\n| A    | B    |\n| C    | D    |";
        let headings = parse_headings(content);
        let tables = parse_tables(content, &headings);

        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].headers, vec!["Col1", "Col2"]);
        assert_eq!(tables[0].rows.len(), 2);
        assert_eq!(tables[0].rows[0], vec!["A", "B"]);
        assert_eq!(tables[0].rows[1], vec!["C", "D"]);
        assert_eq!(tables[0].heading_ref, Some("Data".to_string()));
    }
}
