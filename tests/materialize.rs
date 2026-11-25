//! `fingerprint materialize` (bd-uai): deterministic projection of a matched XLSX range into
//! CSV / JSONL / Parquet with a provenance sidecar, failing closed with typed refusals.
//!
//! The workbooks are SYNTHETIC (tests/fixtures/materialize/make_watl_fixtures.py): they imitate
//! a CMBS watchlist layout with invented values. The expected table below is the data declared
//! in that generator, written out by hand, not read back from the code under test.

use parquet::file::reader::{FileReader, SerializedFileReader};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const WATL_DEFINITION: &str = r#"
fingerprint_id: cmbs-watl.v2
format: xlsx
assertions:
  - name: watl_sheet
    sheet_name_regex:
      pattern: "(?i)watch\\s?list|WATL"
      bind: watl_sheet
  - name: crefc_boilerplate
    cell_regex:
      sheet: "$watl_sheet"
      cell: A2
      pattern: "(?i)CREFC Investor Reporting Package"
extract:
  - name: watl_table
    type: range
    sheet: "$watl_sheet"
    range: "A11:F17"
  - name: not_projectable
    type: text_match
    anchor: "x"
    pattern: "y"
    within_chars: 10
"#;

const EXPECTED_CSV: &str = "Loan ID,Property Name,Servicer Status,Maturity Date,Balance,DSCR\n\
L001,Harbor Plaza,Watchlist,2027-03-01,12500000,1.18\n\
L002,Elm Court,Watchlist,2026-12-15,8400000.5,0.97\n\
L003,\"North Gate, Bldg A\",Special,2028-01-01T09:30:00,21000000,1.42\n\
L004,Riverside Lofts,Watchlist,2027-07-01,5000000,1.05\n\
L005,Station Square,Watchlist,2029-02-01,33250000,1.31\n";

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/materialize")
        .join(name)
}

struct Scratch(PathBuf);
impl Scratch {
    fn new(tag: &str) -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let unique = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("fp-mat-{tag}-{}-{unique}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

/// A scratch definitions directory holding `definitions` as `<id>.fp.yaml`.
fn defs(definitions: &[(&str, &str)]) -> Scratch {
    let dir = Scratch::new("defs");
    for (id, yaml) in definitions {
        fs::write(dir.path(&format!("{id}.fp.yaml")), yaml).unwrap();
    }
    dir
}

fn materialize(defs: &Scratch, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fingerprint"))
        .env("FINGERPRINT_DEFINITIONS", &defs.0)
        .arg("materialize")
        .args(args)
        .arg("--no-witness")
        .output()
        .unwrap()
}

fn stdout_json(out: &Output) -> Value {
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "stdout not JSON ({e}): {}",
            String::from_utf8_lossy(&out.stdout)
        )
    })
}

fn blake3_of(path: &Path) -> String {
    format!("blake3:{}", blake3::hash(&fs::read(path).unwrap()).to_hex())
}

fn watl(defs: &Scratch, workbook: &str, out: &Path, extra: &[&str]) -> Output {
    let mut args = vec![
        s(&fixture(workbook)).to_owned(),
        "--fp".to_owned(),
        "cmbs-watl.v2".to_owned(),
        "--extract".to_owned(),
        "watl_table".to_owned(),
        "--out".to_owned(),
        s(out).to_owned(),
    ];
    args.extend(extra.iter().map(|a| (*a).to_owned()));
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    materialize(defs, &refs)
}

// ---- projection ---------------------------------------------------------------------------

