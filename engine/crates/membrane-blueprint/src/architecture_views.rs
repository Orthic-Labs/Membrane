//! Lane WIRE1: native `Operation::Architecture` `view` dispatch.
//!
//! Ports the legacy `view` switch in `service.mjs`'s `architecture()`
//! handler (~L730-791: `default`/`flows`/`liveness`/`processes`/
//! `contracts`/`signatures`/`orientation`/`projection`/`changes`) onto the
//! already-loaded native [`GraphGeneration`]. Cache-backed projections use
//! the projection-dependency DAG
//! (`crate::dependency_dag::ProjectionCache`) so repeat calls against an
//! unchanged generation retain legacy hit/miss/invalidated accounting.
//!
//! `changes` opens canonical read-only store from generation root so history
//! references remain backed by same persisted source as standalone changes.

use crate::api::{BlueprintError, BlueprintRequest};
use crate::architecture_model::{self, ArchEdge, ArchNode};
use crate::contract_registry;
use crate::dependency_dag::{self, ParentValues, ProjectionCache};
use crate::graph::GraphGeneration;
use crate::liveness::{self, LivenessOptions};
use crate::orientation_projection::{self, OrientationOptions};
use crate::process_projection::{self, ProcessProjectionOptions};
use crate::signature_projection::{self, SignatureProjectionOptions};
use crate::entry_points;
use crate::store::Generation;
use sha2::{Digest, Sha256};
use serde_json::{json, Map, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;

/// Convert an already-loaded [`GraphGeneration`] into the `Value`-shaped
/// [`Generation`] the ported projection modules operate on. Deliberately
/// local (not `crate::engine::to_store_generation`, which is private and
/// owned by a concurrently-edited file): carries only the fields the
/// projections here actually read.
fn to_projection_generation(generation: &GraphGeneration) -> Generation {
    Generation {
        manifest: Some(json!({
            "generationId": generation.generation_id,
            "sourceHash": generation.source_hash,
            "complete": generation.complete,
            "truncated": !generation.truncation_reasons.is_empty(),
            "truncationReasons": generation.truncation_reasons,
        })),
        nodes: generation.nodes.iter().filter_map(|n| serde_json::to_value(n).ok()).collect(),
        edges: generation.edges.iter().filter_map(|e| serde_json::to_value(e).ok()).collect(),
        ..Default::default()
    }
}

fn i64_input(request: &BlueprintRequest, key: &str) -> Option<i64> {
    request.input.get(key).and_then(Value::as_i64)
}
fn str_input(request: &BlueprintRequest, key: &str) -> Option<String> {
    request.input.get(key).and_then(Value::as_str).map(str::to_owned)
}
fn str_vec_input(request: &BlueprintRequest, key: &str) -> Option<Vec<String>> {
    request.input.get(key).and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
}

fn envelope(generation_id: &str, view: &str, mut body: Map<String, Value>, cache: Option<&'static str>) -> Value {
    body.insert("schemaVersion".into(), json!(1));
    body.insert("generationId".into(), json!(generation_id));
    body.insert("state".into(), json!("complete"));
    body.insert("view".into(), json!(view));
    if let Some(cache) = cache {
        body.insert("cache".into(), json!(cache));
    }
    Value::Object(body)
}

fn not_wired(view: &str) -> Result<Value, BlueprintError> {
    Err(BlueprintError::new(
        "architecture_view_not_wired",
        format!("architecture view '{view}' requires a persisted native projection source"),
    ))
}

// Per-execution-thread cache of per-repo `ProjectionCache<Value>` instances.
// The legacy cache belongs to one application-service instance; keeping
// native entries thread-local prevents unrelated query/service instances
// from leaking a prior generation's hit into a fresh request.
thread_local! {
    static CACHE_REGISTRY: RefCell<HashMap<String, ProjectionCache<Value>>> = RefCell::new(HashMap::new());
}

fn cached<F>(generation: &GraphGeneration, projection: &str, builder: F) -> Result<(Value, &'static str), BlueprintError>
where
    F: FnOnce() -> Value,
{
    let dag = dependency_dag::build_projection_dependency_dag(&ParentValues {
        source_hash: Some(generation.source_hash.clone()),
        provider_digest: Some(format!("{}:{}", generation.provider, generation.provider_version)),
        schema_version: Some(generation.schema_version.to_string()),
        generation_id: Some(generation.generation_id.clone()),
        ..Default::default()
    });
    CACHE_REGISTRY.with(|registry| {
        let mut guard = registry.borrow_mut();
        let entry = guard.entry(generation.repo_root.clone()).or_insert_with(|| ProjectionCache::new(16));
        let (value, outcome, _fingerprint) = entry
            .get_or_build(projection, &dag, builder)
            .map_err(|e| BlueprintError::new("architecture_view_invalid", e.to_string()))?;
        let cache = match outcome {
            dependency_dag::CacheOutcome::Hit => "hit",
            dependency_dag::CacheOutcome::Miss => "miss",
            dependency_dag::CacheOutcome::Invalidated => "invalidated",
        };
        Ok((value, cache))
    })
}

fn arch_node_edge_from(generation: &Generation) -> (Vec<ArchNode>, Vec<ArchEdge>) {
    let nodes = generation.nodes.iter().filter_map(|v| serde_json::from_value::<ArchNode>(v.clone()).ok()).collect();
    let edges = generation.edges.iter().filter_map(|v| serde_json::from_value::<ArchEdge>(v.clone()).ok()).collect();
    (nodes, edges)
}

fn canonical_evidence(node: &Value) -> Vec<Value> {
    node.get("evidence").and_then(Value::as_array).map(|items| items.iter().map(|item| json!({
        "path": item.get("path").cloned().unwrap_or(Value::Null),
        "startLine": item.get("startLine").cloned().unwrap_or(Value::Null),
        "endLine": item.get("endLine").cloned().unwrap_or(Value::Null),
        "contentHash": item.get("contentHash").cloned().unwrap_or(Value::Null),
    })).collect()).unwrap_or_default()
}

fn compact_node(node: &Value) -> Value {
    let evidence = node.get("evidence").and_then(Value::as_array).and_then(|rows| rows.first());
    json!({
        "id": node.get("id").cloned().unwrap_or(Value::Null),
        "kind": node.get("kind").cloned().unwrap_or(Value::Null),
        "path": node.get("path").cloned().filter(|value| !value.is_null()).or_else(|| evidence.and_then(|row| row.get("path")).cloned()).unwrap_or(Value::Null),
        "startLine": evidence.and_then(|row| row.get("startLine")).cloned().unwrap_or(Value::Null),
        "endLine": evidence.and_then(|row| row.get("endLine")).cloned().unwrap_or(Value::Null),
    })
}

fn flow_id(path: &[String]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(path.join("\u{0}").as_bytes());
    format!("flow:sha256:{}", hex::encode(hasher.finalize()))
}

fn flow_cursor(generation_id: &str, offset: usize) -> String {
    // Match the legacy Buffer(JSON.stringify(...)).toString("base64url")
    // cursor contract without adding a runtime dependency.
    let payload = json!({ "view": "flows", "generationId": generation_id, "offset": offset });
    base64url_encode(serde_json::to_string(&payload).unwrap_or_default().as_bytes())
}

fn decode_flow_cursor(cursor: Option<String>, generation_id: &str) -> Result<usize, BlueprintError> {
    let Some(cursor) = cursor else { return Ok(0) };
    let Some(bytes) = base64url_decode(&cursor) else {
        return Err(BlueprintError::new("cursor_invalid", "Architecture flow cursor is malformed."));
    };
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| BlueprintError::new("cursor_invalid", "Architecture flow cursor is malformed."))?;
    let valid = value.get("view").and_then(Value::as_str) == Some("flows")
        && value.get("generationId").and_then(Value::as_str) == Some(generation_id)
        && value.get("offset").and_then(Value::as_u64).is_some_and(|offset| offset <= 10_000);
    if !valid {
        return Err(BlueprintError::new("cursor_invalid", "Architecture flow cursor does not match the served generation."));
    }
    Ok(value.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize)
}

fn changes_view(generation: &GraphGeneration, request: &BlueprintRequest) -> Result<Value, BlueprintError> {
    let db_path = Path::new(&generation.repo_root).join(".agent").join("graph").join("graph.db");
    if !db_path.exists() {
        // In-memory GraphGeneration callers have no persisted history; keep
        // that boundary explicit instead of fabricating a current-only delta.
        return not_wired("changes");
    }
    let connection = crate::store::open_store_read_only(&db_path)
        .map_err(|error| BlueprintError::new("architecture_changes_store", error.to_string()))?;
    let snapshot = str_input(request, "snapshot");
    let since_generation = str_input(request, "sinceGeneration");
    let head = str_input(request, "head").unwrap_or_else(|| "HEAD".to_string());
    let treeish = request.input.get("treeish").and_then(|value| match value {
        Value::String(base) => Some((base.clone(), head.clone())),
        Value::Object(map) => {
            let base = map.get("base").or_else(|| map.get("from")).and_then(Value::as_str)?;
            let treeish_head = map.get("head").or_else(|| map.get("to")).and_then(Value::as_str).unwrap_or(&head);
            Some((base.to_string(), treeish_head.to_string()))
        }
        _ => None,
    });
    crate::lib_application_snapshots::changes_since_reference(
        &connection,
        Path::new(&generation.repo_root),
        snapshot.as_deref(),
        since_generation.as_deref(),
        treeish.as_ref().map(|(base, treeish_head)| (base.as_str(), treeish_head.as_str())),
        request.input.get("limit").and_then(Value::as_u64).unwrap_or(100),
    )
}

const BASE64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn base64url_encode(bytes: &[u8]) -> String {
    let mut output = String::with_capacity((bytes.len() * 4 + 2) / 3);
    let mut index = 0;
    while index < bytes.len() {
        let first = bytes[index];
        let second = bytes.get(index + 1).copied();
        let third = bytes.get(index + 2).copied();
        output.push(BASE64URL[(first >> 2) as usize] as char);
        output.push(BASE64URL[((first & 0x03) << 4 | second.unwrap_or(0) >> 4) as usize] as char);
        if let Some(second) = second {
            output.push(BASE64URL[((second & 0x0f) << 2 | third.unwrap_or(0) >> 6) as usize] as char);
        }
        if let Some(third) = third {
            output.push(BASE64URL[(third & 0x3f) as usize] as char);
        }
        index += 3;
    }
    output
}

fn base64url_decode(input: &str) -> Option<Vec<u8>> {
    if input.len() % 4 == 1 || input.bytes().any(|byte| !BASE64URL.contains(&byte)) {
        return None;
    }
    let mut output = Vec::with_capacity(input.len() * 3 / 4);
    let mut buffer = 0u32;
    let mut bits = 0u8;
    for byte in input.bytes() {
        let value = BASE64URL.iter().position(|candidate| *candidate == byte)? as u32;
        buffer = (buffer << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    Some(output)
}

fn flow_page(generation: &GraphGeneration, request: &BlueprintRequest) -> Result<Value, BlueprintError> {
    let requested = i64_input(request, "maxFlows").or_else(|| i64_input(request, "limit")).unwrap_or(50);
    if !(1..=200).contains(&requested) {
        return Err(BlueprintError::new("architecture_bounds_invalid", "Architecture flow maxFlows must be an integer from 1 to 200."));
    }
    let max_flows = requested as usize;
    let offset = decode_flow_cursor(str_input(request, "cursor"), &generation.generation_id)?;
    let projection_generation = to_projection_generation(generation);
    let entries = entry_points::build_entry_point_registry(&projection_generation, true);
    let by_id: std::collections::HashMap<&str, &Value> = projection_generation.nodes.iter()
        .filter_map(|node| node.get("id").and_then(Value::as_str).map(|id| (id, node))).collect();
    let mut outgoing: std::collections::HashMap<&str, Vec<&Value>> = std::collections::HashMap::new();
    for edge in &projection_generation.edges {
        if edge.get("target").and_then(Value::as_str).filter(|target| !target.is_empty()).is_some() {
            if let Some(source) = edge.get("source").and_then(Value::as_str) { outgoing.entry(source).or_default().push(edge); }
        }
    }
    for edges in outgoing.values_mut() {
        edges.sort_by(|left, right| left.get("target").and_then(Value::as_str).unwrap_or("").cmp(right.get("target").and_then(Value::as_str).unwrap_or(""))
            .then_with(|| left.get("id").and_then(Value::as_str).unwrap_or("").cmp(right.get("id").and_then(Value::as_str).unwrap_or(""))));
    }
    let mut rows: Vec<Value> = Vec::new();
    let collection_limit = max_flows + offset + 1;
    for entry in &entries {
        if rows.len() >= collection_limit { break; }
        let root = entry.get("id").and_then(Value::as_str).unwrap_or("").to_string();
        let mut stack: Vec<Vec<String>> = vec![vec![root.clone()]];
        let mut found = false;
        while let Some(path) = stack.pop() {
            if rows.len() >= collection_limit { break; }
            let current = path.last().map(String::as_str).unwrap_or("");
            let edges = outgoing.get(current).cloned().unwrap_or_default();
            if edges.is_empty() && path.len() > 1 {
                found = true;
                let nodes: Vec<Value> = path.iter().filter_map(|id| by_id.get(id.as_str()).map(|node| compact_node(node))).collect();
                let evidence: Vec<Value> = path.iter().filter_map(|id| by_id.get(id.as_str())).flat_map(|node| canonical_evidence(node)).collect();
                rows.push(json!({
                    "id": flow_id(&path), "status": "complete", "entry": nodes.first().cloned().unwrap_or(Value::Null),
                    "terminal": nodes.last().cloned().unwrap_or(Value::Null), "path": nodes, "evidence": evidence,
                }));
                continue;
            }
            if path.len() > 12 { continue; }
            // Push reversed so stack traversal follows canonical target/id order.
            for edge in edges.into_iter().rev() {
                let Some(target) = edge.get("target").and_then(Value::as_str).filter(|value| !value.is_empty()) else { continue };
                if path.iter().any(|id| id == target) { continue; }
                let mut next = path.clone(); next.push(target.to_string()); stack.push(next);
            }
        }
        if !found {
            let entry_node = by_id.get(root.as_str()).copied().map(compact_node).unwrap_or(Value::Null);
            rows.push(json!({ "id": flow_id(std::slice::from_ref(&root)), "status": "broken", "entry": entry_node.clone(), "path": [entry_node], "missingHop": "no terminal reachable", "evidence": by_id.get(root.as_str()).map(|node| canonical_evidence(node)).unwrap_or_default() }));
        }
    }
    rows.sort_by(|left, right| {
        let left_path = left.get("path").and_then(Value::as_array).map(|path| path.iter().filter_map(|node| node.get("id").and_then(Value::as_str)).collect::<Vec<_>>().join("\u{0}")).unwrap_or_default();
        let right_path = right.get("path").and_then(Value::as_array).map(|path| path.iter().filter_map(|node| node.get("id").and_then(Value::as_str)).collect::<Vec<_>>().join("\u{0}")).unwrap_or_default();
        left_path.cmp(&right_path).then_with(|| left.get("id").and_then(Value::as_str).unwrap_or("").cmp(right.get("id").and_then(Value::as_str).unwrap_or("")))
    });
    let truncated = rows.len() > offset + max_flows;
    let flows = rows.into_iter().skip(offset).take(max_flows).collect::<Vec<_>>();
    Ok(json!({
        "schemaVersion": 2, "provider": generation.provider, "kind": "architecture", "view": "flows",
        "generationId": generation.generation_id, "sourceState": if generation.complete { "clean" } else { "stale" }, "dirtyFileCount": 0,
        "ordering": "entry.id,path[].id", "bounds": { "maxFlows": max_flows, "maxDepth": 12 }, "entryPoints": entries.len(),
        "flows": flows, "truncated": truncated, "continuationCursor": if truncated { Some(flow_cursor(&generation.generation_id, offset + max_flows)) } else { None },
    }))
}

/// Dispatch one named `view` for `Operation::Architecture`. Mirrors the
/// legacy `service.mjs` `if (view === "...")` chain.
pub fn dispatch_view(generation: &GraphGeneration, request: &BlueprintRequest, view: &str) -> Result<Value, BlueprintError> {
    let projection_generation = to_projection_generation(generation);
    match view {
        "processes" => {
            let options = ProcessProjectionOptions {
                max_processes: i64_input(request, "maxProcesses"),
                max_depth: i64_input(request, "maxDepth"),
                max_steps: i64_input(request, "maxSteps"),
            };
            let (value, cache) = cached(generation, "processes", || process_projection::build_process_projection(&projection_generation, &options))?;
            let mut body = value.as_object().cloned().unwrap_or_default();
            body.remove("schemaVersion");
            body.remove("generationId");
            Ok(envelope(&generation.generation_id, view, body, Some(cache)))
        }
        "signatures" => {
            let options = SignatureProjectionOptions {
                limit: i64_input(request, "limit"),
                path_prefix: str_input(request, "pathPrefix"),
                kinds: str_vec_input(request, "kinds"),
            };
            let (value, cache) = cached(generation, "signatures", || signature_projection::project_symbol_signatures(&projection_generation, &options))?;
            let mut body = value.as_object().cloned().unwrap_or_default();
            body.remove("schemaVersion");
            body.remove("generationId");
            Ok(envelope(&generation.generation_id, view, body, Some(cache)))
        }
        "orientation" => {
            let files: Vec<String> = projection_generation
                .nodes
                .iter()
                .filter(|n| n.get("kind").and_then(Value::as_str) == Some("file"))
                .filter_map(|n| n.get("path").and_then(Value::as_str).map(str::to_owned))
                .collect();
            let options = OrientationOptions {
                signature_limit: i64_input(request, "signatureLimit"),
                entry_point_limit: i64_input(request, "entryPointLimit"),
                contract_limit: i64_input(request, "contractLimit"),
            };
            let (value, cache) = cached(generation, "orientation", || orientation_projection::build_cold_start_orientation(&projection_generation, &files, &options))?;
            let mut body = value.as_object().cloned().unwrap_or_default();
            body.remove("schemaVersion");
            body.remove("generationId");
            Ok(envelope(&generation.generation_id, view, body, Some(cache)))
        }
        "contracts" => {
            let repo_id = str_input(request, "repoId");
            let (value, cache) = cached(generation, "contracts", || {
                let contract_generation = orientation_projection::contract_generation_from_store(&projection_generation, repo_id.as_deref());
                let registry = contract_registry::build_contract_registry(&contract_generation, repo_id.as_deref());
                orientation_projection::contract_registry_to_json(&registry)
            })?;
            let mut body = value.as_object().cloned().unwrap_or_default();
            body.remove("schemaVersion");
            body.remove("generationId");
            Ok(envelope(&generation.generation_id, view, body, Some(cache)))
        }
        "liveness" => {
            let options = LivenessOptions {
                max_nodes: i64_input(request, "maxNodes"),
                max_edges: i64_input(request, "maxEdges"),
                max_hops: i64_input(request, "maxHops"),
                source_state: Some(if generation.complete { "clean".to_string() } else { "stale".to_string() }),
            };
            // Not cached in legacy (`liveness` carries no dependency-dag
            // entry), so build directly on every call, matching parity.
            let value = liveness::build_liveness_projection(&projection_generation, &options);
            let mut body = value.as_object().cloned().unwrap_or_default();
            body.remove("schemaVersion");
            body.remove("generationId");
            Ok(envelope(&generation.generation_id, view, body, None))
        }
        "projection" => {
            let (nodes, edges) = arch_node_edge_from(&projection_generation);
            // Match the service's `Number(input.max* ?? default)` values;
            // zero is meaningful to the downstream `slice`/ceiling logic.
            let max_components = i64_input(request, "maxComponents").unwrap_or(100).max(0) as usize;
            let max_flows = i64_input(request, "maxFlows").unwrap_or(200).max(0) as usize;
            let projection = architecture_model::build_disposable_architecture_projection(
                &nodes,
                &edges,
                Some(generation.generation_id.clone()),
                max_components,
                max_flows,
            );
            let value = serde_json::to_value(&projection).map_err(|e| BlueprintError::new("architecture_view_invalid", e.to_string()))?;
            let mut body = value.as_object().cloned().unwrap_or_default();
            body.remove("schemaVersion");
            body.remove("generationId");
            Ok(envelope(&generation.generation_id, view, body, None))
        }
        "flows" => flow_page(generation, request),
        "changes" => changes_view(generation, request),
        other => Err(BlueprintError::new(
            "architecture_view_invalid",
            format!("Architecture view must be summary, flows, liveness, processes, contracts, signatures, orientation, projection, or changes (got '{other}')."),
        )),
    }
}
