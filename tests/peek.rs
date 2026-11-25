use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fixture(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("peek")
        .join(path)
}

fn run_fingerprint(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fingerprint"))
        .args(args)
        .output()
        .expect("run fingerprint binary")
}

fn run_fingerprint_with_witness(args: &[&str], witness_path: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fingerprint"))
        .args(args)
        .env("EPISTEMIC_WITNESS", witness_path)
        .output()
        .expect("run fingerprint binary")
}

fn parse_stdout(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("stdout should be JSON")
}

#[test]
fn peek_reports_row_shape_without_run_mode_fingerprints() {
    let path = fixture("preamble.csv");
    let output = run_fingerprint(&[
        "peek",
        path.to_str().expect("fixture path"),
        "--rows",
        "8",
        "--json",
        "--suggest",
        "--no-witness",
    ]);

    assert_eq!(output.status.code(), Some(0));
    let envelope = parse_stdout(&output);
    assert_eq!(envelope["version"], "fingerprint.peek.v0");
    assert_eq!(envelope["outcome"], "SUCCESS");
    assert_eq!(envelope["subcommand"], "peek");
    assert_eq!(envelope["witness_id"], Value::Null);
    assert_eq!(envelope["result"]["summary"]["modal_column_count"], 4);
    assert_eq!(envelope["result"]["summary"]["header_at_row"], 4);
    assert_eq!(
        envelope["result"]["suggestions"]["profile_pre_parse"]["skip_rows"],
        3
    );
}

#[test]
fn peek_detects_units_row_for_profile_suggestions() {
    let path = fixture("units.csv");
    let output = run_fingerprint(&[
        "peek",
        path.to_str().expect("fixture path"),
        "--suggest",
        "--no-witness",
    ]);

    assert_eq!(output.status.code(), Some(0));
    let envelope = parse_stdout(&output);
    assert_eq!(
        envelope["result"]["suggestions"]["profile_pre_parse"]["mode"],
        "preamble_with_units"
    );
    assert_eq!(
        envelope["result"]["suggestions"]["profile_pre_parse"]["unit_rows"],
        Value::Array(vec![Value::from(3)])
    );
}

#[test]
fn peek_never_emits_cell_content_or_witness_params_with_cell_content() {
    let tempdir = tempfile::tempdir().expect("create tempdir");
    let witness_path = tempdir.path().join("witness.jsonl");
    let path = fixture("secret.csv");
    let output = run_fingerprint_with_witness(
        &["peek", path.to_str().expect("fixture path"), "--suggest"],
        &witness_path,
    );

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).expect("stdout utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr utf8");
    assert!(!stdout.contains("SUPER-SECRET-CELL-VALUE"));
    assert!(!stdout.contains("ANOTHER-PRIVATE-VALUE"));
    assert!(!stderr.contains("SUPER-SECRET-CELL-VALUE"));
    assert!(!stderr.contains("ANOTHER-PRIVATE-VALUE"));

    let witness = fs::read_to_string(&witness_path).expect("read witness ledger");
    assert!(!witness.contains("SUPER-SECRET-CELL-VALUE"));
    assert!(!witness.contains("ANOTHER-PRIVATE-VALUE"));
    assert!(parse_stdout_from_str(&stdout)["witness_id"].is_string());
}

#[test]
fn peek_refuses_empty_file_with_json_refusal() {
    let empty = tempfile::NamedTempFile::new().expect("create empty input");
    let path = empty.path();
    let output = run_fingerprint(&["peek", path.to_str().expect("fixture path"), "--no-witness"]);

    assert_eq!(output.status.code(), Some(2));
    let envelope = parse_stdout(&output);
    assert_eq!(envelope["version"], "fingerprint.peek.v0");
    assert_eq!(envelope["outcome"], "REFUSAL");
    assert_eq!(envelope["refusal"]["code"], "E_BAD_INPUT");
}

fn parse_stdout_from_str(stdout: &str) -> Value {
    serde_json::from_str(stdout).expect("stdout should be JSON")
}

// ---- bd-27o: streaming, fixture matrix, shape classes, not-applicable ------------------------

fn peek_result(file: &str, extra: &[&str]) -> Value {
    let path = fixture(file);
    let mut args = vec!["peek", path.to_str().expect("fixture path"), "--no-witness"];
    args.extend_from_slice(extra);
    let output = run_fingerprint(&args);
    assert_eq!(output.status.code(), Some(0), "{file}");
    parse_stdout(&output)["result"].clone()
}

