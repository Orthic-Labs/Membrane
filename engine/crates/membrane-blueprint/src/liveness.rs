//! Generation-bound liveness projection built on the entry-point registry.
//!
//! Native Rust port of `blueprint/src/graph/liveness.mjs`. Operates on the
//! same `nodes`/`edges` JSON shape carried by [`crate::store::Generation`],
//! reusing [`crate::entry_points::build_entry_point_registry`] rather than
//! reimplementing entry-point selection.

use crate::entry_points::build_entry_point_registry;
use crate::store::Generation;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};

pub const LIVENESS_STATES: [&str; 3] = ["LIVE", "UNREACHED", "UNKNOWN"];

#[derive(Debug, Clone, Default)]
pub struct LivenessOptions {
    pub max_nodes: Option<i64>,
    pub max_edges: Option<i64>,
    pub max_hops: Option<i64>,
    pub source_state: Option<String>,
}

fn citation_row(item: &Value) -> Value {
    json!({
        "path": item.get("path").cloned().unwrap_or(Value::Null),
        "startLine": item.get("startLine").cloned().unwrap_or(Value::Null),
        "endLine": item.get("endLine").cloned().unwrap_or(Value::Null),
        "contentHash": item.get("contentHash").cloned().unwrap_or(Value::Null),
    })
}

fn citations(values: &[&Value]) -> Vec<Value> {
    let mut rows = Vec::new();
    let mut seen = HashSet::new();
    for value in values {
        let evidence = value.get("evidence").and_then(Value::as_array).cloned().unwrap_or_default();
        for item in &evidence {
            let row = citation_row(item);
            let key = serde_json::to_string(&row).unwrap_or_default();
            if seen.insert(key) {
                rows.push(row);
            }
        }
    }
    rows
}

fn clamp(value: Option<i64>, default: i64, lo: i64, hi: i64) -> i64 {
    let v = value.unwrap_or(default);
    let v = if v <= 0 { default } else { v };
    v.max(lo).min(hi)
}

