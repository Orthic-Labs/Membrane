//! Parity tests ported from `blueprint/tests/recall-circuit.test.mjs` and
//! `blueprint/tests/recall-candidate-contract.test.mjs` (BPT-026).
//!
//! Scope note (see `src/recall_circuit.rs` module doc): the legacy
//! `executeRecallCircuit` drives its BFS off a live SQLite store via
//! `resolveSeeds`/`selectTraversalPolicy`/`traversalNeighbors`, none of
//! which have native ports in this crate. So the "exact anchor resolution +
//! determinism + evidence completeness" and "abstain without inventing
//! semantic matches" legacy tests are ported here against the native
//! `execute_recall_circuit`'s in-memory `RecallGraph` input instead of a
//! built `.agent/graph/graph.db` — real synthetic node/edge/seed fixtures,
//! not mocks. The `comparePaths` (BPT-026) contract tests are ported
//! field-for-field since that comparator is fully native.

use membrane_blueprint::recall_circuit::{
    adjacency, compare_paths, execute_recall_circuit, make_path, recall_circuit_to_candidate_set,
    stable_digest, AtomicPath, RecallGraph, Seed, TraversalPolicy,
};
use serde_json::{json, Value};
use std::collections::HashMap;

fn path(id: &str, overrides: impl Fn(&mut AtomicPath)) -> AtomicPath {
    let mut p = AtomicPath {
        id: id.to_string(),
        seed_id: format!("symbol:{id}"),
        terminal_id: Some(format!("symbol:{id}")),
        node_ids: vec![],
        edge_ids: vec![],
        minimum_edge_tier: "EXACT_RESOLUTION".to_string(),
        minimum_semantic_authority: None,
        semantic_authority_rank: 0,
        seed_exactness: 0,
        evidence_coverage: 1.0,
        hop_count: 1,
        state: "complete",
        omission_reasons: vec![],
        evidence_envelope: json!({ "id": format!("envelope:{id}") }),
    };
    overrides(&mut p);
    p
}

// "RecallCircuit resolves exact anchors, returns evidence paths, and is
// deterministic" — ported against the in-memory graph input.
#[test]
fn recall_circuit_resolves_exact_anchor_and_is_deterministic() {
    let mut nodes: HashMap<String, Value> = HashMap::new();
    nodes.insert(
        "entry".into(),
        json!({"id": "entry", "name": "main", "evidence": [{"path": "src/entry.js", "startLine": 2, "endLine": 2, "contentHash": "1".repeat(32)}]}),
    );
    nodes.insert(
        "worker".into(),
        json!({"id": "worker", "name": "work", "evidence": [{"path": "src/worker.js", "startLine": 1, "endLine": 1, "contentHash": "2".repeat(32)}]}),
    );
    let edges = vec![json!({
        "id": "edge1", "source": "entry", "target": "worker",
        "confidence_tier": "EXACT_RESOLUTION", "confidenceTier": "EXACT_RESOLUTION",
        "evidence": [{"path": "src/entry.js", "startLine": 1, "endLine": 1, "contentHash": "3".repeat(32)}],
    })];
    let graph = RecallGraph { nodes, edges };
    let seeds = vec![Seed { id: "entry".into(), exactness: 0, reason: None, evidence: Value::Null }];
    let policy = TraversalPolicy { family: "dependency.forward".into(), direction: "out".into(), ..TraversalPolicy::default() };

    let first = execute_recall_circuit(&graph, &seeds, &policy, "gen1");
    let second = execute_recall_circuit(&graph, &seeds, &policy, "gen1");
    assert_eq!(first.id, second.id, "same input must produce the same circuit id");
    assert_eq!(first.state, "complete");
    assert!(!first.paths.is_empty());
    for p in &first.paths {
        assert_eq!(p.node_ids.len(), p.edge_ids.len() + 1);
    }
    // At least the terminal path (entry->worker) must carry evidence.
    assert!(first.paths.iter().any(|p| p.evidence_coverage > 0.0));
    for p in &first.paths {
        // semanticAuthorityRank is always an integer rank (i64 here, so this
        // is structurally guaranteed) — assert it is non-negative instead.
        assert!(p.semantic_authority_rank >= 0);
    }

    let candidates = recall_circuit_to_candidate_set(&first, "trace dependencies", "blueprint-static", "2026-09-10T00:00:00Z");
    assert_eq!(candidates["recallCircuit"]["id"], json!(first.id));
    for c in candidates["candidates"].as_array().unwrap() {
        assert!(c.get("evidencePathId").is_some());
        assert!(c["scoreComponents"]["semanticAuthority"].as_f64().unwrap() > 0.0);
    }
}