fn shape_classes(result: &Value) -> Vec<String> {
    result["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .map(|row| row["shape_class"].as_str().expect("class").to_owned())
        .collect()
}

#[test]
fn every_documented_shape_class_is_produced_for_the_row_it_describes() {
    let result = peek_result("shape_classes.csv", &["--suggest"]);
    assert_eq!(
        shape_classes(&result),
        [
            "blank",
            "paragraph",
            "sparse-label",
            "sparse-multi",
            "dense-header-candidate",
            "dense-units",
            "dense-data",
            "dense-data",
            "dense-data",
            "irregular"
        ]
    );
    let summary = &result["summary"];
    assert_eq!(summary["modal_column_count"], 4);
    assert_eq!(summary["header_at_row"], 5);
    assert_eq!(summary["data_starts_at"], 7);
    assert_eq!(summary["preamble_rows"], 4);
    assert_eq!(
        summary["shape_classes"],
        serde_json::json!({
            "blank": 1, "dense-data": 3, "dense-header-candidate": 1, "dense-units": 1,
            "irregular": 1, "paragraph": 1, "sparse-label": 1, "sparse-multi": 1
        })
    );
    let pre_parse = &result["suggestions"]["profile_pre_parse"];
    assert_eq!(pre_parse["mode"], "preamble_with_units");
    assert_eq!(pre_parse["unit_rows"], serde_json::json!([6]));
}

#[test]
fn fixture_matrix_clean_comments_mixed_tsv_preamble_crlf_and_no_trailing_newline() {
    // (file, delimiter, modal, header_at_row, data_starts_at, classes)
    let header_then_data = |n: usize| {
        let mut v = vec!["dense-header-candidate".to_owned()];
        v.extend(std::iter::repeat_n("dense-data".to_owned(), n));
        v
    };
    let with_comments = |n: usize| {
        let mut v = vec!["sparse-label".to_owned(); 3];
        v.extend(header_then_data(n));
        v
    };
    type Case = (&'static str, &'static str, u64, u64, u64, Vec<String>);
    let cases: [Case; 7] = [
        ("clean_flat.csv", "comma", 4, 1, 2, header_then_data(5)),
        ("comment_lines.csv", "comma", 4, 4, 5, with_comments(3)),
        // Mixed delimiters: the guess follows the majority of the sampled lines (semicolon);
        // the heuristic is pinned as it behaves today, not endorsed.
        (
            "mixed_delimiters.csv",
            "semicolon",
            4,
            4,
            5,
            with_comments(3),
        ),
        (
            "only_preamble.csv",
            "comma",
            1,
            1,
            2,
            vec![
                "sparse-label".into(),
                "sparse-label".into(),
                "paragraph".into(),
            ],
        ),
        ("tsv.tsv", "tab", 4, 1, 2, header_then_data(3)),
        ("crlf.csv", "comma", 4, 1, 2, header_then_data(2)),
        (
            "no_trailing_newline.csv",
            "comma",
            4,
            1,
            2,
            header_then_data(2),
        ),
    ];
    for (file, delimiter, modal, header, data_start, classes) in cases {
        let result = peek_result(file, &[]);
        assert_eq!(result["delimiter"]["name"], delimiter, "{file}");
        assert_eq!(result["summary"]["modal_column_count"], modal, "{file}");
        assert_eq!(result["summary"]["header_at_row"], header, "{file}");
        assert_eq!(result["summary"]["data_starts_at"], data_start, "{file}");
        assert_eq!(shape_classes(&result), classes, "{file}");
    }
}

#[test]
fn peek_does_not_read_past_the_rows_it_reports_on() {
    // 20 clean lines, then 100 MiB of bytes that are not UTF-8. A whole-file read would refuse
    // (or take a long time); streaming inspects only the head and succeeds quickly.
    use std::io::Write;
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("huge.csv");
    {
        let mut file = fs::File::create(&path).expect("create");
        file.write_all(b"loan_id,region,balance,rate\n").unwrap();
        for i in 0..20 {
            writeln!(file, "L{i},East,{i}.5,0.05").unwrap();
        }
        let garbage = vec![0xFFu8; 1024 * 1024];
        for _ in 0..100 {
            file.write_all(&garbage).unwrap();
        }
    }
    let started = std::time::Instant::now();
    let output = run_fingerprint(&[
        "peek",
        path.to_str().unwrap(),
        "--rows",
        "10",
        "--no-witness",
    ]);
    let elapsed = started.elapsed();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result = parse_stdout(&output)["result"].clone();
    assert_eq!(result["rows_observed"], 10);
    assert_eq!(result["summary"]["modal_column_count"], 4);
    assert!(
        elapsed.as_secs() < 20,
        "peek took {elapsed:?} on a 100 MiB file"
    );
}

#[test]
fn witness_hash_and_size_cover_the_whole_file_even_though_peek_streams() {
    use std::io::Write;
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("big.csv");
    {
        let mut file = fs::File::create(&path).expect("create");
        file.write_all(b"a,b\n").unwrap();
        for i in 0..200_000 {
            writeln!(file, "{i},x{i}").unwrap();
        }
    }
    let witness = dir.path().join("witness.jsonl");
    let output =
        run_fingerprint_with_witness(&["peek", path.to_str().unwrap(), "--rows", "5"], &witness);
    assert_eq!(output.status.code(), Some(0));
    let record: Value = serde_json::from_str(
        fs::read_to_string(&witness)
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    let bytes = fs::read(&path).unwrap();
    assert_eq!(record["inputs"][0]["bytes"], bytes.len());
    assert_eq!(
        record["inputs"][0]["hash"],
        format!("blake3:{}", blake3::hash(&bytes).to_hex())
    );
}

#[test]
fn an_overlong_line_is_truncated_for_shape_counted_and_still_hashed() {
    use std::io::Write;
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("long.csv");
    {
        let mut file = fs::File::create(&path).expect("create");
        file.write_all(b"a,b\n1,2\n").unwrap();
        file.write_all(&vec![b'z'; 3 * 1024 * 1024]).unwrap();
        file.write_all(b"\n").unwrap();
    }
    let witness = dir.path().join("witness.jsonl");
    let output =
        run_fingerprint_with_witness(&["peek", path.to_str().unwrap(), "--rows", "5"], &witness);
    assert_eq!(output.status.code(), Some(0));
    let result = parse_stdout(&output)["result"].clone();
    assert_eq!(result["truncated_lines"], 1);
    assert_eq!(result["rows"][2]["shape_class"], "paragraph");
    let record: Value = serde_json::from_str(
        fs::read_to_string(&witness)
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        record["inputs"][0]["hash"],
        format!(
            "blake3:{}",
            blake3::hash(&fs::read(&path).unwrap()).to_hex()
        )
    );
}

#[test]
fn invalid_utf8_inside_the_inspected_rows_still_refuses() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("bad.csv");
    fs::write(&path, b"a,b\n1,\xFF\xFE\n").unwrap();
    let output = run_fingerprint(&["peek", path.to_str().unwrap(), "--no-witness"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(parse_stdout(&output)["refusal"]["code"], "E_BAD_INPUT");
}

#[test]
fn non_csv_containers_are_refused_as_format_not_applicable() {
    let dir = tempfile::tempdir().expect("tempdir");
    // (.xlsx/.xls are supported workbooks since bd-jxd: a fake one is "could not open", below.)
    // (pdf is supported since bd-hra; these are the containers peek still has no reader for.)
    for name in ["archive.zip", "image.png", "letter.docx", "data.gz"] {
        let path = dir.path().join(name);
        fs::write(&path, b"PK\x03\x04 not really").unwrap();
        let output = run_fingerprint(&["peek", path.to_str().unwrap(), "--no-witness"]);
        assert_eq!(output.status.code(), Some(2), "{name}");
        let envelope = parse_stdout(&output);
        assert_eq!(envelope["refusal"]["code"], "E_BAD_INPUT", "{name}");
        assert_eq!(
            envelope["refusal"]["detail"]["reason"], "format_not_applicable",
            "{name}"
        );
    }
}

// ---- bd-jxd: workbooks and the shared inspection vocabulary -------------------------------------

fn materialize_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/materialize")
        .join(name)
}

fn peek_path(path: &Path, extra: &[&str]) -> Output {
    let mut args = vec!["peek", path.to_str().expect("path"), "--no-witness"];
    args.extend_from_slice(extra);
    run_fingerprint(&args)
}

#[test]
fn delimited_text_reports_its_format_in_the_shared_vocabulary() {
    assert_eq!(peek_result("clean_flat.csv", &[])["format"], "csv");
    assert_eq!(peek_result("tsv.tsv", &[])["format"], "tsv");
}

#[test]
fn workbook_peek_reports_row_shapes_by_excel_row_number() {
    // watl_msc.xlsx is the synthetic CMBS watchlist (tests/fixtures/materialize): title rows,
    // a CREFC boilerplate row on 2, a label+value pair on 5, header on row 11, data rows 12-15
    // and 17 with a blank row 16, footnotes on 19-20. Expected values come from that layout.
    let output = peek_path(&materialize_fixture("watl_msc.xlsx"), &["--suggest"]);
    assert_eq!(output.status.code(), Some(0));
    let result = parse_stdout(&output)["result"].clone();
    assert_eq!(result["format"], "xlsx");
    // bd-zwx: sheet names are no longer printed by default (they were printed raw before): an
    // ordinal ref in workbook order stands in; `--explicit` adds the names.
    assert_eq!(
        result["sheet"],
        serde_json::json!({"index": 0, "id": "redacted:ref:s1"})
    );
    assert_eq!(
        result["sheets"],
        serde_json::json!([
            {"index": 0, "id": "redacted:ref:s1", "rows": 20, "columns": 6, "first_row": 1, "first_column": 1},
            {"index": 1, "id": "redacted:ref:s2", "rows": 1, "columns": 1, "first_row": 1, "first_column": 1}
        ])
    );
    let default_stdout = String::from_utf8(output.stdout.clone()).unwrap();
    assert!(!default_stdout.contains("Servicer Watch List") && !default_stdout.contains("Notes"));
    let named = parse_stdout(&peek_path(
        &materialize_fixture("watl_msc.xlsx"),
        &["--suggest", "--explicit"],
    ))["result"]
        .clone();
    assert_eq!(
        named["sheet"],
        serde_json::json!({"index": 0, "id": "redacted:ref:s1", "name": "Servicer Watch List"})
    );
    assert_eq!(named["sheets"][1]["name"], "Notes");
    let indexed: Vec<(u64, String)> = result["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["row_index"].as_u64().unwrap(),
                r["shape_class"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    let expected = [
        (1, "sparse-label"),
        (2, "sparse-label"),
        (3, "blank"),
        (4, "blank"),
        (5, "sparse-label"),
        (6, "blank"),
        (7, "blank"),
        (8, "blank"),
        (9, "blank"),
        (10, "blank"),
        (11, "dense-header-candidate"),
        (12, "dense-data"),
        (13, "dense-data"),
        (14, "dense-data"),
        (15, "dense-data"),
        (16, "blank"),
        (17, "dense-data"),
        (18, "blank"),
        (19, "sparse-label"),
        (20, "sparse-label"),
    ];
    assert_eq!(
        indexed,
        expected
            .iter()
            .map(|(i, c)| (*i, (*c).to_owned()))
            .collect::<Vec<_>>()
    );
    let summary = &result["summary"];
    assert_eq!(summary["header_at_row"], 11);
    assert_eq!(summary["data_starts_at"], 12);
    assert_eq!(summary["modal_column_count"], 6);
    assert_eq!(result["suggestions"]["profile_pre_parse"]["skip_rows"], 10);
    // Excel types drive the classes: row 12 is mixed (L001), text, text, date, numeric, numeric.
    assert_eq!(
        result["rows"][11]["cell_type_counts"],
        serde_json::json!({"boolean": 0, "date": 1, "empty": 0, "mixed": 1, "numeric": 2, "text": 2})
    );
    // No delimiter concept for a workbook.
    assert!(result.get("delimiter").is_none());
}

#[test]
fn workbook_peek_never_emits_a_cell_value_or_header_text() {
    let output = peek_path(&materialize_fixture("watl_msc.xlsx"), &["--suggest"]);
    let stdout = String::from_utf8(output.stdout).expect("utf8");
    for value in [
        "Harbor Plaza",
        "Elm Court",
        "North Gate",
        "Station Square",
        "12500000",
        "8400000",
        "Loan ID",
        "Property Name",
        "Servicer Status",
        "CREFC",
        "Synthetic Servicer Report",
        "2027-03-01",
        "Footnote",
    ] {
        assert!(!stdout.contains(value), "value {value} leaked: {stdout}");
    }
}

#[test]
fn workbook_sheet_selection_by_name_and_index_and_unknown_sheet_refusal() {
    let path = materialize_fixture("watl_msc.xlsx");
    for selector in ["Notes", "1"] {
        let output = peek_path(&path, &["--sheet", selector]);
        assert_eq!(output.status.code(), Some(0), "{selector}");
        let result = parse_stdout(&output)["result"].clone();
        // Default: the second sheet's ref, no name. `--explicit` adds the name.
        assert_eq!(result["sheet"]["id"], "redacted:ref:s2", "{selector}");
        assert!(result["sheet"].get("name").is_none(), "{selector}");
        assert_eq!(result["rows_observed"], 1);
        let named =
            parse_stdout(&peek_path(&path, &["--sheet", selector, "--explicit"]))["result"].clone();
        assert_eq!(named["sheet"]["name"], "Notes", "{selector}");
    }
    for selector in ["Missing", "7"] {
        let output = peek_path(&path, &["--sheet", selector]);
        assert_eq!(output.status.code(), Some(2), "{selector}");
        let envelope = parse_stdout(&output);
        assert_eq!(envelope["refusal"]["detail"]["reason"], "sheet_not_found");
        // Sheet names are withheld by default: the count says which indexes exist.
        assert_eq!(envelope["refusal"]["detail"]["sheet_count"], 2);
        assert!(envelope["refusal"]["detail"].get("sheets").is_none());
        let explicit = parse_stdout(&peek_path(&path, &["--sheet", selector, "--explicit"]));
        assert_eq!(
            explicit["refusal"]["detail"]["sheets"],
            serde_json::json!(["Servicer Watch List", "Notes"])
        );
    }
}

#[test]
fn workbook_rows_flag_limits_rows_and_legacy_xls_and_corrupt_files_are_handled() {
    let path = materialize_fixture("watl_msc.xlsx");
    let result = parse_stdout(&peek_path(&path, &["--rows", "3"]))["result"].clone();
    assert_eq!(result["rows_observed"], 3);

    let files = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/files");
    for name in ["sample.xlsx", "sample.xls"] {
        let output = peek_path(&files.join(name), &[]);
        assert_eq!(output.status.code(), Some(0), "{name}");
        assert!(
            parse_stdout(&output)["result"]["rows_observed"]
                .as_u64()
                .unwrap()
                >= 1
        );
    }

    let output = peek_path(&files.join("corrupt.xlsx"), &[]);
    assert_eq!(output.status.code(), Some(2));
    let envelope = parse_stdout(&output);
    assert_eq!(envelope["refusal"]["code"], "E_BAD_INPUT");
    assert_eq!(envelope["refusal"]["message"], "Could not open workbook");
}

#[test]
fn workbook_witness_hashes_the_whole_file_and_records_the_sheet() {
    let dir = tempfile::tempdir().expect("tempdir");
    let witness = dir.path().join("witness.jsonl");
    let path = materialize_fixture("watl_msc.xlsx");
    let output = run_fingerprint_with_witness(
        &["peek", path.to_str().unwrap(), "--sheet", "Notes"],
        &witness,
    );
    assert_eq!(output.status.code(), Some(0));
    let record: Value = serde_json::from_str(
        fs::read_to_string(&witness)
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    let bytes = fs::read(&path).unwrap();
    assert_eq!(record["inputs"][0]["bytes"], bytes.len());
    assert_eq!(
        record["inputs"][0]["hash"],
        format!("blake3:{}", blake3::hash(&bytes).to_hex())
    );
    assert_eq!(record["params"]["sheet"], "Notes");
    let text = fs::read_to_string(&witness).unwrap();
    assert!(!text.contains("Harbor Plaza"));
}

// ---- bd-hra: parquet footer peek -----------------------------------------------------------------

fn parquet_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/parquet")
        .join(name)
}

#[test]
fn parquet_peek_reports_the_footer_structure() {
    // delivery.parquet is written by pyarrow (tests/fixtures/parquet/make_parquet_fixtures.py):
    // 30 rows in 3 row groups of 10, 8 leaf columns. 20 of the 30 `tags` lists are empty, which
    // is 20 nulls at the list's element leaf.
    let output = peek_path(&parquet_fixture("delivery.parquet"), &[]);
    assert_eq!(output.status.code(), Some(0));
    let result = parse_stdout(&output)["result"].clone();
    assert_eq!(result["format"], "parquet");
    assert_eq!(result["rows_total"], 30);
    assert_eq!(
        result["row_groups"],
        serde_json::json!({"count": 3, "rows_min": 10, "rows_max": 10})
    );
    assert_eq!(result["columns_total"], 8);
    let summary = &result["summary"];
    assert_eq!(
        summary["physical_types"],
        serde_json::json!({"boolean": 1, "byte_array": 2, "double": 1, "fixed_len_byte_array": 1, "int32": 1, "int64": 2})
    );
    assert_eq!(
        summary["logical_types"],
        serde_json::json!({"date": 1, "decimal": 1, "string": 2, "timestamp": 1})
    );
    assert_eq!(summary["nested_columns"], 1);
    assert_eq!(summary["columns_with_unknown_null_count"], 0);
    let columns = result["columns"].as_array().unwrap();
    assert_eq!(columns.len(), 8);
    assert!(
        columns
            .iter()
            .all(|c| c["compression"] == serde_json::json!(["uncompressed"]))
    );
    assert!(columns.iter().take(7).all(|c| c["null_count"] == 0));
    let tags = &columns[7];
    assert_eq!(tags["null_count"], 20);
    assert_eq!(tags["max_repetition_level"], 1);
    assert_eq!(result["key_value_metadata_count"], 4);
    assert!(result.get("key_value_metadata_keys").is_none());
    // bd-zwx: columns are identified by an ordinal ref in file (column) order, never a hash of
    // the name; no name by default.
    assert_eq!(columns[0]["id"], "redacted:ref:c1");
    assert_eq!(columns[7]["id"], "redacted:ref:c8");
    assert!(columns[0].get("name").is_none());
}

#[test]
fn parquet_peek_reads_only_the_footer() {
    // Every data page is zeroed; the footer is intact. Peek still reports everything.
    let output = peek_path(&parquet_fixture("body_zeroed.parquet"), &[]);
    assert_eq!(output.status.code(), Some(0));
    let result = parse_stdout(&output)["result"].clone();
    assert_eq!(result["rows_total"], 3000);
    assert_eq!(result["row_groups"]["count"], 3);
    assert_eq!(result["columns_total"], 8);
}

#[test]
fn parquet_peek_canary_never_prints_cells_or_metadata_values() {
    let dir = tempfile::tempdir().expect("tempdir");
    let witness = dir.path().join("witness.jsonl");
    let path = parquet_fixture("canary.parquet");
    let canaries_always_hidden = [
        "CANARY-CELL-7f3a",
        "CANARY-CELL-7f3b",
        "CANARY-META-VALUE-9c1d",
    ];

    // Default: not even the column NAME or the metadata KEY (witness included).
    let output = run_fingerprint_with_witness(&["peek", path.to_str().unwrap()], &witness);
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    let ledger = fs::read_to_string(&witness).unwrap();
    for text in [&stdout, &ledger] {
        for canary in canaries_always_hidden
            .iter()
            .chain(["canary_column_name", "canary_key"].iter())
        {
            assert!(!text.contains(canary), "{canary} leaked by default: {text}");
        }
    }
    assert!(parse_stdout_from_str(&stdout)["witness_id"].is_string());

    // --explicit adds the names and metadata keys, and still never a cell or a metadata value.
    let output = peek_path(&path, &["--explicit"]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("canary_column_name") && stdout.contains("canary_key"),
        "{stdout}"
    );
    for canary in canaries_always_hidden {
        assert!(
            !stdout.contains(canary),
            "{canary} leaked with --explicit: {stdout}"
        );
    }
}

#[test]
fn parquet_peek_witness_hashes_the_whole_file_and_a_bad_footer_refuses() {
    let dir = tempfile::tempdir().expect("tempdir");
    let witness = dir.path().join("witness.jsonl");
    let path = parquet_fixture("delivery.parquet");
    let output = run_fingerprint_with_witness(&["peek", path.to_str().unwrap()], &witness);
    assert_eq!(output.status.code(), Some(0));
    let record: Value = serde_json::from_str(
        fs::read_to_string(&witness)
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        record["inputs"][0]["hash"],
        format!(
            "blake3:{}",
            blake3::hash(&fs::read(&path).unwrap()).to_hex()
        )
    );

    let fake = dir.path().join("fake.parquet");
    fs::write(&fake, b"loan_id,balance\nL1,1\n").unwrap();
    let output = peek_path(&fake, &[]);
    assert_eq!(output.status.code(), Some(2));
    let envelope = parse_stdout(&output);
    assert_eq!(
        envelope["refusal"]["message"],
        "Could not read parquet footer"
    );
}

// ---- bd-hra: json / jsonl schema-tree peek -------------------------------------------------------

fn json_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/json")
        .join(name)
}

/// The field with this path. Paths are only printed with `--explicit` (bd-zwx: the default id is an
/// ordinal ref, which cannot be looked up by name), so callers pass an `--explicit` result.
fn field<'a>(result: &'a Value, path: &str) -> &'a Value {
    result["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["path"] == path)
        .unwrap_or_else(|| panic!("no field {path}"))
}

#[test]
fn jsonl_peek_reports_presence_rates_and_types_per_field_path() {
    // loans.jsonl: 5 records {loan_id, balance, status, tags[]}; balance is 3 integers and 2
    // non-integer numbers; 3 of 5 records have a non-empty tags list (4 string elements).
    let output = peek_path(&json_fixture("loans.jsonl"), &["--explicit"]);
    assert_eq!(output.status.code(), Some(0));
    let result = parse_stdout(&output)["result"].clone();
    // bd-zwx: the default output is the explicit output minus the printed paths: same fields in
    // the same order with the same statistics, ids being ordinal refs in file order.
    let default = parse_stdout(&peek_path(&json_fixture("loans.jsonl"), &[]))["result"].clone();
    for (plain, named) in default["fields"]
        .as_array()
        .unwrap()
        .iter()
        .zip(result["fields"].as_array().unwrap())
    {
        let mut named = named.clone();
        named.as_object_mut().unwrap().remove("path");
        assert_eq!(*plain, named);
    }
    assert_eq!(default["fields"][0]["id"], "redacted:ref:p1");
    assert_eq!(result["format"], "jsonl");
    assert_eq!(result["rows_observed"], 5);
    assert_eq!(result["fields_total"], 6);
    assert_eq!(result["fields_truncated"], false);
    assert_eq!(
        field(&result, "$")["types"],
        serde_json::json!({"object": 5})
    );
    assert_eq!(
        field(&result, "$.balance")["types"],
        serde_json::json!({"integer": 3, "number": 2})
    );
    assert_eq!(
        field(&result, "$.loan_id")["types"],
        serde_json::json!({"string": 5})
    );
    assert_eq!(
        field(&result, "$.tags")["types"],
        serde_json::json!({"array": 5})
    );
    let elements = field(&result, "$.tags[]");
    assert_eq!(elements["types"], serde_json::json!({"string": 4}));
    assert_eq!(elements["present"], 3);
    assert_eq!(elements["presence_rate"], 0.6);
    // Names are not printed by default.
    assert!(default["fields"][1].get("path").is_none());
}

#[test]
fn json_peek_uses_array_elements_or_the_object_as_records() {
    let result =
        parse_stdout(&peek_path(&json_fixture("delivery.json"), &["--explicit"]))["result"].clone();
    assert_eq!(result["format"], "json");
    assert_eq!(result["rows_observed"], 1);
    let paths: Vec<&str> = result["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap())
        .collect();
    // bd-zwx: FILE order (the document lists schema, filings, meta{generated, count, complete}),
    // no longer serde_json's name-sorted order: field order and ref numbering must not depend on
    // names. Deterministic across runs.
    assert_eq!(
        paths,
        [
            "$",
            "$.schema",
            "$.filings",
            "$.filings[]",
            "$.filings[].accession",
            "$.filings[].form",
            "$.filings[].pages",
            "$.meta",
            "$.meta.generated",
            "$.meta.count",
            "$.meta.complete"
        ]
    );
    // `filings` holds 2 objects: element paths count every element, presence counts records.
    let elements = field(&result, "$.filings[]");
    assert_eq!(elements["types"], serde_json::json!({"object": 2}));
    assert_eq!(elements["present"], 1);

    // A top-level array: each element is a record; --rows limits them.
    let result =
        parse_stdout(&peek_path(&fixture("canary.json"), &["--rows", "1"]))["result"].clone();
    assert_eq!(result["rows_observed"], 1);
    let result = parse_stdout(&peek_path(&fixture("canary.json"), &[]))["result"].clone();
    assert_eq!(result["rows_observed"], 2);
}

#[test]
fn jsonl_invalid_lines_are_counted_by_position_only() {
    let result = parse_stdout(&peek_path(&json_fixture("loans_bad.jsonl"), &[]))["result"].clone();
    assert_eq!(result["rows_observed"], 3);
    assert_eq!(result["invalid_lines"], 1);
    assert_eq!(result["first_invalid_line"], 3);

    let dir = tempfile::tempdir().expect("tempdir");
    let broken = dir.path().join("broken.json");
    fs::write(&broken, "{\"secret_key\": \"SECRET-TEXT-VALUE\" oops}").unwrap();
    let output = peek_path(&broken, &[]);
    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Input is not valid JSON") && stdout.contains("\"line\""));
    assert!(
        !stdout.contains("SECRET-TEXT-VALUE") && !stdout.contains("secret_key"),
        "{stdout}"
    );

    let none = dir.path().join("none.jsonl");
    fs::write(&none, "not json\nalso not json\n").unwrap();
    assert_eq!(peek_path(&none, &[]).status.code(), Some(2));
}

#[test]
fn json_and_jsonl_canary_never_print_keys_by_default_or_values_ever() {
    let dir = tempfile::tempdir().expect("tempdir");
    for name in ["canary.jsonl", "canary.json"] {
        let path = fixture(name);
        let witness = dir.path().join(format!("{name}.witness"));
        let output = run_fingerprint_with_witness(&["peek", path.to_str().unwrap()], &witness);
        assert_eq!(output.status.code(), Some(0), "{name}");
        let stdout = String::from_utf8(output.stdout).unwrap();
        let ledger = fs::read_to_string(&witness).unwrap();
        for text in [&stdout, &ledger] {
            for planted in [
                "CANARY-VALUE",
                "CANARY-INNER",
                "CANARY-SSN",
                "078-05-1120",
                "canary_key_alpha",
                "canary_inner_key",
                "ssn",
                "nested",
                "amounts",
            ] {
                assert!(
                    !text.contains(planted),
                    "{name}: {planted} leaked by default: {text}"
                );
            }
        }
        // --explicit prints key names, still never a value.
        let output = peek_path(&path, &["--explicit"]);
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(
            stdout.contains("canary_key_alpha") && stdout.contains("$.nested.canary_inner_key"),
            "{name}: {stdout}"
        );
        for planted in [
            "CANARY-VALUE",
            "CANARY-INNER",
            "CANARY-SSN",
            "078-05-1120",
            "11.5",
        ] {
            assert!(
                !stdout.contains(planted),
                "{name}: {planted} leaked with --explicit: {stdout}"
            );
        }
    }
}

#[test]
fn jsonl_peek_streams_and_hashes_the_whole_file_in_the_witness() {
    use std::io::Write;
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("big.jsonl");
    {
        let mut file = fs::File::create(&path).expect("create");
        for i in 0..100_000 {
            writeln!(file, "{{\"id\":{i},\"name\":\"n{i}\"}}").unwrap();
        }
        // Garbage after the inspected rows must not matter.
        file.write_all(&vec![0xFFu8; 1024 * 1024]).unwrap();
    }
    let witness = dir.path().join("witness.jsonl");
    let output =
        run_fingerprint_with_witness(&["peek", path.to_str().unwrap(), "--rows", "10"], &witness);
    assert_eq!(output.status.code(), Some(0));
    let result = parse_stdout(&output)["result"].clone();
    assert_eq!(result["rows_observed"], 10);
    let record: Value = serde_json::from_str(
        fs::read_to_string(&witness)
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        record["inputs"][0]["hash"],
        format!(
            "blake3:{}",
            blake3::hash(&fs::read(&path).unwrap()).to_hex()
        )
    );
}

// ---- bd-jxd: shared names with `profile inspect` ------------------------------------------------

#[test]
fn peek_uses_profile_inspects_names_for_the_concepts_they_share() {
    // tests/fixtures/peek/inspect_vocabulary.json records what `profile inspect` printed for
    // these files (provenance inside the file). The two commands answer different questions
    // (peek: row SHAPES before parsing; inspect: column STATS after parsing), so only the
    // shared concepts are compared: format, dialect (name, delimiter), row_count,
    // column_count and, for Parquet, the column type vocabulary.
    let snapshot: Value = serde_json::from_str(
        &fs::read_to_string(fixture("inspect_vocabulary.json")).expect("read snapshot"),
    )
    .expect("snapshot json");
    for (file, expected) in snapshot["files"].as_object().expect("files") {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(file);
        let output = peek_path(&path, &[]);
        assert_eq!(output.status.code(), Some(0), "{file}");
        let result = parse_stdout(&output)["result"].clone();
        assert_eq!(result["format"], expected["format"], "{file}: format");
        assert_eq!(
            result["dialect"]["name"], expected["dialect"]["name"],
            "{file}: dialect.name"
        );
        assert_eq!(
            result["dialect"]["delimiter"], expected["dialect"]["delimiter"],
            "{file}: dialect.delimiter"
        );
        assert_eq!(
            result["column_count"], expected["column_count"],
            "{file}: column_count"
        );
        if let Some(rows) = expected.get("row_count") {
            assert_eq!(&result["row_count"], rows, "{file}: row_count");
        }
        if let Some(types) = expected.get("column_types") {
            let got: Vec<Value> = result["columns"]
                .as_array()
                .map(|columns| columns.iter().map(|c| c["type"].clone()).collect())
                .unwrap_or_default();
            if !got.is_empty() {
                assert_eq!(&Value::Array(got), types, "{file}: column types");
            }
        }
        // Peek's own header concept keeps both names: header_at_row (profile pre_parse's key)
        // and header_row (inspect's name).
        if result.get("summary").is_some() {
            assert_eq!(
                result["summary"]["header_row"], result["summary"]["header_at_row"],
                "{file}"
            );
        }
    }
}

#[test]
fn every_peek_format_carries_the_dialect_object() {
    let cases = [
        (fixture("clean_flat.csv"), "csv", ","),
        (fixture("tsv.tsv"), "tsv", "\\t"),
        (materialize_fixture("watl_msc.xlsx"), "xlsx", "none"),
        (parquet_fixture("canary.parquet"), "parquet", "none"),
        (json_fixture("loans.jsonl"), "jsonl", "none"),
        (json_fixture("delivery.json"), "json", "none"),
    ];
    for (path, name, delimiter) in cases {
        let result = parse_stdout(&peek_path(&path, &[]))["result"].clone();
        assert_eq!(result["dialect"]["name"], name, "{path:?}");
        assert_eq!(result["dialect"]["delimiter"], delimiter, "{path:?}");
    }
    // A semicolon file reports the guessed character in the same object.
    let result = peek_result("mixed_delimiters.csv", &[]);
    assert_eq!(result["dialect"]["delimiter"], ";");
}

// ---- bd-hra: xml element-path skeleton on the three REAL SEC filings -----------------------------

fn xml_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/xml")
        .join(name)
}

