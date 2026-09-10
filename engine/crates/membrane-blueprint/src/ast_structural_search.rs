//! GC7: native port of `blueprint/src/graph/ast-structural-search.mjs`.
//!
//! Bounded structural search over canonical AST/compiler symbol facts
//! (`GraphNode`/`GraphEdge` already produced by the tree-sitter graph
//! builder). This is deliberately not regex-over-source: callers query
//! structural node/edge properties that already exist in a `GraphGeneration`.
//!
//! This module is additive only: it does not modify `graph.rs` (owned by
//! lane GRAM) and only reads the public `GraphGeneration`/`GraphNode`/
//! `GraphEdge` shapes from `model.rs`.

use std::collections::BTreeSet;

use serde::Serialize;
use serde_json::{json, Value};

use crate::graph::GraphGeneration;
use crate::model::{GraphEdge, GraphNode};

const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 200;

/// Mirrors the legacy `pattern` object accepted by `searchAstStructure`.
#[derive(Debug, Clone, Default)]
pub struct SearchPattern {
    pub kind: Option<String>,
    pub name: Option<String>,
    pub exact_name: bool,
    pub path_prefix: Option<String>,
    pub relation: Option<String>,
    pub declaring_type: Option<String>,
    pub label: Option<String>,
    pub limit: Option<i64>,
}

impl SearchPattern {
    fn to_json(&self) -> Value {
        let mut map = serde_json::Map::new();
        if let Some(v) = &self.kind {
            map.insert("kind".into(), json!(v));
        }
        if let Some(v) = &self.name {
            map.insert("name".into(), json!(v));
        }
        if self.exact_name {
            map.insert("exactName".into(), json!(true));
        }
        if let Some(v) = &self.path_prefix {
            map.insert("pathPrefix".into(), json!(v));
        }
        if let Some(v) = &self.relation {
            map.insert("relation".into(), json!(v));
        }
        if let Some(v) = &self.declaring_type {
            map.insert("declaringType".into(), json!(v));
        }
        if let Some(v) = &self.label {
            map.insert("label".into(), json!(v));
        }
        if let Some(v) = self.limit {
            map.insert("limit".into(), json!(v));
        }
        Value::Object(map)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    pub kind: &'static str,
    #[serde(rename = "generationId")]
    pub generation_id: Option<String>,
    pub pattern: Value,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub truncated: bool,
    pub omissions: Vec<Value>,
}

/// `safeLimit(value, fallback = 50)`
fn safe_limit(value: Option<i64>, fallback: usize) -> usize {
    match value {
        Some(v) if v > 0 => (v as usize).min(MAX_LIMIT),
        _ => fallback,
    }
}

fn evidence0(node: &GraphNode) -> Option<&Value> {
    node.evidence.first()
}

fn node_labels(node: &GraphNode) -> BTreeSet<String> {
    evidence0(node)
        .and_then(|e| e.get("labels"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn node_qualified_name(node: &GraphNode) -> Option<String> {
    evidence0(node)
        .and_then(|e| e.get("qualifiedName"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// `kindMatches(node, kind)`
fn kind_matches(node: &GraphNode, kind: &Option<String>) -> bool {
    let Some(kind) = kind else { return true };
    let wanted = kind.to_ascii_lowercase();
    if node.kind.to_ascii_lowercase() == wanted {
        return true;
    }
    node_labels(node)
        .iter()
        .any(|label| label.to_ascii_lowercase() == wanted)
}

/// `nameMatches(node, name, exact)`
fn name_matches(node: &GraphNode, name: &Option<String>, exact: bool) -> bool {
    let Some(name) = name else { return true };
    let values: Vec<String> = [node.name.clone(), node_qualified_name(node)]
        .into_iter()
        .flatten()
        .collect();
    if exact {
        return values.iter().any(|v| v == name);
    }
    let wanted = name.to_ascii_lowercase();
    values
        .iter()
        .any(|v| v.to_ascii_lowercase().contains(&wanted))
}

fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
}

fn sort_key(node: &GraphNode) -> (String, String, String) {
    let path = node.path.clone().unwrap_or_default();
    let secondary = node_qualified_name(node)
        .or_else(|| node.name.clone())
        .unwrap_or_else(|| node.id.clone());
    (path, secondary, node.id.clone())
}

/// Native port of `searchAstStructure(generation, pattern, options)`.
pub fn search_ast_structure(generation: &GraphGeneration, pattern: &SearchPattern) -> SearchResult {
    let limit = safe_limit(pattern.limit, DEFAULT_LIMIT);
    let path_prefix = pattern.path_prefix.as_deref().map(normalize_path);

    let mut nodes: Vec<&GraphNode> = generation
        .nodes
        .iter()
        .filter(|node| kind_matches(node, &pattern.kind))
        .filter(|node| name_matches(node, &pattern.name, pattern.exact_name))
        .filter(|node| match &path_prefix {
            None => true,
            Some(prefix) => normalize_path(node.path.as_deref().unwrap_or("")).starts_with(prefix.as_str()),
        })
        .filter(|node| match &pattern.declaring_type {
            None => true,
            Some(declaring_type) => {
                let declaring = evidence0(node)
                    .and_then(|e| e.get("declaringType"))
                    .and_then(|v| v.as_str());
                if declaring == Some(declaring_type.as_str()) {
                    return true;
                }
                node_qualified_name(node)
                    .map(|qn| qn.starts_with(&format!("{declaring_type}.")))
                    .unwrap_or(false)
            }
        })
        .filter(|node| match &pattern.label {
            None => true,
            Some(label) => node_labels(node).contains(label),
        })
        .collect();

    nodes.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));

    let selected: Vec<GraphNode> = nodes.iter().take(limit).map(|n| (*n).clone()).collect();
    let selected_ids: BTreeSet<&str> = selected.iter().map(|n| n.id.as_str()).collect();

    let mut edges: Vec<GraphEdge> = generation
        .edges
        .iter()
        .filter(|edge| match &pattern.relation {
            None => true,
            Some(relation) => &edge.kind == relation,
        })
        .filter(|edge| {
            selected_ids.contains(edge.source.as_str())
                || edge.target.as_deref().map(|t| selected_ids.contains(t)).unwrap_or(false)
        })
        .cloned()
        .collect();
    edges.sort_by(|a, b| a.id.cmp(&b.id));
    edges.truncate(limit * 4);

    let truncated = nodes.len() > selected.len();
    let omissions = if truncated {
        vec![json!({"reason": "structural_search_limit", "limit": limit})]
    } else {
        Vec::new()
    };

    SearchResult {
        schema_version: 1,
        kind: "ast-structural-search",
        generation_id: if generation.generation_id.is_empty() {
            None
        } else {
            Some(generation.generation_id.clone())
        },
        pattern: pattern.to_json(),
        nodes: selected,
        edges,
        truncated,
        omissions,
    }
}
