use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use fingerprint::document::MarkdownDocument;
use std::path::Path;
use tempfile::NamedTempFile;

fn fixture(path: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

// Mock markdown normalization functions for benchmarking
// In a real implementation, these would call the actual normalization code

/// Normalize markdown content by removing formatting and standardizing structure
fn normalize_markdown(content: &str) -> String {
    let mut normalized = content.to_string();

    // Remove markdown headers (convert # Header to Header)
    normalized = regex::Regex::new(r"^#+\s*")
        .unwrap()
        .replace_all(&normalized, "")
        .to_string();

    // Remove bold/italic formatting
    normalized = regex::Regex::new(r"\*\*([^*]+)\*\*")
        .unwrap()
        .replace_all(&normalized, "$1")
        .to_string();
    normalized = regex::Regex::new(r"\*([^*]+)\*")
        .unwrap()
        .replace_all(&normalized, "$1")
        .to_string();

    // Remove code blocks
    normalized = regex::Regex::new(r"```[^`]*```")
        .unwrap()
        .replace_all(&normalized, "")
        .to_string();
    normalized = regex::Regex::new(r"`([^`]+)`")
        .unwrap()
        .replace_all(&normalized, "$1")
        .to_string();

    // Normalize lists (remove - and * prefixes)
    normalized = regex::Regex::new(r"^[\s]*[-*+]\s+")
        .unwrap()
        .replace_all(&normalized, "")
        .to_string();

    // Normalize links [text](url) -> text
    normalized = regex::Regex::new(r"\[([^\]]+)\]\([^)]+\)")
        .unwrap()
        .replace_all(&normalized, "$1")
        .to_string();

    // Normalize whitespace
    normalized = regex::Regex::new(r"\s+")
        .unwrap()
        .replace_all(&normalized, " ")
        .to_string();

    normalized.trim().to_string()
}

/// Extract structured data from markdown (tables, lists, etc.)
fn extract_markdown_structure(content: &str) -> MarkdownStructure {
    let mut structure = MarkdownStructure::default();
    let header_regex = regex::Regex::new(r"^(#+)\s*(.+)$").unwrap();
    let list_regex = regex::Regex::new(r"^[\s]*[-*+]\s+(.+)$").unwrap();

    // Extract headers
    for line in content.lines() {
        if let Some(captures) = header_regex.captures(line) {
            let level = captures.get(1).unwrap().as_str().len();
            let title = captures.get(2).unwrap().as_str().to_string();
            structure.headers.push((level, title));
        }
    }

    // Extract tables
    let table_regex = regex::Regex::new(r"(?m)^\|(.+)\|$").unwrap();
    let mut current_table = Vec::new();

    for line in content.lines() {
        if table_regex.is_match(line) {
            let cells: Vec<String> = line
                .split('|')
                .skip(1)
                .take_while(|s| !s.is_empty())
                .map(|s| s.trim().to_string())
                .collect();
            current_table.push(cells);
        } else if !current_table.is_empty() {
            structure.tables.push(current_table.clone());
            current_table.clear();
        }
    }
    if !current_table.is_empty() {
        structure.tables.push(current_table);
    }

    // Extract lists
    let mut current_list = Vec::new();
    for line in content.lines() {
        if let Some(captures) = list_regex.captures(line) {
            current_list.push(captures.get(1).unwrap().as_str().to_string());
        } else if !current_list.is_empty() {
            structure.lists.push(current_list.clone());
            current_list.clear();
        }
    }
    if !current_list.is_empty() {
        structure.lists.push(current_list);
    }

    structure
}

#[derive(Default)]
struct MarkdownStructure {
    headers: Vec<(usize, String)>,
    tables: Vec<Vec<Vec<String>>>,
    lists: Vec<Vec<String>>,
}

fn bench_markdown_normalization_basic(c: &mut Criterion) {
    let mut group = c.benchmark_group("markdown_normalization");

    // Test content of varying complexity
    let simple_md = r#"
# Header
Some **bold** and *italic* text.
- List item 1
- List item 2
"#;

    let medium_md = std::fs::read_to_string(fixture("tests/fixtures/files/financial_summary.md"))
        .unwrap_or_else(|_| "# Fallback Content\nFallback content".to_string());

    let complex_md =
        std::fs::read_to_string(fixture("tests/fixtures/files/cbre_appraisal_sample.md"))
            .unwrap_or_else(|_| "# Fallback Content\nFallback content".to_string());

    for (name, content) in [
        ("simple", simple_md),
        ("medium", medium_md.as_str()),
        ("complex", complex_md.as_str()),
    ] {
        group.throughput(Throughput::Bytes(content.len() as u64));
        group.bench_function(name.to_string(), |b| {
            b.iter(|| black_box(normalize_markdown(black_box(content))));
        });
    }

    group.finish();
}

fn bench_markdown_structure_extraction(c: &mut Criterion) {
    let mut group = c.benchmark_group("markdown_structure");

    let table_heavy_md = r#"
# Data Report

| Column 1 | Column 2 | Column 3 |
|----------|----------|----------|
| Value 1  | Value 2  | Value 3  |
| Data A   | Data B   | Data C   |

## Summary

| Metric | Value | Change |
|--------|-------|--------|
| Revenue | $1M | +15% |
| Users | 10K | +25% |
"#;

    let list_heavy_md = r#"
# Tasks

- [ ] Complete documentation
- [x] Write tests
- [ ] Deploy to production

## Features

- Authentication system
  - Login/logout
  - Password reset
  - Two-factor auth
- Data processing
  - CSV import
  - PDF generation
  - Email notifications
"#;

    for (name, content) in [
        ("table_heavy", table_heavy_md),
        ("list_heavy", list_heavy_md),
    ] {
        group.throughput(Throughput::Bytes(content.len() as u64));
        group.bench_function(name.to_string(), |b| {
            b.iter(|| black_box(extract_markdown_structure(black_box(content))));
        });
    }

    group.finish();
}

fn bench_markdown_patterns(c: &mut Criterion) {
    let mut group = c.benchmark_group("markdown_patterns");

    // Test specific markdown patterns that are common in documents

    // Code blocks
    let code_heavy = "`inline code` and ```\nblock code\nwith multiple lines\n```".repeat(10);
    group.bench_function("code_blocks", |b| {
        b.iter(|| black_box(normalize_markdown(black_box(&code_heavy))));
    });

    // Links
    let link_heavy =
        "[Link text](https://example.com) and [Another link](https://test.com)".repeat(20);
    group.bench_function("links", |b| {
        b.iter(|| black_box(normalize_markdown(black_box(&link_heavy))));
    });

    // Headers
    let header_heavy = (1..=6)
        .map(|level| format!("{} Header Level {}\n", "#".repeat(level), level))
        .collect::<Vec<_>>()
        .join("\n")
        .repeat(5);
    group.bench_function("headers", |b| {
        b.iter(|| black_box(normalize_markdown(black_box(&header_heavy))));
    });

    // Mixed formatting
    let formatting_heavy = "**Bold** *italic* ***both*** ~~strikethrough~~ `code`".repeat(15);
    group.bench_function("formatting", |b| {
        b.iter(|| black_box(normalize_markdown(black_box(&formatting_heavy))));
    });

    group.finish();
}

fn bench_large_document_normalization(c: &mut Criterion) {
    let mut group = c.benchmark_group("large_documents");

    // Generate documents of varying sizes
    let base_content = r#"
# Section Header

This is a paragraph with **bold** and *italic* text.

## Subsection

- List item 1
- List item 2 with `inline code`
- List item 3

### Data Table

| Column A | Column B | Column C |
|----------|----------|----------|
| Value 1  | Value 2  | Value 3  |
| Data X   | Data Y   | Data Z   |

Code example:
```rust
fn example() {
    println!("Hello, world!");
}
```

[Link to documentation](https://docs.example.com)
"#;

    for scale in [1usize, 5, 10, 25, 50] {
        let content = base_content.repeat(scale);
        let size_name = format!("{}x", scale);

        group.throughput(Throughput::Bytes(content.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("normalization", &size_name),
            &content,
            |b, content| {
                b.iter(|| black_box(normalize_markdown(black_box(content))));
            },
        );

        group.bench_with_input(
            BenchmarkId::new("structure_extraction", &size_name),
            &content,
            |b, content| {
                b.iter(|| black_box(extract_markdown_structure(black_box(content))));
            },
        );
    }

    group.finish();
}

fn bench_regex_vs_manual_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("parsing_approaches");

    let test_content = r#"
# Main Title
## Subtitle
### Sub-subtitle

**Bold text** and *italic text* and ***bold italic***.

- First item
- Second item
- Third item

`inline code` and:

```rust
// code block
fn main() {
    println!("test");
}
```

[Link](https://example.com) and [Another](https://test.com).
"#;

    // Regex-based approach (current implementation)
    group.bench_function("regex_based", |b| {
        b.iter(|| black_box(normalize_markdown(black_box(test_content))));
    });

    // Manual character-by-character parsing
    group.bench_function("manual_parsing", |b| {
        b.iter(|| black_box(normalize_markdown_manual(black_box(test_content))));
    });

    group.finish();
}

fn normalize_markdown_manual(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    let chars: Vec<char> = content.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            '#' if i == 0 || chars[i - 1] == '\n' => {
                // Skip header markers
                while i < chars.len() && chars[i] == '#' {
                    i += 1;
                }
                if i < chars.len() && chars[i] == ' ' {
                    i += 1;
                }
            }
            '*' => {
                // Skip formatting markers
                if i + 1 < chars.len() && chars[i + 1] == '*' {
                    i += 2; // Skip **
                    if i < chars.len() && chars[i] == '*' {
                        i += 1; // Skip third * for ***
                    }
                } else {
                    i += 1; // Skip single *
                }
            }
            '`' => {
                // Skip code markers
                if i + 2 < chars.len() && chars[i + 1] == '`' && chars[i + 2] == '`' {
                    i += 3; // Skip ```
                } else {
                    i += 1; // Skip single `
                }
            }
            '-' | '+' if (i == 0 || chars[i - 1] == '\n') => {
                // Skip list markers
                i += 1;
                if i < chars.len() && chars[i] == ' ' {
                    i += 1;
                }
            }
            '[' => {
                // Skip link text, find closing ]
                i += 1;
                while i < chars.len() && chars[i] != ']' {
                    result.push(chars[i]);
                    i += 1;
                }
                if i < chars.len() {
                    i += 1; // Skip ]
                }
                // Skip (url) part
                if i < chars.len() && chars[i] == '(' {
                    while i < chars.len() && chars[i] != ')' {
                        i += 1;
                    }
                    if i < chars.len() {
                        i += 1; // Skip )
                    }
                }
            }
            c => {
                result.push(c);
                i += 1;
            }
        }
    }

    // Normalize whitespace
    regex::Regex::new(r"\s+")
        .unwrap()
        .replace_all(result.trim(), " ")
        .to_string()
}

fn temp_markdown(content: &str) -> NamedTempFile {
    let file = NamedTempFile::with_suffix(".md").expect("create markdown temp file");
    std::fs::write(file.path(), content).expect("write markdown fixture");
    file
}

fn bench_real_markdown_normalization_open(c: &mut Criterion) {
    let mut group = c.benchmark_group("markdown_normalization_real");

    let sample = std::fs::read_to_string(fixture("tests/fixtures/files/sample.md"))
        .expect("load sample markdown fixture");
    let financial = std::fs::read_to_string(fixture("tests/fixtures/files/financial_summary.md"))
        .expect("load financial markdown fixture");
    let appraisal =
        std::fs::read_to_string(fixture("tests/fixtures/files/cbre_appraisal_sample.md"))
            .expect("load appraisal markdown fixture");

    let cases = vec![
        ("sample", sample),
        ("financial_summary", financial),
        ("cbre_appraisal", appraisal),
    ];

    for (name, content) in &cases {
        let file = temp_markdown(content);
        group.throughput(Throughput::Bytes(content.len() as u64));
        group.bench_with_input(BenchmarkId::new("open", name), &file, |b, file| {
            b.iter(|| {
                let doc = MarkdownDocument::open(file.path()).expect("open markdown");
                black_box(doc.normalized.len() + doc.headings.len() + doc.tables.len());
            });
        });
    }

    let sample_content = std::fs::read_to_string(fixture("tests/fixtures/files/sample.md"))
        .expect("load sample markdown fixture");
    for multiplier in [1usize, 5, 10, 25] {
        let content = sample_content.repeat(multiplier);
        let file = temp_markdown(&content);
        group.throughput(Throughput::Bytes(content.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("open_scaled", multiplier),
            &file,
            |b, file| {
                b.iter(|| {
                    let doc = MarkdownDocument::open(file.path()).expect("open markdown");
                    black_box(doc.normalized.len() + doc.sections.len() + doc.tables.len());
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_markdown_normalization_basic,
    bench_markdown_structure_extraction,
    bench_markdown_patterns,
    bench_large_document_normalization,
    bench_regex_vs_manual_parsing,
    bench_real_markdown_normalization_open
);
criterion_main!(benches);
