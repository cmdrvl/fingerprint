use fingerprint::compile::validate::validate_definition;
use fingerprint::document::html::{HtmlDocument, is_hard_page_break};
use fingerprint::document::markdown::MarkdownDocument;
use fingerprint::document::{Document, open_document_from_path};
use fingerprint::dsl::content_hash::content_hash;
use fingerprint::dsl::extract::extract;
use fingerprint::dsl::parser::{ExtractSection, FingerprintDefinition};
use serde_json::{Value, json};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::{NamedTempFile, tempdir};

#[test]
fn tokenized_region_offsets_preserve_exact_source_bytes() {
    let raw = "<HTML><BODY>é🙂<H1 title='a > b' class=soi>Schedule of Investments</H1><table><tr><td>Alpha</td></tr></table><H2 class=notes>Notes</H2></BODY></HTML>";
    let file = make_temp_html(raw);
    let mut section = region_section("soi_region", "h1.soi", "h2.notes");
    let legacy = region_value(file.path(), section.clone()).unwrap();
    assert_eq!(legacy["byte_offsets"], Value::Null);
    section.tokenized_byte_offsets = Some(false);
    assert_eq!(region_value(file.path(), section.clone()).unwrap(), legacy);
    section.tokenized_byte_offsets = Some(true);
    let mapped = region_value(file.path(), section).unwrap();
    let start = raw.find("<H1").unwrap();
    let end = raw.find("<H2").unwrap();
    assert_eq!(
        mapped["byte_offsets"],
        serde_json::json!({"start":start,"end":end})
    );
    assert_eq!(mapped["table_indices"], serde_json::json!([0]));
    for field in ["start_line", "end_line", "page_span", "as_of"] {
        assert_eq!(mapped[field], legacy[field]);
    }
    assert!(raw[start..end].contains("Alpha"));
    assert!(!raw[start..end].contains("<H2"));
}

#[test]
fn tokenized_offsets_bind_identical_heading_to_its_selected_ancestor() {
    let raw = "<section class='decoy'><h1>Schedule</h1></section><section class='target'><h1>Schedule</h1><table><tr><td>Kept</td></tr></table></section><h2>Notes</h2>";
    let file = make_temp_html(raw);
    let mut section = region_section("soi_region", ".target h1", "h2");
    section.tokenized_byte_offsets = Some(true);
    let mapped = region_value(file.path(), section).unwrap();
    assert_eq!(mapped["byte_offsets"]["start"], raw.rfind("<h1>").unwrap());
    assert_eq!(mapped["byte_offsets"]["end"], raw.find("<h2>").unwrap());
}

#[test]
fn tokenized_offsets_do_not_claim_lossy_utf8_positions() {
    let mut file = NamedTempFile::with_suffix(".html").unwrap();
    file.write_all(b"<html><body>\xff<h1>Schedule</h1><table><tr><td>Alpha</td></tr></table><h2>Notes</h2></body></html>").unwrap();
    file.flush().unwrap();
    let mut section = region_section("soi_region", "h1", "h2");
    section.tokenized_byte_offsets = Some(true);
    let mapped = region_value(file.path(), section).unwrap();
    assert_eq!(mapped["byte_offsets"], Value::Null);
    assert_eq!(mapped["table_indices"], serde_json::json!([0]));
}

#[test]
fn tokenized_offsets_to_eof_are_deterministic() {
    let raw = "<H1 class=soi>Schedule</H1><table><tr><td>Alpha</td></tr></table>";
    let file = make_temp_html(raw);
    let mut section = region_section("soi_region", "h1.soi", "h2.notes");
    section.tokenized_byte_offsets = Some(true);
    let first = region_value(file.path(), section.clone()).unwrap();
    assert_eq!(
        first["byte_offsets"],
        serde_json::json!({"start":0,"end":raw.len()})
    );
    assert_eq!(first, region_value(file.path(), section).unwrap());
}

#[test]
fn tokenized_option_roundtrips_without_changing_absent_serialization() {
    let absent: ExtractSection = serde_yaml::from_str("name: region\ntype: region\n").unwrap();
    assert!(
        serde_json::to_value(&absent)
            .unwrap()
            .get("tokenized_byte_offsets")
            .is_none()
    );
    let explicit: ExtractSection =
        serde_yaml::from_str("name: region\ntype: region\ntokenized_byte_offsets: true\n").unwrap();
    assert_eq!(explicit.tokenized_byte_offsets, Some(true));
    assert!(
        serde_yaml::from_str::<ExtractSection>(
            "name: region\ntype: region\ntokenized_byte_offsets: invalid\n"
        )
        .is_err()
    );
}

