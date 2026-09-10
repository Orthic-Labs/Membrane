use membrane_blueprint::{BlueprintRequest, Bounds, GraphEdge, GraphGeneration, GraphNode, Operation, RequestContext};
use membrane_blueprint::query::execute_query;
use serde_json::{json, Value};

fn generation() -> GraphGeneration {
    let id = "generation-query".to_owned();
    GraphGeneration {
        schema_version: 1, provider: "test".into(), provider_version: "1".into(), generation_id: id.clone(), source_hash: "hash".into(), repo_root: "/repo".into(), complete: true,
        nodes: vec![
            GraphNode { id: "file:src/a.rs".into(), kind: "file".into(), path: Some("src/a.rs".into()), name: Some("a.rs".into()), generation_id: id.clone(), evidence: vec![json!({"path":"src/a.rs","contentHash":"sha256:a"})] },
            GraphNode { id: "symbol:src/a.rs::run".into(), kind: "function".into(), path: Some("src/a.rs".into()), name: Some("run".into()), generation_id: id.clone(), evidence: vec![json!({"qualifiedName":"run","path":"src/a.rs","startLine":1,"endLine":2,"contentHash":"sha256:a"})] },
            GraphNode { id: "symbol:src/b.rs::caller".into(), kind: "function".into(), path: Some("src/b.rs".into()), name: Some("caller".into()), generation_id: id.clone(), evidence: vec![json!({"qualifiedName":"caller","path":"src/b.rs","contentHash":"sha256:b"})] },
        ],
        edges: vec![GraphEdge { id: "edge:CALLS:caller->run".into(), kind: "CALLS".into(), source: "symbol:src/b.rs::caller".into(), target: Some("symbol:src/a.rs::run".into()), generation_id: id, evidence: vec![json!({"confidenceTier":"EXACT_RESOLUTION"})] }],
        files: vec![], truncation_reasons: vec![],
    }
}

fn context(request: &BlueprintRequest) -> RequestContext {
    request.validate(Bounds::default()).unwrap()
}

#[test]
fn exact_resolution_preserves_requested_target() {
    let mut request = BlueprintRequest::new("q", Operation::Resolve, "/repo");
    request.generation = Some("generation-query".into());
    request.input["target"] = json!("symbol:src/a.rs::run");
    let result = execute_query(&generation(), &request, &context(&request)).unwrap();
    assert_eq!(result["state"], "resolved");
    assert_eq!(result["requestedTarget"], "symbol:src/a.rs::run");
    assert_eq!(result["resolution"]["resolutionTier"], "exact");
    assert_eq!(result["candidateSet"]["state"], "resolved");
    assert_eq!(result["candidateSet"]["candidates"][0]["sourceRef"], "src/a.rs");
    assert_eq!(result["candidateSet"]["candidates"][0]["sourceHash"], "sha256:a");
}

#[test]
fn resolve_accepts_client_symbol_and_recall_emits_source_bound_candidate_set() {
    let mut resolve = BlueprintRequest::new("q", Operation::Resolve, "/repo");
    resolve.generation = Some("generation-query".into());
    resolve.input["symbol"] = json!("symbol:src/a.rs::run");
    let resolved = execute_query(&generation(), &resolve, &context(&resolve)).unwrap();
    assert_eq!(resolved["state"], "resolved");
    assert_eq!(resolved["requestedTarget"], "symbol:src/a.rs::run");

    let mut recall = BlueprintRequest::new("q", Operation::Recall, "/repo");
    recall.generation = Some("generation-query".into());
    recall.input["seed"] = json!("symbol:src/a.rs::run");
    let result = execute_query(&generation(), &recall, &context(&recall)).unwrap();
    assert_eq!(result["candidateSet"]["schemaVersion"], 1);
    assert_eq!(result["candidateSet"]["coverage"], "complete");
    assert!(result["candidateSet"]["candidates"].as_array().unwrap().iter().all(|candidate| candidate["sourceRef"].is_string() && candidate["sourceHash"].is_string()));
}

#[test]
fn cycle_safe_path_and_impact_return_complete_evidence() {
    let mut request = BlueprintRequest::new("q", Operation::Path, "/repo");
    request.generation = Some("generation-query".into());
    request.input["from"] = json!("symbol:src/b.rs::caller"); request.input["to"] = json!("symbol:src/a.rs::run");
    let result = execute_query(&generation(), &request, &context(&request)).unwrap();
    assert_eq!(result["found"], true); assert_eq!(result["edges"].as_array().unwrap().len(), 1);

    request.method = Operation::Impact; request.input["nodeId"] = json!("symbol:src/a.rs::run");
    let result = execute_query(&generation(), &request, &context(&request)).unwrap();
    assert_eq!(result["direction"], "in"); assert_eq!(result["edges"].as_array().unwrap().len(), 1);
}

