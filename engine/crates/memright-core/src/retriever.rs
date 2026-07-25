//! Ranks memory entries against a query.

use crate::embed::cosine;
use crate::quant::QuantizedVector;
use crate::registry::MemoryRegistry;
use crate::types::MemoryEntry;

/// Ranks entries in a [`MemoryRegistry`] against a textual query.
pub struct MemoryRetriever;

/// Reciprocal-rank-fusion constant. Larger = flatter weighting across ranks.
const RRF_K: f64 = 60.0;

impl MemoryRetriever {
    /// Tokenize a query into lowercased whitespace-separated terms.
    fn query_terms(query: &str) -> Vec<String> {
        query
            .to_lowercase()
            .split_whitespace()
            .map(|t| t.to_string())
            .collect()
    }

    /// Compute the rank score for a single entry against pre-tokenized query terms.
    ///
    /// `rank = (keyword_matches * 2.0) + content_substring_hits + entry.score`.
    pub fn score_entry(entry: &MemoryEntry, query_terms: &[String]) -> f64 {
        let entry_keywords: Vec<String> = entry.keywords.iter().map(|k| k.to_lowercase()).collect();
        let content_lower = entry.content.to_lowercase();

        let mut keyword_matches = 0.0_f64;
        let mut content_hits = 0.0_f64;

        for term in query_terms {
            if entry_keywords.iter().any(|k| k == term) {
                keyword_matches += 1.0;
            }
            if content_lower.contains(term.as_str()) {
                content_hits += 1.0;
            }
        }

        (keyword_matches * 2.0) + content_hits + entry.score
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
        let terms = Self::query_terms(query);
        let entries = registry.all();

        // Rank list 1: lexical (keyword + content + stored score).
        let mut lexical: Vec<(usize, f64)> = entries
            .iter()
            .enumerate()
            .map(|(i, e)| (i, Self::score_entry(e, &terms)))
            .collect();
        lexical.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Rank list 2: semantic (cosine vs the query embedding).
        let mut semantic: Vec<(usize, f64)> = entries
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let sim = e
                    .embedding
                    .as_deref()
                    .map(|emb| cosine(emb, qvec) as f64)
                    .unwrap_or(0.0);
                (i, sim)
            })
            .collect();
        semantic.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Reciprocal Rank Fusion: rank position (not raw score) feeds 1/(k+rank).
        // A signal only contributes if the entry actually scored on it.
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
        // Outcome nudge: stored score in [0,1] gently lifts proven memories.
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
}
