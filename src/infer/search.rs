use crate::infer::observer::Observation;
use asupersync::Cx;
use frankensearch_core::traits::{LexicalSearch, cosine_similarity};
use frankensearch_core::types::{IndexableDocument, VectorHit};
use frankensearch_embed::HashEmbedder;
use frankensearch_fusion::{RrfConfig, rrf_fuse};
use frankensearch_lexical::TantivyIndex;
use futures::executor::block_on;
use std::collections::{BTreeMap, BTreeSet};

/// One searchable text unit for hybrid infer lookups.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchDocument {
    pub id: String,
    pub title: Option<String>,
    pub content: String,
}

/// One hybrid (BM25 + semantic) hit ranked with RRF.
#[derive(Debug, Clone, PartialEq)]
pub struct HybridHit {
    pub doc_id: String,
    pub score: f64,
    pub lexical_rank: Option<usize>,
    pub semantic_rank: Option<usize>,
}

/// One line-level unit used by schema-driven infer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineSearchDocument {
    pub line: usize,
    pub heading: Option<String>,
    pub text: String,
}

/// Run deterministic hybrid search over an in-memory corpus.
pub fn hybrid_search(
    query: &str,
    documents: &[SearchDocument],
    limit: usize,
) -> Result<Vec<HybridHit>, String> {
    if query.trim().is_empty() || documents.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }

    let lexical = lexical_hits(query, documents)?;
    let semantic = semantic_hits(query, documents);
    let fused = rrf_fuse(&lexical, &semantic, limit.min(documents.len()), 0, &RrfConfig::default());

    Ok(fused
        .into_iter()
        .map(|hit| HybridHit {
            doc_id: hit.doc_id,
            score: hit.rrf_score,
            lexical_rank: hit.lexical_rank,
            semantic_rank: hit.semantic_rank,
        })
        .collect())
}

/// Build line-level search documents with nearest-heading context.
pub fn build_line_documents(content: &str, headings: &[(String, usize)]) -> Vec<LineSearchDocument> {
    let mut rows = Vec::new();
    let mut heading_index = 0usize;

    for (line_offset, raw_line) in content.lines().enumerate() {
        let line_number = line_offset + 1;
        let text = raw_line.trim();
        if text.is_empty() {
            continue;
        }

        while heading_index + 1 < headings.len() && headings[heading_index + 1].1 <= line_number {
            heading_index += 1;
        }

        let heading = headings
            .get(heading_index)
            .and_then(|(name, line)| (*line <= line_number).then_some(name.clone()));

        rows.push(LineSearchDocument {
            line: line_number,
            heading,
            text: text.to_owned(),
        });
    }

    rows
}

/// Locate the best-matching line for a value query.
pub fn locate_line_for_query(
    query: &str,
    lines: &[LineSearchDocument],
) -> Result<Option<usize>, String> {
    if query.trim().is_empty() || lines.is_empty() {
        return Ok(None);
    }

    let documents = lines
        .iter()
        .map(|line| SearchDocument {
            id: line.line.to_string(),
            title: line.heading.clone(),
            content: line.text.clone(),
        })
        .collect::<Vec<_>>();
    let hits = hybrid_search(query, &documents, documents.len())?;
    if hits.is_empty() {
        return Ok(None);
    }

    let line_by_id = lines
        .iter()
        .map(|line| (line.line.to_string(), line))
        .collect::<BTreeMap<_, _>>();

    let query_lower = query.to_ascii_lowercase();
    for hit in &hits {
        if let Some(line) = line_by_id.get(&hit.doc_id)
            && line.text.to_ascii_lowercase().contains(&query_lower)
        {
            return Ok(Some(line.line));
        }
    }

    Ok(hits
        .first()
        .and_then(|hit| line_by_id.get(&hit.doc_id).map(|line| line.line)))
}

/// Rank observation indices by recurrent corpus patterns.
pub fn rank_observation_indices(observations: &[Observation]) -> Result<Vec<usize>, String> {
    if observations.is_empty() {
        return Ok(Vec::new());
    }

    let documents = observations
        .iter()
        .enumerate()
        .map(|(index, observation)| SearchDocument {
            id: index.to_string(),
            title: Some(observation.filename.clone()),
            content: observation_summary(observation),
        })
        .collect::<Vec<_>>();

    let queries = observation_queries(observations);
    if queries.is_empty() {
        return Ok((0..observations.len()).collect());
    }

    let mut totals = vec![0.0_f64; observations.len()];
    for query in &queries {
        let hits = hybrid_search(query, &documents, documents.len())?;
        for (rank, hit) in hits.iter().enumerate() {
            let Ok(index) = hit.doc_id.parse::<usize>() else {
                continue;
            };
            if index < totals.len() {
                totals[index] += 1.0 / (rank as f64 + 1.0);
            }
        }
    }

    let mut indices = (0..observations.len()).collect::<Vec<_>>();
    indices.sort_by(|left, right| {
        totals[*right]
            .total_cmp(&totals[*left])
            .then_with(|| observation_sort_key(&observations[*left]).cmp(&observation_sort_key(&observations[*right])))
            .then_with(|| left.cmp(right))
    });

    Ok(indices)
}

fn lexical_hits(
    query: &str,
    documents: &[SearchDocument],
) -> Result<Vec<frankensearch_core::types::ScoredResult>, String> {
    let index = TantivyIndex::in_memory().map_err(format_search_error)?;
    let cx = Cx::for_request();

    let rows = documents
        .iter()
        .map(|document| {
            let mut indexable = IndexableDocument::new(document.id.clone(), document.content.clone());
            if let Some(title) = &document.title {
                indexable = indexable.with_title(title.clone());
            }
            indexable
        })
        .collect::<Vec<_>>();

    block_on(index.index_documents(&cx, &rows)).map_err(format_search_error)?;
    block_on(index.commit(&cx)).map_err(format_search_error)?;
    block_on(index.search(&cx, query, documents.len())).map_err(format_search_error)
}

