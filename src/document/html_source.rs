//! Original source locations for opt-in selector-based HTML bounds.
//! The ordinary DOM and frozen normalizer are never modified.
use super::html::HtmlDocument;
use ego_tree::NodeId;
use lol_html::{RewriteStrSettings, element, end_tag, rewrite_str};
use regex::Regex;
use scraper::{Html, Selector};
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

const MARKER: &str = "data-cmdrvl-source-offset";

pub(crate) type SourceLocations = HashMap<NodeId, Range<usize>>;
pub(crate) type SourceElementLocations = HashMap<NodeId, SourceElementLocation>;

#[derive(Debug, Clone)]
pub(crate) struct SourceElementLocation {
    pub(crate) start_tag: Range<usize>,
    pub(crate) element: Option<Range<usize>>,
}

/// Return only uniquely mapped explicit start tags, or no mappings if equivalence
/// cannot be established. Clone reconstruction and implicit tags are not evidence
/// of a unique source node. Ranges describe opening tags, not whole elements.
pub(crate) fn source_locations(html: &HtmlDocument) -> Option<SourceLocations> {
    source_location_map(html, false).map(|locations| {
        locations
            .into_iter()
            .map(|(node_id, location)| (node_id, location.start_tag))
            .collect()
    })
}

/// Return uniquely mapped source spans for explicit HTML elements. A non-void
/// element is bounded only when the tokenizer observes its explicit end tag.
pub(crate) fn source_element_locations(html: &HtmlDocument) -> Option<SourceElementLocations> {
    source_location_map(html, true)
}

fn source_location_map(
    html: &HtmlDocument,
    include_element_bounds: bool,
) -> Option<SourceElementLocations> {
    if !html.source_is_utf8 || html.raw.contains(MARKER) {
        return None;
    }
    let locations = Rc::new(RefCell::new(Vec::<SourceElementLocation>::new()));
    let handler_locations = Rc::clone(&locations);
    let annotated = rewrite_str(
        &html.raw,
        RewriteStrSettings::new().append_element_content_handler(element!("*", move |element| {
            let start_tag = element.source_location().bytes();
            let source_tag_name = element.tag_name();
            let can_have_content = element.can_have_content();
            let ordinal = {
                let mut locations = handler_locations.borrow_mut();
                let ordinal = locations.len();
                let element_bounds =
                    (include_element_bounds && !can_have_content).then(|| start_tag.clone());
                locations.push(SourceElementLocation {
                    start_tag: start_tag.clone(),
                    element: element_bounds,
                });
                ordinal
            };
            if include_element_bounds && can_have_content {
                let end_locations = Rc::clone(&handler_locations);
                element.on_end_tag(end_tag!(move |end_tag| {
                    let end = end_tag.source_location().bytes();
                    if end_tag.name() == source_tag_name
                        && let Some(location) = end_locations.borrow_mut().get_mut(ordinal)
                        && end.start >= location.start_tag.end
                    {
                        location.element = Some(location.start_tag.start..end.end);
                    }
                    Ok(())
                }))?;
            }
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
    let locations = locations.borrow().clone();
    let mut counts = HashMap::new();
    let mut candidates = Vec::new();
    for (original, mapped) in original.into_iter().zip(mapped) {
        let Some(ordinal) = mapped.value().attr(MARKER) else {
            continue;
        };
        let ordinal = ordinal.parse::<usize>().ok()?;
        let location = locations.get(ordinal)?;
        let tag = html.raw.get(location.start_tag.clone())?;
        if !tag.starts_with('<') || !tag.ends_with('>') {
            return None;
        }
        let mut location = location.clone();
        if location.element.as_ref().is_some_and(|span| {
            span.start != location.start_tag.start
                || span.end < location.start_tag.end
                || html.raw.get(span.clone()).is_none()
        }) {
            location.element = None;
        }
        *counts.entry(ordinal).or_insert(0usize) += 1;
        candidates.push((original.id(), ordinal, location));
    }
    Some(
        candidates
            .into_iter()
            .filter(|(_, ordinal, _)| counts[ordinal] == 1)
            .map(|(id, _, location)| (id, location))
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

    #[test]
    fn element_bounds_cover_explicit_start_through_end_tag_bytes() {
        let raw =
            "<table><tr><td><span title='a > b'>é &amp; 🙂</span><b>100</b></td></tr></table>";
        let html = open(raw);
        let span = html.select_nodes("span").unwrap().remove(0);
        let locations = source_element_locations(&html).unwrap();
        let location = &locations[&span.id()];
        let start = raw.find("<span").unwrap();
        let start_tag_end = raw[start..].find("'>").unwrap() + start + 2;
        let end = raw.find("</span>").unwrap() + "</span>".len();

        assert_eq!(location.start_tag, start..start_tag_end);
        assert_eq!(location.element, Some(start..end));
        assert_eq!(
            &raw[location.element.clone().unwrap()],
            "<span title='a > b'>é &amp; 🙂</span>"
        );
        assert!(!raw[location.element.clone().unwrap()].contains("100"));
    }

    #[test]
    fn missing_explicit_end_tag_keeps_dom_node_unbound() {
        let raw = "<table><tr><td><span>(Q)</td><td>100</td></tr></table>";
        let html = open(raw);
        let span = html.select_nodes("span").unwrap().remove(0);
        let locations = source_element_locations(&html).unwrap();

        assert!(locations.contains_key(&span.id()));
        assert_eq!(locations[&span.id()].element, None);
        assert!(raw.contains("(Q)"));
    }

    #[test]
    fn explicit_void_element_uses_its_start_tag_as_its_complete_span() {
        let raw = "<img alt='marker'>";
        let html = open(raw);
        let image = html.select_nodes("img").unwrap().remove(0);
        let locations = source_element_locations(&html).unwrap();

        assert_eq!(locations[&image.id()].element, Some(0..raw.len()));
    }
}
