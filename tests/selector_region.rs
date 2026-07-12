use fingerprint::document::html::{HtmlDocument, is_hard_page_break};
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
