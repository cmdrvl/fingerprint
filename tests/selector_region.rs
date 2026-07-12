use fingerprint::compile::validate::validate_definition;
use fingerprint::document::html::{HtmlDocument, is_hard_page_break};
use fingerprint::document::markdown::MarkdownDocument;
use fingerprint::document::{Document, open_document_from_path};
use fingerprint::dsl::extract::extract;
use fingerprint::dsl::parser::{ExtractSection, FingerprintDefinition};
use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

fn fixture(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

fn open_fixture(path: &str) -> HtmlDocument {
    HtmlDocument::open(&fixture(path)).expect("open html fixture")
}

fn make_temp_html(contents: &str) -> NamedTempFile {
    let mut file = NamedTempFile::with_suffix(".html").expect("create temporary html fixture");
    file.write_all(contents.as_bytes())
        .expect("write temporary html fixture");
    file.flush().expect("flush temporary html fixture");
    file
}

fn hard_break_count(document: &HtmlDocument, selector: &str) -> usize {
    document
        .select_nodes(selector)
        .expect("selector should parse")
        .into_iter()
        .filter(|node| node.value().attr("style").is_some_and(is_hard_page_break))
        .count()
}

fn region_section(name: &str, anchor_selector: &str, stop_selector: &str) -> ExtractSection {
    ExtractSection {
        name: name.to_owned(),
        r#type: "region".to_owned(),
        anchor_selector: Some(anchor_selector.to_owned()),
        stop_selector: Some(stop_selector.to_owned()),
        ..Default::default()
    }
}

fn region_value(path: &Path, section: ExtractSection) -> Option<Value> {
    let document = open_document_from_path(path).expect("open document");
    let mut extracted = extract(&document, &[section]).expect("extract region");
    extracted.remove("soi_region")
}

fn assert_region_has_no_strings(value: &Value) {
    assert!(
        !region_contains_string(value),
        "region output must not contain string values"
    );
}

fn region_contains_string(value: &Value) -> bool {
    match value {
        Value::String(_) => true,
        Value::Array(items) => items.iter().any(region_contains_string),
        Value::Object(map) => map.values().any(region_contains_string),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

#[test]
fn pgb_u01_oxsq_page_break_selectors_match_hard_breaks_in_document_order() {
    let document = open_fixture("tests/fixtures/html/oxsq_pagebreaks.html");
    let selected = document
        .select_nodes(r#"[style*="page-break-after"]"#)
        .expect("select page-break-after nodes");
    let hard_breaks: Vec<_> = selected
        .iter()
        .copied()
        .filter(|node| node.value().attr("style").is_some_and(is_hard_page_break))
        .collect();
    let first_three: Vec<_> = hard_breaks
        .iter()
        .take(3)
        .map(|node| {
            format!(
                "{}.{}",
                node.value().name(),
                node.value().attr("class").unwrap_or("")
            )
        })
        .collect();

    eprintln!(
        "oxsq hard page breaks: count={} first_three={first_three:?}",
        hard_breaks.len()
    );

    assert_eq!(selected.len(), 68);
    assert_eq!(hard_breaks.len(), 68);
    assert!(hard_breaks.iter().all(|node| node.value().name() == "span"));
}

#[test]
fn pgb_u02_ares_hr_page_breaks_are_selectable() {
    let file = make_temp_html(
        r#"
        <html>
          <body>
            <div>Schedule of Investments</div>
            <hr style="page-break-after:always" />
            <div>Continuation</div>
            <hr style="page-break-after: always" />
          </body>
        </html>
        "#,
    );
    let document = HtmlDocument::open(file.path()).expect("open temporary html fixture");
    let selected = document
        .select_nodes(r#"hr[style*="page-break-after"]"#)
        .expect("select hr page breaks");
    let hard_breaks: Vec<_> = selected
        .iter()
        .copied()
        .filter(|node| node.value().attr("style").is_some_and(is_hard_page_break))
        .collect();

    assert_eq!(hard_breaks.len(), 2);
    assert!(hard_breaks.iter().all(|node| node.value().name() == "hr"));
}

#[test]
fn pgb_u03_auto_and_avoid_styles_are_not_hard_breaks() {
    let file = make_temp_html(
        r#"
        <html>
          <body>
            <span style="page-break-after:auto"></span>
            <span style="page-break-after: avoid"></span>
            <span style="page-break-after: always"></span>
          </body>
        </html>
        "#,
    );
    let document = HtmlDocument::open(file.path()).expect("open temporary html fixture");
    let selected = document
        .select_nodes(r#"[style*="page-break-after"]"#)
        .expect("select page-break-after nodes");

    assert_eq!(selected.len(), 3);
    assert_eq!(
        hard_break_count(&document, r#"[style*="page-break-after"]"#),
        1
    );
}

#[test]
fn pgb_u04_data_page_number_model_remains_frozen_when_css_breaks_exist() {
    let file = make_temp_html(
        r#"
        <html>
          <body>
            <section data-page-number="1">
              <h1>Schedule of Investments</h1>
              <span style="page-break-after:always"></span>
            </section>
            <section data-page-number="2">
              <h2>Continuation</h2>
            </section>
          </body>
        </html>
        "#,
    );
    let document = HtmlDocument::open(file.path()).expect("open temporary html fixture");

    assert_eq!(
        document.page_sections, 2,
        "CSS page breaks must not synthesize global page sections"
    );
    assert_eq!(
        hard_break_count(&document, r#"[style*="page-break-after"]"#),
        1
    );
}

#[test]
fn pgb_u05_hard_break_detection_tolerates_whitespace_and_modern_break_properties() {
    let file = make_temp_html(
        r#"
        <html>
          <body>
            <span style="page-break-after : always"></span>
            <span style="page-break-before:  always"></span>
            <span style="break-after: page"></span>
            <span style="break-before : page"></span>
            <span style="page-break-after: avoid"></span>
          </body>
        </html>
        "#,
    );
    let document = HtmlDocument::open(file.path()).expect("open temporary html fixture");

    assert_eq!(
        document.select_nodes(r#"[style*="break"]"#).unwrap().len(),
        5
    );
    assert_eq!(hard_break_count(&document, r#"[style*="break"]"#), 4);
}

#[test]
fn rgn_e01_selector_region_covers_contiguous_tables_and_data_pages() {
    let file = make_temp_html(
        r#"
        <html>
          <body>
            <section data-page-number="5">
              <div class="major">Schedule of Investments</div>
              <table>
                <tr><th>Company</th><th>Fair Value</th></tr>
                <tr><td>Alpha</td><td>10</td></tr>
              </table>
            </section>
            <section data-page-number="6">
              <div class="minor">Healthcare</div>
              <table>
                <tr><th>Company</th><th>Fair Value</th></tr>
                <tr><td>Beta</td><td>20</td></tr>
              </table>
            </section>
            <section data-page-number="68">
              <div class="major">Notes to Financial Statements</div>
              <table>
                <tr><th>Note</th></tr>
                <tr><td>Outside region</td></tr>
              </table>
            </section>
          </body>
        </html>
        "#,
    );

    let region = region_value(
        file.path(),
        region_section("soi_region", ".major", ".major"),
    )
    .expect("region should be extracted");

    eprintln!(
        "RGN-E01 start_line={} end_line={} table_count={} page_span={}",
        region["start_line"],
        region["end_line"],
        region["table_indices"]
            .as_array()
            .expect("table indices")
            .len(),
        region["page_span"]
    );

    assert_eq!(region["table_indices"], serde_json::json!([0, 1]));
    assert_eq!(region["page_span"], serde_json::json!([5, 6]));
    assert!(
        region["start_line"].as_u64().expect("start line")
            < region["end_line"].as_u64().expect("end line")
    );
    assert_region_has_no_strings(&region);
}

#[test]
fn rgn_e02_continue_past_suppresses_running_header_stop_candidate() {
    let file = make_temp_html(
        r#"
        <html>
          <body>
            <section data-page-number="5">
              <div class="major">Schedule of Investments</div>
              <table>
                <tr><th>Company</th></tr>
                <tr><td>Alpha</td></tr>
              </table>
            </section>
            <section data-page-number="6">
              <div class="major">(continued)</div>
              <table>
                <tr><th>Company</th></tr>
                <tr><td>Beta</td></tr>
              </table>
            </section>
            <section data-page-number="7">
              <div class="major">Notes to Financial Statements</div>
            </section>
          </body>
        </html>
        "#,
    );
    let mut section = region_section("soi_region", ".major", ".major");
    section.continue_past = vec![r"(?i)^\(continued\)$".to_owned()];

    let region = region_value(file.path(), section).expect("region should be extracted");

    assert_eq!(region["table_indices"], serde_json::json!([0, 1]));
    assert_eq!(region["page_span"], serde_json::json!([5, 6]));
    assert_region_has_no_strings(&region);
}

#[test]
fn rgn_u03_no_anchor_omits_region_without_error() {
    let file = make_temp_html(
        r#"
        <html>
          <body>
            <h2>Notes to Financial Statements</h2>
            <table><tr><th>Note</th></tr><tr><td>Outside</td></tr></table>
          </body>
        </html>
        "#,
    );

    let region = region_value(
        file.path(),
        region_section("soi_region", ".missing-anchor", "h2"),
    );

    assert!(region.is_none());
}

#[test]
fn rgn_u04_nested_lower_rank_divider_does_not_truncate_region() {
    let file = make_temp_html(
        r#"
        <html>
          <body>
            <h1 class="major">Schedule of Investments</h1>
            <table><tr><th>Company</th></tr><tr><td>Alpha</td></tr></table>
            <h2 class="minor">Healthcare</h2>
            <table><tr><th>Company</th></tr><tr><td>Beta</td></tr></table>
            <h1 class="major">Notes to Financial Statements</h1>
          </body>
        </html>
        "#,
    );

    let region = region_value(
        file.path(),
        region_section("soi_region", "h1.major", "h1.major"),
    )
    .expect("region should be extracted");

    assert_eq!(region["table_indices"], serde_json::json!([0, 1]));
    assert_region_has_no_strings(&region);
}

#[test]
fn rgn_u05_region_output_is_deterministic_with_duplicate_text() {
    let file = make_temp_html(
        r#"
        <html>
          <body>
            <section data-page-number="1">
              <div class="major">Schedule of Investments</div>
              <p>Schedule of Investments</p>
              <table><tr><th>Company</th></tr><tr><td>Alpha</td></tr></table>
            </section>
            <section data-page-number="2">
              <div class="major">Notes to Financial Statements</div>
            </section>
          </body>
        </html>
        "#,
    );
    let section = region_section("soi_region", ".major", ".major");

    let first = region_value(file.path(), section.clone()).expect("first region");
    let second = region_value(file.path(), section).expect("second region");
    let first_bytes = serde_json::to_vec(&first).expect("serialize first region");
    let second_bytes = serde_json::to_vec(&second).expect("serialize second region");

    assert_eq!(first_bytes, second_bytes);
    assert_region_has_no_strings(&first);
}

#[test]
fn rgn_u06_region_is_html_scoped_at_runtime_and_validation() {
    let mut markdown_file =
        NamedTempFile::with_suffix(".md").expect("create temporary markdown fixture");
    markdown_file
        .write_all(b"# Schedule of Investments\n\n| Company |\n| --- |\n| Alpha |\n")
        .expect("write markdown");
    markdown_file.flush().expect("flush markdown");
    let markdown = MarkdownDocument::open(markdown_file.path()).expect("open markdown");
    let section = region_section("soi_region", "h1", "h2");
    let extracted = extract(
        &Document::Markdown(markdown),
        std::slice::from_ref(&section),
    )
    .expect("extract markdown");

    assert!(
        extracted.is_empty(),
        "runtime region extraction must not fake a region from non-html documents"
    );

    let definition = FingerprintDefinition {
        fingerprint_id: "markdown-region.v1".to_owned(),
        format: "markdown".to_owned(),
        valid_from: None,
        valid_until: None,
        parent: None,
        assertions: vec![],
        extract: vec![section],
        content_hash: None,
    };
    let error = validate_definition(&definition).expect_err("markdown region should fail");

    assert!(error.contains("html-only"));
}

#[test]
fn rgn_validation_rejects_malformed_region_selector() {
    let definition = FingerprintDefinition {
        fingerprint_id: "bad-region.v1".to_owned(),
        format: "html".to_owned(),
        valid_from: None,
        valid_until: None,
        parent: None,
        assertions: vec![],
        extract: vec![region_section("soi_region", "div[", "h2")],
        content_hash: None,
    };

    let error = validate_definition(&definition).expect_err("bad region selector should fail");

    assert!(error.contains("invalid CSS selector"));
}
