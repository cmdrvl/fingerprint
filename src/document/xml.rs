#![forbid(unsafe_code)]

//! XML documents for structural fingerprinting (bd-yq3, XML slice).
//!
//! The reader is `quick-xml` 0.31 (`NsReader`): a streaming pull parser, so memory is
//! O(document depth + distinct element paths), never O(document). It does not expand custom
//! entities and never fetches a DTD or external resource.
//!
//! At open, one streaming pass builds a structural index: the root element, its namespace, and
//! for every element path (`/edgarSubmission/formData/genInfo/regName`, local names only) the
//! number of elements and the attribute names seen. Assertions on structure (root, namespace,
//! path exists, path count) use the index. Assertions on values (`xml_attr_regex`,
//! `xml_text_regex`) stream the file again and keep only match counts; no attribute or text
//! value is ever stored in the index or echoed in a failure.
//!
//! Real EDGAR XML is not always strictly well formed: the archived `primary_doc.xml` and ABS-EE
//! exhibits begin with a stray carriage return before the `<?xml ?>` declaration and use CR line
//! endings. Strict parsers (xmllint, Python's ElementTree) reject that; this reader accepts it.

use quick_xml::NsReader;
use quick_xml::escape::unescape;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::ResolveResult;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

/// Most distinct element paths indexed; more is refused at open instead of growing unbounded.
pub const MAX_DISTINCT_PATHS: usize = 200_000;
/// Deepest nesting accepted.
pub const MAX_DEPTH: usize = 512;

#[derive(Debug)]
pub struct XmlDocument {
    pub path: PathBuf,
    /// Local name of the root element.
    pub root: String,
    /// Namespace URI of the root element, `None` when it has none.
    pub root_namespace: Option<String>,
    /// Element path -> number of elements at that path.
    pub paths: BTreeMap<String, u64>,
    /// Element path -> attribute local names seen on elements at that path (never values).
    pub attribute_names: BTreeMap<String, BTreeSet<String>>,
    pub element_count: u64,
}

/// One element start, handed to the walker's callback.
pub struct StartInfo<'a> {
    pub path: &'a str,
    pub attributes: &'a [(String, String)],
}

fn parse_error(path: &Path, reader_position: u64, error: impl std::fmt::Display) -> String {
    format!(
        "invalid xml '{}' at byte {reader_position}: {error}",
        path.display()
    )
}