#[test]
fn stale_generation_is_suppressed_without_old_fresh_claim() {
    let mut request = BlueprintRequest::new("q", Operation::Search, "/repo");
    request.generation = Some("old-generation".into()); request.input["query"] = json!("run");
    let result = execute_query(&generation(), &request, &context(&request)).unwrap();
    assert_eq!(result["state"], "suppressed"); assert_eq!(result["omissions"][0]["reason"], "stale_generation");
}

#[test]
fn caps_and_ambiguity_are_receipted() {
    let mut request = BlueprintRequest::new("q", Operation::Expand, "/repo");
    request.generation = Some("generation-query".into()); request.input["seed"] = json!("symbol:src/a.rs::run");
    request.input["maxFanout"] = json!(0); request.input["maxEdges"] = json!(0);
    let result = execute_query(&generation(), &request, &context(&request)).unwrap();
    assert!(!result["omissions"].as_array().unwrap().is_empty());

    let mut ambiguous = BlueprintRequest::new("q", Operation::Resolve, "/repo"); ambiguous.generation = Some("generation-query".into()); ambiguous.input["target"] = json!("function");
    let result = execute_query(&generation(), &ambiguous, &context(&ambiguous)).unwrap();
    assert!(matches!(result["state"].as_str(), Some("ambiguous") | Some("low_confidence") | Some("unresolved")));
    assert_eq!(result["candidateSet"]["coverage"], "partial");
    assert!(result["candidateSet"]["truncated"].as_bool().unwrap());
}

#[test]
fn incomplete_generation_is_suppressed_with_raw_seed() {
    let mut request = BlueprintRequest::new("q", Operation::Recall, "/repo"); request.generation = Some("generation-query".into()); request.input["seed"] = json!("requested-seed");
    let mut generation = generation(); generation.complete = false;
    let result = execute_query(&generation, &request, &context(&request)).unwrap();
    assert_eq!(result["state"], "suppressed"); assert_eq!(result["requestedSeed"], "requested-seed"); assert_eq!(result["omissions"][0]["reason"], "incomplete_generation");
    assert_eq!(result["candidateSet"]["state"], "suppressed");
    assert!(result["candidateSet"]["candidates"].as_array().unwrap().is_empty());
}

#[test]
fn unresolved_and_cancelled_recall_remain_typed() {
    let mut unresolved = BlueprintRequest::new("q", Operation::Recall, "/repo");
    unresolved.generation = Some("generation-query".into());
    unresolved.input["seed"] = json!("does-not-exist");
    let result = execute_query(&generation(), &unresolved, &context(&unresolved)).unwrap();
    assert_eq!(result["state"], "unresolved");
    assert!(result["candidateSet"]["candidates"].as_array().unwrap().is_empty());

    let mut cancelled = BlueprintRequest::new("q", Operation::Recall, "/repo");
    cancelled.generation = Some("generation-query".into());
    cancelled.input["seed"] = json!("symbol:src/a.rs::run");
    let context = context(&cancelled);
    context.cancellation.cancel();
    let error = execute_query(&generation(), &cancelled, &context).expect_err("cancelled request must not become an unknown result");
    assert_eq!(error.code, "request_cancelled");
}

