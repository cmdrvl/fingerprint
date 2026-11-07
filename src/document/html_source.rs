//! Original source locations for opt-in selector-based HTML bounds.
//! The ordinary DOM and frozen normalizer are never modified.
use super::html::HtmlDocument;
use ego_tree::NodeId;
use lol_html::{RewriteStrSettings, element, end_tag, rewrite_str};
use regex::Regex;
use scraper::{ElementRef, Html, Selector, node::Node};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::rc::Rc;

const MARKER: &str = "data-cmdrvl-source-offset";

pub(crate) type SourceLocations = HashMap<NodeId, Range<usize>>;
pub(crate) type SourceElementLocations = HashMap<NodeId, SourceElementLocation>;

#[derive(Debug, Clone)]
pub(crate) struct SourceElementLocation {
    pub(crate) tag_name: String,
    pub(crate) start_tag: Range<usize>,
    pub(crate) element: Option<Range<usize>>,
    /// Exact explicit closing tag, when the native tokenizer observes one.
    /// This is retained so a table cell whose own close is implicit can use
    /// an explicit row, row-group, or table close as its parser boundary.
    pub(crate) end_tag: Option<Range<usize>>,
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

/// Return uniquely mapped source spans for HTML elements. Non-void elements
/// normally require their explicit end tag; `td` and `th` may also use a
/// uniquely mapped HTML parser boundary when their close is implicit.
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
                    tag_name: source_tag_name.clone(),
                    start_tag: start_tag.clone(),
                    element: element_bounds,
                    end_tag: None,
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
                        location.end_tag = Some(end.clone());
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
    let mut mapped_locations = HashMap::new();
    let mut unique_ordinals = HashMap::new();
    for (id, ordinal, location) in candidates {
        if counts[&ordinal] == 1 {
            unique_ordinals.insert(ordinal, id);
            mapped_locations.insert(id, location);
        }
    }
    infer_implicit_table_cell_bounds(html, &locations, &unique_ordinals, &mut mapped_locations);
    Some(mapped_locations)
}

/// Use the parsed HTML tree to bound cells that HTML implicitly closes. The
/// opening tag still comes from the native tokenizer-to-DOM identity map. A
/// slash on `td`/`th` is not treated as a self-close; the selected boundary is
/// the next cell, an explicit ancestor close, or the next row in the same
/// table. Any source element inside the proposed span must map uniquely to a
/// descendant of that same DOM cell.
fn infer_implicit_table_cell_bounds(
    html: &HtmlDocument,
    source_tokens: &[SourceElementLocation],
    unique_ordinals: &HashMap<usize, NodeId>,
    locations: &mut SourceElementLocations,
) {
    let Ok(cells) = html.select_nodes("td, th") else {
        return;
    };
    let Ok(rows) = html.select_nodes("tr") else {
        return;
    };
    let next_rows = next_rows_in_same_table(&rows);

    for cell in cells {
        let id = cell.id();
        let Some(location) = locations.get(&id).cloned() else {
            continue;
        };
        if location.element.is_some() || location.end_tag.is_some() {
            continue;
        }
        if !matches!(cell.value().name(), "td" | "th") {
            continue;
        }
        let Some(row) = nearest_ancestor_named(cell, "tr") else {
            continue;
        };
        let Some(end) = implicit_cell_boundary(cell, row, source_tokens, &next_rows, locations)
        else {
            continue;
        };
        if end <= location.start_tag.end || end > html.raw.len() {
            continue;
        }
        if !source_tags_are_cell_descendants(
            cell,
            location.start_tag.end,
            end,
            source_tokens,
            unique_ordinals,
        ) {
            continue;
        }
        if let Some(location) = locations.get_mut(&id) {
            location.element = Some(location.start_tag.start..end);
        }
    }
}

