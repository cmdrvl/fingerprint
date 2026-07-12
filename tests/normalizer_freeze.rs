use fingerprint::document::html::HtmlDocument;
use fingerprint::document::markdown::MarkdownDocument;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

#[derive(Debug, Deserialize)]
struct HtmlFixtureInventory {
    fixtures: Vec<HtmlFixture>,
}

#[derive(Debug, Deserialize)]
struct HtmlFixture {
    id: String,
    path: PathBuf,
    expected_headings: usize,
    expected_tables: usize,
    expected_pages: usize,
}

#[derive(Debug, PartialEq, Eq)]
struct OwnedHtmlSnapshot {
    id: String,
    normalized_hash: String,
    headings: Vec<String>,
    section_ranges: Vec<String>,
    table_count: usize,
    page_sections: usize,
}

struct ExpectedHtmlSnapshot {
    id: &'static str,
    normalized_hash: &'static str,
    headings: &'static [&'static str],
    section_ranges: &'static [&'static str],
    table_count: usize,
    page_sections: usize,
}

const EXPECTED_HTML_SNAPSHOTS: &[ExpectedHtmlSnapshot] = &[
    ExpectedHtmlSnapshot {
        id: "ambiguity_trap_dual_headers",
        normalized_hash: "88463a35d1c03750609b73c4593803517c8d5af885e820b747ecfe1cc6450de6",
        headings: &["h1:Schedule of Investments@1", "h2:Supplemental Holdings@6"],
        section_ranges: &[
            "Schedule of Investments:1-9:None",
            "Supplemental Holdings:6-9:None",
        ],
        table_count: 2,
        page_sections: 0,
    },
    ExpectedHtmlSnapshot {
        id: "bdc_soi_ares_like",
        normalized_hash: "f6a6de8c93b5152c6b4367e8eadd00da9c6b091625c5631dcc8da8ef0e19e442",
        headings: &[
            "h1:Schedule of Investments@1",
            "h2:Portfolio Companies@9",
            "h2:Schedule Continuation@17",
        ],
        section_ranges: &[
            "Schedule of Investments:1-19:Some(1)",
            "Portfolio Companies:9-16:Some(2)",
            "Schedule Continuation:17-19:Some(3)",
        ],
        table_count: 2,
        page_sections: 3,
    },
    ExpectedHtmlSnapshot {
        id: "bdc_soi_blackrock_like",
        normalized_hash: "439a96d78bde2588485d711dd5cace905de7893ea47519c07ed9a919ecc28e4e",
        headings: &[
            "h1:Schedule of Investments@1",
            "h2:Credit Portfolio@3",
            "h2:Additional Credit Holdings@8",
        ],
        section_ranges: &[
            "Schedule of Investments:1-11:Some(1)",
            "Credit Portfolio:3-7:Some(1)",
            "Additional Credit Holdings:8-11:Some(2)",
        ],
        table_count: 2,
        page_sections: 2,
    },
    ExpectedHtmlSnapshot {
        id: "bdc_soi_bxsl_like",
        normalized_hash: "b49278af4c7ea22ad5fd1adf72cb03216e38091969ddb64b0016cab89be10723",
        headings: &[
            "h1:Schedule of Investments@1",
            "h2:First Lien Investments@3",
            "h2:Equity Investments@8",
        ],
        section_ranges: &[
            "Schedule of Investments:1-11:Some(1)",
            "First Lien Investments:3-7:Some(1)",
            "Equity Investments:8-11:Some(2)",
        ],
        table_count: 2,
        page_sections: 2,
    },
    ExpectedHtmlSnapshot {
        id: "bdc_soi_golub_like",
        normalized_hash: "f63478a7e58d790af7b4e7f20719f9074c0a38b0aab25819302f248530997422",
        headings: &[
            "h1:Schedule of Investments@1",
            "h2:Portfolio Details@3",
            "h2:Additional Portfolio Details@9",
        ],
        section_ranges: &[
            "Schedule of Investments:1-13:Some(1)",
            "Portfolio Details:3-8:Some(1)",
            "Additional Portfolio Details:9-13:Some(2)",
        ],
        table_count: 2,
        page_sections: 2,
    },
    ExpectedHtmlSnapshot {
        id: "bdc_soi_pennant_like",
        normalized_hash: "15c612eb46eb3475fab037e9cf75744c286ca94ed33fbbc2d79d9d2e54c7d387",
        headings: &[
            "h1:Schedule of Investments@1",
            "h2:Asset Classes@3",
            "h2:Additional Asset Classes@9",
        ],
        section_ranges: &[
            "Schedule of Investments:1-13:Some(1)",
            "Asset Classes:3-8:Some(1)",
            "Additional Asset Classes:9-13:Some(2)",
        ],
        table_count: 2,
        page_sections: 2,
    },
    ExpectedHtmlSnapshot {
        id: "generic_page_sections_schedule",
        normalized_hash: "0a29e7756536f8bacd8386ef800b1047fae146fa81eb31b827f7d99ec7b9ebef",
        headings: &[
            "h1:Schedule of Investments@1",
            "h2:Portfolio Summary@8",
            "h2:Notes@15",
        ],
        section_ranges: &[
            "Schedule of Investments:1-17:Some(1)",
            "Portfolio Summary:8-14:Some(2)",
            "Notes:15-17:Some(3)",
        ],
        table_count: 2,
        page_sections: 3,
    },
    ExpectedHtmlSnapshot {
        id: "hash_pair_base",
        normalized_hash: "972526db01d185636d0c65ccd0924215370e9d18610f278a13d8d573c06c5420",
        headings: &["h1:Rent Roll@1", "h2:Income Capitalization@6"],
        section_ranges: &["Rent Roll:1-8:None", "Income Capitalization:6-8:None"],
        table_count: 1,
        page_sections: 0,
    },
    ExpectedHtmlSnapshot {
        id: "hash_pair_markup_variant",
        normalized_hash: "972526db01d185636d0c65ccd0924215370e9d18610f278a13d8d573c06c5420",
        headings: &["h1:Rent Roll@1", "h2:Income Capitalization@6"],
        section_ranges: &["Rent Roll:1-8:None", "Income Capitalization:6-8:None"],
        table_count: 1,
        page_sections: 0,
    },
    ExpectedHtmlSnapshot {
        id: "hash_pair_value_change",
        normalized_hash: "72c973d8af096978c1cae00a4dcae8ae2281dbc305aa4eeee36ed437289303ef",
        headings: &["h1:Rent Roll@1", "h2:Income Capitalization@6"],
        section_ranges: &["Rent Roll:1-8:None", "Income Capitalization:6-8:None"],
        table_count: 1,
        page_sections: 0,
    },
    ExpectedHtmlSnapshot {
        id: "malformed_static_schedule",
        normalized_hash: "8ee8159abc848682daa4b24b3bd9ddfaead982fefe75466b19ca1eec9c08c540",
        headings: &["h1:Broken Schedule@1", "h2:Residual Notes@8"],
        section_ranges: &[
            "Broken Schedule:1-10:Some(1)",
            "Residual Notes:8-10:Some(2)",
        ],
        table_count: 1,
        page_sections: 2,
    },
    ExpectedHtmlSnapshot {
        id: "minimal_empty_shell",
        normalized_hash: "b1bbef892b749d6ed88ffb5f51726614d2b5782221d6d01d484b8ca5423465b7",
        headings: &[],
        section_ranges: &["<preamble>:1-1:None"],
        table_count: 0,
        page_sections: 0,
    },
    ExpectedHtmlSnapshot {
        id: "oxsq_pagebreaks",
        normalized_hash: "5a5bb0d5b6278d5224882e3377ecfc3e0e12c7d776171586ec0cb5a0d48071dc",
        headings: &[],
        section_ranges: &["<preamble>:1-1:None"],
        table_count: 0,
        page_sections: 0,
    },
    ExpectedHtmlSnapshot {
        id: "span_edge_cases",
        normalized_hash: "43c9f4a7ea73a566fba85f9b1e8707d08c9ffef4fb736fe91ffdbc29eb49a5d8",
        headings: &["h1:Schedule Layout@1"],
        section_ranges: &["Schedule Layout:1-6:None"],
        table_count: 1,
        page_sections: 0,
    },
    ExpectedHtmlSnapshot {
        id: "styled_heading_ares",
        normalized_hash: "1696cf6dc16f47a4412f2333f381c610cc92694e738cf5525a5bf84977dc4633",
        headings: &[],
        section_ranges: &["<preamble>:1-6:None"],
        table_count: 1,
        page_sections: 0,
    },
];

