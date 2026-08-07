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
    pub decision: String,
    pub reason: String,
}

/// Content-free audit record for one fixed-lane fusion pass.
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
    pub const POLICY: &'static str = "membrane-fusion-fixed-v1";
    pub const FALLBACK_POLICY: &'static str = "fixed-lanes-v1";
}
