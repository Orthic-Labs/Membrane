//! Content-free, bounded projection for the Memory Sentinel Hub view.
//! This module only reads an already assembled report; it never mutates data-plane state.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const SCHEMA_VERSION: u32 = 1;
pub const MAX_ITEMS: usize = 64;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SentinelCounts {
    pub active: Option<u64>,
    pub demoted: Option<u64>,
    pub superseded: Option<u64>,
    pub expired: Option<u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SentinelList {
    pub count: Option<u64>,
    pub ids: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySentinelView {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    pub lifecycle: SentinelCounts,
    pub proposals: SentinelList,
    pub contradictions: SentinelList,
    pub scratchpad: SentinelList,
    pub working: SentinelList,
    #[serde(rename = "taskCriteria")]
    pub task_criteria: SentinelList,
    pub evidence: EvidenceState,
    pub gate: GateState,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceState {
    pub state: String,
    pub valid: bool,
    pub reason: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateState {
    pub state: String,
    pub reason: String,
    pub authoritative: bool,
}

fn list(value: Option<&Value>) -> SentinelList {
    let ids = value
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .take(MAX_ITEMS)
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    SentinelList {
        count: value.and_then(Value::as_array).map(|a| a.len() as u64),
        ids,
    }
}
/// Project report into a bounded, content-free read model. Missing evidence remains unknown.
pub fn project(report: &Value) -> MemorySentinelView {
    let data = report
        .get("result")
        .and_then(|v| v.get("data"))
        .unwrap_or(report);
    let lifecycle = data.get("lifecycle").cloned().unwrap_or_else(|| json!({}));
    let state = |key: &str| lifecycle.get(key).and_then(Value::as_u64);
    let evidence = data.get("evidence").unwrap_or(&Value::Null);
    let gate = data.get("gate").unwrap_or(&Value::Null);
    MemorySentinelView {
        schema_version: SCHEMA_VERSION,
        lifecycle: SentinelCounts {
            active: state("active"),
            demoted: state("demoted"),
            superseded: state("superseded"),
            expired: state("expired"),
        },
        proposals: list(data.get("proposals")),
        contradictions: list(data.get("contradictions")),
        scratchpad: list(data.get("scratchpad")),
        working: list(data.get("working")),
        task_criteria: list(data.get("taskCriteria")),
        evidence: EvidenceState {
            state: evidence
                .get("state")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .into(),
            valid: evidence
                .get("valid")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            reason: evidence
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("unknown — no evidence")
                .into(),
        },
        gate: GateState {
            state: gate
                .get("state")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .into(),
            reason: gate
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("unknown — no authoritative gate evidence")
                .into(),
            authoritative: gate
                .get("authoritative")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        },
    }
}