#[test]
fn csv_projection_is_the_header_anchored_range_with_canonical_cells() {
    let defs = defs(&[("cmbs-watl.v2", WATL_DEFINITION)]);
    let out_dir = Scratch::new("out");
    let out = out_dir.path("watl.csv");
    let result = watl(&defs, "watl_msc.xlsx", &out, &[]);
    assert_eq!(
        result.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    // Header on row 11, the blank row inside the range skipped, footnotes (outside the range)
    // absent; dates as ISO text; integral floats without ".0".
    assert_eq!(fs::read_to_string(&out).unwrap(), EXPECTED_CSV);
    let summary = stdout_json(&result);
    assert_eq!(summary["outcome"], "MATERIALIZED");
    assert_eq!(summary["rows"], 5);
    assert_eq!(summary["columns"], 6);
}

#[test]
fn jsonl_and_parquet_carry_the_same_cells_and_the_same_content_hash() {
    let defs = defs(&[("cmbs-watl.v2", WATL_DEFINITION)]);
    let out_dir = Scratch::new("formats");
    let mut hashes = Vec::new();
    for name in ["t.csv", "t.jsonl", "t.parquet"] {
        let out = out_dir.path(name);
        let result = watl(&defs, "watl_msc.xlsx", &out, &[]);
        assert_eq!(result.status.code(), Some(0), "{name}");
        hashes.push(
            stdout_json(&result)["content_hash"]
                .as_str()
                .unwrap()
                .to_owned(),
        );
    }
    assert!(hashes.iter().all(|h| h == &hashes[0]), "{hashes:?}");

    let expected: Vec<Vec<String>> = csv::ReaderBuilder::new()
        .from_reader(EXPECTED_CSV.as_bytes())
        .records()
        .map(|r| r.unwrap().iter().map(str::to_owned).collect())
        .collect();
    let headers: Vec<String> = csv::ReaderBuilder::new()
        .from_reader(EXPECTED_CSV.as_bytes())
        .headers()
        .unwrap()
        .iter()
        .map(str::to_owned)
        .collect();

    // JSONL: one object per row, keys in column order, all text.
    let jsonl = fs::read_to_string(out_dir.path("t.jsonl")).unwrap();
    let rows: Vec<Value> = jsonl
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(rows.len(), 5);
    for (row, want) in rows.iter().zip(&expected) {
        for (header, cell) in headers.iter().zip(want) {
            assert_eq!(row[header], Value::String(cell.clone()), "{header}");
        }
    }

    // Parquet: read the data back with an independent reader path (the parquet row iterator).
    let reader =
        SerializedFileReader::new(fs::File::open(out_dir.path("t.parquet")).unwrap()).unwrap();
    let mut got: Vec<Vec<String>> = Vec::new();
    for row in reader.get_row_iter(None).unwrap() {
        let row = row.unwrap();
        got.push(
            row.get_column_iter()
                .map(|(_, field)| match field {
                    parquet::record::Field::Str(text) => text.clone(),
                    parquet::record::Field::Null => String::new(),
                    other => panic!("unexpected parquet field {other:?}"),
                })
                .collect(),
        );
    }
    assert_eq!(got, expected);
}

#[test]
fn provenance_pins_source_fingerprint_projection_and_locator() {
    let defs = defs(&[("cmbs-watl.v2", WATL_DEFINITION)]);
    let out_dir = Scratch::new("prov");
    let out = out_dir.path("watl.parquet");
    assert_eq!(
        watl(&defs, "watl_msc.xlsx", &out, &[]).status.code(),
        Some(0)
    );
    let sidecar = out_dir.path("watl.parquet.provenance.json");
    let prov: Value = serde_json::from_slice(&fs::read(&sidecar).unwrap()).unwrap();

    assert_eq!(prov["version"], "fingerprint.materialize.v0");
    assert_eq!(
        prov["source"]["bytes_hash"],
        blake3_of(&fixture("watl_msc.xlsx"))
    );
    assert_eq!(
        prov["fingerprint"]["source_hash"],
        blake3_of(&defs.path("cmbs-watl.v2.fp.yaml"))
    );
    assert_eq!(prov["fingerprint"]["id"], "cmbs-watl.v2");
    assert_eq!(prov["output"]["bytes_hash"], blake3_of(&out));
    assert_eq!(
        prov["content_hash"],
        format!("blake3:{}", blake3::hash(EXPECTED_CSV.as_bytes()).to_hex())
    );
    let locator = &prov["extract"]["locator"];
    assert_eq!(
        locator["sheet"], "Servicer Watch List",
        "the BOUND sheet name, not the pattern"
    );
    assert_eq!(locator["range"], "A11:F17");
    assert_eq!(locator["header_row"], 11);
    assert_eq!(locator["first_data_row"], 12);
    assert_eq!(prov["skipped"]["empty_rows_in_range"], 1);
    assert_eq!(prov["output"]["rows"], 5);
    assert_eq!(prov["output"]["columns"][0], "Loan ID");
}

#[test]
fn projection_is_byte_identical_on_repeat_in_every_format() {
    let defs = defs(&[("cmbs-watl.v2", WATL_DEFINITION)]);
    for name in ["r.csv", "r.jsonl", "r.parquet"] {
        let a = Scratch::new("rep-a");
        let b = Scratch::new("rep-b");
        assert_eq!(
            watl(&defs, "watl_msc.xlsx", &a.path(name), &[])
                .status
                .code(),
            Some(0)
        );
        assert_eq!(
            watl(&defs, "watl_msc.xlsx", &b.path(name), &[])
                .status
                .code(),
            Some(0)
        );
        assert_eq!(
            fs::read(a.path(name)).unwrap(),
            fs::read(b.path(name)).unwrap(),
            "{name}"
        );
    }
}

#[test]
fn explicit_format_flag_overrides_the_extension() {
    let defs = defs(&[("cmbs-watl.v2", WATL_DEFINITION)]);
    let out_dir = Scratch::new("fmt");
    let out = out_dir.path("table.dat");
    assert_eq!(
        watl(&defs, "watl_msc.xlsx", &out, &["--format", "jsonl"])
            .status
            .code(),
        Some(0)
    );
    assert!(
        fs::read_to_string(&out)
            .unwrap()
            .starts_with("{\"Loan ID\":\"L001\"")
    );
}

// ---- fail closed: typed refusals, nothing written ---------------------------------------------

fn refusal(out: &Output) -> (String, Value) {
    assert_eq!(
        out.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let envelope = stdout_json(out);
    assert_eq!(envelope["outcome"], "REFUSAL");
    (
        envelope["refusal"]["code"].as_str().unwrap().to_owned(),
        envelope["refusal"].clone(),
    )
}

#[test]
fn refusals_are_typed_leak_no_cell_values_and_write_nothing() {
    let defs = defs(&[("cmbs-watl.v2", WATL_DEFINITION)]);
    let out_dir = Scratch::new("refuse");

    let cases: [(&str, &str, &str, &str); 5] = [
        // (workbook, fp id, extract, expected code)
        (
            "not_watl.xlsx",
            "cmbs-watl.v2",
            "watl_table",
            "E_NOT_MATCHED",
        ),
        ("watl_msc.xlsx", "no-such.v1", "watl_table", "E_UNKNOWN_FP"),
        ("watl_msc.xlsx", "cmbs-watl.v2", "nope", "E_UNKNOWN_EXTRACT"),
        (
            "watl_msc.xlsx",
            "cmbs-watl.v2",
            "not_projectable",
            "E_EXTRACT_UNSUPPORTED",
        ),
        (
            "watl_error_cell.xlsx",
            "cmbs-watl.v2",
            "watl_table",
            "E_PROJECTION",
        ),
    ];
    for (index, (workbook, fp, extract, code)) in cases.into_iter().enumerate() {
        let out = out_dir.path(&format!("case{index}.csv"));
        let result = materialize(
            &defs,
            &[
                s(&fixture(workbook)),
                "--fp",
                fp,
                "--extract",
                extract,
                "--out",
                s(&out),
            ],
        );
        let (got, detail) = refusal(&result);
        assert_eq!(got, code, "{workbook} {fp} {extract}");
        assert!(!out.exists(), "{code}: no table written");
        assert!(!sidecar(&out).exists(), "{code}: no provenance written");
        let text = detail.to_string() + &String::from_utf8_lossy(&result.stderr);
        for value in ["Harbor Plaza", "Elm Court", "12500000", "8400000"] {
            assert!(
                !text.contains(value),
                "{code}: value {value} leaked: {text}"
            );
        }
    }
}

fn sidecar(out: &Path) -> PathBuf {
    let mut name = out.file_name().unwrap().to_os_string();
    name.push(".provenance.json");
    out.with_file_name(name)
}

#[test]
fn error_cell_refusal_names_the_coordinate_not_the_value() {
    let defs = defs(&[("cmbs-watl.v2", WATL_DEFINITION)]);
    let out_dir = Scratch::new("errcell");
    let result = watl(&defs, "watl_error_cell.xlsx", &out_dir.path("x.csv"), &[]);
    let (code, detail) = refusal(&result);
    assert_eq!(code, "E_PROJECTION");
    let message = detail["message"].as_str().unwrap();
    assert!(message.contains("E13"), "{message}");
    assert!(!message.contains("DIV"), "{message}");
}

#[test]
fn bad_headers_refuse() {
    let defs = defs(&[("cmbs-watl.v2", WATL_DEFINITION)]);
    let out_dir = Scratch::new("hdr");
    for (workbook, needle) in [
        ("watl_dup_header.xlsx", "repeats an earlier column name"),
        ("watl_empty_header.xlsx", "is empty"),
    ] {
        let result = watl(
            &defs,
            workbook,
            &out_dir.path(&format!("{workbook}.csv")),
            &[],
        );
        let (code, detail) = refusal(&result);
        assert_eq!(code, "E_PROJECTION", "{workbook}");
        assert!(
            detail["message"].as_str().unwrap().contains(needle),
            "{workbook}: {detail}"
        );
    }
}

#[test]
fn an_existing_output_is_never_overwritten() {
    let defs = defs(&[("cmbs-watl.v2", WATL_DEFINITION)]);
    let out_dir = Scratch::new("exists");
    let out = out_dir.path("keep.csv");
    fs::write(&out, "precious\n").unwrap();
    let (code, _) = refusal(&watl(&defs, "watl_msc.xlsx", &out, &[]));
    assert_eq!(code, "E_OUT_EXISTS");
    assert_eq!(fs::read_to_string(&out).unwrap(), "precious\n");
    // An existing sidecar alone is refused too.
    let out2 = out_dir.path("other.csv");
    fs::write(sidecar(&out2), "{}").unwrap();
    let (code, _) = refusal(&watl(&defs, "watl_msc.xlsx", &out2, &[]));
    assert_eq!(code, "E_OUT_EXISTS");
    assert!(!out2.exists());
}

#[test]
fn ambiguous_child_routing_refuses_instead_of_guessing() {
    let child = |id: &str| {
        format!(
            "fingerprint_id: {id}\nformat: xlsx\nparent: cmbs-watl.v2\nassertions:\n  - sheet_name_regex:\n      pattern: \"(?i)watch\"\n"
        )
    };
    let child_a = child("cmbs-watl.v2/servicer-a");
    let child_b = child("cmbs-watl.v2/servicer-b");
    let defs = defs(&[
        ("cmbs-watl.v2", WATL_DEFINITION),
        ("servicer-a", &child_a),
        ("servicer-b", &child_b),
    ]);
    let out_dir = Scratch::new("amb");
    let out = out_dir.path("a.csv");
    let (code, detail) = refusal(&watl(&defs, "watl_msc.xlsx", &out, &[]));
    assert_eq!(code, "E_AMBIGUOUS_ROUTING");
    assert_eq!(
        detail["detail"]["matching_children"],
        serde_json::json!(["cmbs-watl.v2/servicer-a", "cmbs-watl.v2/servicer-b"])
    );
    assert!(!out.exists());
}

// ---- downstream: the projection is a clean table the CSV tools accept --------------------------

#[test]
fn projected_csv_is_rectangular_and_has_unique_nonempty_headers() {
    let defs = defs(&[("cmbs-watl.v2", WATL_DEFINITION)]);
    let out_dir = Scratch::new("shape");
    let out = out_dir.path("clean.csv");
    assert_eq!(
        watl(&defs, "watl_msc_next.xlsx", &out, &[]).status.code(),
        Some(0)
    );
    let mut reader = csv::ReaderBuilder::new()
        .flexible(false)
        .from_path(&out)
        .unwrap();
    let headers: Vec<String> = reader
        .headers()
        .unwrap()
        .iter()
        .map(str::to_owned)
        .collect();
    let unique: std::collections::BTreeSet<&String> = headers.iter().collect();
    assert_eq!(unique.len(), headers.len());
    assert!(headers.iter().all(|h| !h.trim().is_empty()));
    let rows = reader
        .records()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .len();
    assert_eq!(rows, 5);
}

// ---- HTML region projection -------------------------------------------------------------------

fn soi_definition() -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("docs/evidence/source-offsets-20261106/soi-schedule.v2.fp.yaml"),
    )
    .unwrap()
}

