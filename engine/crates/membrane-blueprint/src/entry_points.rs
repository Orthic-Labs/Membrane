//! Entry-point registry for derived architecture/liveness views.
//!
//! Native Rust port of `blueprint/src/graph/entry-points.mjs`. Explicit
//! source-backed entry points are separated from structural candidates: zero
//! inbound degree is useful orientation evidence but never proves execution.
//!
//! Operates on the same `nodes`/`edges` JSON shape already carried by
//! [`crate::store::Generation`] (top-level `labels`/`entryPoint` fields, as
//! persisted by the store — not the pre-storage [`crate::model::GraphNode`]
//! struct), so behavior matches the legacy JS registry exactly.

use crate::store::Generation;
use serde_json::{json, Value};
use std::collections::HashSet;

fn labels(node: &Value) -> HashSet<String> {
    node.get("labels")
        .and_then(Value::as_array)
        .map(|labels| labels.iter().filter_map(Value::as_str).map(|label| label.to_ascii_lowercase()).collect())
        .unwrap_or_default()
}

fn evidence(node: &Value) -> Vec<Value> {
    node.get("evidence")
        .and_then(Value::as_array)
        .map(|evidence| evidence.iter().filter(|value| !value.is_null()).cloned().collect())
        .unwrap_or_default()
}

/// Build the entry-point registry from a loaded [`Generation`].
pub fn build_entry_point_registry(generation: &Generation, include_structural_candidates: bool) -> Vec<Value> {
    build_entry_point_registry_from(&generation.nodes, &generation.edges, include_structural_candidates)
}

/// Build the entry-point registry from raw node/edge JSON. Mirrors
/// `buildEntryPointRegistry(generation, { includeStructuralCandidates })`.
pub fn build_entry_point_registry_from(nodes: &[Value], edges: &[Value], include_structural_candidates: bool) -> Vec<Value> {
    let incoming: HashSet<&str> = edges.iter().filter_map(|edge| edge.get("target").and_then(Value::as_str)).collect();
    let outgoing: HashSet<&str> = edges.iter().filter_map(|edge| edge.get("source").and_then(Value::as_str)).collect();

    let mut rows: Vec<Value> = Vec::new();
    for node in nodes {
        let id = node.get("id").and_then(Value::as_str).unwrap_or("");
        let node_labels = labels(node);
        let tagged = node.get("entryPoint").and_then(Value::as_bool) == Some(true)
            || node_labels.contains("entrypoint")
            || node_labels.contains("entry_point");
        if tagged {
            rows.push(json!({
                "id": id,
                "node": node,
                "authority": "explicit",
                "reason": "source_backed_entrypoint_marker",
                "evidence": evidence(node),
            }));
            continue;
        }
        if include_structural_candidates
            && node.get("kind").and_then(Value::as_str) == Some("symbol")
            && outgoing.contains(id)
            && !incoming.contains(id)
        {
            rows.push(json!({
                "id": id,
                "node": node,
                "authority": "structural_candidate",
                "reason": "outgoing_with_zero_observed_inbound",
                "evidence": evidence(node),
            }));
        }
    }

    rows.sort_by(|a, b| {
        let a_id = a.get("id").and_then(Value::as_str).unwrap_or("");
        let b_id = b.get("id").and_then(Value::as_str).unwrap_or("");
        a_id.cmp(b_id)
    });
    rows
}
