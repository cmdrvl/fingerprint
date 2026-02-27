use frankensearch_core::{Cx, IndexableDocument, LexicalSearch, ScoredResult, VectorHit};
use frankensearch_embed::{HashAlgorithm, HashEmbedder};
use frankensearch_lexical::TantivyIndex;
use futures::executor::block_on;
use std::collections::HashMap;

const SEMANTIC_EMBED_DIM: usize = 384;
const SEMANTIC_EMBED_SEED: u64 = 0x5EED_CAFE_F00D_BAAD;
const DEFAULT_SEMANTIC_SUPPORT_THRESHOLD: f32 = 0.25;
const RRF_K: f64 = 60.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchDocument {
    pub id: String,
    pub title: Option<String>,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HybridHit {
    pub doc_id: String,
    pub fused_score: f64,
    pub lexical_rank: Option<usize>,
    pub semantic_rank: Option<usize>,
    pub lexical_score: Option<f32>,
    pub semantic_score: Option<f32>,
}

#[derive(Debug, Clone)]
struct SemanticDocument {
    index: u32,
    doc_id: String,
    embedding: Vec<f32>,
}

#[derive(Debug)]
pub struct HybridSearcher {
    lexical_index: TantivyIndex,
    semantic_documents: Vec<SemanticDocument>,
    semantic_embedder: HashEmbedder,
}

impl HybridSearcher {
    pub fn new(documents: &[SearchDocument]) -> Result<Self, String> {
        if documents.is_empty() {
            return Err("frankensearch requires at least one document".to_owned());
        }

        let mut sorted_documents = documents.to_vec();
        sorted_documents.sort_by(|left, right| left.id.cmp(&right.id));

        let lexical_index = TantivyIndex::in_memory()
            .map_err(|error| format!("failed creating in-memory frankensearch index: {error}"))?;

        let indexable_documents = sorted_documents
            .iter()
            .map(|document| {
                let mut indexable = IndexableDocument::new(&document.id, &document.content);
                if let Some(title) = &document.title {
                    indexable = indexable.with_title(title);
                }
                indexable
            })
            .collect::<Vec<_>>();

        let cx = Cx::for_testing();
        block_on(async {
            lexical_index
                .index_documents(&cx, &indexable_documents)
                .await
                .map_err(|error| format!("failed indexing frankensearch documents: {error}"))?;
            lexical_index
                .commit(&cx)
                .await
                .map_err(|error| format!("failed committing frankensearch index: {error}"))?;
            Ok::<(), String>(())
        })?;

        let semantic_embedder = HashEmbedder::new(
            SEMANTIC_EMBED_DIM,
            HashAlgorithm::JLProjection {
                seed: SEMANTIC_EMBED_SEED,
            },
        );

        let semantic_documents = sorted_documents
            .iter()
            .enumerate()
            .map(|(index, document)| SemanticDocument {
                index: u32::try_from(index).unwrap_or(u32::MAX),
                doc_id: document.id.clone(),
                embedding: semantic_embedder.embed_sync(&document.content),
            })
            .collect::<Vec<_>>();

        Ok(Self {
            lexical_index,
            semantic_documents,
            semantic_embedder,
        })
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<HybridHit>, String> {
        let normalized_query = query.trim();
        if normalized_query.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }

        let candidate_limit = self.semantic_documents.len().max(limit);
        let cx = Cx::for_testing();
        let lexical_hits = block_on(async {
            self.lexical_index
                .search(&cx, normalized_query, candidate_limit)
                .await
                .map_err(|error| format!("frankensearch BM25 query failed: {error}"))
        })?;
        let semantic_hits = self.semantic_hits(normalized_query);

        Ok(fuse_hits(&lexical_hits, &semantic_hits, limit))
    }

    pub fn support_for_query_default(&self, query: &str) -> Result<usize, String> {
        self.support_for_query(query, DEFAULT_SEMANTIC_SUPPORT_THRESHOLD)
    }

    pub fn support_for_query(&self, query: &str, semantic_threshold: f32) -> Result<usize, String> {
        let hits = self.search(query, self.semantic_documents.len())?;
        Ok(hits
            .into_iter()
            .filter(|hit| {
                hit.lexical_rank.is_some()
                    || hit
                        .semantic_score
                        .is_some_and(|score| score >= semantic_threshold)
            })
            .count())
    }

    fn semantic_hits(&self, query: &str) -> Vec<VectorHit> {
        let query_embedding = self.semantic_embedder.embed_sync(query);
        let mut hits = self
            .semantic_documents
            .iter()
            .filter_map(|document| {
                let score = cosine_similarity(&query_embedding, &document.embedding);
                (score.is_finite() && score > 0.0).then(|| VectorHit {
                    index: document.index,
                    score,
                    doc_id: document.doc_id.clone(),
                })
            })
            .collect::<Vec<_>>();

        hits.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| left.doc_id.cmp(&right.doc_id))
        });
        hits
    }
}

