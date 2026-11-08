//! Owner regression: reporting dates belong to explicit source-bound title headers.
//! Real fixtures are contiguous retained filing bytes; these tests do not certify
//! acquisition identity, row truth, first-page coverage, or downstream parsing.
use fingerprint::compile::{
    codegen::generate_rust, schema::dsl_json_schema, validate::validate_definition,
};
use fingerprint::document::open_document_from_path;
use fingerprint::dsl::extract::extract;
use fingerprint::dsl::parser::{ExtractSection, FingerprintDefinition, ReportingDateConfig};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};
use tempfile::{NamedTempFile, tempdir};

const TITLE: &str = r"(?i)^consolidated\s+schedu\s*les?\s+of\s+investments(?:\s*\(continued\))?$";
const DATE: &str = r"(?i)(?:as\s+of\s+)?[a-z]+\s+[0-9]{1,2},\s*[0-9]{4}";

fn section() -> ExtractSection {
    ExtractSection {
        name: "schedule".to_owned(),
        r#type: "region".to_owned(),
        anchor_selector: Some("p".to_owned()),
        anchor_text_regex: Some(TITLE.to_owned()),
        stop_selector: Some("p".to_owned()),
        stop_text_regex: Some(format!(
            r"{TITLE}|(?i)^notes to consolidated financial statements$"
        )),
        tokenized_byte_offsets: Some(true),
        reporting_date: Some(ReportingDateConfig {
            selector: "p".to_owned(),
            text_regex: DATE.to_owned(),
        }),
        ..Default::default()
    }
}

fn definition(section: ExtractSection) -> FingerprintDefinition {
    FingerprintDefinition {
        fingerprint_id: "reporting-date-test.v1".to_owned(),
        format: "html".to_owned(),
        valid_from: None,
        valid_until: None,
        parent: None,
        assertions: vec![],
        extract: vec![section],
        content_hash: None,
    }
}

fn run_bytes(raw: &[u8], section: ExtractSection) -> Value {
    let file = NamedTempFile::with_suffix(".htm").unwrap();
    fs::write(file.path(), raw).unwrap();
    let doc = open_document_from_path(file.path()).unwrap();
    extract(&doc, &[section])
        .unwrap()
        .remove("schedule")
        .unwrap()
}

fn run(raw: &str, section: ExtractSection) -> Value {
    run_bytes(raw.as_bytes(), section)
}

fn regions(value: &Value) -> Vec<&Value> {
    match value.get("regions") {
        Some(regions) => regions.as_array().unwrap().iter().collect(),
        None => vec![value],
    }
}

fn header(date: &str, continued: bool) -> String {
    format!(
        "<p>Consolidated Schedule of Investments{}</p>{date}",
        if continued { " (continued)" } else { "" }
    )
}

fn table() -> &'static str {
    "<table><tr><th>Borrower</th><th>Maturity</th></tr><tr><td>Alpha</td><td><p>December 30, 2030</p></td></tr></table>"
}

fn notes() -> &'static str {
    "<p>Notes to Consolidated Financial Statements</p><p>March 31, 2026</p><table><tr><td>Not a schedule</td></tr></table>"
}

fn fixture(fund: &str) -> Vec<u8> {
    fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("tests/fixtures/html/reporting-date-{fund}.htm")),
    )
    .unwrap()
}

