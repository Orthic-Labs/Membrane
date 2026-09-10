//! Port of legacy `mcp/host/candidate-set.mjs`.
//!
//! Normalizes a Blueprint candidate envelope at Membrane's consumer boundary.
//! `indexedAt` is copied only when Blueprint supplied it; no host timestamp is
//! invented for an incomplete graph receipt.

use membrane_protocol::digest_str;
use serde_json::{json, Value};

const SCHEMA: &str = "membrane.context-candidate-set.v1";

pub struct ConsumedCandidateSet {
    pub state: &'static str,
    pub reason: &'static str,
    pub candidate_set: Option<Value>,
    pub receipt: Option<Value>,
}

fn degraded(reason: &'static str) -> ConsumedCandidateSet {
    ConsumedCandidateSet {
        state: "degraded",
        reason,
        candidate_set: None,
        receipt: None,
    }
}

/// Mirrors `consumeCandidateSet(value, { traceId })`.
pub fn consume_candidate_set(value: &Value, trace_id: Option<&str>) -> ConsumedCandidateSet {
    if !value.is_object() {
        return degraded("candidate_set_missing");
    }
    let resolved_trace = value
        .get("traceId")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .or(trace_id);
    let indexed_at = value
        .get("indexedAt")
        .and_then(Value::as_str)
        .or_else(|| value.get("freshness").and_then(|f| f.get("indexedAt")).and_then(Value::as_str));

    let (Some(resolved_trace), Some(indexed_at)) = (resolved_trace, indexed_at) else {
        return degraded("candidate_set_identity_incomplete");
    };
    if resolved_trace.is_empty() || indexed_at.is_empty() {
        return degraded("candidate_set_identity_incomplete");
    }

    let mut candidate_set = value.clone();
    if let Value::Object(map) = &mut candidate_set {
        map.insert("schema".into(), json!(SCHEMA));
        map.insert("traceId".into(), json!(resolved_trace));
        map.insert("indexedAt".into(), json!(indexed_at));
    }
    let digest = digest_str(&serde_json::to_string(&candidate_set).unwrap());
    let receipt = json!({
        "schema": "membrane.candidate-set-receipt.v1",
        "traceId": resolved_trace,
        "indexedAt": indexed_at,
        "candidateSetDigest": digest,
    });
    ConsumedCandidateSet {
        state: "available",
        reason: "candidate_set_consumed",
        candidate_set: Some(candidate_set),
        receipt: Some(receipt),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_trace_and_index_identity() {
        let value = json!({
            "traceId": "t1",
            "freshness": {"indexedAt": "2026-08-20T00:00:00.000Z"},
            "candidates": [],
            "omissions": [],
        });
        let consumed = consume_candidate_set(&value, None);
        assert_eq!(consumed.state, "available");
        assert_eq!(
            consumed.candidate_set.unwrap()["indexedAt"],
            "2026-08-20T00:00:00.000Z"
        );
        assert!(consumed.receipt.unwrap()["candidateSetDigest"]
            .as_str()
            .unwrap()
            .starts_with("sha256:"));

        let incomplete = json!({"traceId": "t1", "freshness": {}});
        assert_eq!(consume_candidate_set(&incomplete, None).state, "degraded");
    }
}
