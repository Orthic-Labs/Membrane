//! Native Rust port of `blueprint/src/graph/recall-circuit.mjs`.
//!
//! Operates on `serde_json::Value` node/edge/path shapes, the same idiom
//! used by [`crate::evidence_authority`] and [`crate::entry_points`], so
//! behavior matches the legacy JS field-for-field. Reuses
//! [`crate::confidence_tiers`] for tier vocabulary/ordering and
//! [`crate::evidence_authority::semantic_authority_rank_for_fact`] for the
//! authority-rank leg of `comparePaths` (BPT-026).
//!
//! Scope note: `executeRecallCircuit` in the legacy source drives its BFS
//! from a live SQLite store via `resolveSeeds` / `selectTraversalPolicy` /
//! `traversalNeighbors` (store-sqlite.mjs, seed-resolver.mjs,
//! traversal-policy.mjs). This crate has no native port of those three
//! modules yet (no `seed_resolver.rs` / `traversal_policy.rs` exist here),
//! so [`execute_recall_circuit`] below takes an already-hydrated in-memory
//! node/edge/seed set (`RecallGraph`) instead of a DB handle + task string.
//! The traversal, path construction, dedup, and ordering are otherwise an
//! exact port. This is recorded as a finding in the lane receipt rather
//! than forced.

use crate::confidence_tiers::EDGE_CONFIDENCE_TIER_ORDER;
use crate::evidence_authority::{semantic_authority_for_fact, semantic_authority_rank_for_fact};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet, VecDeque};

/// Mirrors `tierRank`: known tiers map to their position in
/// [`EDGE_CONFIDENCE_TIER_ORDER`]; anything else (including missing/None)
/// is worst-of-all at rank 4, exactly like the JS `?? 4` fallback.
pub fn tier_rank(tier: Option<&str>) -> i64 {
    match tier {
        Some(t) => EDGE_CONFIDENCE_TIER_ORDER
            .iter()
            .position(|v| *v == t)
            .map(|i| i as i64)
            .unwrap_or(4),
        None => 4,
    }
}

fn evidence_for(value: &Value) -> Vec<Value> {
    match value.get("evidence") {
        Some(Value::Array(items)) => items
            .iter()
            .filter(|v| !v.is_null() && *v != &Value::Bool(false))
            .cloned()
            .collect(),
        _ => Vec::new(),
    }
}

/// Mirrors `stableDigest`: `sha256:<hex>` of the JSON-serialized value.
/// Note: JS `JSON.stringify` preserves object-key insertion order; Rust
/// `serde_json::Value` (default features) also preserves insertion order
/// only when the `preserve_order` feature is enabled. We build digest
/// inputs from explicit field lists below (not passthrough re-serialized
/// upstream objects) so ordering is deterministic regardless.
pub fn stable_digest(value: &Value) -> String {
    let bytes = serde_json::to_vec(value).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

/// A directed adjacency edge: target node id + full edge payload.
#[derive(Debug, Clone)]
pub struct AdjacencyEntry {
    pub to: String,
    pub edge: Value,
}

/// Mirrors `adjacency(edgeRows, direction)`. `direction` is `"out"`,
/// `"in"`, or anything else (both), matching the JS `direction !== "in"` /
/// `direction !== "out"` guards.
pub fn adjacency(edge_rows: &[Value], direction: &str) -> HashMap<String, Vec<AdjacencyEntry>> {
    let mut result: HashMap<String, Vec<AdjacencyEntry>> = HashMap::new();
    let mut add = |from: Option<&str>, to: Option<&str>, edge: &Value| {
        let (Some(from), Some(to)) = (from, to) else { return };
        if from.is_empty() || to.is_empty() {
            return;
        }
        result
            .entry(from.to_string())
            .or_default()
            .push(AdjacencyEntry { to: to.to_string(), edge: edge.clone() });
    };
    for edge in edge_rows {
        let source = edge.get("source").and_then(Value::as_str);
        let target = edge.get("target").and_then(Value::as_str);
        if direction != "in" {
            add(source, target, edge);
        }
        if direction != "out" {
            add(target, source, edge);
        }
    }
    // Resolution specificity is structural traversal order only. It is not
    // a scalar confidence competition between producers.
    for entries in result.values_mut() {
        entries.sort_by(|a, b| {
            let ta = a.edge.get("confidence_tier").and_then(Value::as_str);
            let tb = b.edge.get("confidence_tier").and_then(Value::as_str);
            tier_rank(ta).cmp(&tier_rank(tb)).then_with(|| {
                let ia = a.edge.get("id").and_then(Value::as_str).unwrap_or("");
                let ib = b.edge.get("id").and_then(Value::as_str).unwrap_or("");
                ia.cmp(ib)
            })
        });
    }
    result
}

/// A seed, mirroring the `resolveSeeds` output shape used by this module.
#[derive(Debug, Clone)]
pub struct Seed {
    pub id: String,
    pub exactness: i64,
    pub reason: Option<String>,
    pub evidence: Value,
}

/// AtomicEvidencePath-shaped output. Mirrors the object built by `makePath`.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct AtomicPath {
    pub id: String,
    pub seed_id: String,
    pub terminal_id: Option<String>,
    pub node_ids: Vec<String>,
    pub edge_ids: Vec<String>,
    pub minimum_edge_tier: String,
    pub minimum_semantic_authority: Option<Value>,
    pub semantic_authority_rank: i64,
    pub seed_exactness: i64,
    pub evidence_coverage: f64,
    pub hop_count: usize,
    pub state: &'static str, // "complete" | "partial"
    pub omission_reasons: Vec<&'static str>,
    pub evidence_envelope: Value,
}