#[test]
fn element_extract_cli_binds_each_repeated_source_marker_without_copying_text() {
    let raw = "<html><body><table><tr><td>Borrower A <span>(Q)</span></td><td>100</td></tr><tr><td>Borrower B <span>(Q)</span></td><td>200</td></tr></table><p>Footer</p></body></html>";
    let file = make_temp_html(raw);
    let markers = element_section("markers", "span", Some(r"^\(Q\)$"));
    let marker_text = ExtractSection {
        name: "marker_text".to_owned(),
        r#type: "text_match".to_owned(),
        anchor: Some("Borrower".to_owned()),
        pattern: Some("Borrower".to_owned()),
        within_chars: Some(200),
        ..Default::default()
    };
    let document = open_document_from_path(file.path()).expect("open BDC marker HTML");
    let with_elements = extract(&document, &[markers.clone(), marker_text.clone()])
        .expect("extract source-bounded markers");
    let without_elements =
        extract(&document, &[marker_text]).expect("extract text match without element locator");
    let hash_scope = ["marker_text".to_owned()];
    let expected_hash = content_hash(&without_elements, &hash_scope);
    assert_eq!(
        content_hash(&with_elements, &hash_scope),
        expected_hash,
        "adding locator metadata must not change a content hash scoped to another extract"
    );

    let definitions = tempdir().expect("create installed definitions directory");
    fs::write(
        definitions.path().join("marker-probe.fp.yaml"),
        r#"
fingerprint_id: marker-probe.v1
format: html
assertions:
  - node_count:
      selector: table
      min: 1
extract:
  - name: markers
    type: element
    selector: span
    text_regex: '^\(Q\)$'
  - name: marker_text
    type: text_match
    anchor: Borrower
    pattern: Borrower
    within_chars: 200
content_hash:
  algorithm: blake3
  over: [marker_text]
"#,
    )
    .expect("write marker probe definition");
    let trust = NamedTempFile::new().expect("create trust config");
    fs::write(trust.path(), "trust:\n  - \"installed:*\"\n").expect("write trust config");
    let manifest = write_jsonl(&[json!({
        "version": "hash.v0",
        "path": file.path().display().to_string(),
        "extension": ".html",
        "bytes_hash": "sha256:79659abf6c28a62aac0c956a323925329baeea947c127d0e24e6043d897dc954",
        "tool_versions": { "hash": "0.1.0" }
    })]);
    let output = Command::new(env!("CARGO_BIN_EXE_fingerprint"))
        .arg(manifest.path())
        .args(["--fp", "marker-probe.v1", "--no-witness"])
        .env("FINGERPRINT_DEFINITIONS", definitions.path())
        .env("FINGERPRINT_TRUST", trust.path())
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run native fingerprint CLI");
    assert_eq!(
        output.status.code(),
        Some(0),
        "marker CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let records = parse_jsonl(&output.stdout);
    assert_eq!(records.len(), 1);
    let extracted = &records[0]["fingerprint"]["extracted"];
    assert_eq!(extracted["markers"], with_elements["markers"]);
    assert_eq!(records[0]["fingerprint"]["content_hash"], expected_hash);
    let elements = extracted["markers"]["elements"]
        .as_array()
        .expect("marker element array");
    assert_eq!(
        elements.len(),
        2,
        "equal marker text retains both DOM nodes"
    );
    assert_eq!(elements[0]["source_bound"], true);
    assert_eq!(elements[1]["source_bound"], true);
    assert_eq!(elements[0]["dom_table_index"], 0);
    assert_eq!(elements[0]["dom_row_index"], 0);
    assert_eq!(elements[1]["dom_row_index"], 1);
    assert_eq!(elements[0]["dom_cell_index"], 0);
    assert_eq!(elements[1]["dom_cell_index"], 2);
    let expected_ranges = raw
        .match_indices("<span>(Q)</span>")
        .map(|(start, source)| (start, start + source.len()))
        .collect::<Vec<_>>();
    assert_eq!(expected_ranges.len(), 2);
    for (element, (start, end)) in elements.iter().zip(expected_ranges) {
        assert_eq!(element["byte_offsets"]["start"], start);
        assert_eq!(element["byte_offsets"]["end"], end);
        assert_eq!(&raw[start..end], "<span>(Q)</span>");
    }
    let serialized = serde_json::to_string(&extracted["markers"]).unwrap();
    for source_text in ["Borrower A", "Borrower B", "100", "200", "Footer", "(Q)"] {
        assert!(
            !serialized.contains(source_text),
            "copied {source_text:?} into output"
        );
    }
}

#[test]
fn element_extract_maps_utf8_entities_and_nested_selected_nodes() {
    let raw = "<table><tr><td><span>(Q &amp; <em>é🙂</em>)</span></td><td>1&nbsp;000</td></tr></table><p>Footer</p>";
    let file = make_temp_html(raw);
    let extracted = element_value(
        file.path(),
        element_section("elements", "span, em", Some(r"(?s)(Q|é🙂)")),
    )
    .expect("matching nested elements should be emitted");
    let elements = extracted["elements"].as_array().unwrap();
    assert_eq!(elements.len(), 2);
    assert_eq!(elements[0]["tag_name"], "span");
    assert_eq!(elements[1]["tag_name"], "em");
    assert!(
        elements[0]["dom_element_index"].as_u64().unwrap()
            < elements[1]["dom_element_index"].as_u64().unwrap()
    );
    assert!(
        elements
            .iter()
            .all(|element| element["source_bound"] == true)
    );

    let span_start = raw.find("<span>").unwrap();
    let span_end = raw.find("</span>").unwrap() + "</span>".len();
    let em_start = raw.find("<em>").unwrap();
    let em_end = raw.find("</em>").unwrap() + "</em>".len();
    assert_eq!(
        elements[0]["byte_offsets"],
        json!({ "start": span_start, "end": span_end })
    );
    assert_eq!(
        elements[1]["byte_offsets"],
        json!({ "start": em_start, "end": em_end })
    );
    assert!(raw[span_start..span_end].contains("&amp;"));
    assert_eq!(&raw[em_start..em_end], "<em>é🙂</em>");
    let serialized = serde_json::to_string(&extracted).unwrap();
    for source_text in ["(Q", "&amp;", "é🙂", "1&nbsp;000", "Footer"] {
        assert!(!serialized.contains(source_text));
    }
}

#[test]
fn many_source_elements_keep_original_dom_indexes_and_exact_bounds() {
    let row = "<tr><td><span>(2)<em>é</em></span></td><td>100</td></tr>";
    let raw = format!(
        "<html><head><title>Schedule</title></head><body><p>Before</p><table>{}</table></body></html>",
        row.repeat(96)
    );
    let file = make_temp_html(&raw);
    let document = open_document_from_path(file.path()).unwrap();
    let Document::Html(html) = &document else {
        panic!("HTML fixture expected");
    };
    let expected_nodes = html.select_nodes("span, em").unwrap();
    let extracted = extract(&document, &[element_section("elements", "span, em", None)]).unwrap();
    let elements = extracted["elements"]["elements"].as_array().unwrap();
    assert_eq!(elements.len(), 192);
    for (index, (element, node)) in elements.iter().zip(expected_nodes).enumerate() {
        // The pre-existing single-node traversal is the reference for DOM order.
        assert_eq!(
            element["dom_element_index"],
            html.element_order_index(&node).unwrap()
        );
        assert_eq!(element["dom_table_index"], 0);
        assert_eq!(element["dom_row_index"], index / 2);
        assert_eq!(element["dom_cell_index"], (index / 2) * 2);
        assert_eq!(element["source_bound"], true);
        let start = element["byte_offsets"]["start"].as_u64().unwrap() as usize;
        let end = element["byte_offsets"]["end"].as_u64().unwrap() as usize;
        assert_eq!(
            &raw[start..end],
            if index % 2 == 0 {
                "<span>(2)<em>é</em></span>"
            } else {
                "<em>é</em>"
            }
        );
    }
}

#[test]
fn element_extract_keeps_matching_but_unproven_nodes_visibly_unbound() {
    let missing_end =
        make_temp_html("<table><tr><td><span>(Q)</td><td>100</td></tr></table><p>Footer</p>");
    let missing = element_value(
        missing_end.path(),
        element_section("elements", "span", Some(r"^\(Q\)$")),
    )
    .expect("text-matched node should remain observable");
    assert_eq!(missing["elements"].as_array().unwrap().len(), 1);
    assert_eq!(missing["elements"][0]["source_bound"], false);
    assert_eq!(missing["elements"][0]["byte_offsets"], Value::Null);

    let reconstructed = make_temp_html("<p><b>(Q)<p>neighbor</b>");
    let cloned = element_value(
        reconstructed.path(),
        element_section("elements", "b", Some(r"Q")),
    )
    .expect("matching reconstructed node should remain observable");
    assert!(
        cloned["elements"]
            .as_array()
            .unwrap()
            .iter()
            .all(|element| element["source_bound"] == false
                && element["byte_offsets"] == Value::Null)
    );

    let mut invalid_utf8 = NamedTempFile::with_suffix(".html").unwrap();
    invalid_utf8.write_all(b"<span>(Q)</span>\xff").unwrap();
    invalid_utf8.flush().unwrap();
    let lossy = element_value(
        invalid_utf8.path(),
        element_section("elements", "span", Some(r"Q")),
    )
    .expect("lossy DOM node remains visible");
    assert_eq!(lossy["elements"][0]["source_bound"], false);
    assert_eq!(lossy["elements"][0]["byte_offsets"], Value::Null);
}

#[test]
fn element_extract_maps_implicit_table_cell_boundaries_without_treating_slash_as_close() {
    let raw = "<table><tr><td/>kept <b>nested</b><td>next</td></tr></table>";
    assert_element_bounds(raw, "td", &["<td/>kept <b>nested</b>", "<td>next</td>"]);

    let raw = "<table><tr><td/><td/></tr></table>";
    assert_element_bounds(raw, "td", &["<td/>", "<td/>"]);

    let raw = "<table><tr><td>first<td>second</tr></table>";
    assert_element_bounds(raw, "td", &["<td>first", "<td>second"]);

    let raw = "<table><tr><td/>first<tr><td>last</table>";
    assert_element_bounds(raw, "td", &["<td/>first", "<td>last"]);

    let raw = "<table><tbody><tr><td/>row group</tbody></table>";
    assert_element_bounds(raw, "td", &["<td/>row group"]);

    let raw = "<table><tr><th/>header<th>next</tr></table>";
    assert_element_bounds(raw, "th", &["<th/>header", "<th>next"]);
}

#[test]
fn element_extract_uses_nearest_nested_table_boundaries_and_refuses_incomplete_outer_cells() {
    let raw = "<table><tr><td/>outer<table><tr><td/>inner<td>inner-next</table><td>outer-next</tr></table>";
    assert_element_bounds(
        raw,
        "td",
        &[
            "<td/>outer<table><tr><td/>inner<td>inner-next</table>",
            "<td/>inner",
            "<td>inner-next",
            "<td>outer-next",
        ],
    );

    // The nested table close is not a close for the still-open outer cell.
    let raw = "<table><tr><td/>outer<table><tr><td/>inner</table>";
    let file = make_temp_html(raw);
    let extracted = element_value(file.path(), element_section("cells", "td", None))
        .expect("native table cells should remain observable");
    let elements = extracted["elements"].as_array().unwrap();
    assert_eq!(elements.len(), 2);
    assert_eq!(elements[0]["source_bound"], false);
    assert_eq!(elements[0]["byte_offsets"], Value::Null);
    assert_eq!(elements[1]["source_bound"], true);
    let start = elements[1]["byte_offsets"]["start"].as_u64().unwrap() as usize;
    let end = elements[1]["byte_offsets"]["end"].as_u64().unwrap() as usize;
    assert_eq!(&raw[start..end], "<td/>inner");

    // With no next cell or explicit row/table boundary, EOF is not guessed as
    // a source close for malformed content.
    let raw = "<table><tr><td/>unterminated";
    let file = make_temp_html(raw);
    let extracted = element_value(file.path(), element_section("cells", "td", None))
        .expect("unclosed table cell should stay observable");
    assert_eq!(extracted["elements"].as_array().unwrap().len(), 1);
    assert_eq!(extracted["elements"][0]["source_bound"], false);
    assert_eq!(extracted["elements"][0]["byte_offsets"], Value::Null);
}

fn assert_element_bounds(raw: &str, selector: &str, expected: &[&str]) {
    let file = make_temp_html(raw);
    let extracted = element_value(file.path(), element_section("cells", selector, None))
        .expect("selected HTML elements should be extracted");
    let elements = extracted["elements"].as_array().unwrap();
    assert_eq!(elements.len(), expected.len());
    for (element, expected_source) in elements.iter().zip(expected) {
        assert_eq!(element["source_bound"], true);
        let start = element["byte_offsets"]["start"].as_u64().unwrap() as usize;
        let end = element["byte_offsets"]["end"].as_u64().unwrap() as usize;
        assert_eq!(&raw[start..end], *expected_source);
    }
}

#[test]
fn element_extract_maps_duplicate_attribute_source_without_guessing_bounds() {
    let raw = "<span data-kind='first' data-kind='second'>(Q)</span>";
    let file = make_temp_html(raw);
    let extracted = element_value(
        file.path(),
        element_section("elements", "span", Some(r"^\(Q\)$")),
    )
    .expect("explicit duplicate-attribute element should be selected");
    let element = &extracted["elements"][0];
    assert_eq!(element["source_bound"], true);
    assert_eq!(
        element["byte_offsets"],
        json!({ "start": 0, "end": raw.len() })
    );
    assert_eq!(
        &raw[element["byte_offsets"]["start"].as_u64().unwrap() as usize
            ..element["byte_offsets"]["end"].as_u64().unwrap() as usize],
        raw
    );
}

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

fn element_section(name: &str, selector: &str, text_regex: Option<&str>) -> ExtractSection {
    ExtractSection {
        name: name.to_owned(),
        r#type: "element".to_owned(),
        selector: Some(selector.to_owned()),
        text_regex: text_regex.map(ToOwned::to_owned),
        ..Default::default()
    }
}

fn region_value(path: &Path, section: ExtractSection) -> Option<Value> {
    let document = open_document_from_path(path).expect("open document");
    let mut extracted = extract(&document, &[section]).expect("extract region");
    extracted.remove("soi_region")
}

fn element_value(path: &Path, section: ExtractSection) -> Option<Value> {
    let document = open_document_from_path(path).expect("open document");
    let name = section.name.clone();
    let mut extracted = extract(&document, &[section]).expect("extract source elements");
    extracted.remove(&name)
}

fn assert_region_has_only_bounds_strings(value: &Value, allowed_selectors: &[&str]) {
    assert!(
        !region_contains_disallowed_string(value, allowed_selectors),
        "region output must not contain document-text string values"
    );
}

fn region_contains_disallowed_string(value: &Value, allowed_selectors: &[&str]) -> bool {
    match value {
        Value::String(text) => !is_iso_date(text) && !allowed_selectors.contains(&text.as_str()),
        Value::Array(items) => items
            .iter()
            .any(|item| region_contains_disallowed_string(item, allowed_selectors)),
        Value::Object(map) => map
            .values()
            .any(|item| region_contains_disallowed_string(item, allowed_selectors)),
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
    assert_region_has_only_bounds_strings(&region, &[".anchor", ".major"]);
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
    assert_region_has_only_bounds_strings(&region, &[".anchor", ".major"]);
}

#[test]
fn rgn_e03_text_filters_select_real_section_start_and_exclude_stop_page() {
    let file = make_temp_html(
        r#"
        <html>
          <body>
            <div id="toc"></div>
            <hr style="page-break-after:always" />
            <div style="min-height:72pt;width:100%">
              <div>Issuer Name</div>
              <div>Consolidated Schedule of Investments</div>
              <div>As of September 30, 2025</div>
            </div>
            <table>
              <tr><th>Portfolio Company</th><th>Fair Value</th></tr>
              <tr><td>Alpha</td><td>10</td></tr>
            </table>
            <hr style="page-break-after:always" />
            <div style="min-height:42pt;width:100%"><div><br /></div></div>
            <div><span>Derivative Instruments</span></div>
            <table>
              <tr><th>Outside</th></tr>
              <tr><td>Excluded</td></tr>
            </table>
          </body>
        </html>
        "#,
    );
    let mut section = region_section(
        "soi_region",
        r#"div[id] + hr + div[style*="min-height"]"#,
        r#"hr + div[style*="min-height"] + div"#,
    );
    section.anchor_text_regex = Some(r"(?i)schedules?\s+of\s+investments".to_owned());
    section.stop_text_regex = Some(r"(?i)derivative\s+instruments".to_owned());

    let region = region_value(file.path(), section).expect("region should be extracted");

    assert_eq!(region["table_indices"], serde_json::json!([0]));
    assert_eq!(region["page_span"], serde_json::json!([2, 2]));
    assert_eq!(region["as_of"], serde_json::json!("2025-09-30"));
    assert_region_has_only_bounds_strings(
        &region,
        &[
            r#"div[id] + hr + div[style*="min-height"]"#,
            r#"hr + div[style*="min-height"] + div"#,
        ],
    );
}

#[test]
fn rgn_e04_same_date_running_headers_merge_without_continued_marker() {
    let file = make_temp_html(
        r#"
        <html>
          <body>
            <section data-page-number="11">
              <div class="page-header">Condensed Consolidated Schedule of Investments September 30, 2025</div>
              <table>
                <tr><th>Portfolio Company</th><th>Fair Value</th></tr>
                <tr><td>Alpha</td><td>10</td></tr>
              </table>
            </section>
            <section data-page-number="12">
              <div class="page-header">Condensed Consolidated Schedule of Investments September 30, 2025</div>
              <table>
                <tr><th>Portfolio Company</th><th>Fair Value</th></tr>
                <tr><td>Beta</td><td>20</td></tr>
              </table>
            </section>
            <section data-page-number="13">
              <div class="page-header">Condensed Consolidated Schedule of Investments December 31, 2024</div>
              <table>
                <tr><th>Portfolio Company</th><th>Fair Value</th></tr>
                <tr><td>Prior</td><td>30</td></tr>
              </table>
            </section>
          </body>
        </html>
        "#,
    );
    let mut section = region_section("soi_region", ".page-header", ".page-header");
    section.anchor_text_regex = Some(r"(?i)schedules?\s+of\s+investments".to_owned());
    section.stop_text_regex = Some(r"(?i)schedules?\s+of\s+investments".to_owned());

    let region = region_value(file.path(), section).expect("region should be extracted");
    let regions = region["regions"].as_array().expect("regions array");

    assert_eq!(regions.len(), 2);
    assert_eq!(regions[0]["as_of"], serde_json::json!("2025-09-30"));
    assert_eq!(regions[0]["table_indices"], serde_json::json!([0, 1]));
    assert_eq!(regions[0]["page_span"], serde_json::json!([11, 12]));
    assert_eq!(regions[1]["as_of"], serde_json::json!("2024-12-31"));
    assert_eq!(regions[1]["table_indices"], serde_json::json!([2]));
}

#[test]
fn rgn_e05_dom_node_location_does_not_resolve_to_toc_duplicate_text() {
    let file = make_temp_html(
        r##"
        <html>
          <body>
            <table id="toc">
              <tr>
                <td><a href="#consolidated_schedules_investments">Consolidated Schedules of Investments as of September 30, 2023</a></td>
              </tr>
            </table>
            <hr style="page-break-after:always" />
            <p>Preface page</p>
            <hr style="page-break-after:always" />
            <p id="consolidated_schedules_investments" style="text-align:center">
              <span>Consolidated Schedu</span><span>les of Investments</span>
              <span>September 30, 2023</span>
            </p>
            <table id="current">
              <tr><th>Portfolio Company</th><th>Fair Value</th></tr>
              <tr><td>Alpha</td><td>10</td></tr>
            </table>
            <p style="text-align:center"><span>Consolidated Schedules of Investments</span></p>
            <p style="text-align:center"><span>December 31, 2022</span></p>
            <table id="prior">
              <tr><th>Portfolio Company</th><th>Fair Value</th></tr>
              <tr><td>Prior</td><td>5</td></tr>
            </table>
          </body>
        </html>
        "##,
    );
    let mut section = region_section(
        "soi_region",
        "p[id*='schedules'][id*='investments'], p[style*='text-align:center'] > span",
        "p[style*='text-align:center'] > span",
    );
    section.anchor_text_regex = Some(r"(?i)schedu\s*les?\s+of\s+investments".to_owned());
    section.stop_text_regex = Some(r"(?i)schedu\s*les?\s+of\s+investments".to_owned());

    let region = region_value(file.path(), section).expect("region should be extracted");
    let regions = region["regions"].as_array().expect("regions array");

    assert_eq!(regions[0]["as_of"], serde_json::json!("2023-09-30"));
    assert_eq!(regions[0]["page_span"], serde_json::json!([3, 3]));
    assert_eq!(regions[0]["table_indices"], serde_json::json!([1]));
    assert_eq!(regions[1]["as_of"], serde_json::json!("2022-12-31"));
    assert_eq!(regions[1]["table_indices"], serde_json::json!([2]));
}

#[test]
fn rgn_e06_region_table_indices_are_raw_dom_table_order() {
    let file = make_temp_html(
        r#"
        <html>
          <body>
            <h1 class="anchor">Schedule of Investments September 30, 2025</h1>
            <table id="outer">
              <tr>
                <td>
                  <table id="inner">
                    <tr><th>Nested</th></tr>
                    <tr><td>Still a raw DOM table</td></tr>
                  </table>
                </td>
              </tr>
            </table>
            <h2>Notes</h2>
          </body>
        </html>
        "#,
    );

    let region = region_value(file.path(), region_section("soi_region", ".anchor", "h2"))
        .expect("region should be extracted");

    assert_eq!(region["table_indices"], serde_json::json!([0, 1]));
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
    assert_region_has_only_bounds_strings(&region, &["h1.anchor", "h1.major"]);
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
    assert_region_has_only_bounds_strings(&first, &[".anchor", ".major"]);
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
    assert_region_has_only_bounds_strings(&region, &["h1.soi", "h2.notes"]);
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
    assert_region_has_only_bounds_strings(&region, &["h1.soi", "h2.notes"]);
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
    assert_region_has_only_bounds_strings(&default_tz, &["h1.soi", "h2.notes"]);
}

#[test]
fn bnd_g01_multi_region_bounds_record_is_golden_stable() {
    let region = region_value(
        &fixture("tests/fixtures/html/ares_multi_soi.html"),
        region_section("soi_region", "h1.soi", "h2.notes"),
    )
    .expect("fixture region should be extracted");

    assert_eq!(
        region,
        serde_json::json!({
            "regions": [
                {
                    "anchor_selector": "h1.soi",
                    "stop_selector": "h2.notes",
                    "start_line": 1,
                    "end_line": 6,
                    "table_indices": [0],
                    "page_span": [5, 5],
                    "byte_offsets": { "start": 57, "end": 370 },
                    "as_of": "2025-09-30",
                },
                {
                    "anchor_selector": "h1.soi",
                    "stop_selector": "h2.notes",
                    "start_line": 6,
                    "end_line": 11,
                    "table_indices": [1],
                    "page_span": [68, 68],
                    "byte_offsets": { "start": 370, "end": 681 },
                    "as_of": "2024-12-31",
                },
            ],
        })
    );
}

#[test]
fn bnd_g02_region_bounds_do_not_emit_document_text() {
    let region = region_value(
        &fixture("tests/fixtures/html/ares_multi_soi.html"),
        region_section("soi_region", "h1.soi", "h2.notes"),
    )
    .expect("fixture region should be extracted");

    assert_region_has_only_bounds_strings(&region, &["h1.soi", "h2.notes"]);
}

#[test]
fn bnd_u03_present_byte_offsets_point_into_raw_source() {
    let html_path = fixture("tests/fixtures/html/ares_multi_soi.html");
    let document = HtmlDocument::open(&html_path).expect("open html fixture");
    let region = region_value(
        &html_path,
        region_section("soi_region", "h1.soi", "h2.notes"),
    )
    .expect("fixture region should be extracted");
    let regions = region["regions"].as_array().expect("regions array");
    let first_region = regions.first().expect("first region");
    let offsets = first_region["byte_offsets"]
        .as_object()
        .expect("byte_offsets object");
    let start = offsets["start"].as_u64().expect("offset start") as usize;
    let end = offsets["end"].as_u64().expect("offset end") as usize;
    let raw_slice = document.raw.get(start..end).expect("offsets index raw");
    let anchor_html = document
        .select_nodes("h1.soi")
        .expect("select anchor")
        .first()
        .expect("anchor node")
        .html();
    let contains_anchor = raw_slice.contains(anchor_html.as_str());

    eprintln!("BND-U03 byte_offsets start={start} end={end}");
    assert!(
        contains_anchor,
        "byte_offsets span should contain the anchor node"
    );
}

#[test]
fn bnd_u04_unlocatable_raw_nodes_keep_authoritative_line_bounds() {
    let file = make_temp_html(
        r#"
        <HTML>
          <BODY>
            <SECTION data-page-number=5>
              <H1 class=soi>Schedule of Investments September 30, 2025</H1>
              <table><tr><th>Company</th></tr><tr><td>Alpha</td></tr></table>
            </SECTION>
            <SECTION data-page-number=6>
              <H2 class=notes>Notes to Financial Statements</H2>
            </SECTION>
          </BODY>
        </HTML>
        "#,
    );
    let region = region_value(
        file.path(),
        region_section("soi_region", "h1.soi", "h2.notes"),
    )
    .expect("region should be extracted even when raw spans cannot be located");

    assert_eq!(region["byte_offsets"], Value::Null);
    assert_eq!(region["anchor_selector"], "h1.soi");
    assert_eq!(region["stop_selector"], "h2.notes");
    assert_eq!(region["table_indices"], serde_json::json!([0]));
    assert!(
        region["start_line"].as_u64().expect("start line")
            < region["end_line"].as_u64().expect("end line")
    );
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

#[test]
fn element_validation_requires_html_and_valid_selector_and_text_regex() {
    let definition = |format: &str, section: ExtractSection| FingerprintDefinition {
        fingerprint_id: "element-validation.v1".to_owned(),
        format: format.to_owned(),
        valid_from: None,
        valid_until: None,
        parent: None,
        assertions: vec![],
        extract: vec![section],
        content_hash: None,
    };

    validate_definition(&definition(
        "html",
        element_section("elements", "span", Some(r"^\(Q\)$")),
    ))
    .expect("valid element extractor should compile");

    let missing_selector = validate_definition(&definition(
        "html",
        ExtractSection {
            name: "elements".to_owned(),
            r#type: "element".to_owned(),
            ..Default::default()
        },
    ))
    .expect_err("element selector is required");
    assert!(missing_selector.contains("requires field 'selector'"));

    let invalid_selector = validate_definition(&definition(
        "html",
        element_section("elements", "div[", None),
    ))
    .expect_err("malformed CSS selector should be rejected");
    assert!(invalid_selector.contains("invalid CSS selector"));

    let invalid_regex = validate_definition(&definition(
        "html",
        element_section("elements", "span", Some("[")),
    ))
    .expect_err("malformed visible text regex should be rejected");
    assert!(invalid_regex.contains("invalid element text_regex"));

    let non_html = validate_definition(&definition(
        "pdf",
        element_section("elements", "span", None),
    ))
    .expect_err("element extraction is HTML-only");
    assert!(non_html.contains("extract type 'element' is html-only"));
}

#[test]
fn same_period_subsections_partition_without_coalescing_and_keep_continuations() {
    let path = fixture("tests/fixtures/html/soi_subsections_repeated_period.html");
    let mut section = region_section("soi_region", "p.subsec", "p.subsec, p.notes");
    section.continue_past = vec!["(?i)continued".to_owned()];
    // Missing and explicit true preserve the historical single-region behavior.
    let default = region_value(&path, section.clone()).expect("default region");
    assert_eq!(default["table_indices"], serde_json::json!([0, 1, 2, 3]));
    section.coalesce_by_as_of = Some(true);
    assert_eq!(region_value(&path, section.clone()).unwrap(), default);
    section.coalesce_by_as_of = Some(false);
    let segmented = region_value(&path, section.clone()).expect("segmented regions");
    let regions = segmented["regions"].as_array().expect("three segments");
    assert_eq!(regions.len(), 3);
    assert_eq!(regions[0]["table_indices"], serde_json::json!([0, 1]));
    assert_eq!(regions[1]["table_indices"], serde_json::json!([2]));
    assert_eq!(regions[2]["table_indices"], serde_json::json!([3]));
    for pair in regions.windows(2) {
        assert_eq!(pair[0]["end_line"], pair[1]["start_line"]);
        assert_eq!(
            pair[0]["byte_offsets"]["end"],
            pair[1]["byte_offsets"]["start"]
        );
    }
    assert_eq!(region_value(&path, section.clone()).unwrap(), segmented);
    let without_dates = std::fs::read_to_string(&path)
        .unwrap()
        .replace("<p class=\"date\">September 30, 2025</p>", "");
    let control = make_temp_html(&without_dates);
    let control_regions = region_value(control.path(), section).unwrap();
    assert_eq!(
        control_regions["regions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|region| region["table_indices"].clone())
            .collect::<Vec<_>>(),
        vec![
            serde_json::json!([0, 1]),
            serde_json::json!([2]),
            serde_json::json!([3])
        ]
    );
    assert_region_has_only_bounds_strings(&segmented, &["p.subsec", "p.subsec, p.notes"]);
}

#[test]
fn coalescing_option_roundtrips_and_absence_preserves_serialization() {
    let absent: ExtractSection = serde_yaml::from_str("name: parts\ntype: region\n").unwrap();
    assert_eq!(absent.coalesce_by_as_of, None);
    assert!(
        serde_json::to_value(&absent)
            .unwrap()
            .get("coalesce_by_as_of")
            .is_none()
    );
    let explicit: ExtractSection =
        serde_yaml::from_str("name: parts\ntype: region\ncoalesce_by_as_of: false\n").unwrap();
    assert_eq!(explicit.coalesce_by_as_of, Some(false));
    assert_eq!(
        serde_json::from_value::<ExtractSection>(serde_json::to_value(&explicit).unwrap()).unwrap(),
        explicit
    );
}
