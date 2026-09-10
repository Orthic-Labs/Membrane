//! Parity coverage for the native, store-driven `Operation::Recall`
//! (lane GC15): legacy `blueprint/src/graph/recall-circuit.mjs`'s
//! `executeRecallCircuit` + `recallCircuitToCandidateSet`, wired through
//! `blueprint/src/lib/application/service.mjs`'s `recall` handler
//! (seed/anchor/policy wiring, `staleRow`/`staleSourcePolicy`/`suppressRows`
//! stale-source suppression, `recallOrientation`).
//!
//! Ported to this crate's actual storage shape: `query.rs` is a projection
//! over an already-loaded [`GraphGeneration`] (no live SQLite handle at
//! this layer), so seed resolution and traversal are in-memory scans over
//! `generation.nodes`/`generation.edges` (see `recall_circuit::resolve_seeds_native`
//! / `traversal_neighbors_native`) rather than SQL against
//! `files`/`symbols`/`symbol_terms`. See the doc comment on `query::recall_op`
//! for the stale-source-suppression scoping note (input-driven, since no
//! freshness receipt reaches this layer).

use membrane_blueprint::query::execute_query;
use membrane_blueprint::{BlueprintRequest, Bounds, GraphEdge, GraphGeneration, GraphNode, Operation, RequestContext};
use serde_json::json;

fn context(request: &BlueprintRequest) -> RequestContext {
    request.validate(Bounds::default()).unwrap()
}

fn node(id: &str, path: &str, name: &str, qualified: &str) -> GraphNode {
    GraphNode {
        id: id.into(),
        kind: "function".into(),
        path: Some(path.into()),
        name: Some(name.into()),
        generation_id: "gen-recall".into(),
        evidence: vec![json!({"qualifiedName": qualified, "path": path, "startLine": 1, "endLine": 4, "contentHash": "sha256:c"})],
    }
}

fn edge(id: &str, source: &str, target: &str, tier: &str) -> GraphEdge {
    GraphEdge {
        id: id.into(),
        kind: "CALLS".into(),
        source: source.into(),
        target: Some(target.into()),
        generation_id: "gen-recall".into(),
        evidence: vec![json!({"confidenceTier": tier})],
    }
}

/// (a) basic: single seed, one-hop chain -> candidateSet + recallCircuit
/// paths are both populated, and the circuit is genuinely exercised (not a
/// stubbed empty fallback).
#[test]
fn recall_produces_candidate_set_and_circuit_paths() {
    let generation = GraphGeneration {
        schema_version: 1, provider: "test".into(), provider_version: "1".into(), generation_id: "gen-recall".into(),
        source_hash: "hash".into(), repo_root: "/repo".into(), complete: true,
        nodes: vec![
            node("symbol:seed", "src/seed.rs", "seed", "seed"),
            node("symbol:target", "src/target.rs", "target", "target"),
        ],
        edges: vec![edge("edge:seed->target", "symbol:seed", "symbol:target", "EXACT_RESOLUTION")],
        files: vec![], truncation_reasons: vec![],
    };
    let mut request = BlueprintRequest::new("q", Operation::Recall, "/repo");
    request.generation = Some("gen-recall".into());
    request.input["seed"] = json!("symbol:seed");
    let result = execute_query(&generation, &request, &context(&request)).unwrap();

    assert_eq!(result["candidateSet"]["schemaVersion"], 1);
    assert!(result["candidateSet"]["candidates"].as_array().unwrap().iter().any(|c| c["id"] == "symbol:target"), "candidateSet must contain the reached terminal node");
    assert!(!result["candidateSet"]["candidates"].as_array().unwrap().is_empty());

    let circuit = &result["recallCircuit"];
    assert_eq!(circuit["kind"], "RecallCircuit");
    assert_eq!(circuit["state"], "complete");
    let paths = circuit["paths"].as_array().unwrap();
    assert!(!paths.is_empty(), "recall circuit must produce at least one evidence path, proving the circuit ran rather than a stub");
    assert!(paths.iter().any(|p| p["terminalId"] == "symbol:target"));
    assert_eq!(result["orientation"]["action"], "allow");
}