/// Mirrors `makePath(seed, nodeIds, edgeIds, nodeMap, edgeMap, complete, generationId)`.
pub fn make_path(
    seed: &Seed,
    node_ids: &[String],
    edge_ids: &[String],
    node_map: &HashMap<String, Value>,
    edge_map: &HashMap<String, Value>,
    complete: bool,
    generation_id: &str,
) -> AtomicPath {
    let nodes: Vec<Value> = node_ids.iter().filter_map(|id| node_map.get(id)).cloned().collect();
    let edges: Vec<Value> = edge_ids.iter().filter_map(|id| edge_map.get(id)).cloned().collect();

    let mut evidence: Vec<Value> = Vec::new();
    for n in &nodes {
        evidence.extend(evidence_for(n));
    }
    for e in &edges {
        evidence.extend(evidence_for(e));
    }

    let edge_tiers: Vec<String> = edges
        .iter()
        .filter_map(|e| e.get("confidenceTier").and_then(Value::as_str).map(|s| s.to_string()))
        .collect();

    let authority_ranks: Vec<i64> = edges.iter().map(semantic_authority_rank_for_fact).collect();
    let weakest_authority_rank = authority_ranks.iter().copied().max().unwrap_or(0);
    let weakest_authority_edge = edges
        .iter()
        .find(|e| semantic_authority_rank_for_fact(e) == weakest_authority_rank);
    let minimum_semantic_authority =
        weakest_authority_edge.map(|e| json!(semantic_authority_for_fact(e)));

    let terminal_id = node_ids.last().cloned();
    let projection = json!({
        "seed": seed.id,
        "terminal": terminal_id,
        "nodeIds": node_ids,
        "edgeIds": edge_ids,
    });
    let id = stable_digest(&projection);
    let omission_reasons: Vec<&'static str> = if complete { vec![] } else { vec!["bound_reached"] };

    let node_evidence: Vec<Value> = nodes
        .iter()
        .map(|n| json!({ "nodeId": n.get("id"), "evidence": evidence_for(n) }))
        .collect();
    let edge_evidence: Vec<Value> = edges
        .iter()
        .map(|e| {
            json!({
                "edgeId": e.get("id"),
                "source": e.get("source"),
                "target": e.get("target"),
                "evidence": evidence_for(e),
            })
        })
        .collect();
    let evidence_envelope = json!({
        "schemaVersion": 1,
        "kind": "AtomicEvidencePath",
        "id": id,
        "generationId": generation_id,
        "completeness": if complete { "exact" } else { "lower_bound" },
        "nodeEvidence": node_evidence,
        "edgeEvidence": edge_evidence,
        "omissions": omission_reasons.iter().map(|r| json!({"reason": r})).collect::<Vec<_>>(),
    });

    // minimumEdgeTier: worst (highest rank) tier among edges; default
    // EXACT_RESOLUTION when there are none, mirroring `?? "EXACT_RESOLUTION"`.
    let minimum_edge_tier = edge_tiers
        .iter()
        .max_by_key(|t| tier_rank(Some(t.as_str())))
        .cloned()
        .unwrap_or_else(|| "EXACT_RESOLUTION".to_string());

    let denom = nodes.len() + edges.len();
    let evidence_coverage = if denom > 0 { evidence.len() as f64 / denom as f64 } else { 0.0 };

    AtomicPath {
        id,
        seed_id: seed.id.clone(),
        terminal_id,
        node_ids: node_ids.to_vec(),
        edge_ids: edge_ids.to_vec(),
        minimum_edge_tier,
        minimum_semantic_authority,
        semantic_authority_rank: weakest_authority_rank,
        seed_exactness: seed.exactness,
        evidence_coverage,
        hop_count: edge_ids.len(),
        state: if complete { "complete" } else { "partial" },
        omission_reasons,
        evidence_envelope,
    }
}

/// The non-compensatory Recall ordering (BPT-026). Exact lexicographic
/// chain, field by field, matching the legacy `comparePaths` order:
/// (1) complete before partial, (2) semanticAuthorityRank ascending,
/// (3) minimumEdgeTier rank ascending, (4) seedExactness ascending,
/// (5) evidenceCoverage DESCENDING, (6) hopCount ascending,
/// (7) id lexicographic tiebreak. This must NEVER become a weighted/summed
/// score.
pub fn compare_paths(left: &AtomicPath, right: &AtomicPath) -> Ordering {
    let complete = |p: &AtomicPath| if p.state == "complete" { 0 } else { 1 };
    complete(left)
        .cmp(&complete(right))
        .then_with(|| left.semantic_authority_rank.cmp(&right.semantic_authority_rank))
        .then_with(|| {
            tier_rank(Some(left.minimum_edge_tier.as_str()))
                .cmp(&tier_rank(Some(right.minimum_edge_tier.as_str())))
        })
        .then_with(|| left.seed_exactness.cmp(&right.seed_exactness))
        // evidenceCoverage DESCENDING: right.compare(left)
        .then_with(|| {
            right
                .evidence_coverage
                .partial_cmp(&left.evidence_coverage)
                .unwrap_or(Ordering::Equal)
        })
        .then_with(|| left.hop_count.cmp(&right.hop_count))
        .then_with(|| left.id.cmp(&right.id))
}

/// Bounds for one traversal, mirroring `selectTraversalPolicy`'s output
/// shape as consumed by `executeRecallCircuit`. The 7 traversal-policy
/// families (dependency.forward, impact.reverse, callgraph.forward,
/// test.coverage, config.consumers, architecture.boundary, explore.both)
/// live in the legacy `traversal-policy.mjs`, which has no native port in
/// this crate yet; callers of [`execute_recall_circuit`] supply the
/// resolved policy directly.
#[derive(Debug, Clone)]
pub struct TraversalPolicy {
    pub family: String,
    pub direction: String, // "out" | "in" | "both"
    pub max_hops: usize,
    pub max_nodes: usize,
    pub max_edges: usize,
    pub max_paths: usize,
    pub evidence_required: bool,
    pub max_seeds: usize,
}

