//! Native, generation-bound Blueprint query projections.
//!
//! This module is deliberately a projection over [`GraphGeneration`].  It does
//! not open storage or maintain a second index/ranker; every answer is derived
//! from the served generation and reports bounded work explicitly.

use crate::api::{BlueprintError, BlueprintRequest, RequestContext};
use crate::contracts::{BlueprintCandidateSetV1, BlueprintCandidateV1};
use crate::graph::GraphGeneration;
use crate::model::{GraphEdge, GraphNode, Operation};
use crate::recall_circuit::{self, AtomicPath};
use serde_json::{json, Map, Value};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Clone, Copy)]
struct Limits { seeds: usize, depth: usize, fanout: usize, nodes: usize, edges: usize, paths: usize, path_len: usize, candidates: usize, bytes: usize }

impl Limits {
    fn from(request: &BlueprintRequest, context: &RequestContext) -> Self {
        let input = &request.input;
        let number = |name: &str, fallback: usize| input.get(name).and_then(Value::as_u64).map(|v| v as usize).unwrap_or(fallback);
        Self {
            seeds: number("maxSeeds", 8).min(context.bounds.max_candidates),
            depth: number("maxDepth", 3), fanout: number("maxFanout", 64),
            nodes: number("maxNodes", 512), edges: number("maxEdges", 1024),
            paths: number("maxPaths", context.bounds.max_paths).min(context.bounds.max_paths),
            path_len: number("maxPathLength", 32), candidates: number("maxCandidates", context.bounds.max_candidates).min(context.bounds.max_candidates),
            bytes: number("maxBytes", context.bounds.max_response_bytes),
        }
    }
}

fn text(node: &GraphNode) -> Vec<String> {
    let mut fields = Vec::new();
    if let Some(v) = &node.name { fields.push(v.clone()); }
    if let Some(v) = &node.path { fields.push(v.clone()); }
    for evidence in &node.evidence {
        // Provider projections keep their domain attributes in evidence so
        // the closed V1 graph shape remains stable. Include those searchable
        // fields in the native text surface; absent fields are harmless and
        // provider-specific values never become graph authority.
        for key in ["qualifiedName", "text", "symbol", "displayName", "resourceName", "routePath", "topic", "action", "kindName"] {
            if let Some(v) = evidence.get(key).and_then(Value::as_str) { fields.push(v.to_owned()); }
        }
    }
    fields
}

fn norm(value: &str) -> String { value.replace('\\', "/").to_ascii_lowercase() }
fn requested(input: &Value, key: &str) -> Value { input.get(key).cloned().unwrap_or(Value::Null) }
fn envelope(_request: &BlueprintRequest, generation_id: &str, state: &str, mut body: Map<String, Value>) -> Value {
    body.insert("schemaVersion".into(), json!(1));
    body.insert("generationId".into(), json!(generation_id));
    body.insert("state".into(), json!(state));
    Value::Object(body)
}
fn omission(reason: &str, count: Option<usize>) -> Value {
    match count { Some(n) => json!({"reason": reason, "count": n}), None => json!({"reason": reason}) }
}
fn check(context: &RequestContext) -> Result<(), BlueprintError> { context.check() }
fn stale(request: &BlueprintRequest, generation: &GraphGeneration) -> bool {
    request.generation.as_deref().is_some_and(|id| id != generation.generation_id)
        || request.input.get("generation").and_then(Value::as_str).is_some_and(|id| id != generation.generation_id)
}

fn node_map<'a>(generation: &'a GraphGeneration) -> BTreeMap<&'a str, &'a GraphNode> { generation.nodes.iter().map(|n| (n.id.as_str(), n)).collect() }
fn node_value(node: &GraphNode) -> Value { serde_json::to_value(node).unwrap_or_else(|_| json!({"id":node.id})) }
fn edge_value(edge: &GraphEdge) -> Value { serde_json::to_value(edge).unwrap_or_else(|_| json!({"id":edge.id})) }

fn source_evidence(node: &GraphNode) -> Option<(&str, &str)> {
    let path = node.path.as_deref()?;
    let hash = node.evidence.iter().find_map(|e| {
        (e.get("path").and_then(Value::as_str) == Some(path))
            .then(|| e.get("contentHash").and_then(Value::as_str))
            .flatten()
    })?;
    (!path.is_empty() && !hash.is_empty()).then_some((path, hash))
}

fn source_bound_candidate(node: &GraphNode) -> Result<BlueprintCandidateV1, Value> {
    let Some((source_ref, source_hash)) = source_evidence(node) else {
        return Err(omission("source_evidence_missing", None));
    };
    let text = node.name.clone().unwrap_or_else(|| source_ref.to_owned());
    Ok(BlueprintCandidateV1 {
        id: node.id.clone(), layer: 3, provider: None, source_kind: "graph".into(),
        source_ref: source_ref.to_owned(), source_hash: source_hash.to_owned(),
        trust_class: "workspace_tracked".into(), instruction_policy: "data_only".into(),
        // Blueprint exposes graph evidence but does not rank across providers.
        provider_score: 0.0, score_components: BTreeMap::new(), base_commit: None,
        overlay_digest: None, freshness_class: Some("current".into()), snapshot_id: None,
        estimated_tokens: 0, protected: false, exact: true, recoverable: true,
        resolver: "blueprint_graph_generation".into(), text,
    })
}

fn candidate_set<'a>(
    state: &str,
    nodes: impl IntoIterator<Item = &'a GraphNode>,
    total_known_count: Option<usize>,
    truncated: bool,
    mut omissions: Vec<Value>,
) -> Value {
    let mut candidates = Vec::new();
    for node in nodes {
        match source_bound_candidate(node) {
            Ok(candidate) => candidates.push(candidate),
            Err(omission) => omissions.push(omission),
        }
    }
    let coverage = if state == "complete" && !truncated && omissions.is_empty() { "complete" } else { "partial" };
    let state = if state == "complete" && coverage == "partial" { "partial" } else { state };
    let set = BlueprintCandidateSetV1 {
        schema_version: 1, state: state.to_owned(), candidate_count: candidates.len() as u64,
        candidates, total_known_count: total_known_count.map(|count| count as u64), truncated,
        coverage: coverage.into(), freshness: "current".into(), omissions,
    };
    serde_json::to_value(set).unwrap_or_else(|_| json!({"schemaVersion":1,"state":"partial","candidates":[],"candidateCount":0,"truncated":true,"coverage":"partial","freshness":"unknown","omissions":[omission("candidate_serialization_failed",None)]}))
}