fn xml_result(name: &str, extra: &[&str]) -> Value {
    let output = peek_path(&xml_fixture(name), extra);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{name}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    parse_stdout(&output)["result"].clone()
}

fn record_candidates(result: &Value) -> Vec<(String, u64, u64)> {
    result["summary"]["record_candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c["path"].as_str().unwrap_or("").to_owned(),
                c["count"].as_u64().unwrap(),
                c["leaf_children"].as_u64().unwrap(),
            )
        })
        .collect()
}

#[test]
fn xml_peek_reports_the_element_skeleton_of_each_real_filing() {
    // Expected values were computed independently with Python's xml.etree (see
    // tests/fixtures/xml/README.md): element totals, the repeating record paths and the number
    // of distinct leaf children of each record element.
    let comments = xml_result("abs-ee/exh_103.xml", &["--explicit"]);
    assert_eq!(comments["format"], "xml");
    assert_eq!(
        comments["dialect"],
        serde_json::json!({"name": "xml", "delimiter": "none"})
    );
    assert_eq!(comments["root"]["name"], "comments");
    assert_eq!(comments["root"]["namespace"], Value::Null);
    assert_eq!(comments["element_count"], 129);
    assert_eq!(comments["paths_total"], 5);
    assert_eq!(comments["max_depth"], 3);
    assert_eq!(
        record_candidates(&comments),
        [("/comments/commentData".to_owned(), 32, 3)]
    );

    let assets = xml_result("abs-ee/exh_102.xml", &["--explicit"]);
    assert_eq!(assets["root"]["name"], "assetData");
    assert_eq!(
        assets["root"]["namespace"],
        "http://www.sec.gov/edgar/document/absee/cmbs/assetdata"
    );
    assert_eq!(assets["element_count"], 4156);
    assert_eq!(
        record_candidates(&assets),
        [
            ("/assetData/assets".to_owned(), 47, 68),
            ("/assetData/assets/property".to_owned(), 53, 53)
        ]
    );

    let nport = xml_result("nport-p/primary_doc.xml", &["--explicit"]);
    assert_eq!(nport["root"]["name"], "edgarSubmission");
    assert_eq!(nport["root"]["namespace"], "http://www.sec.gov/edgar/nport");
    assert_eq!(nport["element_count"], 1057);
    assert_eq!(
        record_candidates(&nport),
        [(
            "/edgarSubmission/formData/invstOrSecs/invstOrSec".to_owned(),
            45,
            15
        )]
    );
}