impl Default for TraversalPolicy {
    fn default() -> Self {
        TraversalPolicy {
            family: "explore.both".to_string(),
            direction: "both".to_string(),
            max_hops: 4,
            max_nodes: 500,
            max_edges: 1000,
            max_paths: 50,
            evidence_required: false,
            max_seeds: 8,
        }
    }
}

/// Mirrors `traversal-policy.mjs`'s `POLICY_DEFINITIONS`: family name,
/// direction, relationship kinds, and default max hop count.
const POLICY_DEFINITIONS: &[(&str, &str, &[&str], usize)] = &[
    ("dependency.forward", "out", &["IMPORTS", "CALLS", "CONFIGURES"], 3),
    ("impact.reverse", "in", &["IMPORTS", "CALLS", "TESTS", "CONFIGURES", "REFERENCES", "OVERRIDES"], 3),
    ("callgraph.forward", "out", &["CALLS"], 4),
    ("test.coverage", "both", &["TESTS", "REFERENCES", "IMPORTS"], 3),
    ("config.consumers", "out", &["CONFIGURES", "REFERENCES"], 3),
    ("architecture.boundary", "both", &["IMPORTS", "CALLS", "OVERRIDES", "CONTAINS", "DEFINES", "REFERENCES", "DOCS_LINK"], 2),
    ("explore.both", "both", &["IMPORTS", "CALLS", "OVERRIDES", "TESTS", "CONFIGURES", "CONTAINS", "DEFINES", "REFERENCES", "DOCS_LINK"], 2),
];

const DEFAULT_MAX_SEEDS: usize = 8;
const DEFAULT_MAX_PATHS: usize = 40;
const DEFAULT_MAX_NODES: usize = 160;
const DEFAULT_MAX_EDGES: usize = 320;

fn clamp_limit(requested: Option<usize>, fallback: usize) -> usize {
    let r = requested.unwrap_or(fallback);
    r.max(1).min(fallback)
}

/// Mirrors `selectTraversalPolicy(task, requested, limits)`. `task` drives
/// family auto-selection via the same substring checks as the legacy regex
/// (which itself has no word-boundary anchors, so `.contains` is an exact
/// behavioral match, not an approximation). Returns `Err` for an explicitly
/// `requested` family that is not one of the 7 known families, mirroring
/// the legacy `traversal_policy_unknown` error.
pub fn select_traversal_policy_native(
    task: &str,
    requested: Option<&str>,
    max_hops: Option<usize>,
    max_seeds: Option<usize>,
    max_paths: Option<usize>,
    max_nodes: Option<usize>,
    max_edges: Option<usize>,
) -> Result<TraversalPolicy, String> {
    let text = task.to_ascii_lowercase();
    fn auto_family(text: &str) -> &'static str {
        if ["impact", "affected", "caller", "consumer", "break"].iter().any(|k| text.contains(k)) {
            "impact.reverse"
        } else if ["test", "coverage", "spec"].iter().any(|k| text.contains(k)) {
            "test.coverage"
        } else if ["config", "setting", "environment"].iter().any(|k| text.contains(k)) {
            "config.consumers"
        } else if ["call", "invoke", "execution"].iter().any(|k| text.contains(k)) {
            "callgraph.forward"
        } else if ["architecture", "boundary", "component", "layer"].iter().any(|k| text.contains(k)) {
            "architecture.boundary"
        } else if ["depend", "import", "require"].iter().any(|k| text.contains(k)) {
            "dependency.forward"
        } else {
            "explore.both"
        }
    }
    let family = requested.filter(|f| !f.is_empty()).unwrap_or_else(|| auto_family(&text));
    let Some((_, direction, kinds, default_hops)) = POLICY_DEFINITIONS.iter().find(|(f, ..)| *f == family) else {
        return Err(format!("unknown traversal policy: {family}"));
    };
    let _ = kinds; // kinds re-derived from `family` at traversal time via `policy_kinds`
    Ok(TraversalPolicy {
        family: family.to_string(),
        direction: direction.to_string(),
        max_hops: clamp_limit(max_hops, *default_hops),
        max_nodes: clamp_limit(max_nodes, DEFAULT_MAX_NODES),
        max_edges: clamp_limit(max_edges, DEFAULT_MAX_EDGES),
        max_paths: clamp_limit(max_paths, DEFAULT_MAX_PATHS),
        max_seeds: clamp_limit(max_seeds, DEFAULT_MAX_SEEDS),
        evidence_required: true,
    })
}

/// Relationship kinds for a policy family, mirroring `POLICY_DEFINITIONS`.
/// Empty slice for an unrecognized family (should not happen for a family
/// produced by [`select_traversal_policy_native`]).
pub fn policy_kinds(family: &str) -> &'static [&'static str] {
    POLICY_DEFINITIONS.iter().find(|(f, ..)| *f == family).map(|(_, _, kinds, _)| *kinds).unwrap_or(&[])
}

/// Already-hydrated in-memory node/edge/seed set for one traversal. Stands
/// in for the legacy `db` + `traversalNeighbors` frontier.
pub struct RecallGraph {
    pub nodes: HashMap<String, Value>,
    pub edges: Vec<Value>, // each carries id/source/target/confidence_tier etc.
}

pub struct RecallCircuitResult {
    pub id: String,
    pub generation_id: String,
    pub policy_family: String,
    pub paths: Vec<AtomicPath>,
    pub omissions: Vec<Value>,
    pub state: &'static str, // "complete" | "ambiguous" | "abstained"
}

/// Mirrors the BFS/path-building/dedup/sort/omission-accounting core of
/// `executeRecallCircuit`, operating on an already-resolved `seeds` list
/// and an already-hydrated `graph` (see module doc for the scope note on
/// what is NOT ported: live seed resolution against a DB).
pub fn execute_recall_circuit(
    graph: &RecallGraph,
    seeds: &[Seed],
    policy: &TraversalPolicy,
    generation_id: &str,
) -> RecallCircuitResult {
    execute_recall_circuit_with_totals(graph, seeds, policy, generation_id, graph.nodes.len(), graph.edges.len())
}