// "RecallCircuit abstains without inventing semantic matches"
#[test]
fn recall_circuit_abstains_with_no_seeds() {
    let graph = RecallGraph { nodes: HashMap::new(), edges: vec![] };
    let policy = TraversalPolicy::default();
    let circuit = execute_recall_circuit(&graph, &[], &policy, "gen1");
    assert_eq!(circuit.state, "abstained");
    assert!(circuit.paths.is_empty());
    assert_eq!(circuit.omissions.len(), 1);
    assert!(circuit.omissions[0].get("reason").is_some());
}

// BPT-026 contract, ported field-for-field from recall-candidate-contract.test.mjs:
// "each ordering tier decides in its declared position, and only when the
// tiers above it are equal"
#[test]
fn each_ordering_tier_decides_in_declared_position_only_when_tiers_above_are_equal() {
    let cases: Vec<(&str, Box<dyn Fn(&mut AtomicPath)>)> = vec![
        ("state", Box::new(|p: &mut AtomicPath| p.state = "partial")),
        ("semanticAuthorityRank", Box::new(|p: &mut AtomicPath| p.semantic_authority_rank = 5)),
        ("minimumEdgeTier", Box::new(|p: &mut AtomicPath| p.minimum_edge_tier = "UNRESOLVED".into())),
        ("seedExactness", Box::new(|p: &mut AtomicPath| p.seed_exactness = 9)),
        ("evidenceCoverage", Box::new(|p: &mut AtomicPath| p.evidence_coverage = 0.0)),
        ("hopCount", Box::new(|p: &mut AtomicPath| p.hop_count = 9)),
    ];
    for (label, worsen) in cases {
        let better = path("zzz", |_| {});
        let loser = path("aaa", |p| worsen(p));
        let mut both = vec![loser, better];
        both.sort_by(compare_paths);
        assert_eq!(
            both.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            vec!["zzz", "aaa"],
            "{label} must decide when every tier above it is equal"
        );
    }

    // Precedence: two tiers disagree, higher-priority tier must still win.
    let winner_state = path("zzz", |p| { p.state = "complete"; p.semantic_authority_rank = 5; });
    let loser_state = path("aaa", |p| { p.state = "partial"; p.semantic_authority_rank = 0; });
    let mut v = vec![loser_state, winner_state];
    v.sort_by(compare_paths);
    assert_eq!(v.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), vec!["zzz", "aaa"]);

    let winner_auth = path("zzz", |p| { p.semantic_authority_rank = 0; p.minimum_edge_tier = "UNRESOLVED".into(); });
    let loser_auth = path("aaa", |p| { p.semantic_authority_rank = 5; p.minimum_edge_tier = "EXACT_RESOLUTION".into(); });
    let mut v = vec![loser_auth, winner_auth];
    v.sort_by(compare_paths);
    assert_eq!(v.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), vec!["zzz", "aaa"]);

    let winner_tier = path("zzz", |p| { p.minimum_edge_tier = "EXACT_RESOLUTION".into(); p.seed_exactness = 9; });
    let loser_tier = path("aaa", |p| { p.minimum_edge_tier = "UNRESOLVED".into(); p.seed_exactness = 0; });
    let mut v = vec![loser_tier, winner_tier];
    v.sort_by(compare_paths);
    assert_eq!(v.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), vec!["zzz", "aaa"]);

    let winner_exact = path("zzz", |p| { p.seed_exactness = 0; p.evidence_coverage = 0.0; });
    let loser_exact = path("aaa", |p| { p.seed_exactness = 9; p.evidence_coverage = 1.0; });
    let mut v = vec![loser_exact, winner_exact];
    v.sort_by(compare_paths);
    assert_eq!(v.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), vec!["zzz", "aaa"]);

    let winner_cov = path("zzz", |p| { p.evidence_coverage = 1.0; p.hop_count = 9; });
    let loser_cov = path("aaa", |p| { p.evidence_coverage = 0.0; p.hop_count = 1; });
    let mut v = vec![loser_cov, winner_cov];
    v.sort_by(compare_paths);
    assert_eq!(v.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), vec!["zzz", "aaa"]);

    // tierRank ordering: each tier must rank strictly better than the next.
    let by_tier = ["EXACT_RESOLUTION", "SAME_FILE_LEXICAL", "CROSS_FILE_HEURISTIC", "UNRESOLVED"];
    for w in by_tier.windows(2) {
        let better = path("zzz", |p| p.minimum_edge_tier = w[0].to_string());
        let worse = path("aaa", |p| p.minimum_edge_tier = w[1].to_string());
        let mut v = vec![worse, better];
        v.sort_by(compare_paths);
        assert_eq!(v.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), vec!["zzz", "aaa"]);
    }

    // Full tie falls through to deterministic id tie-break.
    let mut v = vec![path("zzz", |_| {}), path("aaa", |_| {})];
    v.sort_by(compare_paths);
    assert_eq!(v.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), vec!["aaa", "zzz"]);
}

