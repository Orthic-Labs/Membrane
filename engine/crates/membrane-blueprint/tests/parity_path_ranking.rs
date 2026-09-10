//! Regression coverage for lane GC14: `Operation::Path` must rank bounded
//! alternative from->to candidates with the ported
//! `recall_circuit::compare_paths` non-compensatory ordering (BPT-026)
//! rather than returning whichever path bounded BFS happened to reach
//! first (shortest hop count). Ported from the intent of the legacy
//! `blueprint/src/graph/recall-circuit.mjs` `comparePaths` contract tests:
//! semantic authority / edge confidence tier outranks hop count.

use membrane_blueprint::query::execute_query;
use membrane_blueprint::{BlueprintRequest, Bounds, GraphEdge, GraphGeneration, GraphNode, Operation, RequestContext};
use serde_json::json;

/// Two routes from `a` to `d`:
/// - direct 1-hop edge `a->d` carrying `UNRESOLVED` confidence (worst
///   semantic authority rank).
/// - 2-hop `a->b->d` where both edges carry `EXACT_RESOLUTION` confidence
///   (best semantic authority rank).
///
/// A plain shortest-hop BFS returns the direct 1-hop edge first. The ranked
/// picker must instead prefer the 2-hop route: BPT-026 places semantic
/// authority strictly ahead of hop count in the non-compensatory ordering,
/// so a stronger-evidence longer path always outranks a weaker-evidence
/// shorter one.
fn generation() -> GraphGeneration {
    let id = "generation-path-ranking".to_owned();
    let node = |node_id: &str| GraphNode {
        id: node_id.into(), kind: "function".into(), path: Some("src/x.rs".into()), name: Some(node_id.into()),
        generation_id: id.clone(), evidence: vec![json!({"path":"src/x.rs","contentHash":"sha256:x"})],
    };
    GraphGeneration {
        schema_version: 1, provider: "test".into(), provider_version: "1".into(), generation_id: id.clone(), source_hash: "hash".into(), repo_root: "/repo".into(), complete: true,
        nodes: vec![node("a"), node("b"), node("d")],
        edges: vec![
            GraphEdge {
                id: "edge:a-d:direct".into(), kind: "CALLS".into(), source: "a".into(), target: Some("d".into()),
                generation_id: id.clone(), evidence: vec![json!({"confidenceTier":"UNRESOLVED"})],
            },
            GraphEdge {
                id: "edge:a-b:strong".into(), kind: "CALLS".into(), source: "a".into(), target: Some("b".into()),
                generation_id: id.clone(), evidence: vec![json!({"confidenceTier":"EXACT_RESOLUTION"})],
            },
            GraphEdge {
                id: "edge:b-d:strong".into(), kind: "CALLS".into(), source: "b".into(), target: Some("d".into()),
                generation_id: id, evidence: vec![json!({"confidenceTier":"EXACT_RESOLUTION"})],
            },
        ],
        files: vec![], truncation_reasons: vec![],
    }
}

fn context(request: &BlueprintRequest) -> RequestContext { request.validate(Bounds::default()).unwrap() }

#[test]
fn ranked_path_prefers_stronger_evidence_over_fewer_hops() {
    let mut request = BlueprintRequest::new("q", Operation::Path, "/repo");
    request.generation = Some("generation-path-ranking".into());
    request.input["from"] = json!("a");
    request.input["to"] = json!("d");
    let result = execute_query(&generation(), &request, &context(&request)).unwrap();

    assert_eq!(result["found"], true);
    let edges = result["edges"].as_array().unwrap();
    // Ranked-best is the 2-hop EXACT_RESOLUTION route, not the 1-hop
    // UNRESOLVED shortcut a BFS-first-found picker would have returned.
    assert_eq!(edges.len(), 2);
    let edge_ids: Vec<&str> = edges.iter().map(|e| e["id"].as_str().unwrap()).collect();
    assert_eq!(edge_ids, vec!["edge:a-b:strong", "edge:b-d:strong"]);

    let path_ids: Vec<&str> = result["path"].as_array().unwrap().iter().map(|n| n["id"].as_str().unwrap()).collect();
    assert_eq!(path_ids, vec!["a", "b", "d"]);
}

#[test]
fn same_node_path_still_matches_trivially() {
    let mut request = BlueprintRequest::new("q", Operation::Path, "/repo");
    request.generation = Some("generation-path-ranking".into());
    request.input["from"] = json!("a");
    request.input["to"] = json!("a");
    let result = execute_query(&generation(), &request, &context(&request)).unwrap();
    assert_eq!(result["found"], true);
    assert_eq!(result["edges"].as_array().unwrap().len(), 0);
    assert_eq!(result["path"].as_array().unwrap().len(), 1);
}

#[test]
fn unreachable_target_is_unresolved_not_fabricated() {
    let mut request = BlueprintRequest::new("q", Operation::Path, "/repo");
    request.generation = Some("generation-path-ranking".into());
    request.input["from"] = json!("d");
    request.input["to"] = json!("a");
    let result = execute_query(&generation(), &request, &context(&request)).unwrap();
    assert_eq!(result["found"], false);
    assert_eq!(result["state"], "unresolved");
    assert_eq!(result["omissions"][0]["reason"], "path_not_found");
}