/// Store-driving variant of [`execute_recall_circuit`]: identical BFS/path
/// core, but `total_seen_nodes`/`total_edge_rows` carry the *unbounded*
/// frontier size a live caller observed BEFORE it capped `graph` down to
/// `policy.max_nodes`/`policy.max_edges` (mirrors legacy's
/// `frontier.seenNodes.length` / `frontier.edgeRows.length` used for the
/// `node_ceiling`/`edge_ceiling` omission counts in `executeRecallCircuit`,
/// which are computed against the pre-cap frontier, not the post-cap
/// hydrated graph). [`execute_recall_circuit`] itself passes
/// `graph.nodes.len()`/`graph.edges.len()` here, preserving its existing
/// (pre-capped-graph-is-the-whole-frontier) behavior exactly for callers
/// that already hand it a pre-hydrated `RecallGraph`.
pub fn execute_recall_circuit_with_totals(
    graph: &RecallGraph,
    seeds: &[Seed],
    policy: &TraversalPolicy,
    generation_id: &str,
    total_seen_nodes: usize,
    total_edge_rows: usize,
) -> RecallCircuitResult {
    if seeds.is_empty() {
        let visible = json!({
            "generationId": generation_id,
            "policy": policy.family,
            "paths": Vec::<Value>::new(),
            "omissions": [{ "reason": "no_seeds_resolved" }],
        });
        return RecallCircuitResult {
            id: stable_digest(&visible),
            generation_id: generation_id.to_string(),
            policy_family: policy.family.clone(),
            paths: vec![],
            omissions: vec![json!({"reason": "no_seeds_resolved"})],
            state: "abstained",
        };
    }

    let node_map = &graph.nodes;
    let edge_map: HashMap<String, Value> = graph
        .edges
        .iter()
        .filter_map(|e| e.get("id").and_then(Value::as_str).map(|id| (id.to_string(), e.clone())))
        .collect();
    let adj = adjacency(&graph.edges, &policy.direction);

    let mut paths: Vec<AtomicPath> = Vec::new();
    let mut seen_path_ids: HashSet<String> = HashSet::new();

    struct Frame {
        node_id: String,
        node_ids: Vec<String>,
        edge_ids: Vec<String>,
        visited: HashSet<String>,
    }

    for seed in seeds {
        let mut queue: VecDeque<Frame> = VecDeque::new();
        queue.push_back(Frame {
            node_id: seed.id.clone(),
            node_ids: vec![seed.id.clone()],
            edge_ids: vec![],
            visited: HashSet::from([seed.id.clone()]),
        });
        while let Some(current) = queue.pop_front() {
            if paths.len() >= policy.max_paths {
                break;
            }
            let empty = Vec::new();
            let next: Vec<&AdjacencyEntry> = adj
                .get(&current.node_id)
                .unwrap_or(&empty)
                .iter()
                .filter(|item| !current.visited.contains(&item.to))
                .collect();
            let is_terminal = !current.edge_ids.is_empty()
                && (next.is_empty() || current.edge_ids.len() >= policy.max_hops);
            if is_terminal {
                let path = make_path(
                    seed,
                    &current.node_ids,
                    &current.edge_ids,
                    node_map,
                    &edge_map,
                    next.is_empty(),
                    generation_id,
                );
                if (!policy.evidence_required || path.evidence_coverage > 0.0) && !seen_path_ids.contains(&path.id) {
                    seen_path_ids.insert(path.id.clone());
                    paths.push(path);
                }
            }
            if current.edge_ids.len() >= policy.max_hops {
                continue;
            }
            for item in next {
                let mut node_ids = current.node_ids.clone();
                node_ids.push(item.to.clone());
                let mut edge_ids = current.edge_ids.clone();
                if let Some(id) = item.edge.get("id").and_then(Value::as_str) {
                    edge_ids.push(id.to_string());
                }
                let mut visited = current.visited.clone();
                visited.insert(item.to.clone());
                queue.push_back(Frame { node_id: item.to.clone(), node_ids, edge_ids, visited });
            }
        }
    }

    if paths.is_empty() {
        for seed in seeds {
            paths.push(make_path(seed, &[seed.id.clone()], &[], node_map, &edge_map, true, generation_id));
        }
    }
    paths.sort_by(compare_paths);

    let mut omissions: Vec<Value> = Vec::new();
    if total_seen_nodes > policy.max_nodes {
        omissions.push(json!({"reason": "node_ceiling", "count": total_seen_nodes - policy.max_nodes}));
    }
    if total_edge_rows > policy.max_edges {
        omissions.push(json!({"reason": "edge_ceiling", "count": total_edge_rows - policy.max_edges}));
    }
    if paths.len() >= policy.max_paths {
        omissions.push(json!({"reason": "path_ceiling"}));
    }

    let id = stable_digest(&json!({
        "generationId": generation_id,
        "policy": policy.family,
        "paths": paths.iter().map(|p| p.id.clone()).collect::<Vec<_>>(),
        "omissions": omissions,
    }));

    RecallCircuitResult {
        id,
        generation_id: generation_id.to_string(),
        policy_family: policy.family.clone(),
        paths,
        omissions,
        state: "complete",
    }
}

