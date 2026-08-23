//! Deterministic, index-only skill ranking.
//!
//! This lane intentionally ranks only fields present in the resident skill
//! index.  Skill bodies remain behind the owner resolver and never enter the
//! ranking input.

use membrane_provider_sdk::SkillCatalogEntry;
use std::collections::BTreeSet;

/// Ranking weights are fixed protocol policy, not learned or model-derived.
pub const EXACT_TERM_WEIGHT: f64 = 0.50;
pub const TAG_CAPABILITY_WEIGHT: f64 = 0.30;
pub const LEXICAL_WEIGHT: f64 = 0.20;

#[derive(Clone, Debug, PartialEq)]
pub struct RankedSkill {
    pub entry: SkillCatalogEntry,
    pub score: f64,
    pub exact_terms: usize,
    pub tag_capabilities: usize,
    pub lexical_matches: usize,
}

/// Rank one snapshot using exact ID terms, keyword tags/capabilities, and
/// lexical title/keyword matches.  Equal scores always use canonical ID order.
pub fn rank(
    task: &str,
    entries: &[SkillCatalogEntry],
    limit: usize,
) -> Vec<RankedSkill> {
    let query = tokens(task);
    let query_set: BTreeSet<String> = query.iter().cloned().collect();
    let mut ranked = entries
        .iter()
        .cloned()
        .map(|entry| {
            let id_terms = tokens(&entry.id);
            let keyword_terms: BTreeSet<String> = entry
                .keywords
                .iter()
                .flat_map(|value| tokens(value))
                .collect();
            let lexical_terms: BTreeSet<String> = tokens(&entry.title)
                .into_iter()
                .chain(keyword_terms.iter().cloned())
                .collect();

            let exact_terms = query
                .iter()
                .filter(|term| id_terms.iter().any(|candidate| candidate == *term))
                .count();
            let tag_capabilities = query
                .iter()
                .filter(|term| keyword_terms.contains(*term))
                .count();
            let lexical_matches = query
                .iter()
                .filter(|term| lexical_terms.iter().any(|candidate| candidate.contains(*term)))
                .count();
            let denominator = query_set.len().max(1) as f64;
            let score = (EXACT_TERM_WEIGHT * exact_terms as f64 / denominator
                + TAG_CAPABILITY_WEIGHT * tag_capabilities as f64 / denominator
                + LEXICAL_WEIGHT * lexical_matches as f64 / denominator)
                .clamp(0.0, 1.0);
            RankedSkill {
                entry,
                score,
                exact_terms,
                tag_capabilities,
                lexical_matches,
            }
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.entry.id.cmp(&right.entry.id))
    });
    ranked.truncate(limit);
    ranked
}

fn tokens(value: &str) -> Vec<String> {
    value
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(|part| part.to_ascii_lowercase())
        .collect()
}
