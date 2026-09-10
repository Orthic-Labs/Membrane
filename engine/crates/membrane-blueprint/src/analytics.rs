//! Deterministic graph analytics (ported from `blueprint/src/graph/analytics/index.mjs`).
//!
//! Derived scores here never become source truth; `decompose_change_risk`
//! keeps co-change deliberately low-authority per the legacy doctrine.

use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

pub const ANALYTICS_VERSION: &str = "1.0.0";

#[derive(Clone, Debug)]
pub struct Edge {
    pub source: String,
    pub target: String,
}

/// Tarjan SCC with stable tie-breaking (sorted adjacency, sorted node visit
/// order, sorted component members) to match the legacy deterministic order.
pub fn find_sccs(edges: &[Edge]) -> Vec<Vec<String>> {
    let mut graph: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for edge in edges {
        graph.entry(edge.source.clone()).or_default().insert(edge.target.clone());
        graph.entry(edge.target.clone()).or_default();
    }

    let mut index: HashMap<String, usize> = HashMap::new();
    let mut lowlink: HashMap<String, usize> = HashMap::new();
    let mut on_stack: HashSet<String> = HashSet::new();
    let mut stack: Vec<String> = Vec::new();
    let mut sccs: Vec<Vec<String>> = Vec::new();
    let mut counter = 0usize;

    // Iterative strongconnect to avoid unbounded recursion depth.
    struct Frame {
        node: String,
        neighbors: Vec<String>,
        pos: usize,
    }

    let nodes: Vec<String> = graph.keys().cloned().collect();
    for start in nodes {
        if index.contains_key(&start) {
            continue;
        }
        let mut call_stack: Vec<Frame> = Vec::new();
        index.insert(start.clone(), counter);
        lowlink.insert(start.clone(), counter);
        counter += 1;
        stack.push(start.clone());
        on_stack.insert(start.clone());
        let neighbors: Vec<String> = graph.get(&start).cloned().unwrap_or_default().into_iter().collect();
        call_stack.push(Frame { node: start, neighbors, pos: 0 });

        while let Some(frame) = call_stack.last_mut() {
            if frame.pos < frame.neighbors.len() {
                let neighbor = frame.neighbors[frame.pos].clone();
                frame.pos += 1;
                if !index.contains_key(&neighbor) {
                    index.insert(neighbor.clone(), counter);
                    lowlink.insert(neighbor.clone(), counter);
                    counter += 1;
                    stack.push(neighbor.clone());
                    on_stack.insert(neighbor.clone());
                    let next_neighbors: Vec<String> = graph.get(&neighbor).cloned().unwrap_or_default().into_iter().collect();
                    call_stack.push(Frame { node: neighbor, neighbors: next_neighbors, pos: 0 });
                } else if on_stack.contains(&neighbor) {
                    let node_low = *lowlink.get(&frame.node).unwrap();
                    let neighbor_idx = *index.get(&neighbor).unwrap();
                    lowlink.insert(frame.node.clone(), node_low.min(neighbor_idx));
                }
            } else {
                let node = frame.node.clone();
                let finished = call_stack.pop().unwrap();
                if let Some(parent) = call_stack.last() {
                    let parent_low = *lowlink.get(&parent.node).unwrap();
                    let child_low = *lowlink.get(&finished.node).unwrap();
                    lowlink.insert(parent.node.clone(), parent_low.min(child_low));
                }
                if lowlink.get(&node) == index.get(&node) {
                    let mut component = Vec::new();
                    loop {
                        let member = stack.pop().unwrap();
                        on_stack.remove(&member);
                        component.push(member.clone());
                        if member == node {
                            break;
                        }
                    }
                    component.sort();
                    sccs.push(component);
                }
            }
        }
    }
    sccs
}

pub fn find_cycles(edges: &[Edge]) -> Vec<Vec<String>> {
    find_sccs(edges)
        .into_iter()
        .filter(|component| {
            component.len() > 1
                || (component.len() == 1 && edges.iter().any(|e| e.source == component[0] && e.target == component[0]))
        })
        .collect()
}

pub struct NodeRef {
    pub id: String,
}

pub fn dead_code_candidates(nodes: &[NodeRef], edges: &[Edge]) -> Vec<Value> {
    let referenced: HashSet<&str> = edges.iter().map(|e| e.target.as_str()).collect();
    nodes
        .iter()
        .filter(|node| !referenced.contains(node.id.as_str()))
        .map(|node| json!({"id": node.id, "candidate": true, "reason": "no_incoming_edges"}))
        .collect()
}

/// Deterministic longest-path-from-roots layering, stable tie-break by id.
pub fn assign_layers(nodes: &[NodeRef], edges: &[Edge]) -> Vec<(String, u64)> {
    let mut incoming: BTreeMap<String, Vec<String>> = nodes.iter().map(|n| (n.id.clone(), Vec::new())).collect();
    for edge in edges {
        if let Some(list) = incoming.get_mut(&edge.target) {
            list.push(edge.source.clone());
        }
    }
    let mut layers: HashMap<String, u64> = HashMap::new();

    fn visit(node_id: &str, depth: u64, incoming: &BTreeMap<String, Vec<String>>, layers: &mut HashMap<String, u64>) {
        let current = *layers.get(node_id).unwrap_or(&0);
        layers.insert(node_id.to_owned(), current.max(depth));
        for (target, sources) in incoming {
            if sources.iter().any(|s| s == node_id) {
                visit(target, depth + 1, incoming, layers);
            }
        }
    }

    let mut sorted_nodes: Vec<&NodeRef> = nodes.iter().collect();
    sorted_nodes.sort_by(|a, b| a.id.cmp(&b.id));
    for node in sorted_nodes {
        if incoming.get(&node.id).map(|s| s.is_empty()).unwrap_or(true) {
            visit(&node.id, 0, &incoming, &mut layers);
        }
    }
    nodes.iter().map(|n| (n.id.clone(), *layers.get(&n.id).unwrap_or(&0))).collect()
}