/// Mirrors `canonicalCandidateSet`: the strict V1 candidate projection.
/// Operates on already-built `CandidateSetV1`-shaped JSON (from
/// `recall_circuit_to_candidate_set` or an equivalent native builder) since
/// this crate's own `CandidateSet`-shaped types are owned by `query.rs` /
/// `contracts.rs`; this stays a JSON-shape function to avoid creating a
/// parallel duplicate candidate-set type.
pub fn canonical_candidate_set(candidate_set: &Value) -> Value {
    let candidates = candidate_set
        .get("candidates")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let projected_candidates: Vec<Value> = candidates
        .iter()
        .map(|c| {
            let mut out = Map::new();
            out.insert("id".into(), c.get("id").cloned().unwrap_or(Value::Null));
            out.insert("layer".into(), c.get("layer").cloned().unwrap_or(Value::Null));
            if let Some(v) = c.get("provider") {
                out.insert("provider".into(), v.clone());
            }
            for key in ["sourceKind", "sourceRef", "sourceHash", "trustClass", "instructionPolicy", "providerScore"] {
                out.insert(key.into(), c.get(key).cloned().unwrap_or(Value::Null));
            }
            for key in ["scoreComponents", "baseCommit", "overlayDigest", "freshnessClass", "snapshotId"] {
                if let Some(v) = c.get(key) {
                    out.insert(key.into(), v.clone());
                }
            }
            for key in ["estimatedTokens", "protected", "exact", "recoverable", "resolver", "text"] {
                out.insert(key.into(), c.get(key).cloned().unwrap_or(Value::Null));
            }
            Value::Object(out)
        })
        .collect();

    let trace_id = candidate_set.get("traceId").cloned().unwrap_or(Value::Null);
    let omissions = candidate_set
        .get("omissions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let projected_omissions: Vec<Value> = omissions
        .iter()
        .enumerate()
        .map(|(index, o)| {
            let mut out = Map::new();
            let id = o
                .get("id")
                .cloned()
                .unwrap_or_else(|| json!(format!("{}:omission:{}", trace_id.as_str().unwrap_or(""), index)));
            out.insert("id".into(), id);
            if let Some(l) = o.get("layer") {
                out.insert("layer".into(), l.clone());
            }
            out.insert("reason".into(), o.get("reason").cloned().unwrap_or(Value::Null));
            Value::Object(out)
        })
        .collect();

    json!({
        "schemaVersion": candidate_set.get("schemaVersion"),
        "traceId": trace_id,
        "indexedAt": candidate_set.get("indexedAt"),
        "task": candidate_set.get("task"),
        "mode": candidate_set.get("mode"),
        "provider": candidate_set.get("provider"),
        "freshness": candidate_set.get("freshness"),
        "providerCeiling": candidate_set.get("providerCeiling"),
        "candidates": projected_candidates,
        "omissions": projected_omissions,
    })
}

/// Mirrors `recallCircuitToCandidateSet`. `providerScore`/`scoreComponents`
/// are PRESENTATIONAL ONLY (see the long comment in the legacy source and
/// the doc comment reproduced here): ordering is already final by the time
/// this runs, decided upstream by [`compare_paths`]'s non-compensatory
/// lexicographic chain. These emitted numbers must never be re-summed or
/// re-weighted into ranking.
pub fn recall_circuit_to_candidate_set(
    circuit: &RecallCircuitResult,
    task: &str,
    provider: &str,
    indexed_at: &str,
) -> Value {
    let mut candidates: Vec<Value> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for path in &circuit.paths {
        let Some(terminal_id) = &path.terminal_id else { continue };
        if seen.contains(terminal_id) {
            continue;
        }
        seen.insert(terminal_id.clone());
        let score_index = candidates.len() as f64;
        candidates.push(json!({
            "id": terminal_id,
            "layer": 3,
            "sourceKind": "repo_code",
            "trustClass": "workspace_tracked",
            "instructionPolicy": "data_only",
            // PRESENTATIONAL ONLY — see BPT-026 note above; never re-summed.
            "providerScore": 1.0 / (score_index + 1.0),
            "scoreComponents": {
                "semanticAuthority": 1.0 / ((path.semantic_authority_rank as f64) + 1.0),
                "evidenceTier": 1.0 / (tier_rank(Some(path.minimum_edge_tier.as_str())) as f64 + 1.0),
                "pathCompleteness": if path.state == "complete" { 1 } else { 0 },
                "evidenceCoverage": path.evidence_coverage,
            },
            "protected": &path.seed_id == terminal_id,
            "exact": path.seed_exactness <= 2,
            "recoverable": true,
            "resolver": format!("blueprint graph resolve --node {terminal_id}"),
            "text": terminal_id,
            "recallCircuitId": circuit.id,
            "evidencePathId": path.id,
            "evidenceEnvelope": path.evidence_envelope,
        }));
    }
    json!({
        "schemaVersion": 1,
        "traceId": circuit.id,
        "indexedAt": indexed_at,
        "task": task,
        "mode": "survey",
        "provider": provider,
        "freshness": { "revision": circuit.generation_id, "indexedAt": indexed_at, "stale": false },
        "providerCeiling": { "maxCandidates": candidates.len(), "maxEstimatedTokens": 8000 },
        "candidates": candidates,
        "omissions": circuit.omissions,
        "recallCircuit": { "id": circuit.id, "state": circuit.state, "policy": circuit.policy_family },
    })
}

// ---------------------------------------------------------------------
// Store-driving pieces (GC15): seed resolution and traversal hydration
// over an in-memory `GraphGeneration`'s nodes/edges, mirroring
// `seed-resolver.mjs` (`resolveSeeds`) and the `traversalNeighbors`
// frontier from `store-sqlite.mjs`. The native engine has no live SQLite
// query surface at this layer (`query.rs` is a projection over an
// already-loaded `GraphGeneration`, not a DB handle) so these lanes are
// re-expressed as in-memory scans over `&[GraphNode]` / `&[GraphEdge]`
// rather than SQL against `files`/`symbols`/`symbol_terms` tables. Lane
// ordering (explicit id, source address, qualified name, exact name,
// bounded lexical) and ambiguity/allow-many semantics are ported
// field-for-field; per-symbol *authority ranking* within a lane (legacy's
// `symbolAuthorityOrder` SQL expression) has no native counterpart and is
// replaced with a stable `id` sort — recorded as a scoping note in the
// lane receipt, not a silent behavior claim.
use crate::model::{GraphEdge, GraphNode};

/// Mirrors `terms(task)` in `seed-resolver.mjs`: camelCase-split, lowercase,
/// split on everything outside `[a-z0-9_./:-]`, drop stopwords/length<=1,
/// dedupe preserving first-seen order.
pub fn task_terms(task: &str) -> Vec<String> {
    const STOPWORDS: &[&str] = &[
        "a", "an", "and", "are", "as", "at", "be", "by", "for", "from", "in", "is", "it", "of", "on", "or", "that",
        "the", "this", "to", "with",
    ];
    let chars: Vec<char> = task.chars().collect();
    let mut spaced = String::with_capacity(task.len() + 8);
    for (i, c) in chars.iter().enumerate() {
        if i > 0 {
            let prev = chars[i - 1];
            if (prev.is_ascii_lowercase() || prev.is_ascii_digit()) && c.is_ascii_uppercase() {
                spaced.push(' ');
            }
        }
        spaced.push(*c);
    }
    let lower = spaced.to_ascii_lowercase();
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for tok in lower.split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '/' | ':' | '-'))) {
        if tok.len() > 1 && !STOPWORDS.contains(&tok) && seen.insert(tok.to_string()) {
            out.push(tok.to_string());
        }
    }
    out
}

