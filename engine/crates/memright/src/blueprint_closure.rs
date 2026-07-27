//! Bounded, deterministic dependency closure over Blueprint graph edges.
//!
//! This module is deliberately graph-store agnostic: Blueprint providers adapt their raw graph
//! rows into these typed edges before planner allocation owns final token admission.

use std::collections::{BTreeSet, HashMap, VecDeque};

use serde::{Deserialize, Serialize};

/// Edge classes supplied by Blueprint's static repository graph.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlueprintEdgeKindV1 {
    Symbol,
    Call,
    Import,
    Test,
}

/// A directed Blueprint graph relationship from a selected item to its dependency.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlueprintGraphEdgeV1 {
    pub from: String,
    pub to: String,
    pub kind: BlueprintEdgeKindV1,
}

impl BlueprintGraphEdgeV1 {
    pub fn new(from: impl Into<String>, to: impl Into<String>, kind: BlueprintEdgeKindV1) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            kind,
        }
    }
}

/// Hard limits applied before a dependency enters the closure.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlueprintClosureOptionsV1 {
    /// Number of edges allowed between a root & an admitted dependency.
    pub max_depth: usize,
    /// Total admitted nodes, including explicit roots.
    pub max_nodes: usize,
}

impl Default for BlueprintClosureOptionsV1 {
    fn default() -> Self {
        Self {
            max_depth: 2,
            max_nodes: 24,
        }
    }
}

/// A closure node paired with its shortest discovered path length from any root.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlueprintClosureNodeV1 {
    pub id: String,
    pub depth: usize,
}

/// Deterministic bounded closure plus explicit limit/cycle receipts.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlueprintDependencyClosureV1 {
    pub roots: Vec<String>,
    pub nodes: Vec<BlueprintClosureNodeV1>,
    pub included_edges: Vec<BlueprintGraphEdgeV1>,
    pub cycle_edges_skipped: usize,
    pub truncated_by_depth: bool,
    pub truncated_by_node_limit: bool,
}

/// Expand directed Blueprint dependency edges in stable breadth-first order.
///
/// Each node is admitted once at its shortest path depth. Edges back to an ancestor are counted
/// as cycles; other revisits are ignored so diamonds cannot duplicate context.
pub fn close_blueprint_dependencies(
    roots: &[String],
    edges: &[BlueprintGraphEdgeV1],
    options: BlueprintClosureOptionsV1,
) -> BlueprintDependencyClosureV1 {
    let mut roots_out = Vec::new();
    let mut seen = BTreeSet::new();
    let mut nodes = Vec::new();
    let mut pending = VecDeque::new();
    let mut parents = HashMap::<String, String>::new();
    let mut truncated_by_node_limit = false;

    for root in roots {
        if !seen.insert(root.clone()) {
            continue;
        }
        roots_out.push(root.clone());
        if nodes.len() == options.max_nodes {
            truncated_by_node_limit = true;
            continue;
        }
        nodes.push(BlueprintClosureNodeV1 {
            id: root.clone(),
            depth: 0,
        });
        pending.push_back((root.clone(), 0usize));
    }

    let mut included_edges = Vec::new();
    let mut cycle_edges_skipped = 0;
    let mut truncated_by_depth = false;

    while let Some((current, depth)) = pending.pop_front() {
        for edge in edges.iter().filter(|edge| edge.from == current) {
            if is_ancestor(&edge.to, &current, &parents) {
                cycle_edges_skipped += 1;
                continue;
            }
            if seen.contains(&edge.to) {
                continue;
            }
            if depth >= options.max_depth {
                truncated_by_depth = true;
                continue;
            }
            if nodes.len() == options.max_nodes {
                truncated_by_node_limit = true;
                continue;
            }

            seen.insert(edge.to.clone());
            parents.insert(edge.to.clone(), current.clone());
            nodes.push(BlueprintClosureNodeV1 {
                id: edge.to.clone(),
                depth: depth + 1,
            });
            included_edges.push(edge.clone());
            pending.push_back((edge.to.clone(), depth + 1));
        }
    }

    BlueprintDependencyClosureV1 {
        roots: roots_out,
        nodes,
        included_edges,
        cycle_edges_skipped,
        truncated_by_depth,
        truncated_by_node_limit,
    }
}

fn is_ancestor(target: &str, current: &str, parents: &HashMap<String, String>) -> bool {
    let mut cursor = current;
    loop {
        if cursor == target {
            return true;
        }
        let Some(parent) = parents.get(cursor) else {
            return false;
        };
        cursor = parent;
    }
}
