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
    for name in ["doc.pdf", "archive.zip"] {
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
    assert_eq!(
        result["sheet"],
        serde_json::json!({"index": 0, "name": "Servicer Watch List"})
    );
    assert_eq!(
        result["sheets"],
        serde_json::json!([
            {"index": 0, "name": "Servicer Watch List", "rows": 20, "columns": 6, "first_row": 1, "first_column": 1},
            {"index": 1, "name": "Notes", "rows": 1, "columns": 1, "first_row": 1, "first_column": 1}
        ])
    );
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
        assert_eq!(result["sheet"]["name"], "Notes", "{selector}");
        assert_eq!(result["rows_observed"], 1);
    }
    for selector in ["Missing", "7"] {
        let output = peek_path(&path, &["--sheet", selector]);
        assert_eq!(output.status.code(), Some(2), "{selector}");
        let envelope = parse_stdout(&output);
        assert_eq!(envelope["refusal"]["detail"]["reason"], "sheet_not_found");
        assert_eq!(
            envelope["refusal"]["detail"]["sheets"],
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
    // Columns are identified by a short hash of the name; no name by default.
    assert_eq!(
        columns[0]["id"],
        format!("blake3:{}", &blake3::hash(b"loan_id").to_hex()[..16])
    );
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

fn field<'a>(result: &'a Value, path_hash_of: &str) -> &'a Value {
    let id = format!(
        "blake3:{}",
        &blake3::hash(path_hash_of.as_bytes()).to_hex()[..16]
    );
    result["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["id"] == id.as_str())
        .unwrap_or_else(|| panic!("no field {path_hash_of}"))
}

#[test]
fn jsonl_peek_reports_presence_rates_and_types_per_field_path() {
    // loans.jsonl: 5 records {loan_id, balance, status, tags[]}; balance is 3 integers and 2
    // non-integer numbers; 3 of 5 records have a non-empty tags list (4 string elements).
    let output = peek_path(&json_fixture("loans.jsonl"), &[]);
    assert_eq!(output.status.code(), Some(0));
    let result = parse_stdout(&output)["result"].clone();
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
    assert!(result["fields"][1].get("path").is_none());
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
    // Document order, with object keys sorted (serde_json's map order): deterministic.
    assert_eq!(
        paths,
        [
            "$",
            "$.filings",
            "$.filings[]",
            "$.filings[].accession",
            "$.filings[].form",
            "$.filings[].pages",
            "$.meta",
            "$.meta.complete",
            "$.meta.count",
            "$.meta.generated",
            "$.schema"
        ]
    );
    // `filings` holds 2 objects: element paths count every element, presence counts records.
    let id = |p: &str| format!("blake3:{}", &blake3::hash(p.as_bytes()).to_hex()[..16]);
    let elements = result["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["id"] == id("$.filings[]").as_str())
        .unwrap();
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