#[test]
fn xml_peek_is_name_free_by_default_and_deterministic() {
    let one = xml_result("abs-ee/exh_103.xml", &[]);
    let two = xml_result("abs-ee/exh_103.xml", &[]);
    assert_eq!(one, two);
    // bd-zwx: ordinal refs in document order, no hash of any name. The root shares the ref of
    // its own path row (equal names in one output share a ref).
    assert_eq!(one["root"]["id"], "redacted:ref:p1");
    assert_eq!(one["paths"][0]["id"], "redacted:ref:p1");
    assert_eq!(one["root"]["namespace_id"], Value::Null);
    let candidates = one["summary"]["record_candidates"].as_array().unwrap();
    assert_eq!(candidates[0]["id"], "redacted:ref:p2");
    assert!(candidates[0].get("path").is_none());
    // The three depth-3 leaves are numbered in FILE order (itemNumber, fieldName, comment), not
    // in name order (comment, fieldName, itemNumber).
    let named = xml_result("abs-ee/exh_103.xml", &["--explicit"]);
    let leaves: Vec<(&str, &str)> = named["paths"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["depth"] == 3)
        .map(|p| (p["id"].as_str().unwrap(), p["path"].as_str().unwrap()))
        .collect();
    assert_eq!(
        leaves,
        [
            ("redacted:ref:p3", "/comments/commentData/itemNumber"),
            ("redacted:ref:p4", "/comments/commentData/fieldName"),
            ("redacted:ref:p5", "/comments/commentData/comment"),
        ]
    );
    // Ordered by depth, then first appearance: the order says nothing about element names.
    let depths: Vec<u64> = one["paths"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["depth"].as_u64().unwrap())
        .collect();
    let mut sorted = depths.clone();
    sorted.sort_unstable();
    assert_eq!(depths, sorted);
}

#[test]
fn xml_peek_canary_never_prints_text_attribute_values_or_names_by_default() {
    // exh_103 is prose: its comment text must never appear. The element and namespace NAMES of
    // all three filings are hidden unless --explicit, and no text or attribute value is ever
    // printed (registrant, LEI, loan amounts, state codes).
    let dir = tempfile::tempdir().expect("tempdir");
    let hidden_text = [
        "Hilton Hawaiian Village",
        "pari passu",
        "anticipated repayment",
        "Cut-off Date",
        "Item 2(c)(15)",
        "Original Loan Term Number",
        "SEASONS SERIES TRUST",
        "811-07725",
        "549300O318N6OBWGKN57",
        "Memorial Drive",
        "US-TX",
        "S000008051",
        "95000000",
        "Prospectus Loan ID",
        "CREFI",
        "SA MID CAP INDEX",
    ];
    let hidden_names = [
        "commentData",
        "itemNumber",
        "fieldName",
        "edgarSubmission",
        "regName",
        "regStateConditional",
        "assetData",
        "originalLoanAmount",
        "invstOrSec",
        "sec.gov",
        "http://",
    ];
    for name in [
        "abs-ee/exh_103.xml",
        "abs-ee/exh_102.xml",
        "nport-p/primary_doc.xml",
    ] {
        let path = xml_fixture(name);
        let witness = dir
            .path()
            .join(format!("{}.witness", name.replace('/', "_")));
        let output = run_fingerprint_with_witness(&["peek", path.to_str().unwrap()], &witness);
        assert_eq!(output.status.code(), Some(0), "{name}");
        let stdout = String::from_utf8(output.stdout).unwrap();
        let ledger = fs::read_to_string(&witness).unwrap();
        for text in [&stdout, &ledger] {
            for planted in hidden_text.iter().chain(hidden_names.iter()) {
                assert!(
                    !text.contains(planted),
                    "{name}: {planted} leaked by default: {text}"
                );
            }
        }
        // --explicit adds names, never text or attribute values.
        let explicit = String::from_utf8(peek_path(&path, &["--explicit"]).stdout).unwrap();
        for planted in hidden_text {
            assert!(
                !explicit.contains(planted),
                "{name}: {planted} leaked with --explicit"
            );
        }
    }
    assert!(
        String::from_utf8(peek_path(&xml_fixture("abs-ee/exh_103.xml"), &["--explicit"]).stdout)
            .unwrap()
            .contains("/comments/commentData/comment")
    );
}

#[test]
fn xml_peek_witness_hashes_the_whole_file_and_bad_xml_refuses_without_content() {
    let dir = tempfile::tempdir().expect("tempdir");
    let witness = dir.path().join("witness.jsonl");
    let path = xml_fixture("abs-ee/exh_103.xml");
    let output = run_fingerprint_with_witness(&["peek", path.to_str().unwrap()], &witness);
    assert_eq!(output.status.code(), Some(0));
    let record: Value = serde_json::from_str(
        fs::read_to_string(&witness)
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        record["inputs"][0]["hash"],
        format!(
            "blake3:{}",
            blake3::hash(&fs::read(&path).unwrap()).to_hex()
        )
    );

    let bad = dir.path().join("bad.xml");
    fs::write(&bad, "<a><secret_element>SECRET-TEXT-VALUE</a>").unwrap();
    let output = peek_path(&bad, &[]);
    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("Could not parse xml") && stdout.contains("at byte"),
        "{stdout}"
    );
    assert!(!stdout.contains("SECRET-TEXT-VALUE"), "{stdout}");
    let empty = dir.path().join("empty.xml");
    fs::write(&empty, "").unwrap();
    assert_eq!(peek_path(&empty, &[]).status.code(), Some(2));
}

// ---- bd-hra: peek --view brief --------------------------------------------------------------------

