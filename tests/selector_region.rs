use fingerprint::compile::validate::validate_definition;
use fingerprint::document::html::{HtmlDocument, is_hard_page_break};
use fingerprint::document::markdown::MarkdownDocument;
use fingerprint::document::{Document, open_document_from_path};
use fingerprint::dsl::extract::extract;
use fingerprint::dsl::parser::{ExtractSection, FingerprintDefinition};
use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::{NamedTempFile, tempdir};

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

fn write_jsonl(records: &[Value]) -> NamedTempFile {
    let mut file = NamedTempFile::new().expect("create temporary JSONL manifest");
    for record in records {
        serde_json::to_writer(&mut file, record).expect("write JSONL record");
        file.write_all(b"\n").expect("write JSONL newline");
    }
    file.flush().expect("flush JSONL manifest");
    file
}

fn parse_jsonl(stdout: &[u8]) -> Vec<Value> {
    String::from_utf8(stdout.to_vec())
        .expect("stdout should be UTF-8")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("parse JSONL line"))
        .collect()
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
        !region_contains_disallowed_string(value),
        "region output must not contain document-text string values"
    );
}

fn region_contains_disallowed_string(value: &Value) -> bool {
    match value {
        Value::String(text) => !is_iso_date(text),
        Value::Array(items) => items.iter().any(region_contains_disallowed_string),
        Value::Object(map) => map.values().any(region_contains_disallowed_string),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

fn is_iso_date(value: &str) -> bool {
    matches!(
        value.as_bytes(),
        [y0, y1, y2, y3, b'-', m0, m1, b'-', d0, d1]
            if y0.is_ascii_digit()
                && y1.is_ascii_digit()
                && y2.is_ascii_digit()
                && y3.is_ascii_digit()
                && m0.is_ascii_digit()
                && m1.is_ascii_digit()
                && d0.is_ascii_digit()
                && d1.is_ascii_digit()
    )
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
              <div class="major anchor">Schedule of Investments</div>
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
        region_section("soi_region", ".anchor", ".major"),
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
              <div class="major anchor">Schedule of Investments</div>
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
    let mut section = region_section("soi_region", ".anchor", ".major");
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
            <h1 class="major anchor">Schedule of Investments</h1>
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
        region_section("soi_region", "h1.anchor", "h1.major"),
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
              <div class="major anchor">Schedule of Investments</div>
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
    let section = region_section("soi_region", ".anchor", ".major");

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
fn date_e03_multiple_anchor_regions_emit_document_order_regions_with_local_as_of_tags() {
    let file = make_temp_html(
        r#"
        <html>
          <body>
            <section data-page-number="5">
              <h1 class="soi">CONSOLIDATED SCHEDULE OF INVESTMENTS September 30, 2025</h1>
              <table>
                <tr><th>Company</th></tr>
                <tr><td>Alpha</td></tr>
              </table>
            </section>
            <section data-page-number="82">
              <h1 class="soi">CONSOLIDATED SCHEDULE OF INVESTMENTS December 31, 2024</h1>
              <table>
                <tr><th>Company</th></tr>
                <tr><td>Beta</td></tr>
              </table>
            </section>
            <section data-page-number="90">
              <h2 class="notes">Notes to Financial Statements</h2>
            </section>
          </body>
        </html>
        "#,
    );

    let region = region_value(
        file.path(),
        region_section("soi_region", "h1.soi", "h2.notes"),
    )
    .expect("multi-region wrapper should be extracted");
    let regions = region["regions"].as_array().expect("regions array");
    let current = regions.first().expect("current region");
    let comparative = regions.get(1).expect("comparative region");

    assert_eq!(regions.len(), 2);
    assert_eq!(current["as_of"], serde_json::json!("2025-09-30"));
    assert_eq!(comparative["as_of"], serde_json::json!("2024-12-31"));
    assert_eq!(current["table_indices"], serde_json::json!([0]));
    assert_eq!(comparative["table_indices"], serde_json::json!([1]));
    assert_eq!(current["page_span"], serde_json::json!([5, 5]));
    assert_eq!(comparative["page_span"], serde_json::json!([82, 82]));

    let second = region_value(
        file.path(),
        region_section("soi_region", "h1.soi", "h2.notes"),
    )
    .expect("second multi-region extraction");
    assert_eq!(
        serde_json::to_vec(&region).unwrap(),
        serde_json::to_vec(&second).unwrap()
    );
    assert_region_has_no_strings(&region);
}

#[test]
fn date_e04_single_anchor_keeps_single_object_shape_with_as_of() {
    let file = make_temp_html(
        r#"
        <html>
          <body>
            <section data-page-number="1">
              <h1 class="soi">Schedule of Investments Sep 30, 2025</h1>
              <table><tr><th>Company</th></tr><tr><td>Alpha</td></tr></table>
            </section>
            <section data-page-number="2">
              <h2 class="notes">Notes to Financial Statements</h2>
            </section>
          </body>
        </html>
        "#,
    );

    let region = region_value(
        file.path(),
        region_section("soi_region", "h1.soi", "h2.notes"),
    )
    .expect("single region should be extracted");

    assert!(region.get("regions").is_none());
    assert_eq!(region["as_of"], serde_json::json!("2025-09-30"));
    assert_eq!(region["table_indices"], serde_json::json!([0]));
    assert_region_has_no_strings(&region);
}

#[test]
fn date_u02_run_mode_region_as_of_is_timezone_independent() {
    let definitions_dir = tempdir().expect("create definitions dir");
    std::fs::write(
        definitions_dir.path().join("tz-region.fp.yaml"),
        r#"
fingerprint_id: tz-region.v1
format: html
assertions:
  - node_exists:
      selector: "h1.soi"
extract:
  - name: soi_region
    type: region
    anchor_selector: "h1.soi"
    stop_selector: "h2.notes"
"#,
    )
    .expect("write timezone test definition");
    let trust_file = NamedTempFile::new().expect("create trust file");
    std::fs::write(trust_file.path(), "trust:\n  - \"installed:*\"\n").expect("write trust file");
    let html_path = fixture("tests/fixtures/html/ares_multi_soi.html");
    let manifest = write_jsonl(&[serde_json::json!({
        "version": "hash.v0",
        "path": html_path.display().to_string(),
        "extension": ".html",
        "bytes_hash": "blake3:ares-multi-soi",
        "tool_versions": { "hash": "0.1.0" }
    })]);

    let run_with_tz = |timezone: Option<&str>| -> Value {
        let mut command = Command::new(env!("CARGO_BIN_EXE_fingerprint"));
        command
            .arg(manifest.path())
            .args(["--fp", "tz-region.v1", "--no-witness"])
            .env("FINGERPRINT_DEFINITIONS", definitions_dir.path())
            .env("FINGERPRINT_TRUST", trust_file.path())
            .current_dir(env!("CARGO_MANIFEST_DIR"));
        if let Some(timezone) = timezone {
            command.env("TZ", timezone);
        }
        let output = command.output().expect("run fingerprint binary");
        assert_eq!(
            output.status.code(),
            Some(0),
            "stdout={}\nstderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        let records = parse_jsonl(&output.stdout);
        let record = records.first().expect("one output record");
        record["fingerprint"]["extracted"]["soi_region"].clone()
    };

    let default_tz = run_with_tz(None);
    let asia_kolkata = run_with_tz(Some("Asia/Kolkata"));

    assert_eq!(default_tz, asia_kolkata);
    let default_regions = default_tz["regions"].as_array().expect("regions array");
    assert_eq!(
        default_regions.first().expect("current region")["as_of"],
        "2025-09-30"
    );
    assert_eq!(
        default_regions.get(1).expect("comparative region")["as_of"],
        "2024-12-31"
    );
    assert_region_has_no_strings(&default_tz);
}

#[test]
fn date_u05_as_of_current_is_not_resolved_by_fingerprint() {
    let definition: FingerprintDefinition = serde_yaml::from_str(
        r#"
fingerprint_id: ignored-current.v1
format: html
assertions: []
extract:
  - name: soi_region
    type: region
    anchor_selector: "h1"
    stop_selector: "h2"
    as_of: current
"#,
    )
    .expect("parse definition with unsupported as_of key");

    validate_definition(&definition).expect("unknown as_of key is ignored, not resolved");
    let serialized_extract = serde_json::to_value(
        definition
            .extract
            .first()
            .expect("definition should contain one extract"),
    )
    .expect("serialize extract section");

    assert!(serialized_extract.get("as_of").is_none());
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
