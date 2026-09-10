//! Parity tests ported from `blueprint/tests/test-recommendation.test.mjs`
//! and behavior documented in `blueprint/src/graph/analytics/change-impact.mjs`
//! and `blueprint/src/graph/analytics/index.mjs`. Ported against the native
//! `src/analytics.rs`, `src/change_impact.rs`, and `src/test_recommendation.rs`
//! modules operating over a real synthetic in-memory `GraphGeneration`
//! fixture (the native crate's own idiom -- there is no SQLite store here).

use membrane_blueprint::analytics::{
    assign_layers, decompose_change_risk, find_cycles, find_sccs, Edge, NodeRef, RiskInput,
};
use membrane_blueprint::change_impact::{resolve_impact_seed_envelope, SeedEnvelopeInput};
use membrane_blueprint::test_recommendation::recommend_tests_for_impact;
use membrane_blueprint::{GraphEdge, GraphGeneration, GraphNode};
use serde_json::json;

fn ev(path: &str, start: u64, end: u64) -> Vec<serde_json::Value> {
    vec![json!({"path": path, "startLine": start, "endLine": end, "contentHash": format!("{path}-hash")})]
}

fn fixture_generation() -> GraphGeneration {
    let id = "g-tests".to_owned();
    GraphGeneration {
        schema_version: 1, provider: "test".into(), provider_version: "1".into(), generation_id: id.clone(),
        source_hash: "hash".into(), repo_root: "/repo".into(), complete: true,
        nodes: vec![
            GraphNode { id: "test:a".into(), kind: "symbol".into(), path: Some("tests/a.test.ts".into()), name: Some("testA".into()), generation_id: id.clone(), evidence: ev("tests/a.test.ts", 1, 2) },
            GraphNode { id: "prod:a".into(), kind: "symbol".into(), path: Some("src/a.ts".into()), name: Some("a".into()), generation_id: id.clone(), evidence: ev("src/a.ts", 1, 2) },
            GraphNode { id: "prod:b".into(), kind: "symbol".into(), path: Some("src/b.ts".into()), name: Some("b".into()), generation_id: id.clone(), evidence: ev("src/b.ts", 5, 9) },
        ],
        edges: vec![
            GraphEdge { id: "tests:a".into(), kind: "TESTS".into(), source: "test:a".into(), target: Some("prod:a".into()), generation_id: id.clone(), evidence: vec![json!({"confidenceTier":"EXACT_RESOLUTION"})] },
        ],
        files: vec![], truncation_reasons: vec![],
    }
}

// "test recommendations use first-class TESTS evidence and report uncovered impact"
#[test]
fn test_recommendations_use_first_class_tests_evidence() {
    let generation = fixture_generation();
    let result = recommend_tests_for_impact(&generation, "g-tests", &["prod:a".to_owned(), "prod:b".to_owned()], None);
    assert_eq!(result["recommendations"].as_array().unwrap().len(), 1);
    assert_eq!(result["recommendations"][0]["testId"], "test:a");
    assert_eq!(result["recommendations"][0]["coveredTargets"], json!(["prod:a"]));
    assert_eq!(result["uncoveredImpact"], json!(["prod:b"]));
    assert_eq!(result["coverage"]["impacted"], 2);
    assert_eq!(result["coverage"]["covered"], 1);
    assert_eq!(result["coverage"]["ratio"], 0.5);
    assert_eq!(result["minimality"], "not_proven");
    assert!(result["recommendations"][0]["evidence"].as_array().unwrap().len() > 0);
}

// "absence of TESTS evidence is an omission, never a claim that no tests exist"
#[test]
fn absence_of_tests_evidence_is_an_omission() {
    let id = "g-empty".to_owned();
    let generation = GraphGeneration {
        schema_version: 1, provider: "test".into(), provider_version: "1".into(), generation_id: id.clone(),
        source_hash: "hash".into(), repo_root: "/repo".into(), complete: true,
        nodes: vec![GraphNode { id: "prod:a".into(), kind: "symbol".into(), path: Some("src/a.ts".into()), name: Some("a".into()), generation_id: id.clone(), evidence: ev("src/a.ts", 1, 2) }],
        edges: vec![], files: vec![], truncation_reasons: vec![],
    };
    let result = recommend_tests_for_impact(&generation, "g-empty", &["prod:a".to_owned()], None);
    assert_eq!(result["recommendations"], json!([]));
    assert_eq!(result["uncoveredImpact"], json!(["prod:a"]));
    assert!(result["omissions"].as_array().unwrap().iter().any(|row| row["reason"] == "no_static_test_reachability_evidence"));
    assert_eq!(result["minimality"], "not_proven");
}

#[test]
fn no_impacted_symbols_is_reported_not_silently_empty() {
    let generation = fixture_generation();
    let result = recommend_tests_for_impact(&generation, "g-tests", &[], None);
    assert!(result["omissions"].as_array().unwrap().iter().any(|row| row["reason"] == "no_impacted_symbols"));
}

// change-impact.mjs: `symbolAtLine` picks the smallest enclosing span, and a
// stack-trace location resolves to a seed via file:line -> node id.
#[test]
fn seed_envelope_resolves_stack_location_to_enclosing_symbol() {
    let generation = fixture_generation();
    let input = SeedEnvelopeInput {
        file: None, files: vec![], diff: None,
        stack: Some("at a (src/a.ts:1:3)\n    at b (src/b.ts:6:1)"),
        treeish_base: None, treeish_head: None, line: None, node_id: None, anchor: None, test: None, max_seeds: 32,
    };
    let envelope = resolve_impact_seed_envelope(&generation, "/repo", "g-tests", input);
    assert_eq!(envelope["kind"], "ImpactSeedEnvelope");
    assert_eq!(envelope["families"]["stack"], true);
    let seed_ids: Vec<&str> = envelope["seeds"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap()).collect();
    assert!(seed_ids.contains(&"prod:a"));
    assert!(seed_ids.contains(&"prod:b"));
}

