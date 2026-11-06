use lol_html::{element, rewrite_str, RewriteStrSettings};
use scraper::{Html, Selector};
use std::{collections::HashMap, ops::Range};

const MARKER: &str = "data-cmdrvl-source-probe";

fn probe(raw: &str) -> Result<(usize, usize, usize), String> {
    probe_with(raw, |_, _| {})
}

fn probe_with(
    raw: &str,
    mut inspect: impl FnMut(&str, Range<usize>),
) -> Result<(usize, usize, usize), String> {
    if raw.contains(MARKER) {
        return Err("source contains reserved probe marker".into());
    }
    let mut locations: Vec<Range<usize>> = vec![];
    let annotated = rewrite_str(
        raw,
        RewriteStrSettings::new().append_element_content_handler(element!("*", |el| {
            let index = locations.len();
            locations.push(el.source_location().bytes());
            el.set_attribute(MARKER, &index.to_string())?;
            Ok(())
        })),
    )
    .map_err(|e| e.to_string())?;
    let original = Html::parse_document(raw);
    let mapped = Html::parse_document(&annotated);
    let marker_attribute = regex::Regex::new(r#" data-cmdrvl-source-probe="[0-9]+""#).unwrap();
    if original.html() != marker_attribute.replace_all(&mapped.html(), "") {
        return Err("annotated parse differs from original DOM".into());
    }
    let selector = Selector::parse("*").unwrap();
    let mut occurrences = HashMap::new();
    let mut implicit = 0;
    for element in mapped.select(&selector) {
        if let Some(id) = element.value().attr(MARKER) {
            let index = id.parse::<usize>().map_err(|e| e.to_string())?;
            let span = locations.get(index).ok_or("invalid source marker")?;
            let source_tag = raw.get(span.clone()).ok_or("invalid UTF-8 source offset")?;
            if !source_tag.starts_with('<') || !source_tag.ends_with('>') {
                return Err(format!("range is not a source tag: {span:?}"));
            }
            inspect(element.value().name(), span.clone());
            *occurrences.entry(index).or_insert(0usize) += 1;
        } else {
            implicit += 1;
        }
    }
    let unique = occurrences.values().filter(|&&n| n == 1).count();
    let duplicated = occurrences.values().filter(|&&n| n > 1).count();
    Ok((unique, duplicated, implicit))
}

fn main() {
    let mut failed = false;
    for path in std::env::args().skip(1) {
        let started = std::time::Instant::now();
        let result = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|raw| probe(&raw));
        failed |= result.is_err();
        println!(
            "{}",
            serde_json::json!({"path":path,"result":result,"elapsed_ms":started.elapsed().as_millis()})
        );
    }
    if failed {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn case_quotes_entities_and_multibyte() {
        assert_eq!(
            probe("é<HTML><BODY><H1 class=soi>SOI &#160; title</H1></BODY></HTML>").unwrap(),
            (3, 0, 1)
        );
    }
    #[test]
    fn implicit_table_body_is_not_given_a_source_span() {
        assert_eq!(
            probe("<table><tr><td>x</td></tr></table>").unwrap(),
            (3, 0, 4)
        );
    }
    #[test]
    fn script_content_is_not_treated_as_markup() {
        assert_eq!(
            probe("<script>let x='<h1>fake</h1>';</script><h1>real</h1>").unwrap(),
            (2, 0, 3)
        );
    }
    #[test]
    fn repeated_identical_nodes_have_distinct_source_markers() {
        assert_eq!(probe("<p>A</p><p>A</p>").unwrap(), (2, 0, 3));
    }
    #[test]
    fn refuses_marker_collision() {
        assert!(probe("<p data-cmdrvl-source-probe='0'>x</p>").is_err());
    }
    #[test]
    fn source_locations_are_original_bytes_not_reserialized_positions() {
        let raw = "<html><body>é🙂<H1 title='a > b' class=soi>A</H1><H1 title='a > b' class=soi>A</H1></body></html>";
        let mut found = vec![];
        probe_with(raw, |name, span| {
            if name == "h1" {
                found.push(span);
            }
        })
        .unwrap();
        let expected: Vec<_> = raw
            .match_indices("<H1 title='a > b' class=soi>")
            .map(|(start, tag)| start..start + tag.len())
            .collect();
        assert_eq!(found, expected);
        assert_eq!(found.len(), 2);
        assert!(found[0].end < found[1].start);
    }
    #[test]
    fn reconstructed_formatting_nodes_are_marked_ambiguous() {
        let (_, duplicates, _) = probe("<p><b>one<p>two</b>").unwrap();
        assert!(
            duplicates > 0,
            "cloned formatting nodes must not become unique source mappings"
        );
    }
}