fn resolve(generation: &GraphGeneration, raw: &str, limits: Limits, context: &RequestContext) -> Result<(String, Value), BlueprintError> {
    let query = raw.trim();
    let wanted = norm(query);
    let mut tiers: Vec<(&str, Vec<&GraphNode>)> = Vec::new();
    let exact: Vec<_> = generation.nodes.iter().filter(|n| n.id == query || n.path.as_deref() == Some(query)).collect();
    tiers.push(("exact", exact));
    let folded: Vec<_> = generation.nodes.iter().filter(|n| n.id != query && (norm(&n.id) == wanted || n.path.as_deref().is_some_and(|p| norm(p) == wanted) || text(n).iter().any(|v| norm(v) == wanted))).collect();
    tiers.push(("case_folded", folded));
    let fuzzy: Vec<_> = if wanted.is_empty() { Vec::new() } else { generation.nodes.iter().filter(|n| n.id != query && text(n).iter().any(|v| norm(v).contains(&wanted))).collect() };
    tiers.push(("fuzzy", fuzzy));
    for (tier, mut matches) in tiers {
        check(context)?;
        matches.sort_by(|a,b| a.id.cmp(&b.id));
        if matches.is_empty() { continue; }
        let total_matches = matches.len();
        let ambiguous = total_matches > 1;
        let limited: Vec<_> = matches.into_iter().take(limits.seeds.max(1)).collect();
        let candidates: Vec<Value> = limited.iter().map(|n| node_value(n)).collect();
        if ambiguous {
            return Ok((String::new(), json!({"state":"ambiguous","resolutionTier":tier,"requested":query,"resolved":Value::Null,"candidates":candidates,"candidateCount":candidates.len(),"ambiguous":true,"omissions":[omission("same_tier_ambiguity",Some(total_matches.saturating_sub(limits.seeds)))]})));
        }
        let state = if tier == "fuzzy" { "low_confidence" } else { "resolved" };
        return Ok((if state == "resolved" { limited[0].id.clone() } else { String::new() }, json!({"state":state,"resolutionTier":tier,"requested":query,"resolved":if state == "resolved" { node_value(limited[0]) } else { Value::Null },"candidates":candidates,"candidateCount":1,"ambiguous":false,"omissions":if state == "low_confidence" { vec![omission("low_confidence_resolution",None)] } else { vec![] }})));
    }
    Ok((String::new(), json!({"state":"unresolved","resolutionTier":"unresolved","requested":query,"resolved":Value::Null,"candidates":[],"candidateCount":0,"ambiguous":false,"omissions":[omission("no_match",None)]})))
}

fn adjacent<'a>(generation: &'a GraphGeneration, root: &str, direction: &str, limits: Limits, context: &RequestContext) -> Result<(BTreeSet<String>, Vec<&'a GraphEdge>, BTreeMap<String, usize>, Vec<Value>), BlueprintError> {
    let mut seen = BTreeSet::from([root.to_owned()]);
    let mut frontier = vec![root.to_owned()];
    let mut depths = BTreeMap::from([(root.to_owned(), 0usize)]);
    let mut edges = Vec::new();
    let mut omissions = Vec::new();
    let mut bytes = 0usize;
    for depth in 0..limits.depth {
        check(context)?;
        let mut next = Vec::new();
        let mut acquired = 0usize;
        for edge in &generation.edges {
            let out = direction != "in" && frontier.iter().any(|id| id == &edge.source);
            // BM04: a `target: None` edge (dynamic dispatch/reflection/unresolved
            // call) must still be carried into the frontier for classification
            // rather than dropped here. When walking "in"/"both" we cannot
            // confirm a targetless edge points at a frontier member, but an
            // edge whose *source* is already in the frontier is a real,
            // observed unresolved-dynamic-surface attached to that frontier
            // node, so it is admitted rather than silently filtered.
            let incoming = direction != "out"
                && (edge.target.as_ref().is_some_and(|target| frontier.iter().any(|id| id == target))
                    || (edge.target.is_none() && frontier.iter().any(|id| id == &edge.source)));
            if !out && !incoming { continue; }
            // `next_target` is the node this edge would extend the frontier
            // to, when resolvable. A `None` here (unresolved/targetless edge)
            // is not a reason to discard the edge: it is admitted into
            // `edges` below (bounded exactly like every other edge) so the
            // caller can classify it, it just cannot grow the BFS frontier.
            let next_target: Option<String> = if out { edge.target.clone() } else { Some(edge.source.clone()) };
            if acquired >= limits.fanout { omissions.push(omission("fanout_ceiling", Some(1))); break; }
            if edges.len() >= limits.edges { omissions.push(omission("edge_ceiling", Some(generation.edges.len().saturating_sub(edges.len())))); break; }
            if let Some(target) = &next_target {
                if seen.len() >= limits.nodes && !seen.contains(target) { omissions.push(omission("node_ceiling", Some(1))); break; }
            }
            let edge_bytes = serde_json::to_vec(edge).map(|v| v.len()).unwrap_or(limits.bytes);
            if bytes.saturating_add(edge_bytes) > limits.bytes { omissions.push(omission("byte_ceiling", Some(1))); break; }
            bytes = bytes.saturating_add(edge_bytes);
            acquired += 1;
            edges.push(edge);
            if let Some(target) = next_target {
                if seen.insert(target.clone()) { depths.insert(target.clone(), depth + 1); next.push(target); }
            }
        }
        frontier = next;
        if frontier.is_empty() { break; }
    }
    if edges.len() >= limits.edges { omissions.push(omission("edge_ceiling", Some(edges.len().saturating_sub(limits.edges)+1))); }
    Ok((seen, edges, depths, omissions))
}

/// Builds the edge candidate `Value` shape [`recall_circuit::make_path`] and
/// [`crate::evidence_authority::semantic_authority_rank_for_fact`] expect: a
/// top-level `confidenceTier` (native `GraphEdge` carries it only inside
/// `evidence`, mirroring how the Impact-frontier classifier above already
/// reads it) alongside `id`/`source`/`target`/`evidence` passthrough.
fn edge_ranking_value(edge: &GraphEdge) -> Value {
    let tier = edge.evidence.iter().find_map(|e| e.get("confidenceTier").and_then(Value::as_str));
    json!({
        "id": edge.id,
        "source": edge.source,
        "target": edge.target,
        "confidenceTier": tier,
        "evidence": edge.evidence,
    })
}

/// Bounded enumeration of alternative `from -> to` paths, mirroring the
/// bound discipline of `blueprint/src/graph/recall-circuit.mjs`'s BFS
/// (node/edge/fanout/path-length/path-count ceilings, each receipted as a
/// typed omission) but targeted at a fixed destination instead of open
/// frontier exploration, so more than the single BFS-first path can be
/// collected and ranked. `from == to` matches immediately with an empty
/// edge list, exactly like the prior single-path BFS.
fn enumerate_paths<'a>(
    generation: &'a GraphGeneration,
    from: &str,
    to: &str,
    limits: Limits,
    context: &RequestContext,
) -> Result<(Vec<(Vec<String>, Vec<&'a GraphEdge>)>, Vec<Value>), BlueprintError> {
    let mut omissions = Vec::new();
    let mut results: Vec<(Vec<String>, Vec<&GraphEdge>)> = Vec::new();

    let mut by_source: BTreeMap<&str, Vec<&GraphEdge>> = BTreeMap::new();
    for edge in &generation.edges {
        by_source.entry(edge.source.as_str()).or_default().push(edge);
    }
    for edges in by_source.values_mut() {
        edges.sort_by(|a, b| a.id.cmp(&b.id));
    }

    struct Frame<'a> {
        node: String,
        node_ids: Vec<String>,
        edge_ids: Vec<&'a GraphEdge>,
        visited: BTreeSet<String>,
    }
    let max_paths = limits.paths.max(1);
    let mut stack: Vec<Frame> = vec![Frame {
        node: from.to_owned(),
        node_ids: vec![from.to_owned()],
        edge_ids: vec![],
        visited: BTreeSet::from([from.to_owned()]),
    }];
    let mut edge_budget = 0usize;
    while let Some(frame) = stack.pop() {
        check(context)?;
        if frame.node == to {
            if results.len() < max_paths {
                results.push((frame.node_ids.clone(), frame.edge_ids.clone()));
            } else {
                omissions.push(omission("path_ceiling", None));
            }
            continue;
        }
        if frame.edge_ids.len() >= limits.path_len {
            omissions.push(omission("path_length_ceiling", Some(1)));
            continue;
        }
        if frame.node_ids.len() >= limits.nodes {
            omissions.push(omission("node_ceiling", Some(1)));
            continue;
        }
        let candidates = by_source.get(frame.node.as_str()).cloned().unwrap_or_default();
        let mut taken = 0usize;
        for edge in candidates {
            if taken >= limits.fanout {
                omissions.push(omission("fanout_ceiling", Some(1)));
                break;
            }
            let Some(next) = edge.target.clone() else { continue };
            if frame.visited.contains(&next) { continue; }
            if edge_budget >= limits.edges {
                omissions.push(omission("edge_ceiling", Some(1)));
                break;
            }
            edge_budget += 1;
            taken += 1;
            let mut node_ids = frame.node_ids.clone();
            node_ids.push(next.clone());
            let mut edge_ids = frame.edge_ids.clone();
            edge_ids.push(edge);
            let mut visited = frame.visited.clone();
            visited.insert(next.clone());
            stack.push(Frame { node: next, node_ids, edge_ids, visited });
        }
    }
    Ok((results, omissions))
}

