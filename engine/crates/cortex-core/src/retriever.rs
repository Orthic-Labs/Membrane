//! Ranks memory entries against a query.

use std::collections::HashMap;

use crate::embed::cosine;
use crate::quant::QuantizedVector;
use crate::registry::MemoryRegistry;
use crate::types::MemoryEntry;

#[path = "lexical.rs"]
mod lexical;
pub use lexical::LexicalHit;

/// Ranks entries in a [`MemoryRegistry`] against a textual query.
pub struct MemoryRetriever;

/// Reciprocal-rank-fusion constant. Larger = flatter weighting across ranks.
const RRF_K: f64 = 60.0;

impl MemoryRetriever {
    /// Tokenize a query into lowercased whitespace-separated terms.
    fn query_terms(query: &str) -> Vec<String> {
        lexical::query_terms(query)
    }

    /// Compute the rank score for a single entry against pre-tokenized query terms.
    ///
    /// `rank = (keyword_matches * 2.0) + content_substring_hits + entry.score`.
    pub fn score_entry(entry: &MemoryEntry, query_terms: &[String]) -> f64 {
        lexical::fallback_score(entry, query_terms)
    }

    /// Fuse pre-ranked FTS5 lexical hits with the existing semantic lane.
    /// Qualification is applied before ranking; unknown or stale projection
    /// rows are ignored, allowing callers to rebuild or use the fallback lane.
    pub fn retrieve_hybrid_with_lexical_hits<'a>(
        registry: &'a MemoryRegistry,
        lexical_hits: &[LexicalHit],
        query_embedding: Option<&[f32]>,
        limit: usize,
        eligible_scopes: Option<&[&str]>,
    ) -> Vec<&'a MemoryEntry> {
        let entries = registry
            .all()
            .into_iter()
            .filter(|entry| {
                eligible_scopes.is_none_or(|scopes| scopes.contains(&entry.scope_id.as_str()))
            })
            .collect::<Vec<_>>();
        if entries.is_empty() || limit == 0 {
            return Vec::new();
        }
        let lexical_rank = lexical::rank_hits(&entries, lexical_hits);
        let mut semantic = query_embedding.map(|query| {
            let mut ranked = entries
                .iter()
                .map(|entry| {
                    let score = entry
                        .embedding
                        .as_deref()
                        .map(|embedding| cosine(embedding, query) as f64)
                        .unwrap_or(0.0);
                    (entry.id.as_str(), score)
                })
                .collect::<Vec<_>>();
            ranked.sort_by(|left, right| {
                right
                    .1
                    .total_cmp(&left.1)
                    .then_with(|| left.0.cmp(right.0))
            });
            ranked
                .into_iter()
                .enumerate()
                .map(|(rank, (entry_id, score))| (entry_id, (rank, score)))
                .collect::<HashMap<_, _>>()
        });
        let mut ranked = entries
            .into_iter()
            .filter_map(|entry| {
                let mut score = 0.05 * entry.score.clamp(0.0, 1.0);
                if let Some((rank, lexical_score)) = lexical_rank.get(entry.id.as_str()) {
                    if *lexical_score > 0.0 {
                        score += 1.0 / (RRF_K + *rank as f64);
                    }
                }
                if let Some(semantic_rank) = semantic.as_mut() {
                    if let Some((rank, semantic_score)) = semantic_rank.get(entry.id.as_str()) {
                        if *semantic_score > 0.0 {
                            score += 1.0 / (RRF_K + *rank as f64);
                        }
                    }
                }
                (score > 0.0).then_some((entry, score))
            })
            .collect::<Vec<_>>();
        ranked.sort_by(|left, right| {
            right
                .1
                .total_cmp(&left.1)
                .then_with(|| left.0.id.cmp(&right.0.id))
        });
        ranked
            .into_iter()
            .take(limit)
            .map(|(entry, _)| entry)
            .collect()
    }

    /// Retrieve the top `limit` entries ranked by relevance to `query`.
    ///
    /// Entries with zero matches AND zero stored score are excluded.
    pub fn retrieve<'a>(
        registry: &'a MemoryRegistry,
        query: &str,
        limit: usize,
    ) -> Vec<&'a MemoryEntry> {
        let terms = Self::query_terms(query);

        let mut scored: Vec<(f64, &MemoryEntry)> = registry
            .all()
            .into_iter()
            .filter_map(|e| {
                let rank = Self::score_entry(e, &terms);
                // Exclude entries with zero matches and zero score. A non-zero rank
                // means there was at least one match or a positive stored score.
                if rank > 0.0 {
                    Some((rank, e))
                } else {
                    None
                }
            })
            .collect();

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.into_iter().take(limit).map(|(_, e)| e).collect()
    }

    /// Hybrid retrieval: fuse semantic similarity (cosine vs `query_embedding`)
    /// with the lexical keyword score via Reciprocal Rank Fusion, then nudge by
    /// the entry's stored outcome score. This is strictly better than either
    /// signal alone — semantic finds meaning-similar memories that share no
    /// keywords, lexical anchors exact-term matches, and the outcome weight lets
    /// a memory from a *successful* run outrank a similar one from a failure.
    ///
    /// Falls back to pure lexical [`retrieve`](Self::retrieve) when no query
    /// embedding is supplied (e.g. the embedder is unavailable).
    pub fn retrieve_hybrid<'a>(
        registry: &'a MemoryRegistry,
        query: &str,
        query_embedding: Option<&[f32]>,
        limit: usize,
    ) -> Vec<&'a MemoryEntry> {
        let Some(qvec) = query_embedding else {
            return Self::retrieve(registry, query, limit);
        };
        let entries = registry.all();
        let lexical = lexical::fallback_hits(&entries, query);
        Self::retrieve_hybrid_with_lexical_hits(registry, &lexical, Some(qvec), limit, None)
    }

    /// Hybrid retrieval using the registry's resident contiguous vector index.
    /// Scope filtering is applied as an eligibility mask without cloning entries
    /// into temporary registries. Any index/query mismatch fails closed to the
    /// scalar reference path.
    pub fn retrieve_hybrid_indexed<'a>(
        registry: &'a MemoryRegistry,
        query: &str,
        query_embedding: Option<&[f32]>,
        limit: usize,
        eligible_scopes: Option<&[&str]>,
    ) -> Vec<&'a MemoryEntry> {
        let Some(qvec) = query_embedding else {
            return Self::retrieve_filtered(registry, query, limit, eligible_scopes);
        };
        let scope_matches = |entry: &&MemoryEntry| {
            eligible_scopes.is_none_or(|scopes| scopes.contains(&entry.scope_id.as_str()))
        };
        let entries = registry
            .all()
            .into_iter()
            .filter(scope_matches)
            .collect::<Vec<_>>();
        if entries.is_empty() || limit == 0 {
            return Vec::new();
        }

        let terms = Self::query_terms(query);
        let mut lexical = entries
            .iter()
            .map(|entry| (entry.id.as_str(), Self::score_entry(entry, &terms)))
            .collect::<Vec<_>>();
        lexical.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        let lexical_rank = lexical
            .iter()
            .enumerate()
            .map(|(rank, (id, score))| (*id, (rank, *score)))
            .collect::<HashMap<_, _>>();

        let semantic_limit = limit.saturating_mul(32).max(128).min(entries.len());
        let Some(index) = registry.vector_index() else {
            return Self::retrieve_hybrid_filtered_scalar(
                registry,
                query,
                qvec,
                limit,
                eligible_scopes,
            );
        };
        let semantic = match index.top_k(qvec, eligible_scopes, semantic_limit) {
            Ok(candidates) => candidates,
            Err(_) => {
                return Self::retrieve_hybrid_filtered_scalar(
                    registry,
                    query,
                    qvec,
                    limit,
                    eligible_scopes,
                )
            }
        };
        let semantic_rank = semantic
            .iter()
            .enumerate()
            .map(|(rank, candidate)| (candidate.id.as_str(), (rank, candidate.score)))
            .collect::<HashMap<_, _>>();

        let mut fused = entries
            .into_iter()
            .filter_map(|entry| {
                let mut score = 0.05 * entry.score.clamp(0.0, 1.0);
                if let Some((rank, lexical_score)) = lexical_rank.get(entry.id.as_str()) {
                    if *lexical_score > 0.0 {
                        score += 1.0 / (RRF_K + *rank as f64);
                    }
                }
                if let Some((rank, semantic_score)) = semantic_rank.get(entry.id.as_str()) {
                    if *semantic_score > 0.0 {
                        score += 1.0 / (RRF_K + *rank as f64);
                    }
                }
                (score > 0.0).then_some((entry, score))
            })
            .collect::<Vec<_>>();
        fused.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.id.cmp(&b.0.id)));
        fused
            .into_iter()
            .take(limit)
            .map(|(entry, _)| entry)
            .collect()
    }

    fn retrieve_filtered<'a>(
        registry: &'a MemoryRegistry,
        query: &str,
        limit: usize,
        eligible_scopes: Option<&[&str]>,
    ) -> Vec<&'a MemoryEntry> {
        let terms = Self::query_terms(query);
        let mut scored = registry
            .all()
            .into_iter()
            .filter(|entry| {
                eligible_scopes.is_none_or(|scopes| scopes.contains(&entry.scope_id.as_str()))
            })
            .filter_map(|entry| {
                let score = Self::score_entry(entry, &terms);
                (score > 0.0).then_some((entry, score))
            })
            .collect::<Vec<_>>();
        scored.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.id.cmp(&b.0.id)));
        scored
            .into_iter()
            .take(limit)
            .map(|(entry, _)| entry)
            .collect()
    }

    fn retrieve_hybrid_filtered_scalar<'a>(
        registry: &'a MemoryRegistry,
        query: &str,
        query_embedding: &[f32],
        limit: usize,
        eligible_scopes: Option<&[&str]>,
    ) -> Vec<&'a MemoryEntry> {
        let terms = Self::query_terms(query);
        let entries = registry
            .all()
            .into_iter()
            .filter(|entry| {
                eligible_scopes.is_none_or(|scopes| scopes.contains(&entry.scope_id.as_str()))
            })
            .collect::<Vec<_>>();
        let mut lexical = entries
            .iter()
            .enumerate()
            .map(|(index, entry)| (index, Self::score_entry(entry, &terms)))
            .collect::<Vec<_>>();
        lexical.sort_by(|a, b| b.1.total_cmp(&a.1));
        let mut semantic = entries
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let score = entry
                    .embedding
                    .as_deref()
                    .map(|embedding| cosine(embedding, query_embedding) as f64)
                    .unwrap_or(0.0);
                (index, score)
            })
            .collect::<Vec<_>>();
        semantic.sort_by(|a, b| b.1.total_cmp(&a.1));
        let mut fused = vec![0.0_f64; entries.len()];
        for (rank, (index, score)) in lexical.into_iter().enumerate() {
            if score > 0.0 {
                fused[index] += 1.0 / (RRF_K + rank as f64);
            }
        }
        for (rank, (index, score)) in semantic.into_iter().enumerate() {
            if score > 0.0 {
                fused[index] += 1.0 / (RRF_K + rank as f64);
            }
        }
        let mut ranked = entries
            .into_iter()
            .enumerate()
            .filter_map(|(index, entry)| {
                let score = fused[index] + 0.05 * entry.score.clamp(0.0, 1.0);
                (score > 0.0).then_some((entry, score))
            })
            .collect::<Vec<_>>();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.id.cmp(&b.0.id)));
        ranked
            .into_iter()
            .take(limit)
            .map(|(entry, _)| entry)
            .collect()
    }

    /// Hybrid retrieval using i8-quantized stored embeddings for the semantic
    /// rank list. This keeps the same fusion behavior as
    /// [`retrieve_hybrid`](Self::retrieve_hybrid) while exercising the compact
    /// TurboQuant/turbovec scoring path.
    pub fn retrieve_hybrid_quantized<'a>(
        registry: &'a MemoryRegistry,
        query: &str,
        query_embedding: Option<&[f32]>,
        limit: usize,
    ) -> Vec<&'a MemoryEntry> {
        let Some(qvec) = query_embedding else {
            return Self::retrieve(registry, query, limit);
        };
        let terms = Self::query_terms(query);
        let entries = registry.all();

        let mut lexical: Vec<(usize, f64)> = entries
            .iter()
            .enumerate()
            .map(|(i, e)| (i, Self::score_entry(e, &terms)))
            .collect();
        lexical.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut semantic: Vec<(usize, f64)> = entries
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let sim = e
                    .embedding
                    .as_deref()
                    .map(|emb| QuantizedVector::quantize(emb).cosine_with(qvec) as f64)
                    .unwrap_or(0.0);
                (i, sim)
            })
            .collect();
        semantic.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut fused: Vec<(usize, f64)> = (0..entries.len()).map(|i| (i, 0.0)).collect();
        for (rank, (idx, score)) in lexical.iter().enumerate() {
            if *score > 0.0 {
                fused[*idx].1 += 1.0 / (RRF_K + rank as f64);
            }
        }
        for (rank, (idx, sim)) in semantic.iter().enumerate() {
            if *sim > 0.0 {
                fused[*idx].1 += 1.0 / (RRF_K + rank as f64);
            }
        }
        for (idx, fscore) in fused.iter_mut() {
            *fscore += 0.05 * entries[*idx].score.clamp(0.0, 1.0);
        }

        let mut ranked: Vec<(usize, f64)> = fused.into_iter().filter(|(_, s)| *s > 0.0).collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        ranked
            .into_iter()
            .take(limit)
            .map(|(i, _)| entries[i])
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::MemoryTier;

    fn entry(
        id: &str,
        content: &str,
        keywords: &[&str],
        score: f64,
        embedding: Option<Vec<f32>>,
    ) -> MemoryEntry {
        MemoryEntry {
            id: id.to_string(),
            tier: MemoryTier::Working,
            content: content.to_string(),
            keywords: keywords.iter().map(|keyword| keyword.to_string()).collect(),
            score,
            created_at: "2026-06-28T00:00:00Z".to_string(),
            access_count: 0,
            embedding,
            scope_id: crate::default_scope(),
        }
    }

    #[test]
    fn query_terms_lowercase_and_split_whitespace() {
        let terms = MemoryRetriever::query_terms("  Rust\nASYNC\tTokio  ");

        assert_eq!(terms, vec!["rust", "async", "tokio"]);
    }

    #[test]
    fn score_entry_counts_keywords_case_insensitively_and_content_hits() {
        let entry = entry(
            "case",
            "Tokio runtime and async tasks",
            &["Rust", "ASYNC"],
            0.25,
            None,
        );
        let terms = vec!["rust".to_string(), "async".to_string()];

        assert_eq!(MemoryRetriever::score_entry(&entry, &terms), 5.25);
    }

    #[test]
    fn retrieve_keeps_positive_stored_score_without_query_match() {
        let mut registry = MemoryRegistry::new();
        registry.insert(entry("scored", "apples bananas", &[], 0.4, None));
        registry.insert(entry("zero", "oranges pears", &[], 0.0, None));

        let results = MemoryRetriever::retrieve(&registry, "rust", 10);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "scored");
    }

    #[test]
    fn hybrid_filters_entries_with_no_lexical_semantic_or_outcome_signal() {
        let mut registry = MemoryRegistry::new();
        registry.insert(entry("zero", "apples bananas", &[], 0.0, None));
        let query_embedding = vec![1.0, 0.0];

        let results =
            MemoryRetriever::retrieve_hybrid(&registry, "rust", Some(&query_embedding), 10);

        assert!(results.is_empty());
    }

    #[test]
    fn hybrid_quantized_falls_back_to_lexical_without_query_embedding() {
        let mut registry = MemoryRegistry::new();
        registry.insert(entry(
            "kw",
            "plain text",
            &["rust"],
            0.0,
            Some(vec![0.0, 1.0]),
        ));

        let results = MemoryRetriever::retrieve_hybrid_quantized(&registry, "rust", None, 10);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "kw");
    }

    #[test]
    fn indexed_hybrid_preserves_reference_top_hit() {
        let mut registry = MemoryRegistry::new_indexed();
        registry.insert(entry(
            "semantic",
            "deployment notes",
            &[],
            0.0,
            Some(vec![1.0, 0.0, 0.0]),
        ));
        registry.insert(entry(
            "other",
            "unrelated",
            &[],
            0.0,
            Some(vec![0.0, 1.0, 0.0]),
        ));
        let query = [1.0, 0.0, 0.0];

        let reference =
            MemoryRetriever::retrieve_hybrid(&registry, "nothing lexical", Some(&query), 2);
        let indexed = MemoryRetriever::retrieve_hybrid_indexed(
            &registry,
            "nothing lexical",
            Some(&query),
            2,
            None,
        );

        assert_eq!(indexed[0].id, reference[0].id);
        assert_eq!(indexed[0].id, "semantic");
    }

    #[test]
    fn indexed_hybrid_applies_scope_without_temporary_registry() {
        let mut registry = MemoryRegistry::new_indexed();
        let mut excluded = entry("excluded", "rust", &["rust"], 1.0, Some(vec![1.0, 0.0]));
        excluded.scope_id = "other".into();
        registry.insert(excluded);
        let mut included = entry("included", "rust", &["rust"], 0.0, Some(vec![0.8, 0.2]));
        included.scope_id = "wanted".into();
        registry.insert(included);
        let query = [1.0, 0.0];

        let hits = MemoryRetriever::retrieve_hybrid_indexed(
            &registry,
            "rust",
            Some(&query),
            10,
            Some(&["wanted"]),
        );

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "included");
    }
}
