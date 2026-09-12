//! Agent-to-Membrane durable memory write contract.
//!
//! This operation is deliberately separate from Pull's packet reduction
//! contracts. The caller supplies bytes; Cortex owns admission, provenance,
//! lifecycle, and derived representations.

use serde::{Deserialize, Serialize};

pub const AGENT_MEMORY_PUSH_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentMemoryPushRequestV1 {
    pub schema_version: u32,
    /// Stable caller-provided idempotency key. It is scoped to caller scope.
    pub request_id: String,
    /// UTF-8 JSON transport representation of the exact submitted body.
    pub body: String,
    pub repository_id: String,
    pub scope_id: String,
    pub caller_id: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub lifecycle: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentMemoryPushResponseV1 {
    pub schema_version: u32,
    pub memory_id: String,
    pub content_hash: String,
    pub raw_body_bytes: usize,
    pub authority: String,
    pub provenance: serde_json::Value,
    pub lifecycle: serde_json::Value,
    pub status: String,
}