/// Result of native seed resolution, mirroring the `SeedResolution` envelope
/// `resolveSeeds` returns.
pub struct SeedResolution {
    /// `"resolved" | "ambiguous" | "unresolved"`.
    pub state: &'static str,
    /// Non-empty only when `state == "resolved"`, mirroring
    /// `envelope(state, candidates, ...)`'s `seeds: state === "resolved" ? candidates : []`.
    pub seeds: Vec<Seed>,
    /// The full `SeedResolution`-shaped envelope, for embedding verbatim in
    /// a response the way legacy `recall` embeds `resolution`.
    pub json: Value,
}

fn node_seed_evidence(node: &GraphNode) -> Value {
    json!(node
        .evidence
        .iter()
        .filter(|e| !e.is_null())
        .map(|item| json!({
            "path": item.get("path").cloned().unwrap_or_else(|| node.path.clone().map(Value::String).unwrap_or(Value::Null)),
            "startLine": item.get("startLine").cloned().unwrap_or(Value::Null),
            "endLine": item.get("endLine").cloned().unwrap_or(Value::Null),
            "contentHash": item.get("contentHash").cloned().unwrap_or(Value::Null),
        }))
        .collect::<Vec<_>>())
}

fn seed_envelope(state: &'static str, seeds: &[Seed], attempts: &[Value], reason: Option<&str>) -> Value {
    let candidates: Vec<Value> = seeds
        .iter()
        .map(|s| json!({"id": s.id, "exactness": s.exactness, "reason": s.reason, "evidence": s.evidence}))
        .collect();
    json!({
        "schemaVersion": 1,
        "kind": "SeedResolution",
        "state": state,
        "seeds": if state == "resolved" { Value::Array(candidates.clone()) } else { json!([]) },
        "candidates": candidates,
        "candidateCount": seeds.len(),
        "ambiguous": state == "ambiguous",
        "reason": reason,
        "attempts": attempts,
    })
}

fn seeds_from_nodes(matched: Vec<&GraphNode>, exactness: i64, reason: &'static str, max_seeds: usize) -> Vec<Seed> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for n in matched {
        if out.len() >= max_seeds + 1 {
            break;
        }
        if seen.insert(n.id.clone()) {
            out.push(Seed { id: n.id.clone(), exactness, reason: Some(reason.to_string()), evidence: node_seed_evidence(n) });
        }
    }
    out
}

/// One resolution lane: dedupe+cap `matched` into seeds, decide
/// resolved/ambiguous (mirrors `decide(candidates, attempts, {allowMany})`),
/// and record the attempt. Returns `None` when nothing matched (legacy
/// falls through to the next lane); `Some` short-circuits resolution.
fn decide_lane(
    lane: &'static str,
    requested: usize,
    matched: Vec<&GraphNode>,
    exactness: i64,
    allow_many: bool,
    max_seeds: usize,
    attempts: &mut Vec<Value>,
) -> Option<SeedResolution> {
    let seeds = seeds_from_nodes(matched, exactness, lane, max_seeds);
    attempts.push(json!({"lane": lane, "requested": requested, "matched": seeds.len()}));
    if seeds.is_empty() {
        return None;
    }
    if !allow_many && seeds.len() > 1 {
        let json = seed_envelope("ambiguous", &seeds, attempts, Some("multiple_candidates"));
        return Some(SeedResolution { state: "ambiguous", seeds: vec![], json });
    }
    let json = seed_envelope("resolved", &seeds, attempts, None);
    Some(SeedResolution { state: "resolved", seeds, json })
}

