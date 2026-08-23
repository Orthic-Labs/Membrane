//! Content-free accounting for a deterministic compaction projection.
//!
//! The receipt is deliberately separate from the projection.  Canonical
//! history stays in its source store; this value records the read-time view,
//! its cursor, and every omission or fallback decision.

use serde::{Deserialize, Serialize};

pub const COMPACTION_RECEIPT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompactionSourceCursor {
    pub session_id: String,
    pub last_seq: u64,
}

impl Default for CompactionSourceCursor {
    fn default() -> Self {
        Self {
            session_id: String::new(),
            last_seq: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CacheImpactV1 {
    pub cache_hit: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_key: Option<String>,
    pub reused_tokens: u32,
    pub invalidated: bool,
}

impl Default for CacheImpactV1 {
    fn default() -> Self {
        Self {
            cache_hit: false,
            cache_key: None,
            reused_tokens: 0,
            invalidated: false,
        }
    }
}

/// Exact accounting emitted with each compacted read-time projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompactionReceiptV1 {
    pub schema_version: u32,
    pub source_cursor: CompactionSourceCursor,
    pub projection_hash: String,
    pub retained_obligations: Vec<String>,
    pub omitted_categories: Vec<String>,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub budget_tokens: u32,
    pub budget_met: bool,
    pub fallback_used: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_provider: Option<String>,
    pub cache_impact: CacheImpactV1,
}

impl CompactionReceiptV1 {
    pub const SCHEMA_VERSION: u32 = COMPACTION_RECEIPT_SCHEMA_VERSION;
}