fn html_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/html")
        .join(name)
}

fn soi(defs: &Scratch, file: &Path, out: &Path, extra: &[&str]) -> Output {
    let mut args = vec![
        s(file).to_owned(),
        "--fp".to_owned(),
        "soi-schedule.v2".to_owned(),
        "--extract".to_owned(),
        "schedule_region".to_owned(),
        "--out".to_owned(),
        s(out).to_owned(),
    ];
    args.extend(extra.iter().map(|a| (*a).to_owned()));
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    materialize(defs, &refs)
}

#[test]
fn html_region_projects_a_continued_schedule_with_groups_and_provenance() {
    let soi_yaml = soi_definition();
    let defs = defs(&[("soi-schedule.v2", &soi_yaml)]);
    let out_dir = Scratch::new("html");

    // bdc_soi_golub_like.html: two pages, each a table with the same header row, each with a
    // full-width industry row. Expected table written out by hand from the HTML.
    let out = out_dir.path("golub.csv");
    let result = soi(&defs, &html_fixture("bdc_soi_golub_like.html"), &out, &[]);
    assert_eq!(
        result.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        fs::read_to_string(&out).unwrap(),
        "_group,Company,Spread,Maturity,Cost,Fair Value\n\
Software,Mariner Cloud,SOFR + 5.75%,2028-12-31,\"$6,100\",\"$6,180\"\n\
Healthcare,North Lake,SOFR + 5.25%,2029-09-30,\"$5,400\",\"$5,520\"\n"
    );

    let prov: Value = serde_json::from_slice(&fs::read(sidecar(&out)).unwrap()).unwrap();
    let locator = &prov["extract"]["locator"];
    assert_eq!(locator["kind"], "html_region");
    assert_eq!(locator["table_indices"], serde_json::json!([0, 1]));
    assert_eq!(prov["skipped"]["repeated_header_rows"], 1);
    assert_eq!(prov["skipped"]["full_width_rows_carried_as_group"], 2);
    assert_eq!(prov["skipped"]["padded_rows"], 0);
    assert_eq!(
        prov["source"]["bytes_hash"],
        blake3_of(&html_fixture("bdc_soi_golub_like.html"))
    );
    assert_eq!(
        prov["fingerprint"]["source_hash"],
        blake3_of(&defs.path("soi-schedule.v2.fp.yaml"))
    );

    // The pinned byte offsets really bracket the projected content in the source file.
    let source = fs::read(html_fixture("bdc_soi_golub_like.html")).unwrap();
    let start = locator["byte_offsets"]["start"].as_u64().unwrap() as usize;
    let end = locator["byte_offsets"]["end"].as_u64().unwrap() as usize;
    let region = String::from_utf8_lossy(&source[start..end]).into_owned();
    assert!(
        region.contains("Mariner Cloud") && region.contains("North Lake"),
        "{region}"
    );
    assert!(
        region.starts_with("<h1>Schedule of Investments</h1>"),
        "the region starts at its anchor heading: {region}"
    );
    assert!(end <= source.len());
}

