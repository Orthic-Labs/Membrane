//! Native Rust port of `blueprint/src/graph/process-projection.mjs`.
//!
//! Walks bounded flow-edge chains from each registered entry point,
//! producing an ordered sequence of steps per process. Operates on the same
//! `nodes`/`edges` JSON shape carried by [`crate::store::Generation`],
//! reusing [`crate::entry_points::build_entry_point_registry`] exactly as
//! the legacy module does.

use crate::entry_points::build_entry_point_registry;
use crate::store::Generation;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet, VecDeque};

const FLOW_KINDS: [&str; 9] =
    ["CALLS", "ROUTES_TO", "HANDLES", "PRODUCES", "CONSUMES", "READS", "WRITES", "USES", "DEPLOYS"];

#[derive(Debug, Clone, Default)]
pub struct ProcessProjectionOptions {
    pub max_processes: Option<i64>,
    pub max_depth: Option<i64>,
    pub max_steps: Option<i64>,
}

fn digest(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hex::encode(hasher.finalize())
}

fn node_ref(node: &Value) -> Value {
    let id = node.get("id").cloned().unwrap_or(Value::Null);
    let kind = node.get("kind").cloned().unwrap_or(Value::Null);
    let name = node.get("name").cloned().unwrap_or(Value::Null);
    let path = node
        .get("path")
        .cloned()
        .filter(|v| !v.is_null())
        .or_else(|| {
            node.get("evidence")
                .and_then(Value::as_array)
                .and_then(|e| e.first())
                .and_then(|e| e.get("path"))
                .cloned()
        })
        .unwrap_or(Value::Null);
    let evidence = node.get("evidence").cloned().unwrap_or_else(|| json!([]));
    json!({ "id": id, "kind": kind, "name": name, "path": path, "evidence": evidence })
}

struct QueueItem {
    id: String,
    depth: i64,
    via: Option<Value>,
    parent_step_id: Option<String>,
}

