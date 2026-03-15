use fingerprint::document::{Document, HtmlDocument};
use fingerprint::dsl::content_hash::content_hash;
use fingerprint::dsl::extract::extract;
use fingerprint::dsl::parser::ExtractSection;
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

#[derive(Debug, Deserialize)]
struct HtmlFixtureInventory {
    schema_version: String,
    fixtures: Vec<HtmlFixtureEntry>,
    hash_pairs: Vec<HashPairEntry>,
}

#[derive(Debug, Deserialize)]
struct HtmlFixtureEntry {
    id: String,
    path: String,
    categories: Vec<String>,
    family: Option<String>,
    expected_headings: usize,
    expected_tables: usize,
    expected_pages: usize,
}

#[derive(Debug, Deserialize)]
struct HashPairEntry {
    base: String,
    variant: String,
    expectation: String,
}

fn load_inventory() -> HtmlFixtureInventory {
    let inventory_path = repo_path("tests/fixtures/html/inventory.json");
    let contents = fs::read_to_string(&inventory_path).expect("read html fixture inventory");
    serde_json::from_str(&contents).expect("parse html fixture inventory")
}

fn load_html_manifest_paths() -> BTreeSet<String> {
    let manifest_path = repo_path("tests/fixtures/manifests/html_corpus.jsonl");
    fs::read_to_string(manifest_path)
        .expect("read html fixture manifest")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str::<Value>(line).expect("parse html fixture manifest line"))
        .map(|record| {
            record["path"]
                .as_str()
                .expect("html manifest path")
                .to_owned()
        })
        .collect()
}

fn inventory_by_id(inventory: &HtmlFixtureInventory) -> HashMap<&str, &HtmlFixtureEntry> {
    inventory
        .fixtures
        .iter()
        .map(|fixture| (fixture.id.as_str(), fixture))
        .collect()
}

fn hash_extract_sections() -> Vec<ExtractSection> {
    vec![
        ExtractSection {
            name: "rent_roll".to_owned(),
            r#type: "table".to_owned(),
            anchor_heading: Some("(?i)rent roll".to_owned()),
            index: Some(0),
            anchor: None,
            pattern: None,
            within_chars: None,
            sheet: None,
            range: None,
        },
        ExtractSection {
            name: "income_cap".to_owned(),
            r#type: "section".to_owned(),
            anchor_heading: Some("(?i)income capitali[sz]ation".to_owned()),
            index: None,
            anchor: None,
            pattern: None,
            within_chars: None,
            sheet: None,
            range: None,
        },
        ExtractSection {
            name: "cap_rate".to_owned(),
            r#type: "text_match".to_owned(),
            anchor_heading: None,
            index: None,
            anchor: Some("(?i)cap rate".to_owned()),
            pattern: Some(r"\d+\.\d+%".to_owned()),
            within_chars: Some(12),
            sheet: None,
            range: None,
        },
    ]
}

fn extracted_hash_payload(fixture: &HtmlFixtureEntry) -> HashMap<String, Value> {
    let path = repo_path(&fixture.path);
    let document = Document::Html(HtmlDocument::open(&path).expect("open html fixture"));
    extract(&document, &hash_extract_sections()).expect("extract hash payload from html fixture")
}

