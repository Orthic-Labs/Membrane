//! Native port of `blueprint/src/graph/confidence-tiers.mjs`.
//!
//! Per-edge confidence TIER vocabulary (blueprint B3). Every edge emitted
//! by any graph provider carries a `confidenceTier` alongside its numeric
//! `confidence`. The tier is derived from how the edge was actually
//! resolved at the call site that built it — never hardcoded per provider,
//! never a subjective score. The numeric confidence is a pure function of
//! the tier, so two edges in the same tier always carry the same number.
//!
//! This module is an independent, string-keyed vocabulary (it does not
//! reuse `graph::ConfidenceTier`, which is graph.rs's own internal enum and
//! is off-limits to this port) so that any JSON edge carrying a
//! `confidenceTier` string — from any provider, Rust or legacy — can be
//! ranked and filtered the same way the legacy JS callers did.

use serde_json::Value;

pub const EXACT_RESOLUTION: &str = "EXACT_RESOLUTION";
pub const SAME_FILE_LEXICAL: &str = "SAME_FILE_LEXICAL";
pub const CROSS_FILE_HEURISTIC: &str = "CROSS_FILE_HEURISTIC";
pub const UNRESOLVED: &str = "UNRESOLVED";

/// Most -> least certain, index 0 is the highest tier.
pub const EDGE_CONFIDENCE_TIER_ORDER: [&str; 4] =
    [EXACT_RESOLUTION, SAME_FILE_LEXICAL, CROSS_FILE_HEURISTIC, UNRESOLVED];

pub fn tier_description(tier: &str) -> Option<&'static str> {
    match tier {
        EXACT_RESOLUTION => Some(
            "Deterministic resolution — resolved import path, structural AST relationship, or compiler/indexer-backed (SCIP) symbol reference.",
        ),
        SAME_FILE_LEXICAL => Some(
            "Name match bounded to the calling file's own symbol table — not a resolved binding, but the file boundary rules out cross-module collisions.",
        ),
        CROSS_FILE_HEURISTIC => Some(
            "Name match that crosses file boundaries via an already-resolved import link, a repo-wide unique-name fallback, or a string-literal-to-filename match.",
        ),
        UNRESOLVED => Some(
            "No target could be resolved. Tagged and kept (target=null) — never silently dropped, never promoted.",
        ),
        _ => None,
    }
}

#[derive(Debug)]
pub struct UnknownTierError(pub String);

impl std::fmt::Display for UnknownTierError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown edge confidence tier: {}", self.0)
    }
}
impl std::error::Error for UnknownTierError {}

/// Numeric confidence is a pure function of tier. These are NOT subjective
/// scores tuned per finding — they exist only so numeric ranking/filtering
/// still works for consumers that pre-date the tier field.
pub fn tier_confidence(tier: &str) -> Result<f64, UnknownTierError> {
    match tier {
        EXACT_RESOLUTION => Ok(1.0),
        SAME_FILE_LEXICAL => Ok(0.75),
        CROSS_FILE_HEURISTIC => Ok(0.5),
        UNRESOLVED => Ok(0.0),
        other => Err(UnknownTierError(other.to_string())),
    }
}

/// True when `tier` is at least as certain as `min_tier` (lower index = more certain).
pub fn is_tier_at_least(tier: &str, min_tier: &str) -> Result<bool, UnknownTierError> {
    let tier_index = EDGE_CONFIDENCE_TIER_ORDER
        .iter()
        .position(|t| *t == tier)
        .ok_or_else(|| UnknownTierError(tier.to_string()))?;
    let min_index = EDGE_CONFIDENCE_TIER_ORDER
        .iter()
        .position(|t| *t == min_tier)
        .ok_or_else(|| UnknownTierError(min_tier.to_string()))?;
    Ok(tier_index <= min_index)
}

/// Filter a slice of JSON edge objects (each expected to carry a
/// `confidenceTier` string field) to those at least as certain as
/// `min_tier`. Edges with no (or an unknown) tier are dropped, mirroring
/// the legacy `edge.confidenceTier &&` guard.
pub fn filter_edges_by_min_tier<'a>(edges: &'a [Value], min_tier: &str) -> Vec<&'a Value> {
    edges
        .iter()
        .filter(|edge| {
            edge.get("confidenceTier")
                .and_then(Value::as_str)
                .map(|tier| is_tier_at_least(tier, min_tier).unwrap_or(false))
                .unwrap_or(false)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tier_confidence_is_a_pure_function_of_tier() {
        assert_eq!(tier_confidence(EXACT_RESOLUTION).unwrap(), 1.0);
        assert_eq!(tier_confidence(SAME_FILE_LEXICAL).unwrap(), 0.75);
        assert_eq!(tier_confidence(CROSS_FILE_HEURISTIC).unwrap(), 0.5);
        assert_eq!(tier_confidence(UNRESOLVED).unwrap(), 0.0);
        assert!(tier_confidence("bogus").is_err());
    }

    #[test]
    fn is_tier_at_least_orders_most_to_least_certain() {
        assert!(is_tier_at_least(EXACT_RESOLUTION, CROSS_FILE_HEURISTIC).unwrap());
        assert!(is_tier_at_least(SAME_FILE_LEXICAL, SAME_FILE_LEXICAL).unwrap());
        assert!(!is_tier_at_least(UNRESOLVED, EXACT_RESOLUTION).unwrap());
        assert!(is_tier_at_least(CROSS_FILE_HEURISTIC, UNRESOLVED).unwrap());
        assert!(is_tier_at_least("bogus", UNRESOLVED).is_err());
        assert!(is_tier_at_least(UNRESOLVED, "bogus").is_err());
    }

    #[test]
    fn filter_edges_by_min_tier_keeps_only_sufficiently_certain_edges() {
        let edges = vec![
            json!({"confidenceTier": EXACT_RESOLUTION}),
            json!({"confidenceTier": SAME_FILE_LEXICAL}),
            json!({"confidenceTier": CROSS_FILE_HEURISTIC}),
            json!({"confidenceTier": UNRESOLVED}),
            json!({"confidenceTier": Value::Null}),
            json!({}),
        ];
        let kept = filter_edges_by_min_tier(&edges, SAME_FILE_LEXICAL);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0]["confidenceTier"], json!(EXACT_RESOLUTION));
        assert_eq!(kept[1]["confidenceTier"], json!(SAME_FILE_LEXICAL));
    }

    #[test]
    fn descriptions_cover_every_tier_in_the_order() {
        for tier in EDGE_CONFIDENCE_TIER_ORDER {
            assert!(tier_description(tier).is_some(), "missing description for {tier}");
        }
        assert!(tier_description("bogus").is_none());
    }
}