pub fn analytics_digest(algorithm: &str, input_generation_id: &str, params: &Value) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(format!("{algorithm}:{input_generation_id}:{}", params.to_string()));
    let digest = hasher.finalize();
    let hex = digest.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    hex[..16].to_owned()
}

fn clamp01(value: f64) -> f64 {
    value.max(0.0).min(1.0)
}

pub struct RiskInput {
    pub changed_paths: Vec<String>,
    /// impacted item ids/paths, used only for count-distinct
    pub impacted_ids: Vec<String>,
    pub edge_confidence_tiers: Vec<Option<String>>,
    pub truncated: bool,
    pub ambiguous_seeds: u64,
    pub stale: bool,
    pub cochange_score: f64,
}

/// Compute advisory change-risk score using the same bounded factors as the
/// legacy `decomposeChangeRisk` implementation.  Keep this separate from the
/// envelope builder so callers that only need the score cannot accidentally
/// lose the inspectable factor evidence.
pub fn risk_score(input: &RiskInput) -> f64 {
    let heuristic_edges = input
        .edge_confidence_tiers
        .iter()
        .filter(|tier| matches!(tier.as_deref(), Some("CROSS_FILE_HEURISTIC") | Some("UNRESOLVED")))
        .count();
    let edge_count = input.edge_confidence_tiers.len();
    let changed_distinct: HashSet<&str> = input.changed_paths.iter().map(|s| s.as_str()).collect();
    let impacted_distinct: HashSet<&str> = input.impacted_ids.iter().map(|s| s.as_str()).collect();
    let structural = clamp01(((1.0 + changed_distinct.len() as f64).log2()) / 5.0);
    let reach = clamp01(((1.0 + impacted_distinct.len() as f64).log2()) / 7.0);
    let uncertainty = clamp01(
        (if edge_count > 0 { heuristic_edges as f64 / edge_count as f64 } else { 0.0 })
            + if input.truncated { 0.35 } else { 0.0 }
            + (0.35_f64).min(input.ambiguous_seeds as f64 * 0.1)
            + if input.stale { 0.35 } else { 0.0 },
    );
    let historical = clamp01(input.cochange_score) * 0.15;
    clamp01(structural * 0.3 + reach * 0.35 + uncertainty * 0.35 + historical)
}

/// Inspectable risk decomposition. Co-change is deliberately low-authority.
pub fn decompose_change_risk(input: RiskInput) -> Value {
    let heuristic_edges = input
        .edge_confidence_tiers
        .iter()
        .filter(|tier| matches!(tier.as_deref(), Some("CROSS_FILE_HEURISTIC") | Some("UNRESOLVED")))
        .count();
    let edge_count = input.edge_confidence_tiers.len();

    let changed_distinct: HashSet<&str> = input.changed_paths.iter().map(|s| s.as_str()).collect();
    let impacted_distinct: HashSet<&str> = input.impacted_ids.iter().map(|s| s.as_str()).collect();

    let structural = clamp01(((1.0 + changed_distinct.len() as f64).log2()) / 5.0);
    let reach = clamp01(((1.0 + impacted_distinct.len() as f64).log2()) / 7.0);
    let uncertainty = clamp01(
        (if edge_count > 0 { heuristic_edges as f64 / edge_count as f64 } else { 0.0 })
            + if input.truncated { 0.35 } else { 0.0 }
            + (0.35_f64).min(input.ambiguous_seeds as f64 * 0.1)
            + if input.stale { 0.35 } else { 0.0 },
    );
    let historical = clamp01(input.cochange_score) * 0.15;
    let score = risk_score(&input);
    let band = if score >= 0.67 { "high" } else if score >= 0.34 { "medium" } else { "low" };

    json!({
        "schemaVersion": 1,
        "kind": "ChangeRiskDecomposition",
        "score": score,
        "band": band,
        "factors": [
            {"id": "change_breadth", "authority": "structural", "value": structural, "evidence": {"changedPathCount": changed_distinct.len()}},
            {"id": "impact_reach", "authority": "structural", "value": reach, "evidence": {"impactedCount": input.impacted_ids.len()}},
            {"id": "evidence_uncertainty", "authority": "graph", "value": uncertainty, "evidence": {"heuristicEdgeCount": heuristic_edges, "edgeCount": edge_count, "truncated": input.truncated, "ambiguousSeeds": input.ambiguous_seeds, "stale": input.stale}},
            {"id": "cochange", "authority": "historical_low", "value": historical, "evidence": {"suppliedScore": clamp01(input.cochange_score), "maximumContribution": 0.15}},
        ],
        "authority": "advisory_not_truth",
    })
}