#[test]
fn brief_view_is_bounded_for_every_format_and_a_subset_of_the_full_view() {
    // ~500 tokens at roughly 4 bytes a token: the whole envelope must stay under 2,000 bytes.
    let files: Vec<(&str, PathBuf)> = vec![
        ("csv", fixture("shape_classes.csv")),
        ("tsv", fixture("tsv.tsv")),
        ("xlsx", materialize_fixture("watl_msc.xlsx")),
        ("parquet", parquet_fixture("delivery.parquet")),
        ("jsonl", json_fixture("loans.jsonl")),
        ("json", json_fixture("delivery.json")),
        ("xml 102", xml_fixture("abs-ee/exh_102.xml")),
        ("xml nport", xml_fixture("nport-p/primary_doc.xml")),
        ("xml 103", xml_fixture("abs-ee/exh_103.xml")),
    ];
    for (label, path) in files {
        let brief = peek_path(&path, &["--view", "brief", "--suggest"]);
        assert_eq!(brief.status.code(), Some(0), "{label}");
        let compact = serde_json::to_vec(&parse_stdout(&brief)).unwrap();
        assert!(
            compact.len() < 2000,
            "{label}: brief is {} bytes",
            compact.len()
        );
        let brief_result = parse_stdout(&brief)["result"].clone();
        let full_result = parse_stdout(&peek_path(&path, &["--suggest"]))["result"].clone();

        assert_eq!(brief_result["view"], "brief", "{label}");
        for dropped in ["rows", "columns", "fields", "paths", "sheets"] {
            assert!(
                brief_result.get(dropped).is_none(),
                "{label}: {dropped} must be omitted"
            );
        }
        let omitted: Vec<&str> = brief_result["omitted"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(!omitted.is_empty(), "{label}: something was dropped");
        for key in &omitted {
            assert!(
                full_result.get(*key).is_some(),
                "{label}: full view has {key}"
            );
        }
        // Everything kept in the brief equals the full view's value (candidates may be cut).
        for (key, value) in brief_result.as_object().unwrap() {
            if matches!(key.as_str(), "view" | "omitted" | "summary") {
                continue;
            }
            assert_eq!(&full_result[key], value, "{label}: {key}");
        }
        assert_eq!(
            full_result.get("view"),
            None,
            "{label}: full view is unmarked"
        );
    }
}

#[test]
fn brief_view_keeps_the_decisions_agents_need() {
    let csv = parse_stdout(&peek_path(
        &fixture("shape_classes.csv"),
        &["--view", "brief", "--suggest"],
    ))["result"]
        .clone();
    assert_eq!(csv["summary"]["header_at_row"], 5);
    assert_eq!(
        csv["suggestions"]["profile_pre_parse"]["mode"],
        "preamble_with_units"
    );
    let xml = parse_stdout(&peek_path(
        &xml_fixture("abs-ee/exh_102.xml"),
        &["--view", "brief"],
    ))["result"]
        .clone();
    assert_eq!(
        xml["summary"]["record_candidates"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(xml["element_count"], 4156);
    let parquet = parse_stdout(&peek_path(
        &parquet_fixture("delivery.parquet"),
        &["--view", "brief"],
    ))["result"]
        .clone();
    assert_eq!(parquet["row_count"], 30);
    assert_eq!(parquet["summary"]["nested_columns"], 1);
}

#[test]
fn brief_view_witness_hashes_what_was_printed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let witness = dir.path().join("witness.jsonl");
    let path = parquet_fixture("delivery.parquet");
    let output = run_fingerprint_with_witness(
        &["peek", path.to_str().unwrap(), "--view", "brief"],
        &witness,
    );
    assert_eq!(output.status.code(), Some(0));
    let printed = parse_stdout(&output)["result"].clone();
    let record: Value = serde_json::from_str(
        fs::read_to_string(&witness)
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        record["output_hash"],
        format!(
            "blake3:{}",
            blake3::hash(&serde_json::to_vec(&printed).unwrap()).to_hex()
        )
    );
}

#[test]
fn brief_view_is_still_zero_retention() {
    for (name, planted) in [
        (parquet_fixture("canary.parquet"), "CANARY"),
        (fixture("canary.jsonl"), "CANARY"),
        (xml_fixture("abs-ee/exh_103.xml"), "Hilton"),
    ] {
        let stdout = String::from_utf8(peek_path(&name, &["--view", "brief"]).stdout).unwrap();
        assert!(!stdout.contains(planted), "{name:?}: {stdout}");
    }
}

// ---- bd-hra: html structural skeleton ---------------------------------------------------------------

fn html_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/html")
        .join(name)
}

fn html_result(path: &Path, extra: &[&str]) -> Value {
    let output = peek_path(path, extra);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{path:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    parse_stdout(&output)["result"].clone()
}

fn heading_shape(result: &Value) -> Vec<(u64, u64)> {
    result["headings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| {
            (
                h["level"].as_u64().unwrap(),
                h["tables_in_region"].as_u64().unwrap(),
            )
        })
        .collect()
}

#[test]
fn html_peek_reports_headings_tables_widths_and_candidate_regions() {
    // Expected values are read off the fixtures by hand (tests/fixtures/html/*.html).
    // bdc_soi_ares_like: h1 + two h2, three page sections, two 6-column tables of 3 rows
    // (a header row of <th>, a full-width colspan row, a data row).
    let ares = html_result(&html_fixture("bdc_soi_ares_like.html"), &[]);
    assert_eq!(ares["format"], "html");
    assert_eq!(
        ares["dialect"],
        serde_json::json!({"name": "html", "delimiter": "none"})
    );
    assert_eq!(ares["headings_total"], 3);
    assert_eq!(ares["tables_total"], 2);
    assert_eq!(heading_shape(&ares), [(1, 2), (2, 1), (2, 0)]);
    assert_eq!(ares["summary"]["page_sections"], 3);
    assert_eq!(ares["summary"]["modal_table_columns"], 6);
    assert_eq!(
        ares["summary"]["headings_by_level"],
        serde_json::json!({"1": 1, "2": 2})
    );
    assert_eq!(
        ares["summary"]["candidate_regions"],
        serde_json::json!([
            {"heading_index": 0, "level": 1, "tables": 2},
            {"heading_index": 1, "level": 2, "tables": 1}
        ])
    );
    for table in ares["tables"].as_array().unwrap() {
        assert_eq!(table["rows"], 3);
        assert_eq!(table["columns_max"], 6);
        assert_eq!(table["columns_modal"], 6);
        assert_eq!(table["header_rows"], 1);
        assert_eq!(table["colspan_cells"], 1);
        assert_eq!(table["full_width_rows"], 1);
        assert_eq!(table["row_width_histogram"], serde_json::json!({"6": 3}));
    }

    // bdc_soi_golub_like: h1, h2 (table 1), h2 (table 2): each h2 region holds one table.
    let golub = html_result(&html_fixture("bdc_soi_golub_like.html"), &[]);
    assert_eq!(heading_shape(&golub), [(1, 2), (2, 1), (2, 1)]);
    assert_eq!(golub["summary"]["page_sections"], 2);
    assert_eq!(golub["summary"]["modal_table_columns"], 5);

    // soi_subsections_repeated_period: no <h*> at all, four tables of 6, 6, 5 and 4 columns.
    let subsections = html_result(&html_fixture("soi_subsections_repeated_period.html"), &[]);
    assert_eq!(subsections["headings_total"], 0);
    assert_eq!(subsections["tables_total"], 4);
    let widths: Vec<u64> = subsections["tables"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["columns_max"].as_u64().unwrap())
        .collect();
    assert_eq!(widths, [6, 6, 5, 4]);
    assert_eq!(
        subsections["summary"]["candidate_regions"],
        serde_json::json!([])
    );
}

#[test]
fn html_peek_counts_spans_and_cell_classes_without_text() {
    // canary.html's first table: 4 rows of width 3 (header of <th>, a full-width colspan row,
    // two data rows, one cell with rowspan); cells: 1 date, 3 numeric, 2 text, 4 mixed (they
    // contain digits and letters).
    let result = html_result(&fixture("canary.html"), &[]);
    assert_eq!(result["element_count"], 33);
    assert_eq!(result["summary"]["page_sections"], 2);
    let table = &result["tables"][0];
    assert_eq!(table["rows"], 4);
    assert_eq!(table["columns_max"], 3);
    assert_eq!(table["header_rows"], 1);
    assert_eq!(table["colspan_cells"], 1);
    assert_eq!(table["rowspan_cells"], 1);
    assert_eq!(table["full_width_rows"], 1);
    assert_eq!(table["nested_tables"], 0);
    assert_eq!(table["row_width_histogram"], serde_json::json!({"3": 4}));
    assert_eq!(
        table["cell_type_counts"],
        serde_json::json!({"boolean": 0, "date": 1, "empty": 0, "mixed": 4, "numeric": 3, "text": 2})
    );
    assert_eq!(heading_shape(&result), [(1, 2), (2, 1)]);
    // Deterministic.
    assert_eq!(result, html_result(&fixture("canary.html"), &[]));
}

#[test]
fn html_peek_canary_never_prints_text_attribute_values_or_names_by_default() {
    let dir = tempfile::tempdir().expect("tempdir");
    let witness = dir.path().join("witness.jsonl");
    let path = fixture("canary.html");
    let output = run_fingerprint_with_witness(&["peek", path.to_str().unwrap()], &witness);
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    let ledger = fs::read_to_string(&witness).unwrap();
    // Text, attribute values, comment, script, title, href and SSN-like strings (uppercase
    // CANARY- markers) never appear; neither do the lowercase class/attribute/id names, nor tag
    // names (the `tags` and `classes` objects are --explicit only).
    let always_hidden = [
        "CANARY-",
        "078-05-1120",
        "canary.example",
        "https://",
        "1,250.50",
        "2026-01-31",
        "Prose with",
        "Amount",
        "Share",
    ];
    let hidden_names = [
        "canary-",
        "canary_script_var",
        "data-canary-attr",
        "\"tags\"",
        "\"classes\"",
        "tbody",
        "<table",
        "\"section\"", // a tag-name key; the output's own `page_sections` is not one
        "script",
    ];
    for text in [&stdout, &ledger] {
        for planted in always_hidden.iter().chain(hidden_names.iter()) {
            assert!(
                !text.contains(planted),
                "{planted} leaked by default: {text}"
            );
        }
    }
    assert!(parse_stdout_from_str(&stdout)["witness_id"].is_string());

    // --explicit adds tag and class names and heading pseudonyms, still never text or values.
    let explicit = String::from_utf8(peek_path(&path, &["--explicit"]).stdout).unwrap();
    assert!(
        explicit.contains("canary-table-class") && explicit.contains("\"tbody\""),
        "{explicit}"
    );
    for planted in always_hidden {
        assert!(
            !explicit.contains(planted),
            "{planted} leaked with --explicit: {explicit}"
        );
    }
    for attribute_name in ["data-canary-attr", "canary_script_var"] {
        assert!(
            !explicit.contains(attribute_name),
            "{attribute_name}: {explicit}"
        );
    }
    // bd-zwx: the heading id is an ordinal ref (present by default, never a hash of the text);
    // --explicit adds names but still never the heading text.
    let result = parse_stdout_from_str(&explicit)["result"].clone();
    assert_eq!(result["headings"][0]["id"], "redacted:ref:h1");
    let default = parse_stdout_from_str(&stdout)["result"].clone();
    assert_eq!(default["headings"][0]["id"], "redacted:ref:h1");
}

#[test]
fn html_peek_brief_view_is_bounded_and_witness_and_refusals_work() {
    for name in [
        "bdc_soi_ares_like.html",
        "bdc_soi_golub_like.html",
        "ares_multi_soi.html",
        "soi_subsections_repeated_period.html",
        "oxsq_pagebreaks.html",
    ] {
        let brief = peek_path(&html_fixture(name), &["--view", "brief", "--explicit"]);
        assert_eq!(brief.status.code(), Some(0), "{name}");
        let compact = serde_json::to_vec(&parse_stdout(&brief)).unwrap();
        assert!(
            compact.len() < 2000,
            "{name}: brief is {} bytes",
            compact.len()
        );
        let result = parse_stdout(&brief)["result"].clone();
        assert_eq!(result["view"], "brief");
        for dropped in ["headings", "tables", "tags", "classes"] {
            assert!(result.get(dropped).is_none(), "{name}: {dropped}");
        }
        let full = html_result(&html_fixture(name), &[]);
        assert_eq!(result["tables_total"], full["tables_total"], "{name}");
        assert_eq!(result["headings_total"], full["headings_total"], "{name}");
    }

    let dir = tempfile::tempdir().expect("tempdir");
    let witness = dir.path().join("witness.jsonl");
    let path = html_fixture("bdc_soi_ares_like.html");
    let output = run_fingerprint_with_witness(
        &["peek", path.to_str().unwrap(), "--view", "brief"],
        &witness,
    );
    assert_eq!(output.status.code(), Some(0));
    let record: Value = serde_json::from_str(
        fs::read_to_string(&witness)
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        record["inputs"][0]["hash"],
        format!(
            "blake3:{}",
            blake3::hash(&fs::read(&path).unwrap()).to_hex()
        )
    );
    let printed = parse_stdout(&output)["result"].clone();
    assert_eq!(
        record["output_hash"],
        format!(
            "blake3:{}",
            blake3::hash(&serde_json::to_vec(&printed).unwrap()).to_hex()
        )
    );

    let empty = dir.path().join("empty.html");
    fs::write(&empty, "").unwrap();
    assert_eq!(peek_path(&empty, &[]).status.code(), Some(2));
}

// ---- bd-hra: pdf structure ---------------------------------------------------------------------------

/// Build a PDF with lopdf (an independent writer from the reader under test): 3 pages (two
/// Letter, one A4 rotated 90), an Info dictionary with standard and custom keys, planted
/// strings in every value, page content and bookmark title, and a two-level outline
/// (A with children A1 and A2, then B).
fn write_canary_pdf(path: &Path) {
    use lopdf::{Object, Stream, dictionary};
    let mut document = lopdf::Document::with_version("1.5");
    let pages_id = document.new_object_id();
    let mut kids = Vec::new();
    for (index, (width, height, rotate)) in [(612, 792, 0), (612, 792, 0), (595, 842, 90)]
        .into_iter()
        .enumerate()
    {
        let content_id = document.add_object(Stream::new(
            lopdf::Dictionary::new(),
            format!("BT /F1 12 Tf (CANARY-PAGE-TEXT-{index}-7a8b) Tj ET").into_bytes(),
        ));
        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "MediaBox" => vec![0.into(), 0.into(), width.into(), height.into()],
            "Rotate" => rotate,
        });
        kids.push(Object::Reference(page_id));
    }
    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => kids,
            "Count" => 3,
        }),
    );

    // Outline items with Parent/First/Last/Next links.
    let outline_root = document.new_object_id();
    let ids: Vec<_> = (0..4).map(|_| document.new_object_id()).collect(); // A, A1, A2, B
    let (a, a1, a2, b) = (ids[0], ids[1], ids[2], ids[3]);
    let item = |title: &str,
                parent,
                first_last: Option<(lopdf::ObjectId, lopdf::ObjectId)>,
                next: Option<lopdf::ObjectId>| {
        let mut dict = dictionary! {
            "Title" => Object::string_literal(title),
            "Parent" => parent,
        };
        if let Some((first, last)) = first_last {
            dict.set("First", first);
            dict.set("Last", last);
            dict.set("Count", 2);
        }
        if let Some(next) = next {
            dict.set("Next", next);
        }
        Object::Dictionary(dict)
    };
    document.objects.insert(
        a,
        item(
            "CANARY-OUTLINE-A-11aa",
            outline_root,
            Some((a1, a2)),
            Some(b),
        ),
    );
    document
        .objects
        .insert(a1, item("CANARY-OUTLINE-A1-22bb", a, None, Some(a2)));
    document
        .objects
        .insert(a2, item("CANARY-OUTLINE-A2-33cc", a, None, None));
    document
        .objects
        .insert(b, item("CANARY-OUTLINE-B-44dd", outline_root, None, None));
    document.objects.insert(
        outline_root,
        Object::Dictionary(dictionary! {
            "Type" => "Outlines",
            "First" => a,
            "Last" => b,
            "Count" => 4,
        }),
    );

    let info_id = document.add_object(dictionary! {
        "Title" => Object::string_literal("CANARY-TITLE-1a2b"),
        "Author" => Object::string_literal("CANARY-AUTHOR-3c4d"),
        "Producer" => Object::string_literal("fingerprint-test-writer"),
        "CanaryCustomKey" => Object::string_literal("CANARY-CUSTOM-VALUE-5e6f"),
    });
    let catalog_id = document.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
        "Outlines" => outline_root,
    });
    document.trailer.set("Root", catalog_id);
    document.trailer.set("Info", info_id);
    document.save(path).expect("write canary pdf");
}