fn real_positive(fund: &str, periods: [&str; 2], table_counts: [usize; 2]) {
    let raw = fixture(fund);
    let value = run_bytes(&raw, section());
    let regions = regions(&value);
    assert_eq!(
        regions.len(),
        2,
        "{fund}: exactly current/prior, no Notes region"
    );
    let mut next_table = 0;
    for ((region, period), count) in regions.iter().zip(periods).zip(table_counts) {
        assert_eq!(region["as_of"], period);
        let expected = (next_table..next_table + count).collect::<Vec<_>>();
        assert_eq!(region["table_indices"], json!(expected));
        let lo = region["byte_offsets"]["start"].as_u64().unwrap() as usize;
        let hi = region["byte_offsets"]["end"].as_u64().unwrap() as usize;
        let cut = std::str::from_utf8(&raw[lo..hi]).unwrap();
        // The title stays in the cut; date selection must not move its boundary.
        assert!(cut.starts_with("<p "));
        assert!(
            cut[..250]
                .to_ascii_lowercase()
                .contains("consolidated schedule")
        );
        assert!(
            !cut.to_ascii_lowercase()
                .contains("notes to consolidated financial statements")
        );
        next_table += count;
    }
    let metadata: Value =
        serde_json::from_str(include_str!("fixtures/html/reporting-date-sources.json")).unwrap();
    let provenance = metadata["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["fixture"] == format!("reporting-date-{fund}.htm"))
        .unwrap();
    assert_eq!(
        raw.len() as u64,
        provenance["fixture_length"].as_u64().unwrap()
    );
    assert_eq!(next_table, table_counts.iter().sum::<usize>());
}

#[test]
fn real_pfx_current_and_comparative_keep_title_and_all_71_tables() {
    real_positive("pfx", ["2026-03-31", "2025-09-30"], [35, 36]);
}

#[test]
fn real_sar_current_and_comparative_keep_title_and_all_56_tables() {
    real_positive("sar", ["2026-05-31", "2026-02-28"], [27, 29]);
}

#[test]
fn actual_pfx_conflicting_pretable_date_is_null_and_first_table_survives() {
    let raw = String::from_utf8(fixture("pfx")).unwrap();
    let at = raw.find("<table").unwrap();
    let raw = format!(
        "{}<p style=\"text-align: center\">As of December 31, 2099</p>{}",
        &raw[..at],
        &raw[at..]
    );
    let value = run(&raw, section());
    let regions = regions(&value);
    assert_eq!(regions[0]["as_of"], Value::Null);
    assert_eq!(regions[0]["table_indices"], json!([0]));
    assert_eq!(regions[1]["as_of"], "2026-03-31");
    assert_eq!(regions.last().unwrap()["as_of"], "2025-09-30");
    let all_tables = regions
        .iter()
        .flat_map(|region| region["table_indices"].as_array().unwrap())
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        all_tables,
        (0..71).map(|index| json!(index)).collect::<Vec<_>>()
    );
    assert!(!serde_json::to_string(&value).unwrap().contains("2099"));
}

#[test]
fn actual_pfx_missing_first_date_stays_null_without_borrowing_maturity_or_next_page() {
    let raw = String::from_utf8(fixture("pfx")).unwrap();
    let raw = raw.replacen(
        "As of March 31, 2026",
        "Reporting date not disclosed here",
        1,
    );
    let value = run(&raw, section());
    let regions = regions(&value);
    assert_eq!(regions[0]["as_of"], Value::Null);
    assert_eq!(regions[0]["table_indices"], json!([0]));
    assert_eq!(regions[1]["as_of"], "2026-03-31");
    assert_eq!(
        regions
            .iter()
            .map(|region| region["table_indices"].as_array().unwrap().len())
            .sum::<usize>(),
        71
    );
}

#[test]
fn date_below_first_table_cannot_supply_reporting_date() {
    let raw = format!(
        "{}{}<p>March 31, 2026</p>{}",
        header("", false),
        table(),
        notes()
    );
    let value = run(&raw, section());
    assert_eq!(value["as_of"], Value::Null);
    assert_eq!(value["table_indices"], json!([0]));
}

#[test]
fn maturity_before_title_and_outside_notes_dates_do_not_override_header() {
    let raw = format!(
        "<p>December 31, 2099</p>{}{}{}",
        header("<p>March 31, 2026</p>", false),
        table(),
        notes()
    );
    let value = run(&raw, section());
    assert_eq!(value["as_of"], "2026-03-31");
    assert_eq!(value["table_indices"], json!([0]));
    assert_eq!(value["byte_offsets"]["end"], raw.find("<p>Notes").unwrap());
}