/// Mirrors `buildLivenessProjection(generation, options)`.
pub fn build_liveness_projection(generation: &Generation, options: &LivenessOptions) -> Value {
    let generation_id = generation
        .manifest
        .as_ref()
        .and_then(|m| m.get("generationId"))
        .cloned()
        .unwrap_or(Value::Null);

    let max_nodes = clamp(options.max_nodes, 5000, 1, 20000);
    let max_edges = clamp(options.max_edges, 20000, 1, 100000);
    let max_hops = clamp(options.max_hops, 24, 1, 64);
    let source_state = options.source_state.clone().unwrap_or_else(|| "clean".to_string());

    let all_nodes = &generation.nodes;
    let selected: Vec<&Value> = all_nodes.iter().take(max_nodes as usize).collect();
    let bounded_out = all_nodes.len() > selected.len();
    let manifest_complete = generation.manifest.as_ref().and_then(|m| m.get("complete")).and_then(Value::as_bool) == Some(true);
    let manifest_truncated = generation.manifest.as_ref().and_then(|m| m.get("truncated")).and_then(Value::as_bool) == Some(true);
    let complete = manifest_complete && !manifest_truncated && !bounded_out;

    let registry = build_entry_point_registry(generation, true);
    let admitted: Vec<&Value> = registry
        .iter()
        .filter(|entry| {
            entry.get("authority").and_then(Value::as_str) == Some("explicit")
                && entry.get("evidence").and_then(Value::as_array).map(|e| !e.is_empty()).unwrap_or(false)
        })
        .collect();
    let trustworthy = complete && source_state == "clean" && !admitted.is_empty();

    let entry_points: Vec<Value> = registry
        .iter()
        .map(|entry| {
            let mut obj = entry.as_object().cloned().unwrap_or_default();
            let node = obj.remove("node");
            let node_id = node.as_ref().and_then(|n| n.get("id")).cloned().unwrap_or(Value::Null);
            obj.insert("nodeId".to_string(), node_id);
            Value::Object(obj)
        })
        .collect();

    if !trustworthy {
        let reason = if source_state != "clean" {
            "source_not_current"
        } else if !complete {
            "generation_incomplete_or_bounded"
        } else {
            "no_explicit_entrypoint_evidence"
        };
        let results: Vec<Value> = selected
            .iter()
            .map(|node| {
                json!({
                    "nodeId": node.get("id").cloned().unwrap_or(Value::Null),
                    "path": node.get("path").cloned().unwrap_or(Value::Null),
                    "state": "UNKNOWN",
                    "reason": reason,
                    "evidence": citations(&[node]),
                    "reachabilityPath": Vec::<Value>::new(),
                })
            })
            .collect();
        return json!({
            "schemaVersion": 1,
            "kind": "liveness",
            "generationId": generation_id,
            "sourceState": source_state,
            "entryPoints": entry_points,
            "results": results,
            "counts": { "LIVE": 0, "UNREACHED": 0, "UNKNOWN": selected.len() },
            "omissions": if bounded_out { vec![json!({ "reason": "node_ceiling", "count": all_nodes.len() - selected.len() })] } else { vec![] },
            "truncated": bounded_out,
        });
    }

    let allowed: HashSet<&str> = selected.iter().filter_map(|n| n.get("id").and_then(Value::as_str)).collect();
    let all_edges = &generation.edges;
    let edges: Vec<&Value> = all_edges
        .iter()
        .filter(|edge| {
            let target = edge.get("target").and_then(Value::as_str);
            let resolved_false = edge.get("resolved") == Some(&Value::Bool(false));
            let unresolved_tier = edge.get("confidenceTier").and_then(Value::as_str) == Some("UNRESOLVED");
            let source = edge.get("source").and_then(Value::as_str);
            target.is_some()
                && !resolved_false
                && !unresolved_tier
                && source.map(|s| allowed.contains(s)).unwrap_or(false)
                && target.map(|t| allowed.contains(t)).unwrap_or(false)
        })
        .take(max_edges as usize)
        .collect();

    let mut adjacency: HashMap<&str, Vec<&Value>> = HashMap::new();
    for edge in &edges {
        if let Some(source) = edge.get("source").and_then(Value::as_str) {
            adjacency.entry(source).or_default().push(edge);
        }
    }
    for rows in adjacency.values_mut() {
        rows.sort_by_key(|e| e.get("id").and_then(Value::as_str).unwrap_or("").to_string());
    }

    #[derive(Clone)]
    struct ReachedState<'a> {
        parent: Option<String>,
        edge: Option<&'a Value>,
        #[allow(dead_code)]
        root: String,
    }

    let mut reached: HashMap<String, ReachedState> = HashMap::new();
    let mut queue: VecDeque<(String, i64)> = VecDeque::new();
    for entry in &admitted {
        if let Some(id) = entry.get("id").and_then(Value::as_str) {
            if allowed.contains(id) {
                queue.push_back((id.to_string(), 0));
            }
        }
    }
    for entry in &admitted {
        if let Some(id) = entry.get("id").and_then(Value::as_str) {
            if allowed.contains(id) {
                reached.insert(id.to_string(), ReachedState { parent: None, edge: None, root: id.to_string() });
            }
        }
    }
    while let Some((current_id, hops)) = queue.pop_front() {
        if hops >= max_hops {
            continue;
        }
        let root = reached.get(&current_id).map(|s| s.root.clone()).unwrap_or_else(|| current_id.clone());
        if let Some(rows) = adjacency.get(current_id.as_str()) {
            for edge in rows {
                let target = match edge.get("target").and_then(Value::as_str) {
                    Some(t) => t.to_string(),
                    None => continue,
                };
                if reached.contains_key(&target) {
                    continue;
                }
                reached.insert(target.clone(), ReachedState { parent: Some(current_id.clone()), edge: Some(edge), root: root.clone() });
                queue.push_back((target, hops + 1));
            }
        }
    }

    let by_id: HashMap<&str, &Value> = selected.iter().filter_map(|n| n.get("id").and_then(Value::as_str).map(|id| (id, *n))).collect();

    let live_path = |id: &str| -> (Vec<String>, Vec<Value>) {
        let mut ids = vec![id.to_string()];
        let mut edge_rows: Vec<&Value> = Vec::new();
        let mut cursor = id.to_string();
        while let Some(parent) = reached.get(&cursor).and_then(|s| s.parent.clone()) {
            let edge = reached.get(&cursor).and_then(|s| s.edge);
            if let Some(e) = edge {
                edge_rows.push(e);
            }
            cursor = parent;
            ids.push(cursor.clone());
        }
        ids.reverse();
        edge_rows.reverse();
        let mut refs: Vec<&Value> = ids.iter().filter_map(|nid| by_id.get(nid.as_str()).copied()).collect();
        refs.extend(edge_rows.iter().copied());
        (ids, citations(&refs))
    };

    let results: Vec<Value> = selected
        .iter()
        .map(|node| {
            let id = node.get("id").and_then(Value::as_str).unwrap_or("");
            if !reached.contains_key(id) {
                json!({
                    "nodeId": node.get("id").cloned().unwrap_or(Value::Null),
                    "path": node.get("path").cloned().unwrap_or(Value::Null),
                    "state": "UNREACHED",
                    "reason": "no_path_from_admitted_entrypoint",
                    "evidence": citations(&[*node]),
                    "reachabilityPath": Vec::<Value>::new(),
                })
            } else {
                let (ids, evidence) = live_path(id);
                json!({
                    "nodeId": node.get("id").cloned().unwrap_or(Value::Null),
                    "path": node.get("path").cloned().unwrap_or(Value::Null),
                    "state": "LIVE",
                    "reason": "evidence_backed_path_from_admitted_entrypoint",
                    "evidence": evidence,
                    "reachabilityPath": ids,
                })
            }
        })
        .collect();

    let counts: serde_json::Map<String, Value> = LIVENESS_STATES
        .iter()
        .map(|state| {
            let count = results.iter().filter(|r| r.get("state").and_then(Value::as_str) == Some(*state)).count();
            (state.to_string(), json!(count))
        })
        .collect();

    let total_edges = all_edges.len();
    let omissions = if total_edges > edges.len() {
        vec![json!({ "reason": "edge_ceiling_or_out_of_scope", "count": total_edges - edges.len() })]
    } else {
        vec![]
    };

    json!({
        "schemaVersion": 1,
        "kind": "liveness",
        "generationId": generation_id,
        "sourceState": source_state,
        "entryPoints": entry_points,
        "results": results,
        "counts": counts,
        "omissions": omissions,
        "truncated": (total_edges as i64) > max_edges,
    })
}