fn pdf_result(path: &Path, extra: &[&str]) -> Value {
    let output = peek_path(path, extra);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    parse_stdout(&output)["result"].clone()
}

#[test]
fn pdf_peek_reports_pages_metadata_keys_and_the_outline_tree() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("canary.pdf");
    write_canary_pdf(&path);
    let result = pdf_result(&path, &[]);
    assert_eq!(result["format"], "pdf");
    assert_eq!(
        result["dialect"],
        serde_json::json!({"name": "pdf", "delimiter": "none"})
    );
    assert_eq!(result["version"], "1.5");
    assert_eq!(result["encrypted"], false);
    assert_eq!(result["page_count"], 3);
    assert_eq!(
        result["summary"]["page_sizes"],
        serde_json::json!([
            {"width": 612.0, "height": 792.0, "rotate": 0, "pages": 2},
            {"width": 595.0, "height": 842.0, "rotate": 90, "pages": 1}
        ])
    );
    // Standard Info keys by name, the custom key as an ordinal ref.
    assert_eq!(result["metadata"]["key_count"], 4);
    assert_eq!(
        result["metadata"]["standard_keys"],
        serde_json::json!(["Author", "Producer", "Title"])
    );
    // bd-zwx: the custom key is an ordinal ref (kind k), never a hash of its name.
    assert_eq!(
        result["metadata"]["other_key_ids"],
        serde_json::json!(["redacted:ref:k1"])
    );
    assert!(result["metadata"].get("other_keys").is_none());
    // Outline in document order: A (2 children), A1, A2, B.
    let summary = &result["summary"];
    assert_eq!(summary["outline_entries"], 4);
    assert_eq!(summary["outline_max_depth"], 2);
    assert_eq!(summary["outline_top_level"], 2);
    let shape: Vec<(u64, u64)> = result["outline"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["depth"].as_u64().unwrap(),
                e["children"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(shape, [(1, 2), (2, 0), (2, 0), (1, 0)]);
    assert_eq!(result, pdf_result(&path, &[]), "deterministic");
}

#[test]
fn pdf_peek_canary_never_prints_metadata_values_titles_or_page_text() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("canary.pdf");
    write_canary_pdf(&path);
    let witness = dir.path().join("witness.jsonl");
    let output = run_fingerprint_with_witness(&["peek", path.to_str().unwrap()], &witness);
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    let ledger = fs::read_to_string(&witness).unwrap();
    for text in [&stdout, &ledger] {
        for planted in ["CANARY-", "CanaryCustomKey", "fingerprint-test-writer"] {
            assert!(
                !text.contains(planted),
                "{planted} leaked by default: {text}"
            );
        }
    }
    // The standard key NAMES are public vocabulary and are printed; values never are.
    assert!(stdout.contains("\"Title\"") && stdout.contains("\"Author\""));

    // --explicit adds the custom key name and outline pseudonyms, never a value or a title.
    let explicit = String::from_utf8(peek_path(&path, &["--explicit"]).stdout).unwrap();
    assert!(explicit.contains("CanaryCustomKey"), "{explicit}");
    for planted in ["CANARY-", "fingerprint-test-writer"] {
        assert!(
            !explicit.contains(planted),
            "{planted} leaked with --explicit: {explicit}"
        );
    }
    // bd-zwx: outline ids are ordinal refs in outline order (present by default, equal titles
    // share one); never a hash of the title.
    let result = parse_stdout_from_str(&explicit)["result"].clone();
    assert_eq!(result["outline"][0]["id"], "redacted:ref:o1");
    let default = parse_stdout_from_str(&stdout)["result"].clone();
    assert_eq!(default["outline"][0]["id"], "redacted:ref:o1");
}

#[test]
fn pdf_peek_brief_view_witness_and_refusals() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("canary.pdf");
    write_canary_pdf(&path);
    let witness = dir.path().join("witness.jsonl");
    let output = run_fingerprint_with_witness(
        &[
            "peek",
            path.to_str().unwrap(),
            "--view",
            "brief",
            "--explicit",
        ],
        &witness,
    );
    assert_eq!(output.status.code(), Some(0));
    let compact = serde_json::to_vec(&parse_stdout(&output)).unwrap();
    assert!(compact.len() < 2000, "brief is {} bytes", compact.len());
    let brief = parse_stdout(&output)["result"].clone();
    assert!(brief.get("outline").is_none());
    assert_eq!(brief["summary"]["outline_entries"], 4);
    assert_eq!(brief["page_count"], 3);
    let record: Value = serde_json::from_str(
        fs::read_to_string(&witness)
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        record["inputs"][0]["hash"],
        format!(
            "blake3:{}",
            blake3::hash(&fs::read(&path).unwrap()).to_hex()
        )
    );
    assert_eq!(
        record["output_hash"],
        format!(
            "blake3:{}",
            blake3::hash(&serde_json::to_vec(&brief).unwrap()).to_hex()
        )
    );

    // A real PDF without an outline or Info dictionary, the repo's report.pdf.
    let report = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/test_files/report.pdf");
    let result = pdf_result(&report, &[]);
    assert_eq!(result["page_count"], 1);
    assert_eq!(result["summary"]["outline_entries"], 0);
    assert_eq!(result["metadata"]["key_count"], 0);

    // Not a PDF (the repo's sample.pdf has an invalid header) and an empty file refuse.
    let sample = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/files/sample.pdf");
    let refused = peek_path(&sample, &[]);
    assert_eq!(refused.status.code(), Some(2));
    assert_eq!(
        parse_stdout(&refused)["refusal"]["message"],
        "Could not read pdf"
    );
    let empty = dir.path().join("empty.pdf");
    fs::write(&empty, "").unwrap();
    assert_eq!(peek_path(&empty, &[]).status.code(), Some(2));
}

// ---- bd-hra: workbook column type distributions, merged ranges, formula cells ------------------------