#[test]
fn html_fixture_inventory_is_consistent() {
    let inventory = load_inventory();
    assert_eq!(
        inventory.schema_version, "fingerprint.html-fixtures.v1",
        "fixture inventory schema version should stay explicit"
    );

    let mut ids = BTreeSet::new();
    let mut families = BTreeSet::new();
    let mut ambiguity_count = 0usize;

    for fixture in &inventory.fixtures {
        assert!(
            ids.insert(fixture.id.clone()),
            "duplicate fixture id '{}'",
            fixture.id
        );
        assert!(
            repo_path(&fixture.path).is_file(),
            "fixture path '{}' must exist",
            fixture.path
        );
        assert!(
            fixture.path.ends_with(".html"),
            "fixture path '{}' must be html",
            fixture.path
        );
        if fixture
            .categories
            .iter()
            .any(|category| category == "ambiguity-trap")
        {
            ambiguity_count += 1;
        }
        if let Some(family) = &fixture.family {
            families.insert(family.clone());
        }
    }

    assert_eq!(
        families.len(),
        5,
        "inventory should retain one representative fixture for each known BDC family"
    );
    assert_eq!(
        ambiguity_count, 1,
        "inventory should retain exactly one ambiguity-trap fixture"
    );
    assert_eq!(
        inventory.hash_pairs.len(),
        2,
        "inventory should retain the canonical stable/change hash mutation pairs"
    );

    let manifest_paths = load_html_manifest_paths();
    let inventory_paths: BTreeSet<String> = inventory
        .fixtures
        .iter()
        .map(|fixture| fixture.path.clone())
        .collect();
    assert_eq!(
        manifest_paths, inventory_paths,
        "html manifest should reference every committed html fixture exactly once"
    );

    let html_readme =
        fs::read_to_string(repo_path("tests/fixtures/html/README.md")).expect("read html README");
    for fixture in &inventory.fixtures {
        let filename = Path::new(&fixture.path)
            .file_name()
            .and_then(|value| value.to_str())
            .expect("fixture filename");
        assert!(
            html_readme.contains(filename),
            "html README must mention fixture '{}'",
            filename
        );
    }
}

#[test]
fn html_fixture_corpus_loads_with_expected_shape() {
    let inventory = load_inventory();

    for fixture in &inventory.fixtures {
        let document = HtmlDocument::open(&repo_path(&fixture.path))
            .expect("open html fixture from inventory");
        let page_count = document
            .sections
            .iter()
            .filter_map(|section| section.page)
            .collect::<BTreeSet<_>>()
            .len();

        assert_eq!(
            document.headings.len(),
            fixture.expected_headings,
            "heading count drifted for fixture '{}'",
            fixture.id
        );
        assert_eq!(
            document.tables.len(),
            fixture.expected_tables,
            "table count drifted for fixture '{}'",
            fixture.id
        );
        assert_eq!(
            page_count, fixture.expected_pages,
            "page count drifted for fixture '{}'",
            fixture.id
        );
    }
}

#[test]
fn html_fixture_hash_pairs_capture_stable_and_changed_content() {
    let inventory = load_inventory();
    let fixtures = inventory_by_id(&inventory);
    let over = vec![
        "rent_roll".to_owned(),
        "income_cap".to_owned(),
        "cap_rate".to_owned(),
    ];

    for pair in &inventory.hash_pairs {
        let base_fixture = fixtures
            .get(pair.base.as_str())
            .copied()
            .expect("base fixture in inventory");
        let variant_fixture = fixtures
            .get(pair.variant.as_str())
            .copied()
            .expect("variant fixture in inventory");

        let base_payload = extracted_hash_payload(base_fixture);
        let variant_payload = extracted_hash_payload(variant_fixture);
        let base_hash = content_hash(&base_payload, &over);
        let variant_hash = content_hash(&variant_payload, &over);

        match pair.expectation.as_str() {
            "stable" => {
                assert_eq!(
                    base_payload, variant_payload,
                    "stable hash pair '{}' should keep extracted payloads identical",
                    pair.variant
                );
                assert_eq!(
                    base_hash, variant_hash,
                    "stable hash pair '{}' should keep content hashes identical",
                    pair.variant
                );
            }
            "changed" => {
                assert_ne!(
                    base_payload, variant_payload,
                    "changed hash pair '{}' should alter extracted payloads",
                    pair.variant
                );
                assert_ne!(
                    base_hash, variant_hash,
                    "changed hash pair '{}' should alter content hashes",
                    pair.variant
                );
            }
            other => panic!("unexpected hash-pair expectation '{other}'"),
        }
    }
}
