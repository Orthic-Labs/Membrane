//! Quality gate that filters low-quality memory candidates before promotion/injection.

use crate::types::MemoryEntry;

/// Configuration for [`MemoryRetrievalEvalGate`].
#[derive(Debug, Clone)]
pub struct EvalGateConfig {
    /// Minimum stored score required to pass.
    pub min_score: f64,
    /// Minimum content length (in chars) required to pass.
    pub min_content_len: usize,
    /// Maximum number of entries to allow through.
    pub max_entries: usize,
}

impl Default for EvalGateConfig {
    fn default() -> Self {
        Self {
            min_score: 0.3,
            min_content_len: 8,
            max_entries: 10,
        }
    }
}

/// Filters candidate entries against an [`EvalGateConfig`].
pub struct MemoryRetrievalEvalGate {
    config: EvalGateConfig,
}

impl MemoryRetrievalEvalGate {
    pub fn new(config: EvalGateConfig) -> Self {
        Self { config }
    }

    /// Predicate for whether a single entry passes the quality bar.
    pub fn passes(&self, entry: &MemoryEntry) -> bool {
        entry.score >= self.config.min_score
            && entry.content.chars().count() >= self.config.min_content_len
    }

    /// Filter candidates, dropping those that fail the quality bar and truncating
    /// to `max_entries`. Input order is preserved.
    pub fn filter<'a>(&self, candidates: Vec<&'a MemoryEntry>) -> Vec<&'a MemoryEntry> {
        candidates
            .into_iter()
            .filter(|e| self.passes(e))
            .take(self.config.max_entries)
            .collect()
    }
}

impl Default for MemoryRetrievalEvalGate {
    fn default() -> Self {
        Self::new(EvalGateConfig::default())
    }
}