/// Mirrors `buildProcessProjection(generation, options)`.
pub fn build_process_projection(generation: &Generation, options: &ProcessProjectionOptions) -> Value {
    // The JS API destructuring default applies only when the property is
    // absent; explicit zero remains a valid empty slice bound.
    let max_processes = options.max_processes.unwrap_or(64);
    let max_depth = options.max_depth.unwrap_or(12);
    let max_steps = options.max_steps.unwrap_or(256).max(0) as usize;

    let generation_id = generation
        .manifest
        .as_ref()
        .and_then(|m| m.get("generationId"))
        .cloned()
        .unwrap_or(Value::Null);

    let by_id: HashMap<&str, &Value> = generation
        .nodes
        .iter()
        .filter_map(|n| n.get("id").and_then(Value::as_str).map(|id| (id, n)))
        .collect();

    let mut outgoing: HashMap<String, Vec<&Value>> = HashMap::new();
    let mut frontiers: Vec<Value> = Vec::new();
    let flow_kinds: HashSet<&str> = FLOW_KINDS.into_iter().collect();
    for edge in &generation.edges {
        let kind = edge.get("kind").and_then(Value::as_str).unwrap_or("");
        if !flow_kinds.contains(kind) {
            continue;
        }
        let target = edge.get("target").and_then(Value::as_str).filter(|value| !value.is_empty());
        if target.is_none() {
            frontiers.push(json!({
                "source": edge.get("source").cloned().unwrap_or(Value::Null),
                "relation": kind,
                "reason": edge.get("reason").and_then(Value::as_str).unwrap_or("unresolved_flow_edge"),
                "evidence": edge.get("evidence").cloned().unwrap_or_else(|| json!([])),
            }));
            continue;
        }
        if let Some(source) = edge.get("source").and_then(Value::as_str) {
            outgoing.entry(source.to_string()).or_default().push(edge);
        }
    }
    for edges in outgoing.values_mut() {
        edges.sort_by(|a, b| {
            let a_id = a.get("id").and_then(Value::as_str).unwrap_or("");
            let b_id = b.get("id").and_then(Value::as_str).unwrap_or("");
            a_id.cmp(b_id)
        });
    }

    let all_entries = build_entry_point_registry(generation, false);
    let entry_end = if max_processes >= 0 {
        (max_processes as usize).min(all_entries.len())
    } else {
        all_entries.len().saturating_sub(max_processes.unsigned_abs() as usize)
    };
    let entries: Vec<&Value> = all_entries.iter().take(entry_end).collect();

    let mut processes: Vec<Value> = Vec::new();
    for entry in &entries {
        let root_id = entry
            .get("node")
            .and_then(|n| n.get("id"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let root_node = by_id.get(root_id.as_str()).copied();

        let mut queue: VecDeque<QueueItem> =
            VecDeque::from([QueueItem { id: root_id.clone(), depth: 0, via: None, parent_step_id: None }]);
        let mut seen: HashSet<String> = HashSet::new();
        let mut steps: Vec<Value> = Vec::new();
        let mut truncated = false;

        while !queue.is_empty() && steps.len() < max_steps {
            let current = queue.pop_front().unwrap();
            if seen.contains(&current.id) {
                continue;
            }
            seen.insert(current.id.clone());
            let Some(node) = by_id.get(current.id.as_str()).copied() else { continue };
            let step_id = format!(
                "step:{}",
                digest(&format!("{}\u{0}{}\u{0}{}", root_id, current.id, current.depth))
            );
            let has_out = outgoing.get(&current.id).map(|e| !e.is_empty()).unwrap_or(false);
            steps.push(json!({
                "stepId": step_id,
                "ordinal": steps.len(),
                "node": node_ref(node),
                "viaRelation": current.via.as_ref().map(|edge| json!({
                    "id": edge.get("id").cloned().unwrap_or(Value::Null),
                    "kind": edge.get("kind").cloned().unwrap_or(Value::Null),
                })).unwrap_or(Value::Null),
                "parentStepId": current.parent_step_id.clone().map(Value::String).unwrap_or(Value::Null),
                "terminal": !has_out,
            }));
            if current.depth >= max_depth {
                if has_out {
                    truncated = true;
                }
                continue;
            }
            if let Some(edges) = outgoing.get(&current.id) {
                for edge in edges {
                    if let Some(target) = edge.get("target").and_then(Value::as_str) {
                        queue.push_back(QueueItem {
                            id: target.to_string(),
                            depth: current.depth + 1,
                            via: Some((*edge).clone()),
                            parent_step_id: Some(step_id.clone()),
                        });
                    }
                }
            }
        }
        if !queue.is_empty() {
            truncated = true;
        }

        let entry_evidence = entry.get("evidence").cloned().unwrap_or_else(|| json!([]));
        let _ = root_node;
        // `entry-points.mjs` currently emits no kind/confidence properties;
        // JSON.stringify omits those undefined values. Preserve optional
        // fields when a richer registry supplies them, without manufacturing
        // nulls on the V1 shape.
        let mut entry_point = serde_json::Map::new();
        entry_point.insert("id".to_string(), Value::String(root_id.clone()));
        entry_point.insert("evidence".to_string(), entry_evidence);
        for key in ["kind", "confidence"] {
            if let Some(value) = entry.get(key).filter(|value| !value.is_null()) {
                entry_point.insert(key.to_string(), value.clone());
            }
        }
        processes.push(json!({
            "processId": format!("process:{}", digest(&root_id)),
            "entryPoint": Value::Object(entry_point),
            "steps": steps,
            "truncated": truncated,
            "omissions": if truncated {
                json!([{ "reason": "process_projection_bound", "maxDepth": max_depth, "maxSteps": max_steps }])
            } else {
                json!([])
            },
        }));
    }

    json!({
        "schemaVersion": 1,
        "kind": "process-projection",
        "generationId": generation_id,
        "processes": processes,
        "frontiers": frontiers,
        "truncated": (all_entries.len() as i64) >= max_processes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Generation;

    fn generation_from(nodes: Vec<Value>, edges: Vec<Value>) -> Generation {
        Generation {
            manifest: Some(json!({ "generationId": "gen-1" })),
            nodes,
            edges,
            ..Default::default()
        }
    }

    fn entry_node(id: &str) -> Value {
        json!({ "id": id, "kind": "function", "name": id, "path": "src/lib.rs", "entryPoint": true, "evidence": [{"path": "src/lib.rs"}] })
    }

    fn plain_node(id: &str) -> Value {
        json!({ "id": id, "kind": "function", "name": id, "path": "src/lib.rs", "evidence": [] })
    }

    #[test]
    fn walks_a_bounded_chain_from_an_entry_point() {
        let generation = generation_from(
            vec![entry_node("a"), plain_node("b"), plain_node("c")],
            vec![
                json!({ "id": "e1", "kind": "CALLS", "source": "a", "target": "b" }),
                json!({ "id": "e2", "kind": "CALLS", "source": "b", "target": "c" }),
            ],
        );
        let projection = build_process_projection(&generation, &ProcessProjectionOptions::default());
        assert_eq!(projection["kind"], json!("process-projection"));
        let processes = projection["processes"].as_array().unwrap();
        assert_eq!(processes.len(), 1);
        let steps = processes[0]["steps"].as_array().unwrap();
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0]["node"]["id"], json!("a"));
        assert_eq!(steps[2]["node"]["id"], json!("c"));
        assert_eq!(steps[2]["terminal"], json!(true));
        assert_eq!(processes[0]["truncated"], json!(false));
    }

    #[test]
    fn records_unresolved_dynamic_edges_as_frontiers() {
        let generation = generation_from(
            vec![entry_node("a")],
            vec![json!({ "id": "e1", "kind": "CALLS", "source": "a", "reason": "dynamic_dispatch" })],
        );
        let projection = build_process_projection(&generation, &ProcessProjectionOptions::default());
        let frontiers = projection["frontiers"].as_array().unwrap();
        assert_eq!(frontiers.len(), 1);
        assert_eq!(frontiers[0]["reason"], json!("dynamic_dispatch"));
    }

    #[test]
    fn truncates_at_max_depth() {
        let generation = generation_from(
            vec![entry_node("a"), plain_node("b"), plain_node("c")],
            vec![
                json!({ "id": "e1", "kind": "CALLS", "source": "a", "target": "b" }),
                json!({ "id": "e2", "kind": "CALLS", "source": "b", "target": "c" }),
            ],
        );
        let projection = build_process_projection(
            &generation,
            &ProcessProjectionOptions { max_processes: None, max_depth: Some(1), max_steps: None },
        );
        let processes = projection["processes"].as_array().unwrap();
        assert_eq!(processes[0]["truncated"], json!(true));
        let steps = processes[0]["steps"].as_array().unwrap();
        assert_eq!(steps.len(), 2);
    }
}