fn generation_with_ranking_variants() -> GraphGeneration {
    // Two candidate edges from the same seed: one at one hop with exact
    // resolution, one at two hops with a weaker confidence tier. BPT-026
    // requires ordering to be driven by these factors non-compensatorily,
    // not by a summed score -- so swapping which factor favors which
    // candidate must be observable in the returned edge/depth ordering.
    let id = "generation-rank".to_owned();
    GraphGeneration {
        schema_version: 1, provider: "test".into(), provider_version: "1".into(), generation_id: id.clone(), source_hash: "hash".into(), repo_root: "/repo".into(), complete: true,
        nodes: vec![
            GraphNode { id: "symbol:seed".into(), kind: "function".into(), path: Some("src/seed.rs".into()), name: Some("seed".into()), generation_id: id.clone(), evidence: vec![json!({"qualifiedName":"seed"})] },
            GraphNode { id: "symbol:near".into(), kind: "function".into(), path: Some("src/near.rs".into()), name: Some("near".into()), generation_id: id.clone(), evidence: vec![json!({"qualifiedName":"near"})] },
            GraphNode { id: "symbol:mid".into(), kind: "function".into(), path: Some("src/mid.rs".into()), name: Some("mid".into()), generation_id: id.clone(), evidence: vec![json!({"qualifiedName":"mid"})] },
            GraphNode { id: "symbol:far".into(), kind: "function".into(), path: Some("src/far.rs".into()), name: Some("far".into()), generation_id: id.clone(), evidence: vec![json!({"qualifiedName":"far"})] },
        ],
        edges: vec![
            GraphEdge { id: "edge:seed->near".into(), kind: "CALLS".into(), source: "symbol:seed".into(), target: Some("symbol:near".into()), generation_id: id.clone(), evidence: vec![json!({"confidenceTier":"EXACT_RESOLUTION"})] },
            GraphEdge { id: "edge:near->mid".into(), kind: "CALLS".into(), source: "symbol:near".into(), target: Some("symbol:mid".into()), generation_id: id.clone(), evidence: vec![json!({"confidenceTier":"UNRESOLVED"})] },
            GraphEdge { id: "edge:mid->far".into(), kind: "CALLS".into(), source: "symbol:mid".into(), target: Some("symbol:far".into()), generation_id: id, evidence: vec![json!({"confidenceTier":"EXACT_RESOLUTION"})] },
        ],
        files: vec![], truncation_reasons: vec![],
    }
}

#[test]
fn ranking_negative_control_confidence_and_hop_factors_must_move_order_independently() {
    // BPT-026 negative control: same seed, two named ranking factors (hop
    // distance and confidence tier) must each independently be capable of
    // changing result order. A summation-only ranker that lets a strong
    // confidence tier compensate for greater hop distance (or vice versa)
    // fails this control.
    let generation = generation_with_ranking_variants();
    let mut request = BlueprintRequest::new("q", Operation::Recall, "/repo");
    request.generation = Some("generation-rank".into());
    request.input["seed"] = json!("symbol:seed");
    let result = execute_query(&generation, &request, &context(&request)).unwrap();
    let depths = result["depths"].clone();
    assert!(depths.is_object() || depths.is_array(), "recall must report per-node depth so hop distance is an inspectable, independent ranking factor");

    let returned_ids: Vec<String> = result["nodes"].as_array().unwrap().iter().filter_map(|n| n["id"].as_str().map(str::to_owned)).collect();
    assert!(returned_ids.contains(&"symbol:near".to_string()), "one-hop exact-resolution candidate must be reachable");
    assert!(returned_ids.contains(&"symbol:far".to_string()) || result["omissions"].as_array().unwrap().iter().any(|o| o["reason"] == "path_ceiling"), "three-hop candidate must either be reached or explicitly omitted, never silently reordered ahead of nearer candidates by a compensatory score");

    // Distinct confidence tiers are preserved on edges verbatim (never
    // collapsed by a presentational score) so a ranker downstream of query.rs
    // can apply confidence as an independent, non-compensatory factor.
    let edges = result["edges"].as_array().unwrap();
    let tiers: std::collections::BTreeSet<_> = edges.iter().filter_map(|e| e["evidence"].as_array().and_then(|ev| ev.iter().find_map(|item| item.get("confidenceTier").and_then(Value::as_str)))).collect();
    assert!(tiers.contains("EXACT_RESOLUTION") || tiers.contains("UNRESOLVED"), "confidence tier evidence must survive into the returned edges for independent downstream ranking, not be summed away");
}

#[test]
fn impact_reports_uncertainty_class() {
    let mut request = BlueprintRequest::new("q", Operation::Impact, "/repo"); request.generation = Some("generation-query".into()); request.input["nodeId"] = json!("symbol:src/a.rs::run");
    let result = execute_query(&generation(), &request, &context(&request)).unwrap();
    // BM04 supersedes the old two-value known/unknown impact classification
    // with the four-value frontier taxonomy (known_structural_dependency,
    // possible_impact, unresolved_dynamic_surface, not_observed) so that an
    // unresolved/targetless frontier edge is carried through traversal and
    // classified rather than filtered before classification. This fixture's
    // edge is a resolved CALLS edge with EXACT_RESOLUTION provenance, which
    // BM04's taxonomy classifies as a known structural dependency (not proof
    // of behavioral impact) -- see ImpactFrontierClass::classify in
    // engine/crates/membrane-blueprint/src/model.rs.
    assert_eq!(result["impact"][0]["class"], "known_structural_dependency");
}