/// Mirrors `resolveSeeds(db, task, {generationId, seedIds, anchors, maxSeeds,
/// allowAmbiguousTaskSeeds})`, re-expressed as in-memory lanes over
/// `nodes` (see module note above). Lane order: explicit `seed_ids`
/// (`node_id`, always allow-many), path/anchor match (`source_address`,
/// never allow-many), evidence `qualifiedName` exact match
/// (`qualified_symbol`), `name` exact match (`exact_term`), then `name`/
/// `path`/`qualifiedName` substring match (`bounded_lexical`) — the last
/// three allow-many only when `allow_ambiguous && anchors.is_empty()`,
/// exactly like legacy.
#[allow(clippy::too_many_arguments)]
pub fn resolve_seeds_native(
    nodes: &[GraphNode],
    seed_ids: &[String],
    anchors: &[String],
    task: &str,
    max_seeds: usize,
    allow_ambiguous: bool,
) -> SeedResolution {
    let mut attempts: Vec<Value> = Vec::new();
    let by_id: HashMap<&str, &GraphNode> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();

    if !seed_ids.is_empty() {
        let matched: Vec<&GraphNode> = seed_ids.iter().filter_map(|id| by_id.get(id.as_str()).copied()).collect();
        if let Some(result) = decide_lane("node_id", seed_ids.len(), matched, 0, true, max_seeds, &mut attempts) {
            return result;
        }
    }

    let normalized_anchors: Vec<String> = anchors.iter().map(|a| a.trim().to_string()).filter(|a| !a.is_empty()).collect();
    let terms = task_terms(task);

    let mut addresses: Vec<String> = normalized_anchors.clone();
    addresses.extend(terms.iter().filter(|t| t.contains('/') || t.contains('.')).cloned());
    let mut seen_addr: HashSet<String> = HashSet::new();
    let addresses: Vec<String> = addresses.into_iter().filter(|a| seen_addr.insert(a.clone())).collect();
    if !addresses.is_empty() {
        let normalized: Vec<String> = addresses
            .iter()
            .map(|raw| raw.strip_prefix("file:").unwrap_or(raw).replace('\\', "/"))
            .collect();
        let mut matched: Vec<&GraphNode> = Vec::new();
        for node in nodes {
            let Some(path) = node.path.as_deref() else { continue };
            let normalized_path = path.replace('\\', "/");
            if normalized.iter().any(|a| a == &normalized_path) {
                matched.push(node);
            }
        }
        matched.sort_by(|a, b| a.id.cmp(&b.id));
        if let Some(result) = decide_lane("source_address", addresses.len(), matched, 1, false, max_seeds, &mut attempts) {
            return result;
        }
    }

    let allow_many_task_lanes = allow_ambiguous && normalized_anchors.is_empty();

    let mut qualified: Vec<&GraphNode> = Vec::new();
    for term in &terms {
        for node in nodes {
            if node.evidence.iter().any(|e| e.get("qualifiedName").and_then(Value::as_str) == Some(term.as_str())) {
                qualified.push(node);
            }
        }
    }
    qualified.sort_by(|a, b| a.id.cmp(&b.id));
    if let Some(result) = decide_lane("qualified_symbol", terms.len(), qualified, 2, allow_many_task_lanes, max_seeds, &mut attempts) {
        return result;
    }

    let mut exact: Vec<&GraphNode> = Vec::new();
    for term in &terms {
        for node in nodes {
            if node.name.as_deref() == Some(term.as_str()) {
                exact.push(node);
            }
        }
    }
    exact.sort_by(|a, b| a.id.cmp(&b.id));
    if let Some(result) = decide_lane("exact_term", terms.len(), exact, 3, allow_many_task_lanes, max_seeds, &mut attempts) {
        return result;
    }

    let mut lexical: Vec<&GraphNode> = Vec::new();
    for term in &terms {
        for node in nodes {
            let name_hit = node.name.as_deref().is_some_and(|n| n.to_ascii_lowercase().contains(term.as_str()));
            let path_hit = node.path.as_deref().is_some_and(|p| p.to_ascii_lowercase().contains(term.as_str()));
            let qn_hit = node.evidence.iter().any(|e| {
                e.get("qualifiedName").and_then(Value::as_str).is_some_and(|qn| qn.to_ascii_lowercase().contains(term.as_str()))
            });
            if name_hit || path_hit || qn_hit {
                lexical.push(node);
            }
        }
    }
    lexical.sort_by(|a, b| a.id.cmp(&b.id));
    if let Some(result) = decide_lane("bounded_lexical", terms.len(), lexical, 4, allow_many_task_lanes, max_seeds, &mut attempts) {
        return result;
    }

    SeedResolution { state: "unresolved", seeds: vec![], json: seed_envelope("unresolved", &[], &attempts, Some("no_relevant_seed")) }
}

/// One bounded multi-seed frontier hydration pass, mirroring
/// `traversalNeighbors(db, {seedIds, generationId, direction, maxDepth,
/// kinds})`. Filters edges to `kinds`, follows `direction`, and returns
/// the full (unbounded) reachable node-id order plus every matching edge
/// seen — bounding to `policy.max_nodes`/`policy.max_edges` is the
/// caller's job (mirrors legacy: `executeRecallCircuit` bounds the
/// frontier's output, `traversalNeighbors` itself does not), so omission
/// accounting can compare the true frontier size against the cap.
pub struct Frontier {
    pub seen_nodes: Vec<String>,
    pub edge_rows: Vec<GraphEdge>,
}

