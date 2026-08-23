//! Lexical result adapter shared by Cortex retrieval lanes.
//!
//! Storage owns FTS5 execution.  Core owns qualification-neutral conversion
//! into deterministic rank inputs and keeps an in-memory fallback available
//! when the projection is missing or degraded.

use std::collections::HashMap;

use crate::types::MemoryEntry;

/// A storage-independent lexical result.
#[derive(Debug, Clone, PartialEq)]
pub struct LexicalHit {
    pub record_id: String,
    pub score: f64,
}

impl LexicalHit {
    pub fn new(record_id: impl Into<String>, score: f64) -> Self {
        Self {
            record_id: record_id.into(),
            score,
        }
    }
}

/// Lowercase terms used by the deterministic fallback lane.
pub fn query_terms(query: &str) -> Vec<String> {
    query
        .to_lowercase()
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

/// Preserve the pre-FTS lexical control for typed degradation and old callers.
pub fn fallback_score(entry: &MemoryEntry, terms: &[String]) -> f64 {
    let entry_keywords = entry
        .keywords
        .iter()
        .map(|keyword| keyword.to_lowercase())
        .collect::<Vec<_>>();
    let content = entry.content.to_lowercase();
    let mut keyword_matches = 0.0;
    let mut content_hits = 0.0;
    for term in terms {
        if entry_keywords.iter().any(|keyword| keyword == term) {
            keyword_matches += 1.0;
        }
        if content.contains(term) {
            content_hits += 1.0;
        }
    }
    keyword_matches * 2.0 + content_hits + entry.score
}

/// Convert FTS hits into rank positions for RRF.  Unknown/deleted records are
/// discarded, while ties are stable by record ID.
pub fn rank_hits(entries: &[&MemoryEntry], hits: &[LexicalHit]) -> HashMap<String, (usize, f64)> {
    let known = entries
        .iter()
        .map(|entry| entry.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    let mut ordered = hits
        .iter()
        .filter(|hit| hit.score > 0.0 && known.contains(hit.record_id.as_str()))
        .collect::<Vec<_>>();
    ordered.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.record_id.cmp(&right.record_id))
    });
    ordered
        .into_iter()
        .enumerate()
        .map(|(rank, hit)| (hit.record_id.clone(), (rank, hit.score)))
        .collect()
}

/// Build lexical hits from the fallback control, retaining deterministic IDs.
pub fn fallback_hits(entries: &[&MemoryEntry], query: &str) -> Vec<LexicalHit> {
    let terms = query_terms(query);
    let mut hits = entries
        .iter()
        .map(|entry| LexicalHit::new(&entry.id, fallback_score(entry, &terms)))
        .filter(|hit| hit.score > 0.0)
        .collect::<Vec<_>>();
    hits.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.record_id.cmp(&right.record_id))
    });
    hits
}