fn semantic_hits(query: &str, documents: &[SearchDocument]) -> Vec<VectorHit> {
    let embedder = HashEmbedder::default_384();
    let query_embedding = embedder.embed_sync(query);

    let mut hits = Vec::with_capacity(documents.len());
    for (position, document) in documents.iter().enumerate() {
        let doc_embedding = embedder.embed_sync(&document.content);
        let score = cosine_similarity(&query_embedding, &doc_embedding);
        let semantic_index = u32::try_from(position).unwrap_or(u32::MAX);
        hits.push(VectorHit {
            index: semantic_index,
            score,
            doc_id: document.id.clone(),
        });
    }

    hits.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.doc_id.cmp(&right.doc_id))
    });
    hits
}

fn format_search_error(error: frankensearch_core::SearchError) -> String {
    format!("frankensearch error: {error}")
}

fn observation_summary(observation: &Observation) -> String {
    let mut parts = vec![
        observation.format.clone(),
        observation.extension.clone(),
        observation.filename.clone(),
    ];
    parts.extend(observation.sheet_names.iter().cloned());
    parts.extend(observation.row_counts.keys().cloned());
    parts.extend(observation.cell_values.values().cloned());
    parts.extend(observation.csv_headers.iter().cloned());
    parts.extend(observation.pdf_metadata.keys().cloned());
    parts.extend(observation.pdf_metadata.values().cloned());
    if let Some(page_count) = observation.pdf_page_count {
        parts.push(page_count.to_string());
    }

    parts
        .into_iter()
        .filter(|value| !value.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn observation_queries(observations: &[Observation]) -> Vec<String> {
    let min_support = if observations.len() > 1 { 2 } else { 1 };
    let mut support = BTreeMap::<String, usize>::new();

    for observation in observations {
        let mut seen = BTreeSet::new();
        for token in tokenize_query_candidates(&observation_summary(observation)) {
            seen.insert(token);
        }
        for token in seen {
            *support.entry(token).or_insert(0) += 1;
        }
    }

    let mut ranked = support
        .into_iter()
        .filter(|(_, count)| *count >= min_support)
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));

    ranked.into_iter().map(|(token, _)| token).take(12).collect()
}

fn tokenize_query_candidates(input: &str) -> Vec<String> {
    input
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter_map(|token| {
            let lowered = token.to_ascii_lowercase();
            if lowered.len() < 3 || lowered.chars().all(|character| character.is_ascii_digit()) {
                return None;
            }
            Some(lowered)
        })
        .collect()
}

fn observation_sort_key(observation: &Observation) -> String {
    format!(
        "{}:{}:{}",
        observation.filename, observation.extension, observation.format
    )
}

#[cfg(test)]
mod tests {
    use super::{
        SearchDocument, build_line_documents, hybrid_search, locate_line_for_query,
        rank_observation_indices,
    };
    use crate::infer::observer::Observation;
    use std::collections::HashMap;

    #[test]
    fn hybrid_search_is_deterministic() {
        let documents = vec![
            SearchDocument {
                id: "a".to_owned(),
                title: Some("Executive Summary".to_owned()),
                content: "Cap Rate 6.25 percent and rent roll details".to_owned(),
            },
            SearchDocument {
                id: "b".to_owned(),
                title: Some("Tenant Summary".to_owned()),
                content: "Tenant list and lease schedule".to_owned(),
            },
            SearchDocument {
                id: "c".to_owned(),
                title: Some("Market Section".to_owned()),
                content: "Comparable sales and market overview".to_owned(),
            },
        ];

        let first = hybrid_search("cap rate", &documents, 3).expect("first hybrid search");
        let second = hybrid_search("cap rate", &documents, 3).expect("second hybrid search");
        assert_eq!(first, second);
        assert_eq!(first.first().map(|hit| hit.doc_id.as_str()), Some("a"));
    }

    #[test]
    fn line_search_locates_value_line() {
        let text = "\
# Executive Summary
Cap Rate
6.25%
";
        let headings = vec![("Executive Summary".to_owned(), 1)];
        let lines = build_line_documents(text, &headings);
        let line = locate_line_for_query("6.25%", &lines)
            .expect("line search should succeed")
            .expect("line should be present");
        assert_eq!(line, 3);
    }

    #[test]
    fn observation_ranking_is_stable() {
        let first = Observation {
            format: "csv".to_owned(),
            extension: "csv".to_owned(),
            filename: "a.csv".to_owned(),
            sheet_names: vec!["Sheet1".to_owned()],
            row_counts: HashMap::new(),
            cell_values: HashMap::new(),
            csv_headers: vec!["tenant".to_owned(), "rent".to_owned()],
            csv_row_count: Some(2),
            pdf_page_count: None,
            pdf_metadata: HashMap::new(),
        };
        let second = Observation {
            format: "csv".to_owned(),
            extension: "csv".to_owned(),
            filename: "b.csv".to_owned(),
            sheet_names: vec!["Sheet1".to_owned()],
            row_counts: HashMap::new(),
            cell_values: HashMap::new(),
            csv_headers: vec!["tenant".to_owned(), "state".to_owned()],
            csv_row_count: Some(2),
            pdf_page_count: None,
            pdf_metadata: HashMap::new(),
        };

        let one = rank_observation_indices(&[first.clone(), second.clone()]).expect("rank one");
        let two = rank_observation_indices(&[first, second]).expect("rank two");
        assert_eq!(one, two);
        assert_eq!(one, vec![0, 1]);
    }
}