#[test]
fn seed_envelope_resolves_diff_paths_as_changed_paths() {
    let generation = fixture_generation();
    let diff = "--- a/src/a.ts\n+++ b/src/a.ts\n@@ -1,1 +1,1 @@\n-old\n+new\n";
    let input = SeedEnvelopeInput {
        file: None, files: vec![], diff: Some(diff), stack: None,
        treeish_base: None, treeish_head: None, line: None, node_id: None, anchor: None, test: None, max_seeds: 32,
    };
    let envelope = resolve_impact_seed_envelope(&generation, "/repo", "g-tests", input);
    assert_eq!(envelope["families"]["diff"], true);
    assert_eq!(envelope["changedPaths"], json!(["src/a.ts"]));
}

#[test]
fn seed_envelope_explicit_file_that_does_not_exist_is_an_omission_not_a_guess() {
    let generation = fixture_generation();
    let input = SeedEnvelopeInput {
        file: Some("src/does-not-exist.ts"), files: vec![], diff: None, stack: None,
        treeish_base: None, treeish_head: None, line: None, node_id: None, anchor: None, test: None, max_seeds: 32,
    };
    let envelope = resolve_impact_seed_envelope(&generation, "/repo", "g-tests", input);
    // File-only anchors that don't resolve to an exact node/path are surfaced
    // as an omission rather than silently promoted to a seed.
    assert!(envelope["omissions"].as_array().unwrap().iter().any(|o| o["reason"] == "anchor_not_found"));
}

// analytics/index.mjs: cycle detection is SCCs of size > 1 (or a self-loop).
#[test]
fn find_cycles_detects_mutual_recursion_but_not_a_dag() {
    let cyclic = vec![Edge { source: "a".into(), target: "b".into() }, Edge { source: "b".into(), target: "a".into() }];
    let cycles = find_cycles(&cyclic);
    assert_eq!(cycles.len(), 1);
    assert_eq!(cycles[0], vec!["a".to_owned(), "b".to_owned()]);

    let acyclic = vec![Edge { source: "a".into(), target: "b".into() }, Edge { source: "b".into(), target: "c".into() }];
    assert!(find_cycles(&acyclic).is_empty());
}

#[test]
fn find_sccs_is_deterministic_and_sorted() {
    let edges = vec![
        Edge { source: "x".into(), target: "y".into() }, Edge { source: "y".into(), target: "x".into() },
        Edge { source: "y".into(), target: "z".into() },
    ];
    let sccs = find_sccs(&edges);
    // {x,y} forms a cycle; z is its own trivial component.
    assert!(sccs.iter().any(|c| c == &vec!["x".to_owned(), "y".to_owned()]));
    assert!(sccs.iter().any(|c| c == &vec!["z".to_owned()]));
}

#[test]
fn assign_layers_is_longest_path_from_roots() {
    let nodes = vec![NodeRef { id: "root".into() }, NodeRef { id: "mid".into() }, NodeRef { id: "leaf".into() }];
    let edges = vec![Edge { source: "root".into(), target: "mid".into() }, Edge { source: "mid".into(), target: "leaf".into() }];
    let layers = assign_layers(&nodes, &edges);
    let layer_of = |id: &str| layers.iter().find(|(n, _)| n == id).unwrap().1;
    assert_eq!(layer_of("root"), 0);
    assert_eq!(layer_of("mid"), 1);
    assert_eq!(layer_of("leaf"), 2);
}

// decomposeChangeRisk: co-change is deliberately low-authority (<= 0.15
// contribution) and can never make a structurally-empty change high risk.
#[test]
fn decompose_change_risk_keeps_cochange_low_authority() {
    let risk = decompose_change_risk(RiskInput {
        changed_paths: vec![], impacted_ids: vec![], edge_confidence_tiers: vec![],
        truncated: false, ambiguous_seeds: 0, stale: false, cochange_score: 1.0,
    });
    assert_eq!(risk["authority"], "advisory_not_truth");
    assert_eq!(risk["band"], "low");
    let cochange_factor = risk["factors"].as_array().unwrap().iter().find(|f| f["id"] == "cochange").unwrap();
    assert!(cochange_factor["value"].as_f64().unwrap() <= 0.15 + 1e-9);
}

#[test]
fn decompose_change_risk_raises_uncertainty_for_heuristic_edges_and_staleness() {
    let low = decompose_change_risk(RiskInput {
        changed_paths: vec!["a".into()], impacted_ids: vec!["x".into()],
        edge_confidence_tiers: vec![Some("EXACT_RESOLUTION".into())],
        truncated: false, ambiguous_seeds: 0, stale: false, cochange_score: 0.0,
    });
    let high = decompose_change_risk(RiskInput {
        changed_paths: vec!["a".into()], impacted_ids: vec!["x".into()],
        edge_confidence_tiers: vec![Some("CROSS_FILE_HEURISTIC".into())],
        truncated: true, ambiguous_seeds: 5, stale: true, cochange_score: 0.0,
    });
    assert!(high["score"].as_f64().unwrap() > low["score"].as_f64().unwrap());
}
