//! Test recommendation for an impacted symbol set (ported from
//! `blueprint/src/graph/test-recommendation.mjs`). Operates over an
//! already-loaded [`GraphGeneration`] rather than opening storage directly.

use crate::graph::GraphGeneration;
use crate::model::{GraphEdge, GraphNode};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashSet};

fn public_evidence(node: Option<&GraphNode>, edges: &[&GraphEdge]) -> Vec<Value> {
    let mut rows: Vec<Value> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut items: Vec<&Value> = Vec::new();
    if let Some(node) = node {
        items.extend(node.evidence.iter());
    }
    for edge in edges {
        items.extend(edge.evidence.iter());
    }
    for item in items {
        if item.is_null() {
            continue;
        }
        let row = json!({
            "path": item.get("path").cloned().unwrap_or(Value::Null),
            "startLine": item.get("startLine").cloned().unwrap_or(Value::Null),
            "endLine": item.get("endLine").cloned().unwrap_or(Value::Null),
            "contentHash": item.get("contentHash").cloned().unwrap_or(Value::Null),
        });
        let key = row.to_string();
        if seen.insert(key) {
            rows.push(row);
        }
    }
    rows
}

pub fn recommend_tests_for_impact(
    generation: &GraphGeneration,
    generation_id: &str,
    impacted_ids: &[String],
    max_recommendations: Option<u64>,
) -> Value {
    let mut seen: HashSet<&str> = HashSet::new();
    let targets: Vec<String> = impacted_ids
        .iter()
        .map(|s| s.as_str())
        .filter(|s| !s.is_empty())
        .filter(|s| seen.insert(s))
        .take(500)
        .map(|s| s.to_owned())
        .collect();
    // JavaScript's `Number(value) || 50` treats an explicit zero as the
    // default, before applying the [1, 200] ceiling.
    let requested_cap = max_recommendations.filter(|value| *value != 0).unwrap_or(50);
    let cap = requested_cap.max(1).min(200) as usize;

    if generation_id.is_empty() || targets.is_empty() {
        let omissions = if !targets.is_empty() {
            vec![json!({"reason": "generation_missing"})]
        } else {
            vec![json!({"reason": "no_impacted_symbols"})]
        };
        return json!({
            "schemaVersion": 1, "kind": "test-recommendations", "generationId": if generation_id.is_empty() { Value::Null } else { json!(generation_id) },
            "recommendations": [], "uncoveredImpact": targets,
            "coverage": {"impacted": targets.len(), "covered": 0, "ratio": if targets.is_empty() { Value::Null } else { json!(0) }},
            "omissions": omissions, "minimality": "not_proven", "truncated": false,
        });
    }

    let target_set: HashSet<&str> = targets.iter().map(|s| s.as_str()).collect();
    let mut rows: Vec<&GraphEdge> = generation
        .edges
        .iter()
        .filter(|edge| edge.kind == "TESTS" && edge.target.as_deref().is_some_and(|t| target_set.contains(t)))
        .collect();
    rows.sort_by(|a, b| a.source.cmp(&b.source).then_with(|| a.target.cmp(&b.target)).then_with(|| a.id.cmp(&b.id)));
    rows.truncate(5000);

    let mut grouped: BTreeMap<&str, Vec<&GraphEdge>> = BTreeMap::new();
    for edge in &rows {
        grouped.entry(edge.source.as_str()).or_default().push(edge);
    }

    let nodes_by_id: BTreeMap<&str, &GraphNode> = generation.nodes.iter().map(|n| (n.id.as_str(), n)).collect();

    let mut all: Vec<(usize, Value, Vec<String>)> = grouped
        .into_iter()
        .map(|(test_id, edges)| {
            let node = nodes_by_id.get(test_id).copied();
            let mut covered_targets: BTreeSet<String> = BTreeSet::new();
            for edge in &edges {
                if let Some(target) = &edge.target {
                    covered_targets.insert(target.clone());
                }
            }
            let covered_targets: Vec<String> = covered_targets.into_iter().collect();
            let name = node
                .and_then(|n| n.evidence.iter().find_map(|e| e.get("qualifiedName").and_then(Value::as_str)))
                .map(|s| s.to_owned())
                .or_else(|| node.and_then(|n| n.name.clone()))
                .unwrap_or_else(|| test_id.to_owned());
            let path = node.and_then(|n| n.path.clone());
            let value = json!({
                "testId": test_id,
                "path": path,
                "name": name,
                "reason": "first_class_TESTS_edge_covers_impacted_symbol",
                "coveredTargets": covered_targets,
                "coverageCount": covered_targets.len(),
                "evidence": public_evidence(node, &edges),
            });
            (covered_targets.len(), value, covered_targets)
        })
        .collect();
    // sort desc by coverageCount, then path asc, then testId asc
    all.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| {
                let pa = a.1.get("path").and_then(Value::as_str).unwrap_or("");
                let pb = b.1.get("path").and_then(Value::as_str).unwrap_or("");
                pa.cmp(pb)
            })
            .then_with(|| {
                let ta = a.1.get("testId").and_then(Value::as_str).unwrap_or("");
                let tb = b.1.get("testId").and_then(Value::as_str).unwrap_or("");
                ta.cmp(tb)
            })
    });

    let total = all.len();
    let recommendations: Vec<Value> = all.iter().take(cap).map(|(_, v, _)| v.clone()).collect();
    let mut covered: HashSet<String> = HashSet::new();
    for (_, _, covered_targets) in &all {
        for t in covered_targets {
            covered.insert(t.clone());
        }
    }
    let uncovered_impact: Vec<String> = targets.iter().filter(|id| !covered.contains(id.as_str())).cloned().collect();

    let mut omissions: Vec<Value> = Vec::new();
    if rows.is_empty() {
        omissions.push(json!({"reason": "no_static_test_reachability_evidence"}));
    }
    if total > recommendations.len() {
        omissions.push(json!({"reason": "recommendation_ceiling", "count": total - recommendations.len()}));
    }
    if targets.len() >= 500 && impacted_ids.len() > 500 {
        omissions.push(json!({"reason": "impact_target_ceiling", "count": impacted_ids.len() - 500}));
    }

    json!({
        "schemaVersion": 1, "kind": "test-recommendations", "generationId": generation_id,
        "recommendations": recommendations, "uncoveredImpact": uncovered_impact,
        "coverage": {"impacted": targets.len(), "covered": covered.len(), "ratio": if targets.is_empty() { Value::Null } else { json!(covered.len() as f64 / targets.len() as f64) }},
        "omissions": omissions, "minimality": "not_proven",
        "truncated": total > recommendations.len() || impacted_ids.len() > 500,
    })
}