#[test]
fn workbook_peek_reports_merged_ranges_formula_cells_and_column_types() {
    // merged_formulas.xlsx (tests/fixtures/peek/make_peek_xlsx.py, synthetic): A1:C1 and A7:C7
    // merged (2 ranges, 6 cells), 4 formula cells (D4:D7; openpyxl stores no cached result, so
    // those cells' VALUES are empty), header on row 3, data rows 4-6, a boolean column E.
    let path = fixture("merged_formulas.xlsx");
    let result = parse_stdout(&peek_path(&path, &[]))["result"].clone();
    assert_eq!(
        result["sheet_flags"],
        serde_json::json!({"merged": {"ranges": 2, "cells": 6, "ranges_in_inspected_rows": 2}, "formula_cells": 4})
    );
    assert_eq!(result["summary"]["header_at_row"], 3);
    assert_eq!(result["summary"]["data_starts_at"], 4);
    let columns = result["columns"].as_array().unwrap();
    let kinds: Vec<(&str, &str)> = columns
        .iter()
        .map(|c| (c["column"].as_str().unwrap(), c["type"].as_str().unwrap()))
        .collect();
    assert_eq!(
        kinds,
        [
            ("A", "text"),
            ("B", "number"),
            ("C", "number"),
            ("D", "unknown"),
            ("E", "boolean")
        ]
    );
    // Rows 4-7 are inspected as data: B holds 3 numbers and 1 empty (the merged total row).
    assert_eq!(
        columns[1]["cell_type_counts"],
        serde_json::json!({"boolean": 0, "date": 0, "empty": 1, "mixed": 0, "numeric": 3, "text": 0})
    );
    assert_eq!(columns[1]["null_count"], 1);
    assert_eq!(columns[1]["null_rate"], 0.25);
    // The formula column has formulas but no cached values: all four cells read as empty.
    assert_eq!(columns[3]["dominant_class"], Value::Null);
    assert_eq!(columns[3]["null_count"], 4);
    // Booleans are classified by Excel type.
    assert_eq!(columns[4]["cell_type_counts"]["boolean"], 3);
}

#[test]
fn workbook_flags_are_zero_without_merges_or_formulas_and_types_follow_the_data() {
    // watl_msc.xlsx has neither merged cells nor formulas; its data window (rows 12 on)
    // includes the blank row 16 and the footnotes.
    let result =
        parse_stdout(&peek_path(&materialize_fixture("watl_msc.xlsx"), &[]))["result"].clone();
    assert_eq!(
        result["sheet_flags"],
        serde_json::json!({"merged": {"ranges": 0, "cells": 0, "ranges_in_inspected_rows": 0}, "formula_cells": 0})
    );
    let types: Vec<&str> = result["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["type"].as_str().unwrap())
        .collect();
    // Loan ID, Property Name, Status: text; Maturity Date: dates are text in inspect's words;
    // Balance and DSCR: numbers.
    assert_eq!(types, ["text", "text", "text", "text", "number", "number"]);
    assert_eq!(result["columns"][3]["dominant_class"], "date");
}

#[test]
fn workbook_formula_text_and_values_never_leak_and_brief_keeps_the_flags() {
    let path = fixture("merged_formulas.xlsx");
    let stdout = String::from_utf8(peek_path(&path, &["--explicit"]).stdout).unwrap();
    for planted in [
        "B4*C4",
        "SUM(",
        "Widget",
        "Gadget",
        "Gizmo",
        "Quarterly stock",
        "Item",
    ] {
        assert!(!stdout.contains(planted), "{planted} leaked: {stdout}");
    }
    let brief = peek_path(&path, &["--view", "brief"]);
    let compact = serde_json::to_vec(&parse_stdout(&brief)).unwrap();
    assert!(compact.len() < 2000, "brief is {} bytes", compact.len());
    let result = parse_stdout(&brief)["result"].clone();
    assert!(result.get("columns").is_none() && result.get("rows").is_none());
    assert_eq!(result["sheet_flags"]["formula_cells"], 4);
    assert_eq!(result["sheet_flags"]["merged"]["ranges"], 2);
}

// ---- bd-zwx: ordinal refs by default, opt-in keyed HMAC markers, no hash of any name ------------

const KEY: &str = "0123456789abcdef";
const OTHER_KEY: &str = "fedcba9876543210";
const KEY_ENV: &str = "FINGERPRINT_REDACTION_KEY";

/// `peek` with an optional redaction key and an optional witness ledger.
fn peek_keyed(path: &Path, extra: &[&str], key: Option<&str>, witness: Option<&Path>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fingerprint"));
    command
        .arg("peek")
        .arg(path)
        .args(extra)
        .env_remove(KEY_ENV);
    if let Some(key) = key {
        command.env(KEY_ENV, key);
    }
    match witness {
        Some(ledger) => command.env("EPISTEMIC_WITNESS", ledger),
        None => command.arg("--no-witness"),
    };
    command.output().expect("run fingerprint binary")
}

/// One file per peek format. The PDF is written into `dir` (it is built from planted canaries).
fn every_format(dir: &Path) -> Vec<(&'static str, PathBuf)> {
    let pdf = dir.join("canary.pdf");
    write_canary_pdf(&pdf);
    vec![
        ("csv", fixture("clean_flat.csv")),
        ("tsv", fixture("tsv.tsv")),
        ("xlsx", materialize_fixture("watl_msc.xlsx")),
        ("parquet", parquet_fixture("delivery.parquet")),
        ("json", json_fixture("delivery.json")),
        ("jsonl", json_fixture("loans.jsonl")),
        ("xml", xml_fixture("abs-ee/exh_103.xml")),
        ("html", fixture("canary.html")),
        ("pdf", pdf),
    ]
}

/// Every `redacted:hmac-sha256:<32 hex>` marker in `text`.
fn hmac_markers(text: &str) -> Vec<String> {
    const PREFIX: &str = "redacted:hmac-sha256:";
    text.match_indices(PREFIX)
        .map(|(at, _)| text[at..at + PREFIX.len() + 32].to_owned())
        .collect()
}

fn old_blake3_id(name: &str) -> String {
    blake3::hash(name.as_bytes()).to_hex()[..16].to_owned()
}

#[test]
fn ordinal_refs_follow_the_data_order_not_the_name_order() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ids =
        |file: &str, body: &str, explicit: bool, key: Option<&str>| -> Vec<(String, String)> {
            let path = dir.path().join(file);
            fs::write(&path, body).unwrap();
            let output = peek_keyed(
                &path,
                if explicit { &["--explicit"] } else { &[] },
                key,
                None,
            );
            assert_eq!(output.status.code(), Some(0), "{file}");
            let result = parse_stdout(&output)["result"].clone();
            let rows = result["fields"]
                .as_array()
                .or(result["paths"].as_array())
                .unwrap();
            rows.iter()
                .map(|r| {
                    (
                        r["path"].as_str().unwrap().to_owned(),
                        r["id"].as_str().unwrap().to_owned(),
                    )
                })
                .collect()
        };
    // JSONL: the same two names in both orders.
    let zebra_first = ids("a.jsonl", "{\"zebra\":1,\"apple\":\"x\"}\n", true, None);
    let apple_first = ids("b.jsonl", "{\"apple\":\"x\",\"zebra\":1}\n", true, None);
    assert_eq!(
        zebra_first,
        [
            ("$".into(), "redacted:ref:p1".into()),
            ("$.zebra".into(), "redacted:ref:p2".into()),
            ("$.apple".into(), "redacted:ref:p3".into()),
        ]
    );
    assert_eq!(
        apple_first,
        [
            ("$".into(), "redacted:ref:p1".into()),
            ("$.apple".into(), "redacted:ref:p2".into()),
            ("$.zebra".into(), "redacted:ref:p3".into()),
        ]
    );
    // XML: sibling elements in both orders (a name-sorted order would number apple first).
    let zebra_first = ids("a.xml", "<r><zebra/><apple/></r>", true, None);
    let apple_first = ids("b.xml", "<r><apple/><zebra/></r>", true, None);
    assert_eq!(
        zebra_first,
        [
            ("/r".into(), "redacted:ref:p1".into()),
            ("/r/zebra".into(), "redacted:ref:p2".into()),
            ("/r/apple".into(), "redacted:ref:p3".into()),
        ]
    );
    assert_eq!(
        apple_first[1],
        ("/r/apple".into(), "redacted:ref:p2".into())
    );
    // A `.json` document is walked in file order too (serde_json's Value would sort the keys).
    let nested = ids(
        "c.json",
        "{\"zebra\":{\"y\":1,\"b\":2},\"apple\":3}",
        true,
        None,
    );
    let order: Vec<&str> = nested.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(order, ["$", "$.zebra", "$.zebra.y", "$.zebra.b", "$.apple"]);
    // Keyed mode: a name's marker does not depend on its position (that is the correlation
    // feature), while the ordinal ref does.
    let keyed_a = ids(
        "d.jsonl",
        "{\"zebra\":1,\"apple\":\"x\"}\n",
        true,
        Some(KEY),
    );
    let keyed_b = ids(
        "e.jsonl",
        "{\"apple\":\"x\",\"zebra\":1}\n",
        true,
        Some(KEY),
    );
    let marker_of = |rows: &[(String, String)], path: &str| {
        rows.iter().find(|(p, _)| p == path).unwrap().1.clone()
    };
    assert_eq!(
        marker_of(&keyed_a, "$.zebra"),
        marker_of(&keyed_b, "$.zebra")
    );
    assert_eq!(
        marker_of(&keyed_a, "$.apple"),
        marker_of(&keyed_b, "$.apple")
    );
    assert!(marker_of(&keyed_a, "$.zebra").starts_with("redacted:hmac-sha256:"));

    // HTML: equal heading text in ONE output shares a ref; distinct text does not.
    let html = dir.path().join("h.html");
    fs::write(&html, "<h1>Same</h1><h2>Other</h2><h2>Same</h2>").unwrap();
    let headings = parse_stdout(&peek_keyed(&html, &[], None, None))["result"]["headings"].clone();
    let refs: Vec<&str> = headings
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        refs,
        ["redacted:ref:h1", "redacted:ref:h2", "redacted:ref:h1"]
    );
}

#[test]
fn default_and_keyed_output_are_byte_identical_across_runs_for_every_format() {
    let dir = tempfile::tempdir().expect("tempdir");
    for (format, path) in every_format(dir.path()) {
        for key in [None, Some(KEY)] {
            for extra in [&[][..], &["--view", "brief"][..], &["--explicit"][..]] {
                let one = peek_keyed(&path, extra, key, None);
                let two = peek_keyed(&path, extra, key, None);
                assert_eq!(one.status.code(), Some(0), "{format} {extra:?}");
                assert_eq!(
                    one.stdout,
                    two.stdout,
                    "{format} {extra:?} key={}",
                    key.is_some()
                );
            }
        }
    }
}