/// (b) stale-source suppression: a candidate/path reachable only through a
/// node whose source path is marked stale must be suppressed from BOTH
/// `candidateSet.candidates` and `recallCircuit.paths`, with a typed
/// `stale_source_suppressed` omission recorded — mirroring legacy
/// `suppressRows`/`staleRow` acting on both views independently.
#[test]
fn stale_source_is_suppressed_from_candidates_and_paths() {
    let generation = GraphGeneration {
        schema_version: 1, provider: "test".into(), provider_version: "1".into(), generation_id: "gen-recall".into(),
        source_hash: "hash".into(), repo_root: "/repo".into(), complete: true,
        nodes: vec![
            node("symbol:seed", "src/seed.rs", "seed", "seed"),
            node("symbol:fresh", "src/fresh.rs", "fresh", "fresh"),
            node("symbol:stale", "src/stale.rs", "stale", "stale"),
        ],
        edges: vec![
            edge("edge:seed->fresh", "symbol:seed", "symbol:fresh", "EXACT_RESOLUTION"),
            edge("edge:seed->stale", "symbol:seed", "symbol:stale", "EXACT_RESOLUTION"),
        ],
        files: vec![], truncation_reasons: vec![],
    };
    let mut request = BlueprintRequest::new("q", Operation::Recall, "/repo");
    request.generation = Some("gen-recall".into());
    request.input["seed"] = json!("symbol:seed");
    request.input["staleSourcePaths"] = json!(["src/stale.rs"]);
    let result = execute_query(&generation, &request, &context(&request)).unwrap();

    let candidates = result["candidateSet"]["candidates"].as_array().unwrap();
    assert!(candidates.iter().any(|c| c["id"] == "symbol:fresh"), "fresh candidate must survive suppression");
    assert!(!candidates.iter().any(|c| c["id"] == "symbol:stale"), "stale-source candidate must be suppressed");

    let paths = result["recallCircuit"]["paths"].as_array().unwrap();
    assert!(!paths.iter().any(|p| p["terminalId"] == "symbol:stale"), "stale-source path must be suppressed from recallCircuit.paths");
    assert!(paths.iter().any(|p| p["terminalId"] == "symbol:fresh"), "fresh path must remain in recallCircuit.paths");

    let circuit_omissions = result["recallCircuit"]["omissions"].as_array().unwrap();
    assert!(circuit_omissions.iter().any(|o| o["reason"] == "stale_source_suppressed"), "suppression must be recorded as a typed omission");
}

/// (c) BPT-026 non-compensatory ranking: a shallow (1-hop) but weak-tier
/// path must NOT outrank a deeper (2-hop) but strong-tier path in
/// `recallCircuit.paths` ordering, proving `compare_paths` is actually
/// exercised on the store-driven traversal rather than the response
/// silently falling back to first-discovered (BFS) order.
#[test]
fn circuit_ranking_overrides_bfs_discovery_order() {
    let generation = GraphGeneration {
        schema_version: 1, provider: "test".into(), provider_version: "1".into(), generation_id: "gen-recall".into(),
        source_hash: "hash".into(), repo_root: "/repo".into(), complete: true,
        nodes: vec![
            node("symbol:seed", "src/seed.rs", "seed", "seed"),
            node("symbol:weak_near", "src/weak.rs", "weak_near", "weak_near"),
            node("symbol:mid", "src/mid.rs", "mid", "mid"),
            node("symbol:strong_far", "src/strong.rs", "strong_far", "strong_far"),
        ],
        edges: vec![
            // 1-hop, weak (UNRESOLVED) tier: BFS reaches and completes this first.
            edge("edge:seed->weak", "symbol:seed", "symbol:weak_near", "UNRESOLVED"),
            // 2-hop, both EXACT_RESOLUTION: worse hop count, but the
            // non-compensatory ranking (tier before hop count) must still
            // place it ahead of the shallower weak-tier path.
            edge("edge:seed->mid", "symbol:seed", "symbol:mid", "EXACT_RESOLUTION"),
            edge("edge:mid->strong", "symbol:mid", "symbol:strong_far", "EXACT_RESOLUTION"),
        ],
        files: vec![], truncation_reasons: vec![],
    };
    let mut request = BlueprintRequest::new("q", Operation::Recall, "/repo");
    request.generation = Some("gen-recall".into());
    request.input["seed"] = json!("symbol:seed");
    request.input["maxHops"] = json!(2);
    let result = execute_query(&generation, &request, &context(&request)).unwrap();

    let paths = result["recallCircuit"]["paths"].as_array().unwrap();
    assert!(paths.len() >= 2, "both the weak near path and the strong far path must be produced for ranking to be observable");
    let terminals: Vec<&str> = paths.iter().map(|p| p["terminalId"].as_str().unwrap()).collect();
    let weak_index = terminals.iter().position(|t| *t == "symbol:weak_near").unwrap();
    let strong_index = terminals.iter().position(|t| *t == "symbol:strong_far").unwrap();
    assert!(
        strong_index < weak_index,
        "non-compensatory ranking must place the stronger-tier deeper path ({strong_index}) ahead of the weaker-tier shallower path ({weak_index}), proving compare_paths ran on the store-driven traversal rather than BFS discovery order surviving unranked"
    );
    assert_eq!(paths[0]["terminalId"], "symbol:strong_far");
}

/// Unresolved seed still reports the native circuit's typed abstain state,
/// not a bare BFS "no match".
#[test]
fn unresolved_seed_reports_abstained_circuit() {
    let generation = GraphGeneration {
        schema_version: 1, provider: "test".into(), provider_version: "1".into(), generation_id: "gen-recall".into(),
        source_hash: "hash".into(), repo_root: "/repo".into(), complete: true,
        nodes: vec![node("symbol:seed", "src/seed.rs", "seed", "seed")],
        edges: vec![], files: vec![], truncation_reasons: vec![],
    };
    let mut request = BlueprintRequest::new("q", Operation::Recall, "/repo");
    request.generation = Some("gen-recall".into());
    request.input["seed"] = json!("does-not-exist");
    let result = execute_query(&generation, &request, &context(&request)).unwrap();
    assert_eq!(result["state"], "unresolved");
    assert_eq!(result["recallCircuit"]["state"], "abstained");
    assert!(result["candidateSet"]["candidates"].as_array().unwrap().is_empty());
}