#[test]
fn missing_or_conflicting_continuation_cannot_inherit_prior_date() {
    for continued_date in ["", "<p>March 31, 2026</p><p>December 31, 2099</p>"] {
        for coalesce in [true, false] {
            let raw = format!(
                "{}{}{}{}{}",
                header("<p>March 31, 2026</p>", false),
                table(),
                header(continued_date, true),
                table(),
                notes()
            );
            let mut section = section();
            section.coalesce_by_as_of = Some(coalesce);
            section.continue_past = vec!["(?i)continued".to_owned()];
            let value = run(&raw, section);
            let regions = regions(&value);
            assert_eq!(regions.len(), 2);
            assert_eq!(regions[0]["as_of"], "2026-03-31");
            assert_eq!(regions[0]["table_indices"], json!([0]));
            assert_eq!(regions[1]["as_of"], Value::Null);
            assert_eq!(regions[1]["table_indices"], json!([1]));
            assert_eq!(
                regions[0]["byte_offsets"]["end"],
                regions[1]["byte_offsets"]["start"]
            );
        }
    }
}

#[test]
fn explicit_continuation_requires_same_date_and_notes_always_stops() {
    for coalesce in [true, false] {
        for (continued_date, same) in [("March 31, 2026", true), ("September 30, 2025", false)] {
            let raw = format!(
                "{}{}{}{}{}",
                header("<p>March 31, 2026</p>", false),
                table(),
                header(&format!("<p>{continued_date}</p>"), true),
                table(),
                notes()
            );
            let mut section = section();
            section.coalesce_by_as_of = Some(coalesce);
            section.continue_past = vec!["(?i)continued".to_owned()];
            let value = run(&raw, section);
            let regions = regions(&value);
            assert_eq!(regions.len(), if same { 1 } else { 2 });
            assert_eq!(regions[0]["as_of"], "2026-03-31");
            assert_eq!(
                regions[0]["table_indices"],
                if same { json!([0, 1]) } else { json!([0]) }
            );
            if !same {
                assert_eq!(regions[1]["as_of"], "2025-09-30");
                assert_eq!(regions[1]["table_indices"], json!([1]));
            }
            assert_eq!(
                regions.last().unwrap()["byte_offsets"]["end"],
                raw.find("<p>Notes").unwrap()
            );
        }
    }
}

#[test]
fn dates_inside_one_candidate_must_agree_and_invalid_calendar_is_unknown() {
    for date in [
        "March 31, 2026 and December 31, 2099",
        "February 30, 2026",
        "March 31, 2026 and February 30, 2026",
    ] {
        let raw = format!(
            "{}{}{}",
            header(&format!("<p>{date}</p>"), false),
            table(),
            notes()
        );
        let mut section = section();
        section.reporting_date.as_mut().unwrap().text_regex = ".*".to_owned();
        let value = run(&raw, section);
        assert_eq!(value["as_of"], Value::Null);
        assert_eq!(value["table_indices"], json!([0]));
    }
}

#[test]
fn same_date_repeated_or_different_supported_formats_agree() {
    let raw = format!(
        "{}{}{}",
        header(
            "<p>March 31, 2026</p><p>3/31/2026</p><p>2026-03-31</p>",
            false
        ),
        table(),
        notes()
    );
    let mut section = section();
    section.reporting_date.as_mut().unwrap().text_regex = ".*".to_owned();
    assert_eq!(run(&raw, section)["as_of"], "2026-03-31");
}

#[test]
fn full_text_role_filter_does_not_treat_narrative_substring_as_header_date() {
    let raw = format!(
        "{}{}{}",
        header(
            "<p>Purchased on March 31, 2026 under the agreement</p>",
            false
        ),
        table(),
        notes()
    );
    assert_eq!(run(&raw, section())["as_of"], Value::Null);
}