/// Ranks bounded `from -> to` path candidates with the ported non-compensatory
/// `recall_circuit::compare_paths` ordering (BPT-026: semantic authority
/// before resolution specificity before hop count) rather than returning
/// whichever path bounded search happened to enumerate first. Returns the
/// best-ranked `(node_ids, edges)` pair alongside its `AtomicPath` ranking
/// facts (kept only for the caller's own inspection/tests, not serialized
/// into the existing V1 response shape).
fn rank_paths<'a>(
    generation: &GraphGeneration,
    from: &str,
    raw_paths: Vec<(Vec<String>, Vec<&'a GraphEdge>)>,
) -> Option<(Vec<String>, Vec<&'a GraphEdge>, AtomicPath)> {
    if raw_paths.is_empty() { return None; }
    let node_val_map: HashMap<String, Value> = generation.nodes.iter().map(|n| (n.id.clone(), node_value(n))).collect();
    let seed = recall_circuit::Seed { id: from.to_owned(), exactness: 0, reason: None, evidence: Value::Null };
    let mut best: Option<(Vec<String>, Vec<&GraphEdge>, AtomicPath)> = None;
    for (node_ids, edge_refs) in raw_paths {
        let edge_val_map: HashMap<String, Value> = edge_refs.iter().map(|e| (e.id.clone(), edge_ranking_value(e))).collect();
        let edge_ids: Vec<String> = edge_refs.iter().map(|e| e.id.clone()).collect();
        let atomic = recall_circuit::make_path(&seed, &node_ids, &edge_ids, &node_val_map, &edge_val_map, true, &generation.generation_id);
        let better = match &best {
            None => true,
            Some((_, _, current)) => recall_circuit::compare_paths(&atomic, current) == Ordering::Less,
        };
        if better {
            best = Some((node_ids, edge_refs, atomic));
        }
    }
    best
}

/// Serializes an [`AtomicPath`] into the `AtomicEvidencePath`-shaped JSON
/// legacy `makePath` returns, for embedding in the new `recallCircuit`
/// response field (additive -- see `recall_op` doc comment).
fn atomic_path_value(path: &AtomicPath) -> Value {
    // `nodes`/`edges` are compact identity arrays. Full evidence already lives
    // in `evidenceEnvelope`; duplicating hydrated graph objects here can exceed
    // the bounded 16 KiB production response for even a small repository.
    json!({
        "id": path.id,
        "seedId": path.seed_id,
        "terminalId": path.terminal_id,
        "nodes": path.node_ids,
        "edges": path.edge_ids,
        "nodeIds": path.node_ids,
        "edgeIds": path.edge_ids,
        "minimumEdgeTier": path.minimum_edge_tier,
        "minimumSemanticAuthority": path.minimum_semantic_authority,
        "semanticAuthorityRank": path.semantic_authority_rank,
        "seedExactness": path.seed_exactness,
        "evidenceCoverage": path.evidence_coverage,
        "hopCount": path.hop_count,
        "state": path.state,
        "omissionReasons": path.omission_reasons,
        "evidenceEnvelope": path.evidence_envelope,
    })
}

