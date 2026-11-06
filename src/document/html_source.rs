//! Source start-tag locations for opt-in selector-region bounds.
//! The ordinary DOM and frozen normalizer are never modified.
use super::html::HtmlDocument;
use ego_tree::NodeId;
use lol_html::{RewriteStrSettings, element, rewrite_str};
use regex::Regex;
use scraper::{Html, Selector};
use std::collections::HashMap;
use std::ops::Range;

const MARKER: &str = "data-cmdrvl-source-offset";

pub(crate) type SourceLocations = HashMap<NodeId, Range<usize>>;

/// Return only uniquely mapped explicit start tags, or no mappings if equivalence
/// cannot be established. Clone reconstruction and implicit tags are not evidence
/// of a unique source node. Ranges describe opening tags, not whole elements.
pub(crate) fn source_locations(html: &HtmlDocument) -> Option<SourceLocations> {
    if !html.source_is_utf8 || html.raw.contains(MARKER) {
        return None;
    }
    let mut locations = Vec::new();
    let annotated = rewrite_str(
        &html.raw,
        RewriteStrSettings::new().append_element_content_handler(element!("*", |element| {
            let ordinal = locations.len();
            locations.push(element.source_location().bytes());
            element.set_attribute(MARKER, &ordinal.to_string())?;
            Ok(())
        })),
    )
    .ok()?;
    let mapped = Html::parse_document(&annotated);
    let marker_attribute = Regex::new(r#" data-cmdrvl-source-offset="[0-9]+""#).ok()?;
    if html.dom.html() != marker_attribute.replace_all(&mapped.html(), "") {
        return None;
    }
    let selector = Selector::parse("*").ok()?;
    let original = html.dom.select(&selector).collect::<Vec<_>>();
    let mapped = mapped.select(&selector).collect::<Vec<_>>();
    if original.len() != mapped.len() {
        return None;
    }
    let mut counts = HashMap::new();
    let mut candidates = Vec::new();
    for (original, mapped) in original.into_iter().zip(mapped) {
        let Some(ordinal) = mapped.value().attr(MARKER) else {
            continue;
        };
        let ordinal = ordinal.parse::<usize>().ok()?;
        let span = locations.get(ordinal)?;
        let tag = html.raw.get(span.clone())?;
        if !tag.starts_with('<') || !tag.ends_with('>') {
            return None;
        }
        *counts.entry(ordinal).or_insert(0usize) += 1;
        candidates.push((original.id(), ordinal));
    }
    Some(
        candidates
            .into_iter()
            .filter(|(_, ordinal)| counts[ordinal] == 1)
            .map(|(id, ordinal)| (id, locations[ordinal].clone()))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn open(raw: &str) -> HtmlDocument {
        let mut file = tempfile::NamedTempFile::with_suffix(".html").unwrap();
        file.write_all(raw.as_bytes()).unwrap();
        HtmlDocument::open(file.path()).unwrap()
    }

    #[test]
    fn implicit_nodes_have_no_source_mapping() {
        let html = open("<table><tr><td>x</td></tr></table>");
        let map = source_locations(&html).unwrap();
        let tbody = html.select_nodes("tbody").unwrap()[0];
        assert!(!map.contains_key(&tbody.id()));
        let table = html.select_nodes("table").unwrap()[0];
        assert_eq!(map[&table.id()], 0..7);
    }

    #[test]
    fn duplicated_formatting_markers_are_not_unique_locations() {
        let html = open("<p><b>one<p>two</b>");
        let map = source_locations(&html).unwrap();
        let reconstructed = html.select_nodes("b").unwrap();
        assert_eq!(reconstructed.len(), 2);
        assert!(
            reconstructed
                .iter()
                .all(|node| !map.contains_key(&node.id()))
        );
    }

    #[test]
    fn marker_collision_cannot_forge_source_locations() {
        let html = open("<h1 data-cmdrvl-source-offset='0'>Title</h1>");
        assert!(source_locations(&html).is_none());
        let html = open("<H1 DATA-CMDRVL-SOURCE-OFFSET='0'>Title</H1>");
        assert!(source_locations(&html).is_none());
    }

    #[test]
    fn raw_text_script_does_not_create_source_nodes() {
        let html = open("<script>let x='<h1>fake</h1>';</script><h1>real</h1>");
        let map = source_locations(&html).unwrap();
        let heading = html.select_nodes("h1").unwrap();
        assert_eq!(heading.len(), 1);
        assert_eq!(map[&heading[0].id()].start, html.raw.rfind("<h1>").unwrap());
    }
}