fn local(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Stream `path`, calling `on_start` for every element and, when `want_text_at` is a path,
/// `on_text` with the (trimmed, unescaped) text of every element at that path once it closes.
/// `on_root` receives the root's local name and namespace once.
fn walk(
    path: &Path,
    want_text_at: Option<&str>,
    on_root: &mut dyn FnMut(&str, Option<&str>),
    on_start: &mut dyn FnMut(&StartInfo<'_>) -> Result<(), String>,
    on_text: &mut dyn FnMut(&str),
) -> Result<(), String> {
    let file = File::open(path)
        .map_err(|error| format!("failed opening '{}': {error}", path.display()))?;
    let mut reader = NsReader::from_reader(BufReader::new(file));
    let mut buffer = Vec::new();
    // (local name, collected text when this element is a text target)
    let mut stack: Vec<(String, Option<String>)> = Vec::new();
    let mut current_path = String::new();
    let mut saw_root = false;

    loop {
        let position = reader.buffer_position() as u64;
        let (resolved, event) = reader
            .read_resolved_event_into(&mut buffer)
            .map_err(|error| parse_error(path, position, error))?;
        match event {
            Event::Start(ref element) | Event::Empty(ref element) => {
                let is_empty = matches!(event, Event::Empty(_));
                let name = local(element.local_name().as_ref());
                if stack.len() >= MAX_DEPTH {
                    return Err(format!(
                        "invalid xml '{}': nesting deeper than {MAX_DEPTH} levels",
                        path.display()
                    ));
                }
                if !saw_root {
                    saw_root = true;
                    let namespace = match &resolved {
                        ResolveResult::Bound(ns) => Some(local(ns.as_ref())),
                        _ => None,
                    };
                    on_root(&name, namespace.as_deref());
                }
                let parent_len = current_path.len();
                current_path.push('/');
                current_path.push_str(&name);
                let attributes = collect_attributes(path, element, position)?;
                on_start(&StartInfo {
                    path: &current_path,
                    attributes: &attributes,
                })?;
                if is_empty {
                    if want_text_at == Some(current_path.as_str()) {
                        on_text("");
                    }
                    current_path.truncate(parent_len);
                } else {
                    let text = (want_text_at == Some(current_path.as_str())).then(String::new);
                    stack.push((name, text));
                }
            }
            Event::End(_) => {
                if let Some((name, text)) = stack.pop() {
                    if let Some(text) = text {
                        on_text(text.trim());
                    }
                    let new_len = current_path.len() - name.len() - 1;
                    current_path.truncate(new_len);
                }
            }
            Event::Text(ref text) => {
                if let Some((_, Some(collected))) = stack.last_mut() {
                    let raw = String::from_utf8_lossy(text.as_ref()).into_owned();
                    let value = unescape(&raw)
                        .map_err(|error| parse_error(path, position, error))?
                        .into_owned();
                    collected.push_str(&value);
                }
            }
            Event::CData(ref data) => {
                if let Some((_, Some(collected))) = stack.last_mut() {
                    collected.push_str(&String::from_utf8_lossy(data.as_ref()));
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    if !saw_root {
        return Err(format!("invalid xml '{}': no root element", path.display()));
    }
    Ok(())
}

fn collect_attributes(
    path: &Path,
    element: &BytesStart<'_>,
    position: u64,
) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    for attribute in element.attributes() {
        let attribute = attribute.map_err(|error| parse_error(path, position, error))?;
        let key = attribute.key;
        // Namespace declarations are not data attributes.
        if key.as_ref() == b"xmlns" || key.as_ref().starts_with(b"xmlns:") {
            continue;
        }
        let name = local(key.local_name().as_ref());
        let raw = String::from_utf8_lossy(attribute.value.as_ref()).into_owned();
        let value = unescape(&raw)
            .map_err(|error| parse_error(path, position, error))?
            .into_owned();
        out.push((name, value));
    }
    Ok(out)
}

impl XmlDocument {
    pub fn open(path: &Path) -> Result<Self, String> {
        let mut root = String::new();
        let mut root_namespace = None;
        let mut paths: BTreeMap<String, u64> = BTreeMap::new();
        let mut attribute_names: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut element_count = 0u64;
        let display = path.display().to_string();
        walk(
            path,
            None,
            &mut |name, namespace| {
                root = name.to_owned();
                root_namespace = namespace.map(ToOwned::to_owned);
            },
            &mut |start| {
                element_count += 1;
                if !paths.contains_key(start.path) && paths.len() >= MAX_DISTINCT_PATHS {
                    return Err(format!(
                        "invalid xml '{display}': more than {MAX_DISTINCT_PATHS} distinct element paths"
                    ));
                }
                *paths.entry(start.path.to_owned()).or_insert(0) += 1;
                if !start.attributes.is_empty() {
                    let names = attribute_names.entry(start.path.to_owned()).or_default();
                    for (name, _) in start.attributes {
                        names.insert(name.clone());
                    }
                }
                Ok(())
            },
            &mut |_| {},
        )?;
        Ok(Self {
            path: path.to_path_buf(),
            root,
            root_namespace,
            paths,
            attribute_names,
            element_count,
        })
    }

    pub fn count_at(&self, path: &str) -> u64 {
        self.paths.get(path).copied().unwrap_or(0)
    }

    /// Stream the file and count, at `path`, the attribute values named `attribute` (by local
    /// name) and how many of them satisfy `matches`. Values are never kept.
    pub fn scan_attribute(
        &self,
        path: &str,
        attribute: &str,
        matches: &dyn Fn(&str) -> bool,
    ) -> Result<(u64, u64), String> {
        let (mut seen, mut matched) = (0u64, 0u64);
        walk(
            &self.path,
            None,
            &mut |_, _| {},
            &mut |start| {
                if start.path == path {
                    for (name, value) in start.attributes {
                        if name == attribute {
                            seen += 1;
                            if matches(value) {
                                matched += 1;
                            }
                        }
                    }
                }
                Ok(())
            },
            &mut |_| {},
        )?;
        Ok((seen, matched))
    }

    /// Stream the file and count the elements at `path` and how many have text satisfying
    /// `matches`. Text is never kept beyond the element being evaluated.
    pub fn scan_text(
        &self,
        path: &str,
        matches: &dyn Fn(&str) -> bool,
    ) -> Result<(u64, u64), String> {
        let (mut seen, mut matched) = (0u64, 0u64);
        walk(
            &self.path,
            Some(path),
            &mut |_, _| {},
            &mut |_| Ok(()),
            &mut |text| {
                seen += 1;
                if matches(text) {
                    matched += 1;
                }
            },
        )?;
        Ok((seen, matched))
    }
}

/// A repeating-record projection of an XML document (see [`XmlDocument::project_records`]).
#[derive(Debug)]
pub struct RecordTable {
    pub columns: Vec<String>,
    /// One row per record, aligned with `columns`; a record without that child has "".
    pub rows: Vec<Vec<String>>,
    /// Non-leaf direct children that were left out, with how many times each occurred
    /// (names and counts only).
    pub nested_children: BTreeMap<String, u64>,
    /// Byte span in the source file from the first record's start tag to the last record's end.
    pub byte_start: u64,
    pub byte_end: u64,
}

struct OpenRecord {
    /// Stack depth (after pushing the record element) of the record itself.
    depth: usize,
    cells: BTreeMap<String, String>,
    /// Direct child currently open: (name, text so far, saw a nested element).
    child: Option<(String, String, bool)>,
}

impl XmlDocument {
    /// Project every element at `record_path` as one row. Columns are the record's direct LEAF
    /// children (elements without child elements): the `columns` given, in that order, or all of
    /// them in first-seen order. Non-leaf children (nested or repeated structures) are not
    /// flattened: they are left out and counted in `nested_children`, or refused when named in
    /// `columns`. A leaf that appears twice in one record refuses (it has no single value).
    pub fn project_records(
        &self,
        record_path: &str,
        columns: &[String],
    ) -> Result<RecordTable, String> {
        let file = File::open(&self.path)
            .map_err(|error| format!("failed opening '{}': {error}", self.path.display()))?;
        let mut reader = NsReader::from_reader(BufReader::new(file));
        let mut buffer = Vec::new();
        let mut stack: Vec<String> = Vec::new();
        let mut current_path = String::new();
        let mut record: Option<OpenRecord> = None;
        let mut order: Vec<String> = Vec::new();
        let mut rows: Vec<BTreeMap<String, String>> = Vec::new();
        let mut nested_children: BTreeMap<String, u64> = BTreeMap::new();
        let (mut byte_start, mut byte_end) = (0u64, 0u64);

        loop {
            let position = reader.buffer_position() as u64;
            let event = reader
                .read_event_into(&mut buffer)
                .map_err(|error| parse_error(&self.path, position, error))?;
            match event {
                Event::Start(ref element) | Event::Empty(ref element) => {
                    let is_empty = matches!(event, Event::Empty(_));
                    let name = local(element.local_name().as_ref());
                    let parent_len = current_path.len();
                    current_path.push('/');
                    current_path.push_str(&name);
                    let depth = stack.len() + 1;
                    if let Some(open) = record.as_mut() {
                        if depth == open.depth + 1 {
                            // A direct child of the record opens.
                            if is_empty {
                                finish_child(
                                    open,
                                    &name,
                                    String::new(),
                                    false,
                                    &mut order,
                                    &mut nested_children,
                                    columns,
                                    rows.len(),
                                )?;
                            } else {
                                open.child = Some((name.clone(), String::new(), false));
                            }
                        } else if depth > open.depth + 1
                            && let Some((_, _, nested)) = open.child.as_mut()
                        {
                            *nested = true;
                        }
                    } else if current_path == record_path && !is_empty {
                        if rows.is_empty() {
                            byte_start = position;
                        }
                        record = Some(OpenRecord {
                            depth,
                            cells: BTreeMap::new(),
                            child: None,
                        });
                    } else if current_path == record_path && is_empty {
                        if rows.is_empty() {
                            byte_start = position;
                        }
                        rows.push(BTreeMap::new());
                        byte_end = reader.buffer_position() as u64;
                    }
                    if is_empty {
                        current_path.truncate(parent_len);
                    } else {
                        stack.push(name);
                    }
                }
                Event::End(_) => {
                    let depth = stack.len();
                    let name = stack.pop().unwrap_or_default();
                    if let Some(open) = record.as_mut() {
                        if depth == open.depth + 1 {
                            if let Some((child_name, text, nested)) = open.child.take() {
                                finish_child(
                                    open,
                                    &child_name,
                                    text,
                                    nested,
                                    &mut order,
                                    &mut nested_children,
                                    columns,
                                    rows.len(),
                                )?;
                            }
                        } else if depth == open.depth
                            && let Some(done) = record.take()
                        {
                            rows.push(done.cells);
                            byte_end = reader.buffer_position() as u64;
                        }
                    }
                    let new_len = current_path.len() - name.len() - 1;
                    current_path.truncate(new_len);
                }
                Event::Text(ref text) => {
                    if let Some(open) = record.as_mut()
                        && stack.len() == open.depth + 1
                        && let Some((_, collected, _)) = open.child.as_mut()
                    {
                        let raw = String::from_utf8_lossy(text.as_ref()).into_owned();
                        collected.push_str(
                            &unescape(&raw)
                                .map_err(|error| parse_error(&self.path, position, error))?,
                        );
                    }
                }
                Event::CData(ref data) => {
                    if let Some(open) = record.as_mut()
                        && stack.len() == open.depth + 1
                        && let Some((_, collected, _)) = open.child.as_mut()
                    {
                        collected.push_str(&String::from_utf8_lossy(data.as_ref()));
                    }
                }
                Event::Eof => break,
                _ => {}
            }
            buffer.clear();
        }

        if rows.is_empty() {
            return Err(format!("no elements at record path '{record_path}'"));
        }
        let final_columns: Vec<String> = if columns.is_empty() {
            order
        } else {
            columns.to_vec()
        };
        let table_rows = rows
            .into_iter()
            .map(|mut cells| {
                final_columns
                    .iter()
                    .map(|column| cells.remove(column).unwrap_or_default())
                    .collect()
            })
            .collect();
        Ok(RecordTable {
            columns: final_columns,
            rows: table_rows,
            nested_children,
            byte_start,
            byte_end,
        })
    }
}

/// Close a direct child of the open record: store a leaf's text, or count a non-leaf.
#[allow(clippy::too_many_arguments)]
fn finish_child(
    open: &mut OpenRecord,
    name: &str,
    text: String,
    nested: bool,
    order: &mut Vec<String>,
    nested_children: &mut BTreeMap<String, u64>,
    columns: &[String],
    record_index: usize,
) -> Result<(), String> {
    if nested {
        if columns.iter().any(|column| column == name) {
            return Err(format!(
                "column '{name}' is not a leaf element (record {}): nested structures are not flattened",
                record_index + 1
            ));
        }
        *nested_children.entry(name.to_owned()).or_insert(0) += 1;
        return Ok(());
    }
    if !columns.is_empty() && !columns.iter().any(|column| column == name) {
        return Ok(());
    }
    if open.cells.contains_key(name) {
        return Err(format!(
            "leaf element '{name}' appears more than once in record {}: it has no single value",
            record_index + 1
        ));
    }
    if columns.is_empty() && !order.iter().any(|existing| existing == name) {
        order.push(name.to_owned());
    }
    open.cells.insert(name.to_owned(), text.trim().to_owned());
    Ok(())
}

/// A well-formed absolute element path: `/a/b/c`, local names only.
pub fn validate_path(path: &str) -> Result<(), String> {
    let Some(rest) = path.strip_prefix('/') else {
        return Err(format!("xml path '{path}' must start with '/'"));
    };
    if rest.is_empty() || rest.split('/').any(str::is_empty) {
        return Err(format!(
            "xml path '{path}' must be /name/name/... with no empty segment"
        ));
    }
    if path.contains(['[', ']', '*', '@', ':']) {
        return Err(format!(
            "xml path '{path}' may contain local element names only (no predicates, wildcards, attributes or prefixes)"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(tag: &str, body: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("fp-xml-{tag}-{}.xml", std::process::id()));
        std::fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn indexes_root_namespace_paths_counts_and_attribute_names() {
        let path = write(
            "idx",
            "\r<?xml version=\"1.0\"?><a xmlns=\"urn:x\" xmlns:p=\"urn:p\"><b k=\"v\"/><b/><c><d p:z=\"1\">t</d></c></a>",
        );
        let doc = XmlDocument::open(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(doc.root, "a");
        assert_eq!(doc.root_namespace.as_deref(), Some("urn:x"));
        assert_eq!(doc.count_at("/a/b"), 2);
        assert_eq!(doc.count_at("/a/c/d"), 1);
        assert_eq!(doc.count_at("/a/nope"), 0);
        assert_eq!(doc.element_count, 5);
        assert_eq!(
            doc.attribute_names["/a/b"].iter().collect::<Vec<_>>(),
            ["k"]
        );
        assert_eq!(
            doc.attribute_names["/a/c/d"].iter().collect::<Vec<_>>(),
            ["z"]
        );
        assert!(
            !doc.attribute_names.contains_key("/a"),
            "xmlns is not an attribute"
        );
    }

    #[test]
    fn text_and_attribute_scans_count_without_keeping_values() {
        let path = write(
            "scan",
            "<r><i id=\"1\">alpha</i><i id=\"x\">beta</i><i/></r>",
        );
        let doc = XmlDocument::open(&path).unwrap();
        let text = doc.scan_text("/r/i", &|t| t == "alpha").unwrap();
        let attr = doc
            .scan_attribute("/r/i", "id", &|v| v.chars().all(|c| c.is_ascii_digit()))
            .unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(text, (3, 1));
        assert_eq!(attr, (2, 1));
    }

    #[test]
    fn malformed_xml_is_an_error_naming_the_byte_not_the_content() {
        let path = write("bad", "<a><b></a>");
        let error = XmlDocument::open(&path).unwrap_err();
        let _ = std::fs::remove_file(&path);
        assert!(
            error.contains("invalid xml") && error.contains("at byte"),
            "{error}"
        );
    }

    #[test]
    fn path_grammar() {
        assert!(validate_path("/a/b").is_ok());
        for bad in ["a/b", "/", "/a//b", "/a/b[1]", "/a/*", "/a/@x", "/p:a"] {
            assert!(validate_path(bad).is_err(), "{bad}");
        }
    }
}