fn fixture(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

fn open_html_fixture(path: &str) -> HtmlDocument {
    HtmlDocument::open(&fixture(path)).expect("open html fixture")
}

fn load_inventory() -> HtmlFixtureInventory {
    let inventory_path = fixture("tests/fixtures/html/inventory.json");
    let inventory = fs::read_to_string(&inventory_path).expect("read html fixture inventory");
    serde_json::from_str(&inventory).expect("parse html fixture inventory")
}

fn snapshot_fixture(html_fixture: &HtmlFixture) -> OwnedHtmlSnapshot {
    let doc = HtmlDocument::open(&fixture(&html_fixture.path.to_string_lossy()))
        .expect("open html fixture from inventory");

    assert_eq!(
        doc.headings.len(),
        html_fixture.expected_headings,
        "{} expected heading count drifted",
        html_fixture.id
    );
    assert_eq!(
        doc.tables.len(),
        html_fixture.expected_tables,
        "{} expected table count drifted",
        html_fixture.id
    );
    assert_eq!(
        doc.page_sections, html_fixture.expected_pages,
        "{} expected page section count drifted",
        html_fixture.id
    );

    OwnedHtmlSnapshot {
        id: html_fixture.id.clone(),
        normalized_hash: blake3::hash(doc.normalized.as_bytes()).to_hex().to_string(),
        headings: doc
            .headings
            .iter()
            .map(|heading| format!("h{}:{}@{}", heading.level, heading.text, heading.line))
            .collect(),
        section_ranges: doc
            .sections
            .iter()
            .map(|section| {
                let heading = section
                    .heading
                    .as_ref()
                    .map(|heading| heading.text.as_str())
                    .unwrap_or("<preamble>");
                format!(
                    "{}:{}-{}:{:?}",
                    heading, section.start_line, section.end_line, section.page
                )
            })
            .collect(),
        table_count: doc.tables.len(),
        page_sections: doc.page_sections,
    }
}

fn expected_snapshots() -> Vec<OwnedHtmlSnapshot> {
    EXPECTED_HTML_SNAPSHOTS
        .iter()
        .map(|snapshot| OwnedHtmlSnapshot {
            id: snapshot.id.to_owned(),
            normalized_hash: snapshot.normalized_hash.to_owned(),
            headings: snapshot
                .headings
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
            section_ranges: snapshot
                .section_ranges
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
            table_count: snapshot.table_count,
            page_sections: snapshot.page_sections,
        })
        .collect()
}

#[test]
fn frz_u01_styled_heading_fixture_does_not_promote_visual_heading() {
    let doc = open_html_fixture("tests/fixtures/html/styled_heading_ares.html");

    eprintln!("styled_heading_ares headings: {:?}", doc.headings);
    assert!(
        doc.headings.is_empty(),
        "styled visual title must remain invisible to the frozen heading model"
    );
    assert!(
        doc.normalized
            .contains("CONSOLIDATED SCHEDULE OF INVESTMENTS")
    );
}

#[test]
fn frz_u02_html_fixture_normalizer_snapshot_is_stable() {
    let inventory = load_inventory();
    let actual: Vec<_> = inventory.fixtures.iter().map(snapshot_fixture).collect();
    let expected = expected_snapshots();

    assert_eq!(
        actual, expected,
        "HTML normalizer snapshot changed; actual snapshots:\n{actual:#?}"
    );
}

#[test]
fn frz_u03_markdown_bold_as_heading_pass_is_grandfathered() {
    let file = NamedTempFile::with_suffix(".md").expect("create markdown fixture");
    fs::write(file.path(), "\n**Bold Heading**\n\nBody\n").expect("write markdown fixture");

    let doc = MarkdownDocument::open(file.path()).expect("open markdown fixture");

    assert!(doc.normalized.contains("## Bold Heading"));
    assert_eq!(doc.headings.len(), 1);
    assert_eq!(doc.headings[0].level, 2);
    assert_eq!(doc.headings[0].text, "Bold Heading");
}

#[test]
fn frz_u04_page_break_styles_do_not_create_global_page_sections() {
    let doc = open_html_fixture("tests/fixtures/html/oxsq_pagebreaks.html");

    assert_eq!(
        doc.page_sections, 0,
        "page-break CSS must not synthesize global page sections"
    );
    assert!(
        doc.headings.is_empty(),
        "page-break-only fixture must not synthesize headings"
    );
}