// "emitted candidate order follows the non-compensatory comparator, not a
// sum of scoreComponents"
#[test]
fn candidate_order_follows_comparator_not_sum_of_score_components() {
    let stronger_authority_weaker_else = path("path:a", |p| {
        p.terminal_id = Some("symbol:a".into());
        p.semantic_authority_rank = 0;
        p.minimum_edge_tier = "UNRESOLVED".into();
        p.evidence_coverage = 0.1;
    });
    let weaker_authority_stronger_else = path("path:b", |p| {
        p.terminal_id = Some("symbol:b".into());
        p.semantic_authority_rank = 1;
        p.minimum_edge_tier = "EXACT_RESOLUTION".into();
        p.evidence_coverage = 1.0;
    });

    let mut ordered = vec![weaker_authority_stronger_else.clone(), stronger_authority_weaker_else.clone()];
    ordered.sort_by(compare_paths);
    assert_eq!(ordered.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), vec!["path:a", "path:b"]);

    let circuit = membrane_blueprint::recall_circuit::RecallCircuitResult {
        id: "recall:test".into(),
        generation_id: "generation:test".into(),
        policy_family: "explore.both".into(),
        paths: ordered,
        omissions: vec![],
        state: "complete",
    };
    let result = recall_circuit_to_candidate_set(&circuit, "inspect work", "blueprint-static", "2026-09-10T00:00:00Z");
    let ids: Vec<String> = result["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(ids, vec!["symbol:a", "symbol:b"]);

    let sums: Vec<f64> = result["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            c["scoreComponents"]
                .as_object()
                .unwrap()
                .values()
                .map(|v| match v {
                    Value::Number(n) => n.as_f64().unwrap_or(0.0),
                    Value::Bool(true) => 1.0,
                    Value::Bool(false) => 0.0,
                    _ => 0.0,
                })
                .sum()
        })
        .collect();
    assert!(sums[1] > sums[0], "fixture must make summed scoreComponents disagree with the comparator");
    let mut by_sum: Vec<(usize, f64)> = sums.iter().copied().enumerate().collect();
    by_sum.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    let sum_order: Vec<usize> = by_sum.iter().map(|(i, _)| *i).collect();
    assert_ne!(sum_order, vec![0, 1], "summing scoreComponents must NOT reproduce the emitted (comparator) order");
}

#[test]
fn stable_digest_is_deterministic_for_equal_values() {
    let a = json!({"x": 1, "y": [1,2,3]});
    let b = json!({"x": 1, "y": [1,2,3]});
    assert_eq!(stable_digest(&a), stable_digest(&b));
    assert!(stable_digest(&a).starts_with("sha256:"));
}

#[test]
fn make_path_computes_worst_authority_and_tier_among_edges() {
    let mut nodes: HashMap<String, Value> = HashMap::new();
    nodes.insert("a".into(), json!({"id": "a"}));
    nodes.insert("b".into(), json!({"id": "b"}));
    nodes.insert("c".into(), json!({"id": "c"}));
    let mut edges: HashMap<String, Value> = HashMap::new();
    edges.insert("e1".into(), json!({"id": "e1", "source": "a", "target": "b", "confidenceTier": "EXACT_RESOLUTION"}));
    edges.insert("e2".into(), json!({"id": "e2", "source": "b", "target": "c", "confidenceTier": "CROSS_FILE_HEURISTIC"}));
    let seed = Seed { id: "a".into(), exactness: 0, reason: None, evidence: Value::Null };
    let p = make_path(&seed, &["a".into(), "b".into(), "c".into()], &["e1".into(), "e2".into()], &nodes, &edges, true, "gen1");
    assert_eq!(p.minimum_edge_tier, "CROSS_FILE_HEURISTIC");
    assert_eq!(p.hop_count, 2);
    assert_eq!(p.state, "complete");
}

#[test]
fn adjacency_respects_direction_out_only() {
    let edges = vec![json!({"id": "e1", "source": "a", "target": "b", "confidence_tier": "EXACT_RESOLUTION"})];
    let adj = adjacency(&edges, "out");
    assert!(adj.contains_key("a"));
    assert!(!adj.contains_key("b"));
}