#[test]
fn html_region_second_fixture_and_all_three_formats_agree() {
    let soi_yaml = soi_definition();
    let defs = defs(&[("soi-schedule.v2", &soi_yaml)]);
    let out_dir = Scratch::new("html2");
    let mut hashes = Vec::new();
    for name in ["a.csv", "a.jsonl", "a.parquet"] {
        let result = soi(
            &defs,
            &html_fixture("bdc_soi_ares_like.html"),
            &out_dir.path(name),
            &[],
        );
        assert_eq!(result.status.code(), Some(0), "{name}");
        hashes.push(
            stdout_json(&result)["content_hash"]
                .as_str()
                .unwrap()
                .to_owned(),
        );
    }
    assert!(hashes.iter().all(|h| h == &hashes[0]));
    assert_eq!(
        fs::read_to_string(out_dir.path("a.csv")).unwrap(),
        "_group,Portfolio Company,Investment,Business Description,Coupon,Cost,Fair Value\n\
Software,Atlas Systems,First Lien,Enterprise software,SOFR + 6.50%,\"$18,100\",\"$18,450\"\n\
Healthcare,Beacon Health,Second Lien,Provider services,11.25%,\"$9,400\",\"$9,650\"\n"
    );
    // Repeat run: byte-identical in every format.
    let again = Scratch::new("html2b");
    for name in ["a.csv", "a.jsonl", "a.parquet"] {
        soi(
            &defs,
            &html_fixture("bdc_soi_ares_like.html"),
            &again.path(name),
            &[],
        );
        assert_eq!(
            fs::read(again.path(name)).unwrap(),
            fs::read(out_dir.path(name)).unwrap(),
            "{name}"
        );
    }
}