/// Native, store-driven Recall: BPT-026 non-compensatory path ranking via
/// [`recall_circuit::execute_recall_circuit_with_totals`] over multi-seed
/// resolution ([`recall_circuit::resolve_seeds_native`]) and bounded,
/// policy-selected traversal ([`recall_circuit::select_traversal_policy_native`],
/// [`recall_circuit::traversal_neighbors_native`]) -- the DB-driving gap the
/// `recall_circuit` module doc comment recorded as missing. This crate has
/// no live SQLite handle at this layer (`query.rs` is a projection over an
/// already-loaded [`GraphGeneration`]; see the crate's module doc), so
/// "DB-driving" is re-expressed as in-memory scans over
/// `generation.nodes`/`generation.edges` -- functionally the same
/// seed-resolution/traversal semantics, ported to the storage shape this
/// crate actually has.
///
/// Response shape: the existing V1 fields Expand/Impact/the old shared BFS
/// Recall already produced (`requestedSeed`, `resolution`, `root`,
/// `direction`, `nodes`, `edges`, `depths`, `omissions`, `candidateSet`
/// with `sourceRef`/`sourceHash`-bearing candidates) are preserved exactly,
/// sourced now from the real circuit's reached node/edge set instead of
/// plain single-seed BFS. Two fields are added (never removed/renamed, so
/// existing consumers of the V1 shape are unaffected): `recallCircuit`
/// (paths/state/omissions/policy, mirroring legacy `RecallCircuit`) and
/// `orientation` (mirroring legacy `recallOrientation`'s action/reason
/// verdict). Stale-source suppression: legacy's `staleRow`/
/// `staleSourcePolicy`/`suppressRows` are driven by a freshness *receipt*
/// this native layer is never handed (no field on `BlueprintRequest`/
/// `RequestContext` carries one -- freshness/receipts belong to a
/// different subsystem per `docs/agent-rules.md`'s "Keep provider
/// authority and freshness distinct" invariant). Suppression is instead
/// driven by two optional request inputs a freshness-owning caller can
/// supply: `staleSourcePaths` (array of stale-relative-to-current-worktree
/// paths) and `staleWholeGeneration` (bool, mirrors legacy's
/// `suppression.mode === "whole_generation"`). Absent, no suppression is
/// applied -- existing callers/tests that never set these fields see
/// unchanged behavior.
fn recall_op(generation: &GraphGeneration, request: &BlueprintRequest, context: &RequestContext, limits: Limits) -> Result<Value, BlueprintError> {
    let raw = request.input.get("seed").or_else(|| request.input.get("target")).or_else(|| request.input.get("nodeId")).or_else(|| request.input.get("task")).and_then(Value::as_str).unwrap_or("");
    let task_text = request.input.get("task").and_then(Value::as_str).or_else(|| request.input.get("query").and_then(Value::as_str)).filter(|t| !t.is_empty()).unwrap_or(raw);
    let mut seed_ids: Vec<String> = request.input.get("seedIds").and_then(Value::as_array).into_iter().flatten().filter_map(|v| v.as_str().map(str::to_owned)).collect();
    if seed_ids.is_empty() && !raw.is_empty() { seed_ids.push(raw.to_owned()); }
    let anchors: Vec<String> = request.input.get("anchors").and_then(Value::as_array).into_iter().flatten().filter_map(|v| v.as_str().map(str::to_owned)).collect();

    check(context)?;
    let resolution = recall_circuit::resolve_seeds_native(&generation.nodes, &seed_ids, &anchors, task_text, limits.seeds, true);

    if resolution.seeds.is_empty() {
        let state: &str = if resolution.state == "ambiguous" { "ambiguous" } else { "unresolved" };
        let mut omissions = vec![omission("seed_unresolved", None)];
        if let Some(reason) = resolution.json.get("reason").and_then(Value::as_str) { omissions.push(omission(reason, None)); }
        let set = candidate_set(state, std::iter::empty(), Some(0), true, omissions.clone());
        return Ok(envelope(request, &generation.generation_id, state, Map::from_iter([
            ("requestedSeed".into(), json!(raw)), ("resolution".into(), resolution.json),
            ("nodes".into(), json!([])), ("edges".into(), json!([])), ("depths".into(), json!({})),
            ("omissions".into(), json!(omissions)), ("candidateSet".into(), set),
            ("recallCircuit".into(), json!({"schemaVersion":1,"kind":"RecallCircuit","generationId":generation.generation_id,"paths":[],"state": if state=="ambiguous" {"ambiguous"} else {"abstained"},"omissions":[{"reason":"no_seeds_resolved"}]})),
            ("orientation".into(), json!({"action":"noop","reasonCode":"no_candidates","reason":"Recall resolved no evidence paths for this task."})),
        ])));
    }

    let requested_policy = request.input.get("policy").and_then(Value::as_str);
    let max_hops = request.input.get("maxHops").and_then(Value::as_u64).map(|v| v as usize);
    let (policy, mut policy_omissions) = match recall_circuit::select_traversal_policy_native(task_text, requested_policy, max_hops, Some(limits.seeds), Some(limits.paths), Some(limits.nodes), Some(limits.edges)) {
        Ok(p) => (p, Vec::new()),
        Err(_) => {
            // Unknown explicit `policy` family: fall back to `explore.both`
            // (task-text auto-detection is skipped here since the caller
            // explicitly asked for a family and got it wrong -- silently
            // reinterpreting the task text as if no family were requested
            // could surprise them with an unrelated family).
            let fallback = recall_circuit::select_traversal_policy_native(task_text, Some("explore.both"), max_hops, Some(limits.seeds), Some(limits.paths), Some(limits.nodes), Some(limits.edges)).unwrap_or_default();
            (fallback, vec![omission("unknown_traversal_policy", None)])
        }
    };

    // Compatibility floor: the pre-circuit V1 Recall (shared BFS with
    // Expand/Impact) used the generic request-level `maxDepth` (default 3
    // via `Limits`), not a traversal-policy family's own (often shallower,
    // e.g. `explore.both`'s 2) hop cap. When the caller did not explicitly
    // request a policy family, never let auto-selection regress reachable
    // depth below that existing default -- an explicitly requested family
    // is still honored exactly (its cap is a deliberate choice, not a
    // regression to protect against).
    let mut policy = policy;
    if requested_policy.is_none() { policy.max_hops = policy.max_hops.max(limits.depth); }

    let kinds = recall_circuit::policy_kinds(&policy.family);
    let seed_id_list: Vec<String> = resolution.seeds.iter().map(|s| s.id.clone()).collect();
    check(context)?;
    let frontier = recall_circuit::traversal_neighbors_native(&generation.edges, &seed_id_list, &policy.direction, policy.max_hops, kinds);
    let total_seen = frontier.seen_nodes.len();
    let total_edges = frontier.edge_rows.len();
    let allowed_nodes: BTreeSet<String> = frontier.seen_nodes.iter().take(policy.max_nodes).cloned().collect();
    let edge_rows: Vec<GraphEdge> = frontier.edge_rows.into_iter()
        .filter(|e| allowed_nodes.contains(&e.source) && e.target.as_deref().is_some_and(|t| allowed_nodes.contains(t)))
        .take(policy.max_edges).collect();

    let nodes_by_id = node_map(generation);
    let node_val_map: HashMap<String, Value> = allowed_nodes.iter().filter_map(|id| nodes_by_id.get(id.as_str()).map(|n| (id.clone(), node_value(n)))).collect();
    let edge_values: Vec<Value> = edge_rows.iter().map(edge_ranking_value).collect();
    let graph = recall_circuit::RecallGraph { nodes: node_val_map, edges: edge_values };

    check(context)?;
    let mut circuit = recall_circuit::execute_recall_circuit_with_totals(&graph, &resolution.seeds, &policy, &generation.generation_id, total_seen, total_edges);
    circuit.omissions.append(&mut policy_omissions);

    // Stale-source suppression (see doc comment above for why this is
    // input-driven rather than receipt-driven at this layer).
    let stale_paths: BTreeSet<String> = request.input.get("staleSourcePaths").and_then(Value::as_array).into_iter().flatten()
        .filter_map(|v| v.as_str()).map(|p| p.replace('\\', "/")).collect();
    let whole_generation_stale = request.input.get("staleWholeGeneration").and_then(Value::as_bool).unwrap_or(false);
    let is_stale = |node_ids: &[String]| -> bool {
        if whole_generation_stale { return true; }
        if stale_paths.is_empty() { return false; }
        node_ids.iter().any(|id| graph.nodes.get(id).and_then(|n| n.get("path")).and_then(Value::as_str).is_some_and(|p| stale_paths.contains(&p.replace('\\', "/"))))
    };
    let before_suppression = circuit.paths.len();
    if whole_generation_stale || !stale_paths.is_empty() {
        circuit.paths.retain(|path| !is_stale(&path.node_ids));
        let suppressed = before_suppression - circuit.paths.len();
        if suppressed > 0 || whole_generation_stale {
            circuit.omissions.push(json!({"reason":"stale_source_suppressed","lane":"evidence_path","count":suppressed}));
        }
        if circuit.paths.is_empty() && before_suppression > 0 { circuit.state = "abstained"; }
    }

    // Preserve the existing V1 fields (`nodes`/`edges`/`depths`/
    // `candidateSet`) sourced from the graph the circuit actually reached
    // (post stale-suppression: a node that appears only via a suppressed
    // path is still reachable structurally, so it is not pulled from
    // `nodes`/`edges`/`candidateSet` here -- only `recallCircuit.paths`
    // drops the suppressed path itself, exactly mirroring legacy's
    // `suppressRows` acting on `candidateSet.candidates`/`recallCircuit.paths`
    // as two independently-filtered views over the same underlying graph).
    let candidate_ids: Vec<String> = allowed_nodes.iter().cloned().collect();
    let candidate_nodes: Vec<&GraphNode> = candidate_ids.iter().filter_map(|id| nodes_by_id.get(id.as_str()).copied()).collect();
    let filtered_candidate_nodes: Vec<&GraphNode> = if whole_generation_stale {
        Vec::new()
    } else if stale_paths.is_empty() {
        candidate_nodes
    } else {
        candidate_nodes.into_iter().filter(|n| !n.path.as_deref().is_some_and(|p| stale_paths.contains(&p.replace('\\', "/")))).collect()
    };
    let mut candidate_omissions = circuit.omissions.clone();
    let set = candidate_set("complete", filtered_candidate_nodes.iter().copied(), Some(candidate_ids.len()), !candidate_omissions.is_empty(), std::mem::take(&mut candidate_omissions));

    let values: Vec<Value> = candidate_ids.iter().filter_map(|id| nodes_by_id.get(id.as_str()).map(|n| node_value(n))).take(limits.nodes).collect();
    let edges_json: Vec<Value> = edge_rows.iter().map(edge_value).collect();

    // Simple BFS-depth accounting over the reached edge set, for the
    // `depths` field the pre-existing V1 shape carries (hop distance from
    // the nearest seed) -- presentational/inspectable only, never fed back
    // into `compare_paths` ranking (BPT-026).
    let mut depths: BTreeMap<String, usize> = seed_id_list.iter().map(|s| (s.clone(), 0usize)).collect();
    {
        let mut adjacency: HashMap<&str, Vec<&str>> = HashMap::new();
        for e in &edge_rows {
            adjacency.entry(e.source.as_str()).or_default().push(e.target.as_deref().unwrap_or(""));
            if let Some(t) = e.target.as_deref() { adjacency.entry(t).or_default().push(e.source.as_str()); }
        }
        let mut frontier: Vec<String> = seed_id_list.clone();
        for depth in 1..=policy.max_hops {
            let mut next = Vec::new();
            for node in &frontier {
                for neighbor in adjacency.get(node.as_str()).into_iter().flatten() {
                    if !neighbor.is_empty() && !depths.contains_key(*neighbor) {
                        depths.insert((*neighbor).to_owned(), depth);
                        next.push((*neighbor).to_owned());
                    }
                }
            }
            if next.is_empty() { break; }
            frontier = next;
        }
    }

    let state = set.get("state").and_then(Value::as_str).unwrap_or("partial").to_owned();
    let candidate_count = set.get("candidateCount").and_then(Value::as_u64).unwrap_or(0);
    // Keep ranked prefix within its share of bounded response. Remaining V1
    // projections (nodes, edges, candidates, receipts) need room in same frame.
    let path_budget = limits.bytes / 3;
    let mut path_bytes = 0usize;
    let mut circuit_paths_json = Vec::new();
    for path in &circuit.paths {
        let value = atomic_path_value(path);
        let encoded = serde_json::to_vec(&value).map(|bytes| bytes.len()).unwrap_or(path_budget);
        if path_bytes.saturating_add(encoded) > path_budget { break; }
        path_bytes = path_bytes.saturating_add(encoded);
        circuit_paths_json.push(value);
    }
    let omitted_paths = circuit.paths.len().saturating_sub(circuit_paths_json.len());
    if omitted_paths > 0 {
        circuit.omissions.push(omission("byte_ceiling", Some(omitted_paths)));
        circuit.state = "partial";
    }
    let path_count = circuit_paths_json.len();
    let recall_circuit_json = json!({
        "schemaVersion": 1, "kind": "RecallCircuit", "id": circuit.id, "generationId": circuit.generation_id,
        "policy": circuit.policy_family, "paths": circuit_paths_json, "omissions": circuit.omissions, "state": circuit.state,
        "seeds": resolution.seeds.iter().map(|s| json!({"id": s.id, "exactness": s.exactness, "reason": s.reason, "evidence": s.evidence})).collect::<Vec<_>>(),
    });

    // Orientation, mirroring legacy `recallOrientation`'s decision order:
    // whole-generation suppression blocks, partial suppression continues
    // (admitted but incomplete), zero evidence is a noop, otherwise allow.
    let (action, reason_code, reason_text) = if whole_generation_stale {
        ("block", "stale_generation_withheld", "Stale-source enumeration is incomplete, so every source-backed row is withheld.")
    } else if !stale_paths.is_empty() {
        ("continue", "recalled_stale", "Recall served under a generation that predates current worktree changes; suppressed sources are on the receipt.")
    } else if candidate_count == 0 && path_count == 0 {
        ("noop", "no_candidates", "Recall resolved no evidence paths for this task.")
    } else {
        ("allow", "recalled", "Recall served from the sealed generation.")
    };
    let orientation = json!({
        "action": action, "reasonCode": reason_code, "reason": reason_text,
        "evidence": {"candidateCount": candidate_count, "evidencePathCount": path_count, "recallCircuitState": circuit.state},
        "omissions": circuit.omissions,
    });

    Ok(envelope(request, &generation.generation_id, &state, Map::from_iter([
        ("requestedSeed".into(), json!(raw)), ("resolution".into(), resolution.json),
        ("root".into(), json!(seed_id_list.first().cloned().unwrap_or_default())),
        ("direction".into(), json!(policy.direction)),
        ("nodes".into(), json!(values)), ("edges".into(), json!(edges_json)),
        ("depths".into(), json!(depths)), ("impact".into(), json!([])),
        ("omissions".into(), json!(circuit.omissions)), ("candidateSet".into(), set),
        ("recallCircuit".into(), recall_circuit_json), ("orientation".into(), orientation),
    ])))
}