#[test]
fn the_old_unsalted_blake3_ids_match_nothing_in_any_format_mode_or_view() {
    // The dictionary a guesser would try: every name in these fixtures, each as a bare name and
    // as the path forms peek used to hash. bd-zwx's reproduction: blake3(loan_id)[..16].
    assert_eq!(old_blake3_id("loan_id"), "d3411460f2fd7fe9");
    let names = [
        "loan_id",
        "balance",
        "status",
        "tags",
        "region",
        "rate",
        "comments",
        "commentData",
        "comment",
        "itemNumber",
        "fieldName",
        "schema",
        "filings",
        "meta",
        "accession",
        "form",
        "Servicer Watch List",
        "Notes",
        "CanaryCustomKey",
        "CANARY-OUTLINE-A-11aa",
        "CANARY-OUTLINE-B-44dd",
        "CANARY-HEADING-ONE-a8e3",
        "Title",
        "Author",
        "Producer",
        "edgarSubmission",
        "assetData",
        "http://www.sec.gov/edgar/document/absee/cmbs/assetdata",
    ];
    let mut dictionary: Vec<String> = Vec::new();
    for name in names {
        for form in [
            name.to_owned(),
            format!("/{name}"),
            format!("$.{name}"),
            format!("$.{name}[]"),
            format!("/comments/{name}"),
            format!("/edgarSubmission/{name}"),
            format!("/assetData/{name}"),
        ] {
            dictionary.push(old_blake3_id(&form));
        }
    }
    dictionary.push(old_blake3_id("$"));
    dictionary.push(old_blake3_id("/comments/commentData"));
    let leaks = |text: &str| {
        dictionary
            .iter()
            .filter(|id| text.contains(id.as_str()))
            .count()
    };

    // The check is not vacuous: it fires on the old output shape.
    assert_eq!(leaks("\"id\": \"blake3:d3411460f2fd7fe9\""), 1);

    let dir = tempfile::tempdir().expect("tempdir");
    for (format, path) in every_format(dir.path()) {
        for key in [None, Some(KEY)] {
            for extra in [&[][..], &["--view", "brief"][..], &["--explicit"][..]] {
                let output = peek_keyed(&path, extra, key, None);
                let text = String::from_utf8(output.stdout).unwrap();
                assert_eq!(
                    leaks(&text),
                    0,
                    "{format} {extra:?} key={}: {text}",
                    key.is_some()
                );
                assert!(!text.contains("\"blake3:"), "{format} {extra:?}: {text}");
            }
        }
    }
}

#[test]
fn keyed_markers_equal_independent_openssl_hmac_known_answers() {
    // Each value is `printf '<kind>\0<name>' | openssl dgst -sha256 -hmac 0123456789abcdef`,
    // first 32 hex (OpenSSL 4.1.0), computed outside this crate's HMAC code.
    let marker = |hex: &str| format!("redacted:hmac-sha256:{hex}");
    let dir = tempfile::tempdir().expect("tempdir");
    let formats = every_format(dir.path());
    let path_of = |name: &str| formats.iter().find(|(f, _)| *f == name).unwrap().1.clone();

    // column loan_id
    let parquet =
        parse_stdout(&peek_keyed(&path_of("parquet"), &[], Some(KEY), None))["result"].clone();
    assert_eq!(parquet["redaction"], "keyed");
    assert_eq!(
        parquet["columns"][0]["id"],
        marker("9f6181c104894742edc83d76f18dc14f")
    );
    // path $.loan_id
    let jsonl = parse_stdout(&peek_keyed(
        &path_of("jsonl"),
        &["--explicit"],
        Some(KEY),
        None,
    ))["result"]
        .clone();
    let loan_id = jsonl["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["path"] == "$.loan_id")
        .unwrap();
    assert_eq!(loan_id["id"], marker("e39f8a955a1092ec102a926ece74a431"));
    // path /comments/commentData
    let xml = parse_stdout(&peek_keyed(&path_of("xml"), &[], Some(KEY), None))["result"].clone();
    assert_eq!(
        xml["summary"]["record_candidates"][0]["id"],
        marker("f7a33bc3c68be8cd35f00e8252d5c119")
    );
    // sheet Servicer Watch List
    let sheet = parse_stdout(&peek_keyed(&path_of("xlsx"), &[], Some(KEY), None))["result"].clone();
    assert_eq!(
        sheet["sheet"]["id"],
        marker("56c5353fd9566f00edd608ee7b3999da")
    );
    // metadata_key CanaryCustomKey and outline CANARY-OUTLINE-A-11aa
    let pdf = parse_stdout(&peek_keyed(&path_of("pdf"), &[], Some(KEY), None))["result"].clone();
    assert_eq!(
        pdf["metadata"]["other_key_ids"][0],
        marker("990614e7b6736b5c114e212a880e24c5")
    );
    assert_eq!(
        pdf["outline"][0]["id"],
        marker("235cdcfc3002476758764e968930d8e4")
    );

    // Domain separation: the same spelling under two kinds is two markers (column loan_id above
    // is 9f61..., path $.loan_id is e39f...; and a column and a path "loan_id" differ too).
    assert_ne!(
        parquet["columns"][0]["id"], loan_id["id"],
        "kinds are separate HMAC domains"
    );
}

#[test]
fn a_different_key_changes_every_marker_and_the_same_key_is_stable_across_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    for (format, path) in every_format(dir.path()) {
        if matches!(format, "csv" | "tsv") {
            continue; // no name ids to mark
        }
        let first = String::from_utf8(peek_keyed(&path, &[], Some(KEY), None).stdout).unwrap();
        let second =
            String::from_utf8(peek_keyed(&path, &[], Some(OTHER_KEY), None).stdout).unwrap();
        let (one, two) = (hmac_markers(&first), hmac_markers(&second));
        assert!(!one.is_empty(), "{format} has no keyed markers: {first}");
        assert_eq!(one.len(), two.len(), "{format}");
        for (a, b) in one.iter().zip(&two) {
            assert_ne!(a, b, "{format}: a different key must change every marker");
        }
    }
    // Across FILES with one key: `$.loan_id` has the same marker in two different JSONL files.
    let loan_id_marker = |file: &str| {
        let result = parse_stdout(&peek_keyed(
            &json_fixture(file),
            &["--explicit"],
            Some(KEY),
            None,
        ))["result"]
            .clone();
        result["fields"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["path"] == "$.loan_id")
            .unwrap()["id"]
            .clone()
    };
    assert_eq!(
        loan_id_marker("loans.jsonl"),
        loan_id_marker("loans_bad.jsonl")
    );
    // ...while the default ordinal refs are only comparable inside one output.
}

#[test]
fn an_empty_or_short_key_refuses_with_a_typed_code_and_exit_2_for_every_format() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ledger = dir.path().join("witness.jsonl");
    for (format, path) in every_format(dir.path()) {
        for (bad, reason) in [
            ("", "empty"),
            ("SHORT-KEY-xyz", "too_short"),
            ("0123456789abcde", "too_short"),
        ] {
            let output = peek_keyed(&path, &["--explicit"], Some(bad), Some(&ledger));
            assert_eq!(output.status.code(), Some(2), "{format} {bad:?}");
            let stdout = String::from_utf8(output.stdout.clone()).unwrap();
            let stderr = String::from_utf8(output.stderr.clone()).unwrap();
            let envelope = parse_stdout(&output);
            assert_eq!(envelope["outcome"], "REFUSAL", "{format}");
            assert_eq!(
                envelope["refusal"]["code"], "E_REDACTION_KEY_INVALID",
                "{format}"
            );
            // The context is exactly the variable name, a reason and the minimum: no key, no length.
            assert_eq!(
                envelope["refusal"]["detail"],
                serde_json::json!({"env": KEY_ENV, "reason": reason, "min_bytes": 16}),
                "{format} {bad:?}"
            );
            assert!(
                envelope.get("result").is_none(),
                "{format}: no silent fallback result"
            );
            for text in [&stdout, &stderr] {
                assert!(
                    bad.is_empty() || !text.contains(bad),
                    "{format}: key leaked: {text}"
                );
            }
        }
    }
    // A refusal never writes a witness record.
    assert!(!ledger.exists() || fs::read_to_string(&ledger).unwrap().is_empty());
    // A 16-byte key is accepted (the minimum), an unset variable is ordinal mode.
    let ok = peek_keyed(
        &fixture("clean_flat.csv"),
        &[],
        Some("0123456789abcdef"),
        None,
    );
    assert_eq!(ok.status.code(), Some(0));
}

#[test]
fn the_key_never_appears_in_stdout_stderr_the_witness_ledger_or_doctor() {
    let dir = tempfile::tempdir().expect("tempdir");
    let secret = "SECRET-KEY-MATERIAL-9f3a1c";
    for (format, path) in every_format(dir.path()) {
        let ledger = dir.path().join(format!("witness-{format}.jsonl"));
        for extra in [&[][..], &["--view", "brief"][..], &["--explicit"][..]] {
            let output = peek_keyed(&path, extra, Some(secret), Some(&ledger));
            assert_eq!(output.status.code(), Some(0), "{format} {extra:?}");
            for (what, text) in [
                ("stdout", String::from_utf8(output.stdout).unwrap()),
                ("stderr", String::from_utf8(output.stderr).unwrap()),
            ] {
                assert!(!text.contains(secret), "{format} {what}: {text}");
            }
        }
        let recorded = fs::read_to_string(&ledger).expect("witness ledger");
        assert!(!recorded.is_empty(), "{format}: a record was written");
        assert!(!recorded.contains(secret), "{format}: key in the ledger");
    }
    // Doctor reports only `configured`, never the key, its length or a hash of it.
    for (key, expected_severity, expected_message) in [
        (Some(secret), "info", "configured"),
        (None, "info", "not configured"),
        (Some("tiny"), "error", "unusable"),
    ] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_fingerprint"));
        command.args(["doctor", "--json"]).env_remove(KEY_ENV);
        if let Some(key) = key {
            command.env(KEY_ENV, key);
        }
        let output = command.output().unwrap();
        let text = String::from_utf8(output.stdout).unwrap();
        if let Some(key) = key {
            assert!(!text.contains(key), "doctor leaked the key: {text}");
        }
        let report: Value = serde_json::from_str(&text).unwrap();
        let finding = report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["id"] == "fingerprint.redaction-key")
            .unwrap_or_else(|| panic!("no redaction-key finding: {text}"));
        assert_eq!(finding["severity"], expected_severity, "{key:?}");
        assert!(
            finding["message"]
                .as_str()
                .unwrap()
                .contains(expected_message),
            "{key:?}: {finding}"
        );
        assert_eq!(
            output.status.code(),
            Some(if expected_severity == "error" { 1 } else { 0 }),
            "{key:?}"
        );
    }
}

#[test]
fn delimited_text_prints_no_names_or_ids_and_explicit_still_shows_names() {
    // CSV/TSV peek is row-shape only: the header's names are never printed, with or without a
    // key (there is nothing to mark).
    for (file, header_names) in [
        (
            "clean_flat.csv",
            vec!["loan_id", "region", "balance", "rate"],
        ),
        ("tsv.tsv", vec![]),
    ] {
        for key in [None, Some(KEY)] {
            let text = String::from_utf8(
                peek_keyed(&fixture(file), &["--view", "brief"], key, None).stdout,
            )
            .unwrap();
            for name in &header_names {
                assert!(!text.contains(name), "{file}: {name} printed: {text}");
            }
            assert!(!text.contains(&old_blake3_id("loan_id")), "{file}");
        }
    }
    // --explicit keeps showing names; in keyed mode the ids are markers beside the names.
    let dir = tempfile::tempdir().expect("tempdir");
    let formats = every_format(dir.path());
    let path_of = |name: &str| formats.iter().find(|(f, _)| *f == name).unwrap().1.clone();
    let parquet =
        String::from_utf8(peek_keyed(&path_of("parquet"), &["--explicit"], Some(KEY), None).stdout)
            .unwrap();
    assert!(parquet.contains("\"name\": \"loan_id\"") && parquet.contains("redacted:hmac-sha256:"));
    let xlsx = String::from_utf8(peek_keyed(&path_of("xlsx"), &["--explicit"], None, None).stdout)
        .unwrap();
    assert!(xlsx.contains("Servicer Watch List") && xlsx.contains("redacted:ref:s1"));
}