#[test]
fn source_binding_failure_keeps_unknown_region_and_table_denominator() {
    let raw = format!(
        "{}{}{}",
        header("<p>March 31, 2026</p>", false),
        table(),
        notes()
    );
    for raw in [
        format!("<div data-cmdrvl-source-offset=\"0\"></div>{raw}").into_bytes(),
        [vec![0xff], raw.into_bytes()].concat(),
    ] {
        let value = run_bytes(&raw, section());
        assert_eq!(value["as_of"], Value::Null);
        assert_eq!(value["byte_offsets"], Value::Null);
        assert_eq!(value["table_indices"], json!([0]));
    }
}

#[test]
fn table_wrappers_cannot_be_selected_to_import_maturity_text() {
    let raw = format!(
        "{}<div><table><tr><td>March 31, 2026</td></tr></table></div>{}",
        header("", false),
        notes()
    );
    let mut section = section();
    section.reporting_date = Some(ReportingDateConfig {
        selector: "div, td".to_owned(),
        text_regex: ".*".to_owned(),
    });
    let value = run(&raw, section);
    assert_eq!(value["as_of"], Value::Null);
    assert_eq!(value["table_indices"], json!([0]));
}

#[test]
fn date_selection_is_independent_of_region_offset_mode() {
    let raw = format!(
        "{}{}{}",
        header("<p>March 31, 2026</p>", false),
        table(),
        notes()
    );
    let mut section = section();
    let tokenized = run(&raw, section.clone());
    section.tokenized_byte_offsets = None;
    let legacy_offsets = run(&raw, section);
    assert_eq!(legacy_offsets, tokenized);
}

#[test]
fn real_pfx_moved_start_id_still_exposes_separate_first_page_coverage_gap() {
    let original = String::from_utf8(fixture("pfx")).unwrap();
    let marker = "<div><a id=\"a_005\"></a></div>";
    assert!(original.starts_with(marker));
    // Move only the existing start-ID carrier to the next running header. The
    // title-local date policy must not invent omitted first-page coverage.
    let without = original.replacen(marker, "", 1);
    let second_title = without
        .match_indices("<b>Consolidated Schedule of Investments</b>")
        .nth(1)
        .unwrap()
        .0;
    let second_fund = without[..second_title]
        .rfind("<b>PHENIXFIN CORPORATION</b>")
        .unwrap();
    let paragraph = without[..second_fund].rfind("<p ").unwrap();
    let moved = format!("{}{marker}{}", &without[..paragraph], &without[paragraph..]);
    let mut section = section();
    section.anchor_selector = Some("div:has(> a[id='a_005']) ~ p".to_owned());
    let value = run(&moved, section);
    let values = regions(&value);
    assert_eq!(values[0]["as_of"], "2026-03-31");
    assert_eq!(values[0]["table_indices"][0], 1);
    assert_eq!(
        values
            .iter()
            .map(|region| region["table_indices"].as_array().unwrap().len())
            .sum::<usize>(),
        70
    );
}

#[test]
fn absent_option_preserves_legacy_date_and_serialization() {
    let raw = format!("{}{}{}", header("", false), table(), notes());
    let mut section = section();
    section.reporting_date = None;
    assert!(
        serde_json::to_value(&section)
            .unwrap()
            .get("reporting_date")
            .is_none()
    );
    let before_roundtrip = run(&raw, section.clone());
    assert_eq!(before_roundtrip["as_of"], "2030-12-30");
    let after_roundtrip = run(
        &raw,
        serde_json::from_value(serde_json::to_value(&section).unwrap()).unwrap(),
    );
    assert_eq!(
        serde_json::to_vec(&before_roundtrip).unwrap(),
        serde_json::to_vec(&after_roundtrip).unwrap()
    );
    section.reporting_date = self::section().reporting_date;
    assert_eq!(run(&raw, section)["as_of"], Value::Null);
}