fn search(generation: &GraphGeneration, request: &BlueprintRequest, context: &RequestContext, limits: Limits) -> Result<Value, BlueprintError> {
    let query = request.input.get("query").or_else(|| request.input.get("task")).and_then(Value::as_str).unwrap_or("");
    let q = norm(query);
    let mut found: Vec<_> = generation.nodes.iter().filter(|n| text(n).iter().any(|v| norm(v).contains(&q))).collect();
    found.sort_by(|a,b| a.id.cmp(&b.id));
    let omitted = found.len().saturating_sub(limits.candidates);
    found.truncate(limits.candidates);
    check(context)?;
    Ok(envelope(request, &generation.generation_id, "complete", Map::from_iter([("query".into(),json!(query)),("requestedQuery".into(),json!(query)),("candidates".into(),json!(found.iter().map(|n| node_value(n)).collect::<Vec<_>>())),("omissions".into(),json!(if omitted>0 {vec![omission("candidate_ceiling",Some(omitted))]} else {vec![]}))])))
}

/// Append `incomplete_generation` to an envelope's top-level and candidate-set
/// omission lists, without disturbing anything else the operation produced.
fn note_incomplete_generation(result: &mut Value) {
    let Some(object) = result.as_object_mut() else { return };
    let entry = omission("incomplete_generation", None);
    let already = |list: &Value| {
        list.as_array().is_some_and(|values| {
            values.iter().any(|value| value.get("reason").and_then(Value::as_str) == Some("incomplete_generation"))
        })
    };
    match object.get_mut("omissions") {
        Some(list) if already(list) => {}
        Some(Value::Array(list)) => list.push(entry.clone()),
        _ => { object.insert("omissions".into(), json!([entry.clone()])); }
    }
    // An incomplete generation is never "complete" coverage. `candidate_set`
    // computed state/coverage before this omission existed, so downgrade them
    // here or the envelope would carry `incomplete_generation` alongside a
    // contradictory `state`/`coverage` of "complete".
    if object.get("state").and_then(Value::as_str) == Some("complete") {
        object.insert("state".into(), json!("partial"));
    }
    if let Some(set) = object.get_mut("candidateSet").and_then(Value::as_object_mut) {
        match set.get_mut("omissions") {
            Some(list) if already(list) => {}
            Some(Value::Array(list)) => list.push(entry),
            _ => { set.insert("omissions".into(), json!([entry])); }
        }
        if set.get("state").and_then(Value::as_str) == Some("complete") {
            set.insert("state".into(), json!("partial"));
        }
        if set.get("coverage").and_then(Value::as_str) == Some("complete") {
            set.insert("coverage".into(), json!("partial"));
        }
        set.insert("truncated".into(), json!(true));
    }
}

pub fn execute_query(generation: &GraphGeneration, request: &BlueprintRequest, context: &RequestContext) -> Result<Value, BlueprintError> {
    let mut result = execute_query_impl(generation, request, context)?;
    // An incomplete generation still holds valid indexed evidence: it served
    // it, rather than suppressing every candidate (which turned any oversized
    // or binary file, or a truncated walk, into zero context across the whole
    // system). The incompleteness is recorded as an omission, not fatal. A
    // suppressed response (an explicit exact-generation mismatch) is left as
    // produced -- that remains fail-closed.
    if !generation.complete
        && result.get("state").and_then(Value::as_str) != Some("suppressed")
    {
        note_incomplete_generation(&mut result);
    }
    Ok(result)
}