fn cosine_similarity(left: &[f32], right: &[f32]) -> f32 {
    left.iter().zip(right).map(|(lhs, rhs)| lhs * rhs).sum()
}

#[derive(Debug, Clone)]
struct FusionEntry {
    fused_score: f64,
    lexical_rank: Option<usize>,
    semantic_rank: Option<usize>,
    lexical_score: Option<f32>,
    semantic_score: Option<f32>,
}

fn fuse_hits(
    lexical_hits: &[ScoredResult],
    semantic_hits: &[VectorHit],
    limit: usize,
) -> Vec<HybridHit> {
    let mut entries: HashMap<&str, FusionEntry> =
        HashMap::with_capacity(lexical_hits.len() + semantic_hits.len());

    for (rank, hit) in lexical_hits.iter().enumerate() {
        let score = reciprocal_rank(rank);
        entries
            .entry(hit.doc_id.as_str())
            .and_modify(|entry| {
                entry.fused_score += score;
                entry.lexical_rank = Some(rank);
                entry.lexical_score = Some(hit.score);
            })
            .or_insert(FusionEntry {
                fused_score: score,
                lexical_rank: Some(rank),
                semantic_rank: None,
                lexical_score: Some(hit.score),
                semantic_score: None,
            });
    }

    for (rank, hit) in semantic_hits.iter().enumerate() {
        let score = reciprocal_rank(rank);
        entries
            .entry(hit.doc_id.as_str())
            .and_modify(|entry| {
                entry.fused_score += score;
                entry.semantic_rank = Some(rank);
                entry.semantic_score = Some(hit.score);
            })
            .or_insert(FusionEntry {
                fused_score: score,
                lexical_rank: None,
                semantic_rank: Some(rank),
                lexical_score: None,
                semantic_score: Some(hit.score),
            });
    }

    let mut hits = entries
        .into_iter()
        .map(|(doc_id, entry)| HybridHit {
            doc_id: doc_id.to_owned(),
            fused_score: entry.fused_score,
            lexical_rank: entry.lexical_rank,
            semantic_rank: entry.semantic_rank,
            lexical_score: entry.lexical_score,
            semantic_score: entry.semantic_score,
        })
        .collect::<Vec<_>>();

    hits.sort_by(|left, right| {
        right
            .fused_score
            .total_cmp(&left.fused_score)
            .then_with(|| {
                right
                    .lexical_score
                    .unwrap_or(f32::NEG_INFINITY)
                    .total_cmp(&left.lexical_score.unwrap_or(f32::NEG_INFINITY))
            })
            .then_with(|| left.doc_id.cmp(&right.doc_id))
    });
    if hits.len() > limit {
        hits.truncate(limit);
    }
    hits
}

fn reciprocal_rank(rank: usize) -> f64 {
    1.0 / (RRF_K + rank as f64 + 1.0)
}

#[cfg(test)]
mod tests {
    use super::{HybridSearcher, SearchDocument};

    fn fixture_documents() -> Vec<SearchDocument> {
        vec![
            SearchDocument {
                id: "doc-1".to_owned(),
                title: Some("Appraisal summary".to_owned()),
                content: "cap rate 6.25 tenant Example Co market rent roll".to_owned(),
            },
            SearchDocument {
                id: "doc-2".to_owned(),
                title: Some("Income approach".to_owned()),
                content: "income capitalization approach stabilized noi".to_owned(),
            },
            SearchDocument {
                id: "doc-3".to_owned(),
                title: Some("Lease abstract".to_owned()),
                content: "tenant roster suite 101 annual rent".to_owned(),
            },
        ]
    }

    #[test]
    fn search_results_are_deterministic() {
        let searcher = HybridSearcher::new(&fixture_documents()).expect("build searcher");

        let first = searcher.search("cap rate", 3).expect("run first search");
        let second = searcher.search("cap rate", 3).expect("run second search");

        assert_eq!(first, second);
    }

    #[test]
    fn hybrid_search_finds_expected_document() {
        let searcher = HybridSearcher::new(&fixture_documents()).expect("build searcher");
        let hits = searcher
            .search("cap rate 6.25", 3)
            .expect("run hybrid search");

        assert!(!hits.is_empty());
        assert_eq!(hits[0].doc_id, "doc-1");
        assert!(hits[0].lexical_rank.is_some() || hits[0].semantic_rank.is_some());
    }

    #[test]
    fn support_query_counts_matching_documents() {
        let searcher = HybridSearcher::new(&fixture_documents()).expect("build searcher");
        let support = searcher
            .support_for_query_default("tenant")
            .expect("support query");

        assert!(support >= 2);
    }
}