pub fn traversal_neighbors_native(edges: &[GraphEdge], seed_ids: &[String], direction: &str, max_depth: usize, kinds: &[&str]) -> Frontier {
    let kind_set: HashSet<&str> = kinds.iter().copied().collect();
    let mut seen: Vec<String> = Vec::new();
    let mut seen_set: HashSet<String> = HashSet::new();
    for s in seed_ids {
        if seen_set.insert(s.clone()) {
            seen.push(s.clone());
        }
    }
    let mut frontier: Vec<String> = seed_ids.to_vec();
    let mut edge_rows: Vec<GraphEdge> = Vec::new();
    let mut edge_seen: HashSet<String> = HashSet::new();
    for _ in 0..max_depth {
        if frontier.is_empty() {
            break;
        }
        let frontier_set: HashSet<&str> = frontier.iter().map(|s| s.as_str()).collect();
        let mut next: Vec<String> = Vec::new();
        for edge in edges {
            if !kind_set.contains(edge.kind.as_str()) {
                continue;
            }
            let out = direction != "in" && frontier_set.contains(edge.source.as_str());
            let incoming = direction != "out" && edge.target.as_deref().is_some_and(|t| frontier_set.contains(t));
            if !out && !incoming {
                continue;
            }
            if edge_seen.insert(edge.id.clone()) {
                edge_rows.push(edge.clone());
            }
            let candidate_target = if out { edge.target.clone() } else { Some(edge.source.clone()) };
            if let Some(target) = candidate_target {
                if seen_set.insert(target.clone()) {
                    seen.push(target.clone());
                    next.push(target);
                }
            }
        }
        frontier = next;
    }
    Frontier { seen_nodes: seen, edge_rows }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(state: &'static str, authority: i64, tier: &str, exactness: i64, coverage: f64, hops: usize, id: &str) -> AtomicPath {
        AtomicPath {
            id: id.to_string(),
            seed_id: "s".into(),
            terminal_id: Some("t".into()),
            node_ids: vec![],
            edge_ids: vec![],
            minimum_edge_tier: tier.to_string(),
            minimum_semantic_authority: None,
            semantic_authority_rank: authority,
            seed_exactness: exactness,
            evidence_coverage: coverage,
            hop_count: hops,
            state,
            omission_reasons: vec![],
            evidence_envelope: Value::Null,
        }
    }

    #[test]
    fn complete_beats_partial_regardless_of_other_fields() {
        let complete = path("complete", 5, "UNRESOLVED", 5, 0.0, 5, "z");
        let partial = path("partial", 0, "EXACT_RESOLUTION", 0, 1.0, 0, "a");
        assert_eq!(compare_paths(&complete, &partial), Ordering::Less);
    }

    #[test]
    fn semantic_authority_rank_beats_tier_and_coverage() {
        let better_authority = path("complete", 0, "UNRESOLVED", 0, 0.0, 0, "z");
        let worse_authority = path("complete", 1, "EXACT_RESOLUTION", 0, 1.0, 0, "a");
        assert_eq!(compare_paths(&better_authority, &worse_authority), Ordering::Less);
    }

    #[test]
    fn evidence_coverage_is_descending() {
        let higher_coverage = path("complete", 0, "EXACT_RESOLUTION", 0, 0.9, 0, "z");
        let lower_coverage = path("complete", 0, "EXACT_RESOLUTION", 0, 0.1, 0, "a");
        assert_eq!(compare_paths(&higher_coverage, &lower_coverage), Ordering::Less);
    }

    #[test]
    fn id_is_final_tiebreak() {
        let a = path("complete", 0, "EXACT_RESOLUTION", 0, 0.5, 0, "aaa");
        let b = path("complete", 0, "EXACT_RESOLUTION", 0, 0.5, 0, "bbb");
        assert_eq!(compare_paths(&a, &b), Ordering::Less);
    }

    #[test]
    fn full_lexicographic_chain_never_compensates() {
        // A path that is worse on every later field but better on an
        // earlier one must still win — non-compensatory.
        let earlier_field_wins = path("complete", 1, "UNRESOLVED", 99, 0.0, 99, "zzz");
        let later_fields_lose = path("complete", 2, "EXACT_RESOLUTION", 0, 1.0, 0, "aaa");
        assert_eq!(compare_paths(&earlier_field_wins, &later_fields_lose), Ordering::Less);
    }

    #[test]
    fn tier_rank_unknown_and_none_are_worst() {
        assert_eq!(tier_rank(Some("EXACT_RESOLUTION")), 0);
        assert_eq!(tier_rank(Some("UNRESOLVED")), 3);
        assert_eq!(tier_rank(Some("bogus")), 4);
        assert_eq!(tier_rank(None), 4);
    }

    #[test]
    fn adjacency_sorts_by_tier_then_edge_id() {
        let edges = vec![
            json!({"id": "e2", "source": "a", "target": "b", "confidence_tier": "CROSS_FILE_HEURISTIC"}),
            json!({"id": "e1", "source": "a", "target": "c", "confidence_tier": "EXACT_RESOLUTION"}),
            json!({"id": "e3", "source": "a", "target": "d", "confidence_tier": "EXACT_RESOLUTION"}),
        ];
        let adj = adjacency(&edges, "out");
        let entries = &adj["a"];
        assert_eq!(entries[0].edge["id"], json!("e1"));
        assert_eq!(entries[1].edge["id"], json!("e3"));
        assert_eq!(entries[2].edge["id"], json!("e2"));
    }

    #[test]
    fn adjacency_both_direction_adds_reverse_entries() {
        let edges = vec![json!({"id": "e1", "source": "a", "target": "b", "confidence_tier": "EXACT_RESOLUTION"})];
        let adj = adjacency(&edges, "both");
        assert_eq!(adj["a"][0].to, "b");
        assert_eq!(adj["b"][0].to, "a");
    }

    #[test]
    fn no_seeds_abstains_with_typed_omission() {
        let graph = RecallGraph { nodes: HashMap::new(), edges: vec![] };
        let result = execute_recall_circuit(&graph, &[], &TraversalPolicy::default(), "gen1");
        assert_eq!(result.state, "abstained");
        assert_eq!(result.omissions[0]["reason"], json!("no_seeds_resolved"));
        assert!(result.paths.is_empty());
    }

    #[test]
    fn no_paths_found_falls_back_to_single_node_path_per_seed() {
        let mut nodes = HashMap::new();
        nodes.insert("s1".to_string(), json!({"id": "s1"}));
        let graph = RecallGraph { nodes, edges: vec![] };
        let seeds = vec![Seed { id: "s1".into(), exactness: 1, reason: None, evidence: Value::Null }];
        let result = execute_recall_circuit(&graph, &seeds, &TraversalPolicy::default(), "gen1");
        assert_eq!(result.state, "complete");
        assert_eq!(result.paths.len(), 1);
        assert_eq!(result.paths[0].terminal_id.as_deref(), Some("s1"));
        assert_eq!(result.paths[0].hop_count, 0);
    }
}
