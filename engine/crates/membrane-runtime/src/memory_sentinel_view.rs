//! Content-free, bounded projection for the Memory Sentinel Hub view.
//! This module only reads an already assembled report; it never mutates data-plane state.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

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
/// One MEM-029 health dimension as projected for the Hub. `state` is
/// `observed` when the producer's source read succeeded and `not_evaluated`
/// when it was unavailable — a dimension never silently converts to zero.
/// `count` exists only for observed dimensions; `ids` are bounded
/// drill-down identities (memory or relation ids), never content.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SentinelDimension {
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ids: Vec<String>,
    #[serde(default)]
    pub reason: String,
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
    /// Additive MEM-029 dimension map (duplication, relationIntegrity,
    /// isolation, provenanceGaps, lifecycleAnomaly, projectionDrift,
    /// observedRecall, crowding, reviewBacklog). Absent/empty on reports
    /// produced before this field existed — never fabricated.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dimensions: BTreeMap<String, SentinelDimension>,
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
        dimensions: data
            .get("dimensions")
            .and_then(Value::as_object)
            .map(|object| {
                object
                    .iter()
                    .take(MAX_ITEMS)
                    .map(|(name, value)| {
                        let dimension = SentinelDimension {
                            state: value
                                .get("state")
                                .and_then(Value::as_str)
                                .unwrap_or("unknown")
                                .into(),
                            count: value.get("count").and_then(Value::as_u64),
                            ids: value
                                .get("ids")
                                .and_then(Value::as_array)
                                .map(|a| {
                                    a.iter()
                                        .take(MAX_ITEMS)
                                        .filter_map(Value::as_str)
                                        .map(str::to_owned)
                                        .collect()
                                })
                                .unwrap_or_default(),
                            reason: value
                                .get("reason")
                                .and_then(Value::as_str)
                                .unwrap_or("unknown — no evidence")
                                .into(),
                        };
                        (name.clone(), dimension)
                    })
                    .collect()
            })
            .unwrap_or_default(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_dimensions_stay_absent_in_the_projection() {
        let view = project(&json!({"lifecycle": {"active": 2}}));
        assert!(view.dimensions.is_empty());
        // …and an empty map stays off the wire entirely.
        assert!(serde_json::to_value(&view)
            .unwrap()
            .get("dimensions")
            .is_none());
    }

    #[test]
    fn observed_and_unavailable_dimensions_project_distinctly() {
        let view = project(&json!({
            "dimensions": {
                "isolation": {"state": "observed", "count": 3, "ids": ["m1", "m2", "m3"], "reason": "no incident edge"},
                "provenanceGaps": {"state": "not_evaluated", "reason": "source_unavailable"}
            }
        }));
        let isolation = &view.dimensions["isolation"];
        assert_eq!(isolation.state, "observed");
        assert_eq!(isolation.count, Some(3));
        assert_eq!(isolation.ids, vec!["m1", "m2", "m3"]);
        let gaps = &view.dimensions["provenanceGaps"];
        assert_eq!(gaps.state, "not_evaluated");
        assert_eq!(gaps.count, None);
        assert!(gaps.ids.is_empty());
        let wire = serde_json::to_value(&view).unwrap();
        assert!(wire["dimensions"]["provenanceGaps"].get("count").is_none());
    }
}