fn next_rows_in_same_table(rows: &[ElementRef<'_>]) -> HashMap<NodeId, NodeId> {
    let mut next_by_table = HashMap::new();
    let mut next_by_row = HashMap::new();
    for row in rows.iter().rev() {
        let Some(table) = nearest_ancestor_named(*row, "table") else {
            continue;
        };
        if let Some(next_row) = next_by_table.get(&table.id()) {
            next_by_row.insert(row.id(), *next_row);
        }
        next_by_table.insert(table.id(), row.id());
    }
    next_by_row
}

fn nearest_ancestor_named<'a>(node: ElementRef<'a>, name: &str) -> Option<ElementRef<'a>> {
    let mut parent = node.parent();
    while let Some(candidate) = parent {
        parent = candidate.parent();
        if let Some(element) = ElementRef::wrap(candidate)
            && element.value().name() == name
        {
            return Some(element);
        }
    }
    None
}

fn implicit_cell_boundary<'a>(
    cell: ElementRef<'a>,
    row: ElementRef<'a>,
    source_tokens: &[SourceElementLocation],
    next_rows: &HashMap<NodeId, NodeId>,
    locations: &SourceElementLocations,
) -> Option<usize> {
    let cell_start = locations.get(&cell.id())?.start_tag.end;
    // A later cell in the same row implicitly closes this one. Other element
    // siblings and non-whitespace text indicate a malformed/foster-parented
    // case whose source ownership is not safe to infer.
    for sibling in cell.next_siblings() {
        match sibling.value() {
            Node::Element(_) => {
                let sibling = ElementRef::wrap(sibling)?;
                if !matches!(sibling.value().name(), "td" | "th") {
                    return None;
                }
                return locations
                    .get(&sibling.id())
                    .map(|location| location.start_tag.start);
            }
            Node::Text(text) if !text.text.trim().is_empty() => return None,
            Node::Text(_) | Node::Comment(_) => {}
            _ => return None,
        }
    }

    let mut boundaries = Vec::new();
    let mut ancestor = Some(row);
    while let Some(element) = ancestor {
        let name = element.value().name();
        if matches!(name, "tr" | "tbody" | "thead" | "tfoot" | "table")
            && let Some(end_tag) = locations
                .get(&element.id())
                .and_then(|location| location.end_tag.as_ref())
            && !source_contains_tag(source_tokens, name, cell_start, end_tag.start)
        {
            boundaries.push(end_tag.start);
        }
        if name == "table" {
            break;
        }
        ancestor = element.parent().and_then(ElementRef::wrap);
    }

    // An omitted row end is closed by the next row token in the same table.
    // Filter by the nearest native table ancestor so nested table rows do not
    // close an outer cell.
    if let Some(next_row) = next_rows.get(&row.id())
        && let Some(location) = locations.get(next_row)
    {
        boundaries.push(location.start_tag.start);
    }

    boundaries.into_iter().min()
}

fn source_contains_tag(
    source_tokens: &[SourceElementLocation],
    tag_name: &str,
    start: usize,
    end: usize,
) -> bool {
    let first_token = source_tokens.partition_point(|token| token.start_tag.start < start);
    for token in source_tokens.iter().skip(first_token) {
        if token.start_tag.start >= end {
            break;
        }
        if token.tag_name == tag_name {
            return true;
        }
    }
    false
}

fn source_tags_are_cell_descendants(
    cell: ElementRef<'_>,
    start: usize,
    end: usize,
    source_tokens: &[SourceElementLocation],
    unique_ordinals: &HashMap<usize, NodeId>,
) -> bool {
    let descendants: HashSet<NodeId> = cell.descendants().map(|node| node.id()).collect();
    let first_token = source_tokens.partition_point(|token| token.start_tag.start < start);
    for (ordinal, token) in source_tokens.iter().enumerate().skip(first_token) {
        if token.start_tag.start >= end {
            break;
        }
        let Some(node_id) = unique_ordinals.get(&ordinal) else {
            return false;
        };
        if !descendants.contains(node_id) {
            return false;
        }
    }
    true
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