#[test]
fn reporting_date_serde_codegen_schema_and_validation_contracts() {
    let def = definition(section());
    let roundtrip: FingerprintDefinition =
        serde_yaml::from_str(&serde_yaml::to_string(&def).unwrap()).unwrap();
    assert_eq!(roundtrip, def);
    validate_definition(&roundtrip).unwrap();
    let generated = generate_rust(&roundtrip).unwrap();
    assert!(
        generated.contains("reporting_date: Some(fingerprint::dsl::parser::ReportingDateConfig")
    );
    assert!(generated.contains(&format!("text_regex: {:?}.to_owned()", DATE)));
    assert_eq!(generated, generate_rust(&roundtrip).unwrap());
    let mut absent = def.clone();
    absent.extract[0].reporting_date = None;
    assert!(
        generate_rust(&absent)
            .unwrap()
            .contains("reporting_date: None,")
    );
    let schema: Value = serde_json::from_str(&dsl_json_schema()).unwrap();
    let contract = &schema["$defs"]["extractSection"]["properties"]["reporting_date"];
    assert_eq!(contract["required"], json!(["selector", "text_regex"]));
    assert_eq!(contract["additionalProperties"], false);
    assert!(
        schema["$defs"]["extractSection"]["allOf"]
            .as_array()
            .unwrap()
            .iter()
            .any(|rule| rule["if"]["required"] == json!(["reporting_date"])
                && rule["then"]["properties"]["type"]["const"] == "region")
    );
    for value in [
        json!({"selector":"p"}),
        json!({"selector":"p","text_regex":1}),
        json!({"selector":"p","text_regex":".*","nearest":true}),
    ] {
        assert!(serde_json::from_value::<ReportingDateConfig>(value).is_err());
    }
    for (selector, text_regex) in [("p[", DATE), ("p", "["), ("p", "")] {
        let mut invalid = def.clone();
        invalid.extract[0].reporting_date = Some(ReportingDateConfig {
            selector: selector.to_owned(),
            text_regex: text_regex.to_owned(),
        });
        assert!(validate_definition(&invalid).is_err());
    }
    for format in ["pdf", "markdown"] {
        let mut invalid = def.clone();
        invalid.format = format.to_owned();
        assert!(validate_definition(&invalid).is_err());
    }
    let mut invalid = def;
    invalid.extract[0].r#type = "text_match".to_owned();
    assert!(
        validate_definition(&invalid)
            .unwrap_err()
            .contains("reporting_date")
    );
}

#[test]
fn real_pfx_native_run_emits_title_bound_positive() {
    let dir = tempdir().unwrap();
    let def = definition(section());
    fs::write(
        dir.path().join("reporting.fp.yaml"),
        serde_yaml::to_string(&def).unwrap(),
    )
    .unwrap();
    let trust = dir.path().join("trust.yaml");
    fs::write(&trust, "trust:\n  - \"installed:*\"\n").unwrap();
    let source =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/html/reporting-date-pfx.htm");
    let manifest = dir.path().join("manifest.jsonl");
    fs::write(&manifest, format!("{}\n", json!({"version":"hash.v0","path":source,"extension":".htm","bytes_hash":format!("blake3:{}", blake3::hash(&fixture("pfx"))),"tool_versions":{}}))).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_fingerprint"))
        .args(["--fp", &def.fingerprint_id, "--jobs", "1", "--no-witness"])
        .arg(manifest)
        .env("FINGERPRINT_DEFINITIONS", dir.path())
        .env("FINGERPRINT_TRUST", trust)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let record: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(record["fingerprint"]["matched"], true);
    let values = &record["fingerprint"]["extracted"]["schedule"];
    let regions = regions(values);
    assert_eq!(regions.len(), 2);
    assert_eq!(regions[0]["as_of"], "2026-03-31");
    assert_eq!(regions[1]["as_of"], "2025-09-30");
    assert_eq!(regions[0]["table_indices"].as_array().unwrap().len(), 35);
    assert_eq!(regions[1]["table_indices"].as_array().unwrap().len(), 36);
}
