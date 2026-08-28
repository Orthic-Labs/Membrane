//! Receipt shapes for deterministic cross-provider fusion.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One candidate's deterministic fusion decision. This receipt deliberately
/// records rank-derived data, never a cross-provider interpretation of a
/// provider-local relevance score.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FusionDecisionV1 {
    pub id: String,
    pub provider: String,
    pub provider_rank: u32,
    pub rrf_denominator: u32,
    /// Position after reciprocal-rank contributions are fused. Quota drops
    /// have no fused position because they never enter fusion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fused_rank: Option<u32>,
    pub decision: String,
    pub reason: String,
}

/// Content-free audit record for one selected deterministic fusion pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FusionReceiptV1 {
    pub schema_version: u32,
    pub policy: String,
    pub fallback_policy: String,
    /// Provider ids, sorted bytewise before lane ranking.
    pub provider_order: Vec<String>,
    pub provider_quotas: BTreeMap<String, u32>,
    pub rrf_k: u32,
    pub max_items: u32,
    pub candidates_received: u32,
    pub candidates_selected: u32,
    pub decisions: Vec<FusionDecisionV1>,
}

impl FusionReceiptV1 {
    pub const SCHEMA_VERSION: u32 = 1;
    /// Versioned production control strategy.  Federation uses this policy
    /// unless an explicit RRF strategy is selected by its caller.
    pub const POLICY: &'static str = "membrane-fusion-fixed-v1";
    /// Versioned RRF strategy used by the standalone core implementation and
    /// available to explicitly selected or shadow-evaluation paths.
    pub const RRF_POLICY: &'static str = "membrane-fusion-rrf-v1";
    pub const FALLBACK_POLICY: &'static str = "fixed-lanes-v1";
}
