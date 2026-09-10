//! Native, generation-bound Blueprint query projections.
//!
//! This module is deliberately a projection over [`GraphGeneration`].  It does
//! not open storage or maintain a second index/ranker; every answer is derived
//! from the served generation and reports bounded work explicitly.

use crate::api::{BlueprintError, BlueprintRequest, RequestContext};
use crate::contracts::{BlueprintCandidateSetV1, BlueprintCandidateV1};
use crate::graph::GraphGeneration;
use crate::model::{GraphEdge, GraphNode, Operation};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

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
        for key in ["qualifiedName", "text"] {
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
            return Ok((String::new(), json!({"state":"ambiguous","resolutionTier":tier,"requested":query,"candidates":candidates,"candidateCount":candidates.len(),"ambiguous":true,"omissions":[omission("same_tier_ambiguity",Some(total_matches.saturating_sub(limits.seeds)))]})));
        }
        let state = if tier == "fuzzy" { "low_confidence" } else { "resolved" };
        return Ok((if state == "resolved" { limited[0].id.clone() } else { String::new() }, json!({"state":state,"resolutionTier":tier,"requested":query,"resolved":if state == "resolved" { node_value(limited[0]) } else { Value::Null },"candidates":candidates,"candidateCount":1,"ambiguous":false,"omissions":if state == "low_confidence" { vec![omission("low_confidence_resolution",None)] } else { vec![] }})));
    }
    Ok((String::new(), json!({"state":"unresolved","resolutionTier":"unresolved","requested":query,"candidates":[],"candidateCount":0,"ambiguous":false,"omissions":[omission("no_match",None)]})))
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

pub fn execute_query(generation: &GraphGeneration, request: &BlueprintRequest, context: &RequestContext) -> Result<Value, BlueprintError> {
    check(context)?;
    let limits = Limits::from(request, context);
    if stale(request, generation) || !generation.complete {
        let reason = if stale(request, generation) { "stale_generation" } else { "incomplete_generation" };
        let omissions = vec![omission(reason, None)];
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
            let (_id, resolution) = resolve(generation, raw, limits, context)?;
            let state = resolution.get("state").and_then(Value::as_str).unwrap_or("unresolved").to_owned();
            let resolved = resolution.get("candidates").and_then(Value::as_array).into_iter().flatten()
                .filter_map(|value| serde_json::from_value::<GraphNode>(value.clone()).ok()).collect::<Vec<_>>();
            let total = resolution.get("candidateCount").and_then(Value::as_u64).map(|count| count as usize);
            let resolution_omissions = resolution.get("omissions").and_then(Value::as_array).cloned().unwrap_or_default();
            let set = candidate_set(&state, resolved.iter(), total, state != "resolved", resolution_omissions);
            Ok(envelope(request, &generation.generation_id, &state, Map::from_iter([("requestedTarget".into(),json!(raw)),("resolution".into(),resolution),("candidateSet".into(),set)])))
        }
        Operation::Expand | Operation::Recall | Operation::Impact => {
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
            Ok(envelope(request, &generation.generation_id, &state, Map::from_iter([("requestedSeed".into(),json!(raw)),("resolution".into(),resolution), ("root".into(),json!(id)), ("target".into(),json!(if request.method == Operation::Impact {nodes.get(id.as_str()).map(|n| node_value(n)).unwrap_or(json!({"id":id}))} else {Value::Null})), ("direction".into(),json!(direction)), ("nodes".into(),json!(values)), ("edges".into(),json!(edges.iter().map(|e|edge_value(e)).collect::<Vec<_>>())), ("depths".into(),json!(depths)), ("impact".into(),json!(impact_classes)), ("omissions".into(),json!(omissions)), ("candidateSet".into(),set)])))
        }
        Operation::Path => {
            let from = requested(&request.input,"from").as_str().unwrap_or("").to_owned(); let to = requested(&request.input,"to").as_str().unwrap_or("").to_owned();
            let mut queue = VecDeque::from([(from.clone(), vec![from.clone()], Vec::<&GraphEdge>::new())]); let mut visited = BTreeSet::from([from.clone()]); let mut found = None; let nodes = node_map(generation);
            let mut path_omissions = Vec::new();
            while let Some((current, path, path_edges)) = queue.pop_front() { check(context)?; if current == to { found=Some((path,path_edges)); break; } if path_edges.len() >= limits.path_len { path_omissions.push(omission("path_length_ceiling",Some(1))); continue; } for edge in generation.edges.iter().filter(|e| e.source == current).take(limits.fanout) { let Some(next)=edge.target.clone() else {continue}; if visited.len() >= limits.nodes { path_omissions.push(omission("node_ceiling",Some(1))); break; } if visited.insert(next.clone()) { let mut np=path.clone(); np.push(next.clone()); let mut ne=path_edges.clone(); ne.push(edge); if ne.len() <= limits.edges { queue.push_back((next,np,ne)); } else { path_omissions.push(omission("edge_ceiling",Some(1))); } } } }
            let (path, path_edges, state) = found.map(|(p,e)|(p,e,"complete")).unwrap_or((Vec::new(),Vec::new(),"unresolved"));
            let mut omissions = if state == "complete" { vec![] } else { vec![omission("path_not_found",None)] }; omissions.extend(path_omissions);
            Ok(envelope(request,&generation.generation_id,state,Map::from_iter([("requestedFrom".into(),json!(from)),("requestedTo".into(),json!(to)),("found".into(),json!(state=="complete")),("path".into(),json!(path.iter().filter_map(|i|nodes.get(i.as_str()).map(|n|node_value(n))).collect::<Vec<_>>())),("edges".into(),json!(path_edges.into_iter().map(edge_value).collect::<Vec<_>>())),("omissions".into(),json!(omissions))])))
        }
        Operation::Architecture => {
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