#[test]
fn html_region_refuses_what_it_cannot_flatten() {
    let soi_yaml = soi_definition();
    let defs = defs(&[("soi-schedule.v2", &soi_yaml)]);
    let out_dir = Scratch::new("html3");

    // Real fixture: the region's tables carry DIFFERENT header rows (one schedule is not one
    // table shape), so projection refuses instead of merging them.
    let out = out_dir.path("subsections.csv");
    let result = soi(
        &defs,
        &html_fixture("soi_subsections_repeated_period.html"),
        &out,
        &[],
    );
    let (code, detail) = refusal(&result);
    assert_eq!(code, "E_PROJECTION");
    assert!(
        detail["message"]
            .as_str()
            .unwrap()
            .contains("different header row"),
        "{detail}"
    );
    assert!(!out.exists());

    // Synthetic edge cases, each a tiny page with the same heading the definition anchors on.
    let page =
        |table: &str| format!("<html><body><h1>Schedule of Investments</h1>{table}</body></html>");
    let cases = [
        (
            "rowspan",
            page(
                "<table><tr><th>A</th><th>B</th></tr><tr><td rowspan=\"2\">x</td><td>y</td></tr></table>",
            ),
            "rowspan",
        ),
        (
            "wide",
            page(
                "<table><tr><th>A</th><th>B</th></tr><tr><td>1</td><td>2</td><td>3</td></tr></table>",
            ),
            "wider than the 2-column header",
        ),
        (
            "nested",
            page(
                "<table><tr><th>A</th></tr><tr><td><table><tr><td>z</td></tr></table></td></tr></table>",
            ),
            "nested table",
        ),
        (
            "dupheader",
            page("<table><tr><th>A</th><th>A</th></tr><tr><td>1</td><td>2</td></tr></table>"),
            "repeats a column name",
        ),
        (
            "emptyheader",
            page("<table><tr><th>A</th><th></th></tr><tr><td>1</td><td>2</td></tr></table>"),
            "empty header cell",
        ),
    ];
    for (name, html, needle) in cases {
        let file = out_dir.path(&format!("{name}.html"));
        fs::write(&file, html).unwrap();
        let out = out_dir.path(&format!("{name}.csv"));
        let result = soi(&defs, &file, &out, &[]);
        let (code, detail) = refusal(&result);
        // Some synthetic pages may fail to match the schedule fingerprint before projecting;
        // the assertion below pins the exact outcome so a silent change cannot hide.
        assert_eq!(code, "E_PROJECTION", "{name}: {detail}");
        assert!(
            detail["message"].as_str().unwrap().contains(needle),
            "{name}: {detail}"
        );
        assert!(!out.exists(), "{name}: nothing written");
    }
}

#[test]
fn html_padding_is_counted_not_silent() {
    let soi_yaml = soi_definition();
    let defs = defs(&[("soi-schedule.v2", &soi_yaml)]);
    let out_dir = Scratch::new("html4");
    let file = out_dir.path("short.html");
    fs::write(
        &file,
        "<html><body><h1>Schedule of Investments</h1><table><tr><th>A</th><th>B</th><th>C</th></tr><tr><td>1</td><td>2</td></tr><tr><td>4</td><td>5</td><td>6</td></tr></table></body></html>",
    )
    .unwrap();
    let out = out_dir.path("short.csv");
    let result = soi(&defs, &file, &out, &[]);
    assert_eq!(
        result.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
    assert_eq!(fs::read_to_string(&out).unwrap(), "A,B,C\n1,2,\n4,5,6\n");
    let prov: Value = serde_json::from_slice(&fs::read(sidecar(&out)).unwrap()).unwrap();
    assert_eq!(prov["skipped"]["padded_rows"], 1);
}