fn execute_query_impl(generation: &GraphGeneration, request: &BlueprintRequest, context: &RequestContext) -> Result<Value, BlueprintError> {
    check(context)?;
    let limits = Limits::from(request, context);
    // Only an explicit caller-requested generation that does not match the
    // sealed generation fails closed (an exact-freshness contract violation).
    // Worktree drift and generation incompleteness degrade instead (see the
    // wrapper above); they never reach this suppression.
    if stale(request, generation) {
        let omissions = vec![omission("stale_generation", None)];
        let set = candidate_set("suppressed", std::iter::empty(), Some(0), true, omissions.clone());
        return Ok(envelope(request, &generation.generation_id, "suppressed", Map::from_iter([("requestedSeed".into(),requested(&request.input,"seed")),("requestedTarget".into(),requested(&request.input,"target")),("requestedGeneration".into(),json!(request.generation)),("omissions".into(),json!(omissions)),("candidateSet".into(),set)])));
    }
    match request.method {
        Operation::Search => search(generation, request, context, limits),
        Operation::Resolve => {
            // `symbol` is the existing direct-client spelling.  Retaining it
            // as an alias makes the native producer backward-compatible while
            // target remains the canonical Resolve field.
            let raw = request.input.get("target").or_else(|| request.input.get("symbol")).or_else(|| request.input.get("seed")).or_else(|| request.input.get("nodeId")).or_else(|| request.input.get("query")).and_then(Value::as_str).unwrap_or("");
            let (_id, mut resolution) = resolve(generation, raw, limits, context)?;
            let mut state = resolution.get("state").and_then(Value::as_str).unwrap_or("unresolved").to_owned();
            // Legacy `service.mjs:resolve` reanchor fallback: when the direct
            // id/text lookup finds nothing and the caller supplied
            // `previousEvidence`, attempt a conservative re-anchor (exact
            // entity -> exact fingerprint -> unique normalized text) against
            // the current generation's nodes before giving up. A reanchored
            // hit is re-resolved by id so it goes through the same candidate
            // shaping as a direct hit.
            let mut reanchor_result: Option<Value> = None;
            // Re-anchoring is a fallback for a genuinely missing direct
            // lookup. An ambiguous direct tier is meaningful evidence and
            // must remain ambiguous rather than being silently collapsed by
            // a weaker historical anchor.
            if state == "unresolved" {
                if let Some(previous_evidence) = request.input.get("previousEvidence") {
                    let candidates: Vec<Value> = generation.nodes.iter().map(node_value).collect();
                    let reanchor = crate::reanchor::reanchor_evidence(previous_evidence, &candidates);
                    if reanchor.get("state").and_then(Value::as_str) == Some("reanchored") {
                        if let Some(target_id) = reanchor.get("targetId").and_then(Value::as_str) {
                            let (_reanchored_id, reanchored_resolution) = resolve(generation, target_id, limits, context)?;
                            if reanchored_resolution.get("state").and_then(Value::as_str) == Some("resolved") {
                                resolution = reanchored_resolution;
                                state = "resolved".to_owned();
                            }
                        }
                    }
                    reanchor_result = Some(reanchor);
                }
            }
            let resolved = resolution.get("candidates").and_then(Value::as_array).into_iter().flatten()
                .filter_map(|value| serde_json::from_value::<GraphNode>(value.clone()).ok()).collect::<Vec<_>>();
            let total = resolution.get("candidateCount").and_then(Value::as_u64).map(|count| count as usize);
            let resolution_omissions = resolution.get("omissions").and_then(Value::as_array).cloned().unwrap_or_default();
            let set = candidate_set(&state, resolved.iter(), total, state != "resolved", resolution_omissions);
            // Resolve's own `resolution.resolved`/`resolution.candidates` are raw graph-node
            // projections (id/kind/name/path/evidence) and never carried sourceRef/sourceHash,
            // while the sibling `candidateSet` derived just above always does (via
            // `source_bound_candidate`). Recall and Resolve must agree on those provenance
            // fields for the same node, so backfill them onto the outer envelope's resolution
            // view from the already-computed, source-bound `set` -- after the GraphNode reparse
            // above, so the strict `deny_unknown_fields` reparse of `candidates` is untouched.
            if let Some(by_id) = set.get("candidates").and_then(Value::as_array).map(|candidates| {
                candidates.iter().filter_map(|candidate| {
                    let id = candidate.get("id").and_then(Value::as_str)?;
                    let source_ref = candidate.get("sourceRef").cloned()?;
                    let source_hash = candidate.get("sourceHash").cloned()?;
                    Some((id.to_owned(), (source_ref, source_hash)))
                }).collect::<std::collections::BTreeMap<_, _>>()
            }) {
                let backfill = |node: &mut Value| {
                    let Some(id) = node.get("id").and_then(Value::as_str).map(str::to_owned) else { return };
                    let Some((source_ref, source_hash)) = by_id.get(&id) else { return };
                    if let Some(object) = node.as_object_mut() {
                        object.insert("sourceRef".into(), source_ref.clone());
                        object.insert("sourceHash".into(), source_hash.clone());
                    }
                };
                if let Some(resolved_node) = resolution.get_mut("resolved") { backfill(resolved_node); }
                if let Some(candidates) = resolution.get_mut("candidates").and_then(Value::as_array_mut) {
                    for candidate in candidates.iter_mut() { backfill(candidate); }
                }
            }
            let mut fields = Map::from_iter([("requestedTarget".into(),json!(raw)),("resolution".into(),resolution),("candidateSet".into(),set)]);
            if let Some(reanchor) = reanchor_result { fields.insert("reanchor".into(), reanchor); }
            Ok(envelope(request, &generation.generation_id, &state, fields))
        }
        Operation::Recall => recall_op(generation, request, context, limits),
        Operation::Expand | Operation::Impact => {
            let raw = request.input.get("seed").or_else(|| request.input.get("target")).or_else(|| request.input.get("nodeId")).or_else(|| request.input.get("task")).and_then(Value::as_str).unwrap_or("");
            let (id, resolution) = resolve(generation, raw, limits, context)?;
            if id.is_empty() {
                let state = resolution.get("state").and_then(Value::as_str).unwrap_or("unresolved").to_owned();
                let mut omissions = resolution.get("omissions").and_then(Value::as_array).cloned().unwrap_or_default();
                omissions.push(omission("seed_unresolved", None));
                let set = candidate_set(&state, std::iter::empty(), Some(0), true, omissions.clone());
                return Ok(envelope(request, &generation.generation_id, &state, Map::from_iter([("requestedSeed".into(),json!(raw)),("resolution".into(),resolution),("nodes".into(),json!([])),("edges".into(),json!([])),("omissions".into(),json!(omissions)),("candidateSet".into(),set)])));
            }
            let direction = if request.method == Operation::Impact { "in" } else { request.input.get("direction").and_then(Value::as_str).unwrap_or("both") };
            let (ids, edges, depths, mut omissions) = adjacent(generation, &id, direction, limits, context)?;
            if request.method == Operation::Recall && limits.paths == 0 { omissions.push(omission("path_ceiling",None)); }
            let nodes = node_map(generation); let values: Vec<_> = ids.iter().filter_map(|i| nodes.get(i.as_str()).map(|n| node_value(n))).take(limits.nodes).collect();
            // BM04: classify every edge the bounded traversal actually reached
            // -- including targetless (unresolved dynamic) edges, which
            // `adjacent` now carries through instead of filtering -- via the
            // shared `ImpactFrontierClass` taxonomy. Exact structural
            // resolution never stands in for proof of behavioral impact or
            // test coverage; that distinction lives in the classifier itself.
            let impact_classes = if request.method == Operation::Impact {
                edges.iter().map(|edge| {
                    let tier = edge.evidence.iter().find_map(|e| e.get("confidenceTier").and_then(Value::as_str));
                    let class = crate::model::ImpactFrontierClass::classify(edge.target.as_deref(), tier);
                    json!({"edgeId": edge.id, "class": class.as_str(), "nodeId": edge.source})
                }).collect::<Vec<_>>()
            } else { vec![] };
            if request.method == Operation::Impact && impact_classes.is_empty() {
                // No edge reached this frontier within bounds: absence of
                // evidence, not evidence of absence (ImpactFrontierClass::NotObserved).
                omissions.push(omission(crate::model::ImpactFrontierClass::NotObserved.as_str(), None));
            }
            let candidate_nodes = ids.iter().filter_map(|id| nodes.get(id.as_str()).copied()).collect::<Vec<_>>();
            let candidate_omissions = omissions.clone();
            let set = candidate_set("complete", candidate_nodes, Some(ids.len()), !candidate_omissions.is_empty(), candidate_omissions);
            let state = set.get("state").and_then(Value::as_str).unwrap_or("partial").to_owned();
            let mut fields = Map::from_iter([("requestedSeed".into(),json!(raw)),("resolution".into(),resolution), ("root".into(),json!(id)), ("target".into(),json!(if request.method == Operation::Impact {nodes.get(id.as_str()).map(|n| node_value(n)).unwrap_or(json!({"id":id}))} else {Value::Null})), ("direction".into(),json!(direction)), ("nodes".into(),json!(values)), ("edges".into(),json!(edges.iter().map(|e|edge_value(e)).collect::<Vec<_>>())), ("depths".into(),json!(depths)), ("impact".into(),json!(impact_classes)), ("omissions".into(),json!(omissions)), ("candidateSet".into(),set)]);
            if request.method == Operation::Impact {
                // Legacy `service.mjs:impact` parity: resolve the full seed
                // envelope (node/file/line/stack/diff/treeish families),
                // decompose change risk, and recommend tests -- additive
                // fields alongside the existing single-seed traversal above,
                // which stays the source of `nodes`/`edges`/`impact`.
                let seed_input = crate::change_impact::SeedEnvelopeInput {
                    file: request.input.get("file").and_then(Value::as_str),
                    files: request.input.get("files").and_then(Value::as_array).map(|arr| arr.iter().filter_map(Value::as_str).collect()).unwrap_or_default(),
                    diff: request.input.get("diff").and_then(Value::as_str),
                    stack: request.input.get("stack").and_then(Value::as_str),
                    treeish_base: request.input.get("treeish").and_then(Value::as_str).or_else(|| request.input.get("base").and_then(Value::as_str)),
                    treeish_head: request.input.get("head").and_then(Value::as_str),
                    line: request.input.get("line").and_then(Value::as_u64),
                    node_id: request.input.get("nodeId").and_then(Value::as_str),
                    anchor: request.input.get("anchor").and_then(Value::as_str),
                    test: request.input.get("test").and_then(Value::as_str),
                    max_seeds: limits.seeds as u64,
                };
                let mut seed_envelope = crate::change_impact::resolve_impact_seed_envelope(generation, &generation.repo_root, &generation.generation_id, seed_input);
                if seed_envelope.get("seeds").and_then(Value::as_array).map(|s| s.is_empty()).unwrap_or(true) && !id.is_empty() {
                    // The generic `resolve()` above already found the primary
                    // seed via the fuzzy text index; fold it into the
                    // envelope rather than reporting an empty seed set.
                    if let Some(object) = seed_envelope.as_object_mut() {
                        object.insert("seeds".into(), json!([{"id": id, "reason": "resolved_seed", "exactness": "exact", "evidence": Value::Null}]));
                    }
                }
                let seed_ids: Vec<String> = seed_envelope.get("seeds").and_then(Value::as_array).into_iter().flatten()
                    .filter_map(|s| s.get("id").and_then(Value::as_str)).map(str::to_owned).collect();
                let changed_paths: Vec<String> = seed_envelope.get("changedPaths").and_then(Value::as_array).into_iter().flatten()
                    .filter_map(Value::as_str).map(str::to_owned).collect();
                let impacted_ids: Vec<String> = values.iter().filter_map(|n| n.get("id").and_then(Value::as_str)).map(str::to_owned).collect();
                let edge_tiers: Vec<Option<String>> = edges.iter().map(|edge| edge.evidence.iter().find_map(|e| e.get("confidenceTier").and_then(Value::as_str)).map(str::to_owned)).collect();
                let risk = crate::analytics::decompose_change_risk(crate::analytics::RiskInput {
                    changed_paths,
                    impacted_ids: impacted_ids.clone(),
                    edge_confidence_tiers: edge_tiers,
                    truncated: fields.get("omissions").and_then(Value::as_array).map(|o| !o.is_empty()).unwrap_or(false),
                    ambiguous_seeds: 0,
                    stale: false,
                    cochange_score: request.input.get("cochangeScore").and_then(Value::as_f64).unwrap_or(0.0),
                });
                let max_recs = request.input.get("maxTestRecommendations").and_then(Value::as_u64);
                let all_impacted: Vec<String> = seed_ids.iter().cloned().chain(impacted_ids.into_iter()).collect();
                let test_recommendations = crate::test_recommendation::recommend_tests_for_impact(generation, &generation.generation_id, &all_impacted, max_recs);
                if let Some(recs_omissions) = test_recommendations.get("omissions").and_then(Value::as_array) {
                    if let Some(existing) = fields.get_mut("omissions").and_then(Value::as_array_mut) {
                        existing.extend(recs_omissions.iter().cloned());
                    }
                }
                if let Some(envelope_omissions) = seed_envelope.get("omissions").and_then(Value::as_array) {
                    if let Some(existing) = fields.get_mut("omissions").and_then(Value::as_array_mut) {
                        existing.extend(envelope_omissions.iter().cloned());
                    }
                }
                fields.insert("seedEnvelope".into(), seed_envelope);
                fields.insert("risk".into(), risk);
                fields.insert("testRecommendations".into(), test_recommendations);
            }
            Ok(envelope(request, &generation.generation_id, &state, fields))
        }
        Operation::Path => {
            let from = requested(&request.input,"from").as_str().unwrap_or("").to_owned(); let to = requested(&request.input,"to").as_str().unwrap_or("").to_owned();
            let nodes = node_map(generation);
            // BPT-026 parity (lane GC14): bounded search now collects up to
            // `limits.paths` alternative from->to candidates instead of
            // stopping at the first BFS-found (shortest-hop) path, then
            // ranks them with the ported `recall_circuit::compare_paths`
            // non-compensatory ordering (semantic authority, then edge
            // confidence tier, before hop count) so the served path is at
            // least as good as the legacy JS producer, not merely shortest.
            let (raw_paths, mut path_omissions) = enumerate_paths(generation, &from, &to, limits, context)?;
            let ranked = rank_paths(generation, &from, raw_paths);
            let (path, path_edges, state) = match ranked {
                Some((node_ids, edge_refs, _atomic)) => (node_ids, edge_refs, "complete"),
                None => (Vec::new(), Vec::new(), "unresolved"),
            };
            let mut omissions = if state == "complete" { vec![] } else { vec![omission("path_not_found",None)] }; omissions.append(&mut path_omissions);
            Ok(envelope(request,&generation.generation_id,state,Map::from_iter([("requestedFrom".into(),json!(from)),("requestedTo".into(),json!(to)),("found".into(),json!(state=="complete")),("path".into(),json!(path.iter().filter_map(|i|nodes.get(i.as_str()).map(|n|node_value(n))).collect::<Vec<_>>())),("edges".into(),json!(path_edges.into_iter().map(|e|edge_value(e)).collect::<Vec<_>>())),("omissions".into(),json!(omissions))])))
        }
        Operation::Architecture => {
            // Lane WIRE1: legacy `service.mjs` view dispatch
            // (default/flows/liveness/processes/contracts/signatures/
            // orientation/projection/changes; see
            // blueprint/src/lib/application/service.mjs ~L730-791).
            // `view` absent or "summary" falls through unchanged to the
            // native BM03 task-orientation envelope below. Named views are
            // served by `crate::architecture_views::dispatch_view`, cached
            // through the dependency-dag `ProjectionCache` keyed on this
            // generation's repo root.
            if let Some(view) = request.input.get("view").and_then(Value::as_str) {
                if view != "summary" {
                    let mut result = crate::architecture_views::dispatch_view(generation, request, view)?;
                    // Signature projection's V1 ordering is qualified-symbol
                    // order, with deterministic path/id tie breakers. The
                    // shared view owns cache/invalidation; this final native
                    // projection only restores ordering for closed graph
                    // evidence where qualifiedName is nested.
                    if view == "signatures" {
                        if let Some(rows) = result.get_mut("signatures").and_then(Value::as_array_mut) {
                            rows.sort_by(|left, right| {
                                let line = |row: &Value| row.get("line").and_then(Value::as_u64).unwrap_or(u64::MAX);
                                let key = |row: &Value| row.get("qualifiedName").and_then(Value::as_str)
                                    .or_else(|| row.get("name").and_then(Value::as_str))
                                    .or_else(|| row.get("id").and_then(Value::as_str)).unwrap_or("").to_ascii_lowercase();
                                line(left).cmp(&line(right)).then_with(|| key(left).cmp(&key(right)))
                            });
                        }
                    }
                    return Ok(result);
                }
            }
            // BM03: every orientation section reports an explicit disposition
            // (evaluated/partial/unavailable/not_evaluated) via
            // `OrientationSectionV1` rather than a single hardcoded
            // "complete" envelope with empty arrays. An empty but *performed*
            // section (`empty_evaluated`) must stay distinguishable from a
            // section that was never run (`not_evaluated`) or that could not
            // be run for a resolved reason (`unavailable`).
            use crate::contracts::OrientationSectionV1;
            let orient = |section: OrientationSectionV1| serde_json::to_value(&section).unwrap_or_else(|_| json!({"disposition":"unavailable","reason":"serialize_failed"}));
            let task = request.input.get("task").or_else(||request.input.get("query")).and_then(Value::as_str).unwrap_or("");

            let (anchors_section, alternatives_section, resolved_id, resolution_meta) = if task.trim().is_empty() {
                (
                    orient(OrientationSectionV1::not_evaluated("no_task_provided")),
                    orient(OrientationSectionV1::not_evaluated("no_task_provided")),
                    None,
                    Value::Null,
                )
            } else {
                let (id, resolution) = resolve(generation, task, limits, context)?;
                // Preserve ambiguous alternatives, confidence/basis
                // (resolutionTier), same-name decoys and low-confidence
                // results exactly as `resolve` reports them.
                let candidate_items: Vec<Value> = resolution.get("candidates").and_then(Value::as_array).cloned().unwrap_or_default();
                let ambiguous = resolution.get("ambiguous").and_then(Value::as_bool).unwrap_or(false);
                let anchors = if candidate_items.is_empty() {
                    OrientationSectionV1::empty_evaluated()
                } else {
                    // `resolve` does not truncate its candidate list (no
                    // separate ceiling is applied beyond what it returns), so
                    // the returned items ARE the total known set: state that
                    // count explicitly (BM03) rather than omitting it.
                    let total = candidate_items.len() as u64;
                    OrientationSectionV1::evaluated(candidate_items.clone(), Some(total), false)
                };
                let alternatives = if ambiguous {
                    let total = candidate_items.len() as u64;
                    OrientationSectionV1::evaluated(candidate_items, Some(total), false)
                } else {
                    OrientationSectionV1::empty_evaluated()
                };
                (orient(anchors), orient(alternatives), if id.is_empty() { None } else { Some(id) }, resolution)
            };

            let nodes = node_map(generation);
            let (callers_section, callees_section, impact_section, source_signature_section) = if let Some(id) = &resolved_id {
                check(context)?;
                let (caller_ids, _caller_edges, _caller_depths, _caller_omissions) = adjacent(generation, id, "in", limits, context)?;
                let callers_items: Vec<Value> = caller_ids.iter().filter(|n| n.as_str() != id).filter_map(|n| nodes.get(n.as_str()).map(|node| node_value(node))).collect();
                let (callee_ids, _callee_edges, _callee_depths, _callee_omissions) = adjacent(generation, id, "out", limits, context)?;
                let callees_items: Vec<Value> = callee_ids.iter().filter(|n| n.as_str() != id).filter_map(|n| nodes.get(n.as_str()).map(|node| node_value(node))).collect();
                let (_impact_ids, impact_edges, _impact_depths, _impact_omissions) = adjacent(generation, id, "in", limits, context)?;
                let impact_items: Vec<Value> = impact_edges.iter().map(|edge| {
                    let tier = edge.evidence.iter().find_map(|e| e.get("confidenceTier").and_then(Value::as_str));
                    let class = crate::model::ImpactFrontierClass::classify(edge.target.as_deref(), tier);
                    json!({"edgeId": edge.id, "class": class.as_str(), "nodeId": edge.source})
                }).collect();
                let callers = if callers_items.is_empty() { OrientationSectionV1::empty_evaluated() } else { let total = callers_items.len() as u64; OrientationSectionV1::evaluated(callers_items, Some(total), false) };
                let callees = if callees_items.is_empty() { OrientationSectionV1::empty_evaluated() } else { let total = callees_items.len() as u64; OrientationSectionV1::evaluated(callees_items, Some(total), false) };
                let impact = if impact_items.is_empty() { OrientationSectionV1::empty_evaluated() } else { let total = impact_items.len() as u64; OrientationSectionV1::evaluated(impact_items, Some(total), false) };
                let source_signature = nodes.get(id.as_str())
                    .map(|node| OrientationSectionV1::evaluated(vec![node_value(node)], Some(1), false))
                    .unwrap_or_else(|| OrientationSectionV1::unavailable("anchor_node_missing"));
                (orient(callers), orient(callees), orient(impact), orient(source_signature))
            } else {
                let reason = "no_unique_anchor";
                (
                    orient(OrientationSectionV1::unavailable(reason)),
                    orient(OrientationSectionV1::unavailable(reason)),
                    orient(OrientationSectionV1::unavailable(reason)),
                    orient(OrientationSectionV1::unavailable(reason)),
                )
            };

            // Sections this native Architecture orientation does not yet
            // populate (ownership metadata, test discovery, config surfaces,
            // governing claims/constraints) are reported as genuinely
            // `not_evaluated` -- never rendered as an unimplemented
            // empty-complete section.
            let not_implemented = |reason: &str| orient(OrientationSectionV1::not_evaluated(reason));

            Ok(envelope(request, &generation.generation_id, "complete", Map::from_iter([
                ("task".into(), json!(task)),
                ("resolution".into(), resolution_meta),
                ("anchors".into(), anchors_section),
                ("alternatives".into(), alternatives_section),
                ("sourceSignature".into(), source_signature_section),
                ("owner".into(), not_implemented("ownership_metadata_not_implemented")),
                ("component".into(), not_implemented("ownership_metadata_not_implemented")),
                ("callers".into(), callers_section),
                ("callees".into(), callees_section),
                ("tests".into(), not_implemented("test_discovery_not_implemented")),
                ("impact".into(), impact_section),
                ("config".into(), not_implemented("config_orientation_not_implemented")),
                ("governingClaims".into(), not_implemented("claims_orientation_not_implemented")),
                ("derivedConstraints".into(), not_implemented("claims_orientation_not_implemented")),
                ("freshness".into(), json!({"generationId":generation.generation_id,"complete":generation.complete})),
                ("omissions".into(), json!([])),
            ])))
        }
        _ => Err(BlueprintError::invalid(format!("query does not support operation {}", request.method.as_str()))),
    }
}
