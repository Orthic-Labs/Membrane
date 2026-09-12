//! Canonical in-process Blueprint operation.
//!
//! This is the single owner of repository graph construction, publication,
//! status, and generation-bound query dispatch.  It deliberately has no
//! resident-service or process dependency.

use crate::api::{BlueprintError, BlueprintOperation, BlueprintRequest, RequestContext};
use crate::graph::{self, GraphGeneration, GraphOptions};
use crate::model::Operation;
use crate::query;
use crate::security::{canonical_root, is_confined_path};
use crate::store::{self, Generation, StoreError};
use crate::delta_store::{self, ApplyOptions, EventKind, FactBatch, FileDelta};
use crate::freshness::{stable_read_with_limit, StableReadError, MAX_SOURCE_FILE_BYTES};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Native graph/store operation used by one-shot callers and resident hosts.
#[derive(Debug, Default)]
pub struct NativeBlueprintOperation;

/// Return the canonical Blueprint operation.  The returned trait object is
/// intentionally stateless; all durable state lives at the request root.
pub fn native_blueprint_operation() -> Arc<dyn BlueprintOperation> {
    Arc::new(NativeBlueprintOperation)
}

impl BlueprintOperation for NativeBlueprintOperation {
    fn execute(&self, request: &BlueprintRequest, context: &RequestContext) -> Result<Value, BlueprintError> {
        context.check()?;
        let root = confined_root(request, context)?;
        check_paths(request, &root)?;
        let db_path = store_path(&root);
        match request.method {
            Operation::Init => crate::lib_operations_init::execute_init(request, &root),
            Operation::Update => crate::lib_operations_update::execute_update(request, &root),
            Operation::Doctor => doctor(request, &root),
            Operation::Repair => repair(request, &root),
            Operation::Build | Operation::Refresh => {
                if request.generation.is_some() || request.input.get("generation").and_then(Value::as_str).is_some() {
                    let current = load_current(&db_path)?;
                    ensure_generation(request, &current)?;
                }
                match verified_construction_reason(&db_path)? {
                    Some(verified) => build_and_publish(request, context, &root, &db_path, verified),
                None => incremental_refresh_with_repair(request, context, &root, &db_path),
                }
            }
            Operation::Status | Operation::DbStatus => status(request, context, &root, &db_path)
                .and_then(|value| bounded_generation_response(request, value)),
            Operation::FindingsGet
            | Operation::FindingsExplain
            | Operation::FindingsEvidencePack
            | Operation::FindingsBaselineCapture
            | Operation::FindingsBaselineList
            | Operation::FindingsSarif => {
                // Findings are a projection over the same persisted native
                // generation used by query operations. Keep this dispatch in
                // the native owner so installed CLI callers cannot fall back
                // to a second JS detector or an unpinned live scan.
                let connection = store::open_store_read_only(&db_path).map_err(store_error)?;
                let generation = store::load_generation(&connection)
                    .map_err(store_error)?
                    .ok_or_else(|| BlueprintError::new("blueprint_store_missing", "no persisted Blueprint generation exists"))?;
                let state_dir = root.join(".agent").join("blueprint").join("findings-baselines");
                crate::findings::execute_findings(&generation, request, context, &state_dir)
            }
            Operation::DocumentTruth => {
                // Ported from blueprint/src/lib/application/service.mjs
                // `documentTruth` (lane LIB5). Claims/edges/supersession are
                // relational store rows the GraphGeneration query path does
                // not carry, so this reads store::Generation directly, the
                // same pattern FindingsGet uses above.
                let connection = store::open_store_read_only(&db_path).map_err(store_error)?;
                let generation = store::load_generation(&connection)
                    .map_err(store_error)?
                    .ok_or_else(|| BlueprintError::new("blueprint_store_missing", "no persisted Blueprint generation exists"))?;
                crate::lib_application_document_truth::execute_document_truth(&generation, request)
            }
            Operation::SnapshotGet => {
                let connection = store::open_store_read_only(&db_path).map_err(store_error)?;
                let name = request.input.get("snapshot").or_else(|| request.input.get("node")).and_then(Value::as_str)
                    .ok_or_else(|| BlueprintError::missing("input.snapshot"))?;
                let (generation_id, snapshot) = {
                    let generation = store::load_generation(&connection).map_err(store_error)?
                        .ok_or_else(|| BlueprintError::new("blueprint_store_missing", "no persisted Blueprint generation exists"))?;
                    (generation.generation_id().unwrap_or("").to_owned(), crate::lib_application_snapshots::get_snapshot(&connection, name)?)
                };
                Ok(json!({"schemaVersion": 1, "generationId": generation_id, "snapshot": snapshot}))
            }
            Operation::SnapshotList => {
                let connection = store::open_store_read_only(&db_path).map_err(store_error)?;
                let generation = store::load_generation(&connection).map_err(store_error)?
                    .ok_or_else(|| BlueprintError::new("blueprint_store_missing", "no persisted Blueprint generation exists"))?;
                let generation_id = generation.generation_id().unwrap_or("").to_owned();
                let snapshots = crate::lib_application_snapshots::list_snapshots(&connection)?;
                Ok(json!({"schemaVersion": 1, "generationId": generation_id, "snapshots": snapshots}))
            }
            Operation::Changes => {
                let connection = store::open_store_read_only(&db_path).map_err(store_error)?;
                let generation = store::load_generation(&connection).map_err(store_error)?
                    .ok_or_else(|| BlueprintError::new("blueprint_store_missing", "no persisted Blueprint generation exists"))?;
                let generation_id = generation.generation_id().unwrap_or("").to_owned();
                let limit = request.input.get("limit").and_then(Value::as_u64).unwrap_or(100);
                let snapshot = request.input.get("snapshot").and_then(Value::as_str);
                let since_generation = request.input.get("sinceGeneration").and_then(Value::as_str);
                let treeish = request.input.get("treeish");
                let head = request.input.get("head").and_then(Value::as_str).unwrap_or("HEAD");
                let treeish_pair = match treeish {
                    Some(Value::String(base)) => Some((base.as_str(), head)),
                    Some(Value::Object(map)) => {
                        let base = map.get("base").or_else(|| map.get("from")).and_then(Value::as_str);
                        let treeish_head = map.get("head").or_else(|| map.get("to")).and_then(Value::as_str).unwrap_or(head);
                        base.map(|b| (b, treeish_head))
                    }
                    _ => None,
                };
                let mut result = crate::lib_application_snapshots::changes_since_reference(
                    &connection, &root, snapshot, since_generation, treeish_pair, limit,
                )?;
                if let Value::Object(object) = &mut result {
                    object.insert("generationId".into(), json!(generation_id));
                }
                Ok(result)
            }
            Operation::Federate => crate::lib_application_federate::execute_federate(request, &context.cancellation),
            Operation::Architecture if request.input.get("view").and_then(Value::as_str) == Some("changes") => {
                architecture_changes(request, context, &root, &db_path)
            }
            Operation::Search | Operation::Resolve | Operation::Recall | Operation::Expand
            | Operation::Impact | Operation::Path | Operation::Architecture => {
                let (generation, source_observation) = load_current_with_observation(&db_path)?;
                ensure_generation(request, &generation)?;
                context.check()?;
                let mut result = query::execute_query(&generation, request, context)?;
                if let Some(observation) = source_observation {
                    if let Value::Object(object) = &mut result {
                        object.insert("sourceObservation".into(), observation);
                    }
                }
                let indexed_paths = generation.files.iter().map(|file| file.path.as_str()).collect::<HashSet<_>>();
                let receipt = freshness_receipt(&root, &generation.generation_id, result.get("sourceObservation"), Some(&indexed_paths));
                if let Value::Object(object) = &mut result { object.insert("freshnessReceipt".into(), receipt); }
                context.check()?;
                Ok(result)
            }
            operation => Err(BlueprintError::new(
                "unsupported_operation",
                format!("native Blueprint operation {} is unsupported", operation.as_str()),
            )),
        }
    }
}

fn confined_root(request: &BlueprintRequest, context: &RequestContext) -> Result<PathBuf, BlueprintError> {
    let requested = request
        .input
        .get("repoRoot")
        .and_then(Value::as_str)
        .ok_or_else(|| BlueprintError::missing("input.repoRoot"))?;
    let scoped = canonical_root(&context.scope.repo_root).map_err(|_| BlueprintError::new("root_escape", "repository root is unavailable or escapes its scope"))?;
    let requested_root = canonical_root(Path::new(requested)).map_err(|_| BlueprintError::new("root_escape", "repository root is unavailable or escapes its scope"))?;
    if scoped != requested_root {
        return Err(BlueprintError::new("root_escape", "request repository root does not match canonical request scope"));
    }
    Ok(scoped)
}

fn check_paths(request: &BlueprintRequest, root: &Path) -> Result<(), BlueprintError> {
    if let Some(paths) = request.input.get("paths").and_then(Value::as_array) {
        for raw in paths.iter().filter_map(Value::as_str) {
            let path = Path::new(raw);
            let candidate = if path.is_absolute() { path.to_path_buf() } else { root.join(path) };
            is_confined_path(root, &candidate, true)
                .map_err(|_| BlueprintError::new("root_escape", "request path escapes canonical repository root"))?;
        }
    }
    Ok(())
}

fn store_path(root: &Path) -> PathBuf { root.join(".agent").join("graph").join("graph.db") }

/// Apply one watcher-shaped code-file refresh in place. Unsupported work is
/// reported without mutating or reconstructing the valid generation.
/// Apply a bounded source event set by reparsing only changed files and their
/// dependency-DAG reference closure. Untouched source rows remain addressed
/// by their existing content hashes & are never sent through providers.
fn incremental_refresh_with_repair(
    request: &BlueprintRequest,
    context: &RequestContext,
    root: &Path,
    db_path: &Path,
) -> Result<Value, BlueprintError> {
    let pending = request.input.get("paths").and_then(Value::as_array).cloned().unwrap_or_default();
    let unsupported = |reason: &str| {
        let mut error = BlueprintError::new("blueprint_incremental_unsupported", reason);
        error.details = Some(json!({"preservedGeneration": true, "pendingChanges": pending.clone()}));
        error
    };
    let (current, _) = load_current_with_observation(db_path)?;
    if !current.complete { return Err(unsupported("existing generation is incomplete")); }
    context.check()?;
    // Watcher and explicit callers already identify the changed paths. Keep
    // this path-scoped branch ahead of discovery so a known event never walks
    // the repository just to rediscover the event it supplied.
    if !pending.is_empty() {
        return incremental_known_paths(request, context, root, db_path, current, pending);
    }
    let scan = graph::scan_repository_with_cancellation(root, &graph::ScanOptions::default(), &context.cancellation)
        .map_err(|error| match error {
            graph::GraphError::Cancelled if context.cancellation.deadline_expired() => BlueprintError::deadline(),
            graph::GraphError::Cancelled => BlueprintError::cancelled(),
            error => BlueprintError::new("blueprint_refresh_scan_failed", error.to_string()),
        })?;
    if scan.traversal_truncated || scan.file_limit_reached { return Err(unsupported("change discovery is incomplete")); }
    let changed = source_path_delta(&current, &scan.files).into_iter().collect::<std::collections::BTreeSet<_>>();
    let event_name = request.input.get("eventKind").and_then(Value::as_str).unwrap_or("modify").to_ascii_lowercase();
    let event_kind = match event_name.as_str() {
        "create" => EventKind::Create,
        "modify" | "changed" => EventKind::Modify,
        "delete" => EventKind::Delete,
        "repair" => EventKind::Repair,
        "rename" => EventKind::Rename,
        _ => return Err(unsupported("event kind lacks incremental implementation")),
    };
    let mut paths = pending.iter().filter_map(Value::as_str).map(graph::normalize_path).collect::<Vec<_>>();
    if paths.iter().any(|path| path.is_empty() || path.starts_with('/') || path.contains("..") || path.starts_with(".agent/")) {
        return Err(unsupported("pending path is outside incremental scope"));
    }
    paths.dedup();
    if event_kind != EventKind::Rename { paths.sort(); }
    if paths.is_empty() {
        if !changed.is_empty() {
            if request.method == Operation::Build {
                paths = changed.iter().cloned().collect();
            } else {
                let mut error = unsupported("source changes require an explicit event set");
                error.details = Some(json!({"preservedGeneration": true, "pendingChanges": pending, "changedPaths": changed}));
                return Err(error);
            }
        } else {
            let (generation, source_observation) = load_current_with_observation(db_path)?;
            return bounded_generation_response(request, json!({
                "schemaVersion":1,"operation":request.method.as_str(),"state":"fresh","refreshMode":"incremental_noop",
                "generationId":generation.generation_id,"repoRoot":root.to_string_lossy(),"storePath":db_path.to_string_lossy(),
                "sourceHash":generation.source_hash,"complete":generation.complete,"truncationReasons":generation.truncation_reasons,
                "counts":{"nodes":generation.nodes.len(),"edges":generation.edges.len(),"files":generation.files.len()},
                "sourceObservation":source_observation.unwrap_or(Value::Null)
            }));
        }
    }
    let rename_to = if event_kind == EventKind::Rename {
        if paths.len() != 2 { return Err(unsupported("rename requires old & new paths")); }
        Some(paths[1].clone())
    } else { None };
    if event_kind != EventKind::Repair {
        let expected = if event_kind == EventKind::Rename { paths.iter().cloned().collect::<std::collections::BTreeSet<_>>() } else { paths.iter().cloned().collect() };
        if changed != expected { return Err(unsupported("pending change set does not match observed repository changes")); }
    } else if paths.iter().any(|path| !changed.contains(path) && !scan.files.iter().any(|file| file.path == *path)) {
        return Err(unsupported("repair path is absent from observed repository"));
    }
    let source_clock = request.input.get("sourceClock").and_then(Value::as_u64).and_then(|clock| i64::try_from(clock).ok()).unwrap_or(0);
    let config_digest = crate::static_provider::build_config_digest_for_files(&scan.files);
    let observation = crate::git_source_observation::git_source_observation(&root.to_string_lossy());
    let source_hash = graph::source_hash_for_files(&scan.files);
    let before_paths = current.nodes.iter().filter_map(|node| (node.kind == "file").then(|| node.path.clone()).flatten()).collect::<HashSet<_>>();
    let after_paths = scan.files.iter().map(|file| file.path.clone()).collect::<HashSet<_>>();
    let mut affected = std::collections::BTreeSet::new();
    for path in &paths { affected.extend(affected_reference_closure(&current, path)); affected.insert(path.clone()); }
    // Resolver configuration is itself content addressed. Existing explicit
    // config edges are included above; the config file still gets its own
    // replacement row so equivalent digests are sealed without reparsing all
    // consumers.
    let mut ordered = affected.into_iter().collect::<Vec<_>>();
    ordered.sort();
    let mut target_events = HashMap::new();
    for target in &ordered {
        let target_file = scan.files.iter().find(|file| file.path == *target);
        let is_root_rename = rename_to.as_deref() == Some(target.as_str());
        let target_event = if event_kind == EventKind::Rename && is_root_rename { EventKind::Create }
            else if !after_paths.contains(target) && before_paths.contains(target) { EventKind::Delete }
            else if !before_paths.contains(target) { EventKind::Create }
            else if changed.contains(target) { event_kind } else { EventKind::Repair };
        if target_event != EventKind::Delete && target_file.is_none() { return Err(unsupported(&format!("source facts unavailable for {target}"))); }
        target_events.insert(target.clone(), target_event);
    }

    // Parse the initial affected set once.  Newly introduced symbols can make
    // prior unresolved CALLS edges resolvable; include those callers before
    // the final post-change resolution pass.
    let mut facts_by_path = HashMap::new();
    for target in &ordered {
        context.check()?;
        if target_events[target] == EventKind::Delete { continue; }
        let facts = graph::build_file_facts_from_scan(root, target, &scan.files, &current.nodes, &context.cancellation)
            .map_err(|error| match error {
                graph::GraphError::Cancelled if context.cancellation.deadline_expired() => BlueprintError::deadline(),
                graph::GraphError::Cancelled => BlueprintError::cancelled(),
                error => BlueprintError::new("blueprint_incremental_failed", error.to_string()),
            })?
            .ok_or_else(|| unsupported(&format!("source facts unavailable for {target}")))?;
        facts_by_path.insert(target.clone(), facts);
    }
    let unresolved_dependents = unresolved_reference_dependents(&current, facts_by_path.values(), &scan.files);
    for dependent in unresolved_dependents {
        if target_events.contains_key(&dependent) { continue; }
        if !scan.files.iter().any(|file| file.path == dependent) { continue; }
        target_events.insert(dependent.clone(), EventKind::Repair);
        ordered.push(dependent.clone());
        context.check()?;
        let facts = graph::build_file_facts_from_scan(root, &dependent, &scan.files, &current.nodes, &context.cancellation)
            .map_err(|error| match error {
                graph::GraphError::Cancelled if context.cancellation.deadline_expired() => BlueprintError::deadline(),
                graph::GraphError::Cancelled => BlueprintError::cancelled(),
                error => BlueprintError::new("blueprint_incremental_failed", error.to_string()),
            })?
            .ok_or_else(|| unsupported(&format!("source facts unavailable for {dependent}")))?;
        facts_by_path.insert(dependent, facts);
    }
    ordered.sort();

    // Replace old symbols for every affected path, then resolve all rebuilt
    // edges against the resulting symbol set. This repairs callers that were
    // unresolved before a changed file introduced their target.
    let mut post_change_nodes = current.nodes.iter()
        .filter(|node| node.path.as_deref().is_none_or(|path| !target_events.contains_key(path)))
        .cloned().collect::<Vec<_>>();
    for facts in facts_by_path.values() {
        post_change_nodes.push(facts.file.clone());
        post_change_nodes.extend(facts.nodes.iter().cloned());
    }
    for facts in facts_by_path.values_mut() {
        graph::resolve_file_facts_edges(facts, &post_change_nodes, &scan.files);
    }

    let mut deltas = Vec::with_capacity(ordered.len());
    for target in &ordered {
        context.check()?;
        let target_file = scan.files.iter().find(|file| file.path == *target);
        let target_event = target_events[target];
        let facts = facts_by_path.remove(target);
        let provider_batches = if target_event == EventKind::Delete { Vec::new() } else {
            provider_batches_for_path(root, target, &scan.files)?
        };
        let delta = file_delta_from_graph(target, target_event, facts, target_file, provider_batches, source_clock, &source_hash, config_digest.clone(), observation.clone(), request)?;
        deltas.push(delta);
    }
    context.check()?;
    let mut connection = open_store(db_path)?;
    delta_store::apply_file_deltas(&mut connection, &deltas, ApplyOptions::default())
        .map_err(|error| BlueprintError::new("blueprint_delta_failed", error.to_string()))?;
    let applied = ordered.clone();
    // Rename is represented by an old-path delete followed by destination
    // creation. Both deltas carry the same source identity & final publication
    // therefore remains equivalent to one atomic source event set.
    if event_kind == EventKind::Rename {
        let old = &paths[0];
        if after_paths.contains(old) || !before_paths.contains(old) { return Err(unsupported("rename source/destination is inconsistent")); }
    }
    let (generation, source_observation) = load_current_with_observation(db_path)?;
    bounded_generation_response(request, json!({
        "schemaVersion": 1, "operation": request.method.as_str(), "state": "fresh", "refreshMode": "incremental",
        "generationId": generation.generation_id, "repoRoot": root.to_string_lossy(), "storePath": db_path.to_string_lossy(),
        "sourceHash": generation.source_hash, "complete": generation.complete, "truncationReasons": generation.truncation_reasons,
        "counts": {"nodes": generation.nodes.len(), "edges": generation.edges.len(), "files": generation.files.len()},
        "sourceClock": request.input.get("sourceClock").cloned().unwrap_or(Value::Null), "eventKind": request.input.get("eventKind").cloned().unwrap_or(Value::Null),
        "invalidatedPaths": applied, "reusedFiles": scan.files.len().saturating_sub(ordered.len()), "sourceObservation": source_observation.unwrap_or(Value::Null),
    }))
}

/// Refresh an explicit event set without discovering any other filesystem
/// paths. Existing graph facts provide resolver path identity; only supplied
/// files are stable-read and hashed.
fn incremental_known_paths(
    request: &BlueprintRequest,
    context: &RequestContext,
    root: &Path,
    db_path: &Path,
    current: GraphGeneration,
    pending: Vec<Value>,
) -> Result<Value, BlueprintError> {
    let unsupported = |reason: &str| {
        let mut error = BlueprintError::new("blueprint_incremental_unsupported", reason);
        error.details = Some(json!({"preservedGeneration": true, "pendingChanges": pending.clone()}));
        error
    };
    let event_name = request.input.get("eventKind").and_then(Value::as_str).unwrap_or("modify").to_ascii_lowercase();
    let event_kind = match event_name.as_str() {
        "create" => EventKind::Create,
        "modify" | "changed" => EventKind::Modify,
        "delete" => EventKind::Delete,
        "repair" => EventKind::Repair,
        "rename" => EventKind::Rename,
        _ => return Err(unsupported("event kind lacks incremental implementation")),
    };
    let mut paths = pending.iter().filter_map(Value::as_str).map(graph::normalize_path).collect::<Vec<_>>();
    if paths.iter().any(|path| path.is_empty() || path.starts_with('/') || path.contains("..") || path.starts_with(".agent/")) {
        return Err(unsupported("pending path is outside incremental scope"));
    }
    paths.dedup();
    let rename_to = if event_kind == EventKind::Rename {
        let destination = request.input.get("renameTo").and_then(Value::as_str).map(graph::normalize_path);
        match (paths.len(), destination) {
            (1, Some(destination)) if !destination.is_empty() => {
                paths.push(destination);
                Some(paths[1].clone())
            }
            (2, None) => Some(paths[1].clone()),
            (2, Some(destination)) if destination == paths[1] => Some(destination),
            _ => return Err(unsupported("rename requires old & new paths")),
        }
    } else {
        paths.sort();
        None
    };
    if paths.is_empty() { return Err(unsupported("explicit event set is empty")); }
    context.check()?;

    let mut source_files = stored_file_records(root, &current);
    let before_paths = source_files.iter().map(|file| file.path.clone()).collect::<HashSet<_>>();
    let mut observed = HashMap::<String, graph::FileRecord>::new();
    for path in &paths {
        let is_deleted = event_kind == EventKind::Delete
            || (event_kind == EventKind::Rename && Some(path.as_str()) != rename_to.as_deref());
        if is_deleted { continue; }
        context.check()?;
        match stable_explicit_file(root, path) {
            Ok(Some(file)) => { observed.insert(path.clone(), file); }
            Ok(None) => return Err(unsupported(&format!("source path is absent: {path}"))),
            Err(error) => return Err(error),
        }
    }
    // The event set is explicit, so derive membership from the stored graph
    // and supplied observations only. The small closure below avoids any
    // filesystem metadata probe for untouched paths.
    let after_paths = {
        let mut result = before_paths.clone();
        match event_kind {
            EventKind::Delete => { for path in &paths { result.remove(path); } }
            EventKind::Rename => { result.remove(&paths[0]); result.insert(rename_to.clone().unwrap()); }
            _ => { for path in &paths { result.insert(path.clone()); } }
        }
        result
    };
    let mut changed_paths = Vec::new();
    let mut noop_paths = Vec::new();
    for path in &paths {
        let previous = source_files.iter().find(|file| file.path == *path).map(|file| file.content_hash.as_str());
        let current_digest = observed.get(path).map(|file| file.content_hash.as_str());
        let same_digest = previous.zip(current_digest).is_some_and(|(before, after)| normalize_digest(before) == normalize_digest(after));
        if event_kind != EventKind::Repair && event_kind != EventKind::Rename && same_digest {
            noop_paths.push(path.clone());
        } else {
            changed_paths.push(path.clone());
        }
    }
    if event_kind == EventKind::Delete && paths.iter().any(|path| !before_paths.contains(path)) {
        return Err(unsupported("delete path is absent from stored graph"));
    }
    if event_kind == EventKind::Modify && paths.iter().any(|path| !observed.contains_key(path)) {
        return Err(unsupported("modify path is absent from repository"));
    }

    for path in &paths {
        source_files.retain(|file| file.path != *path);
    }
    for file in observed.values() { source_files.push(file.clone()); }
    source_files.sort_by(|a, b| a.path.cmp(&b.path));
    let source_hash = graph::source_hash_for_files(&source_files);
    let config_digest = crate::static_provider::build_config_digest_for_files(&source_files);
    let observation = crate::git_source_observation::git_source_observation(&root.to_string_lossy());
    if changed_paths.is_empty() {
        let mut deltas = Vec::new();
        for path in &noop_paths {
            deltas.push(file_delta_from_graph(path, event_kind, None, observed.get(path), Vec::new(),
                request.input.get("sourceClock").and_then(Value::as_u64).and_then(|value| i64::try_from(value).ok()).unwrap_or(0),
                &source_hash, config_digest.clone(), observation.clone(), request)?);
        }
        if !deltas.is_empty() {
            let mut connection = open_store(db_path)?;
            delta_store::apply_file_deltas(&mut connection, &deltas, ApplyOptions::default())
                .map_err(|error| BlueprintError::new("blueprint_delta_failed", error.to_string()))?;
        }
        let (generation, source_observation) = load_current_with_observation(db_path)?;
        return bounded_generation_response(request, json!({
            "schemaVersion": 1, "operation": request.method.as_str(), "state": "fresh", "refreshMode": "incremental_noop",
            "generationId": generation.generation_id, "repoRoot": root.to_string_lossy(), "storePath": db_path.to_string_lossy(),
            "sourceHash": generation.source_hash, "complete": generation.complete, "truncationReasons": generation.truncation_reasons,
            "counts": {"nodes": generation.nodes.len(), "edges": generation.edges.len(), "files": generation.files.len()},
            "invalidatedPaths": [], "reusedFiles": source_files.len(), "sourceObservation": source_observation.unwrap_or(Value::Null)
        }));
    }

    let mut affected = std::collections::BTreeSet::new();
    for path in &changed_paths {
        affected.extend(affected_reference_closure(&current, path));
        affected.insert(path.clone());
    }
    let mut ordered = affected.into_iter().collect::<Vec<_>>();
    ordered.sort();
    let mut target_events = HashMap::new();
    for target in &ordered {
        let target_file = source_files.iter().find(|file| file.path == *target);
        let is_old_rename = event_kind == EventKind::Rename && target == &paths[0];
        let target_event = if is_old_rename { EventKind::Delete }
            else if !after_paths.contains(target) { EventKind::Delete }
            else if !before_paths.contains(target) { EventKind::Create }
            else if changed_paths.contains(target) { event_kind }
            else { EventKind::Repair };
        if target_event != EventKind::Delete && target_file.is_none() {
            return Err(unsupported(&format!("source facts unavailable for {target}")));
        }
        target_events.insert(target.clone(), target_event);
    }
    let mut facts_by_path = HashMap::new();
    for target in &ordered {
        context.check()?;
        if target_events[target] == EventKind::Delete { continue; }
        let facts = if changed_paths.contains(target) {
            graph::build_file_facts_from_scan(root, target, &source_files, &current.nodes, &context.cancellation)
                .map_err(|error| match error {
                    graph::GraphError::Cancelled if context.cancellation.deadline_expired() => BlueprintError::deadline(),
                    graph::GraphError::Cancelled => BlueprintError::cancelled(),
                    error => BlueprintError::new("blueprint_incremental_failed", error.to_string()),
                })?.ok_or_else(|| unsupported(&format!("source facts unavailable for {target}")))?
        } else {
            stored_file_facts(&current, target).ok_or_else(|| unsupported(&format!("stored source facts unavailable for {target}")))?
        };
        facts_by_path.insert(target.clone(), facts);
    }
    for dependent in unresolved_reference_dependents(&current, facts_by_path.values(), &source_files) {
        if target_events.contains_key(&dependent) || !source_files.iter().any(|file| file.path == dependent) { continue; }
        target_events.insert(dependent.clone(), EventKind::Repair);
        ordered.push(dependent.clone());
        context.check()?;
        let facts = stored_file_facts(&current, &dependent)
            .ok_or_else(|| unsupported(&format!("stored source facts unavailable for {dependent}")))?;
        facts_by_path.insert(dependent, facts);
    }
    ordered.sort();
    let mut post_change_nodes = current.nodes.iter()
        .filter(|node| node.path.as_deref().is_none_or(|path| !target_events.contains_key(path)))
        .cloned().collect::<Vec<_>>();
    for facts in facts_by_path.values() { post_change_nodes.push(facts.file.clone()); post_change_nodes.extend(facts.nodes.iter().cloned()); }
    for facts in facts_by_path.values_mut() { graph::resolve_file_facts_edges(facts, &post_change_nodes, &source_files); }

    let source_clock = request.input.get("sourceClock").and_then(Value::as_u64).and_then(|value| i64::try_from(value).ok()).unwrap_or(0);
    let mut deltas = Vec::with_capacity(ordered.len() + noop_paths.len());
    for target in &ordered {
        context.check()?;
        let target_file = source_files.iter().find(|file| file.path == *target);
        let provider_batches = if target_events[target] == EventKind::Delete || !changed_paths.contains(target) {
            Vec::new()
        } else {
            provider_batches_for_path(root, target, &source_files)?
        };
        deltas.push(file_delta_from_graph(target, target_events[target], facts_by_path.remove(target), target_file, provider_batches,
            source_clock, &source_hash, config_digest.clone(), observation.clone(), request)?);
    }
    for path in &noop_paths {
        deltas.push(file_delta_from_graph(path, event_kind, None, observed.get(path), Vec::new(), source_clock,
            &source_hash, config_digest.clone(), observation.clone(), request)?);
    }
    context.check()?;
    let mut connection = open_store(db_path)?;
    delta_store::apply_file_deltas(&mut connection, &deltas, ApplyOptions::default())
        .map_err(|error| BlueprintError::new("blueprint_delta_failed", error.to_string()))?;
    let (generation, source_observation) = load_current_with_observation(db_path)?;
    bounded_generation_response(request, json!({
        "schemaVersion": 1, "operation": request.method.as_str(), "state": "fresh", "refreshMode": "incremental",
        "generationId": generation.generation_id, "repoRoot": root.to_string_lossy(), "storePath": db_path.to_string_lossy(),
        "sourceHash": generation.source_hash, "complete": generation.complete, "truncationReasons": generation.truncation_reasons,
        "counts": {"nodes": generation.nodes.len(), "edges": generation.edges.len(), "files": generation.files.len()},
        "sourceClock": request.input.get("sourceClock").cloned().unwrap_or(Value::Null), "eventKind": request.input.get("eventKind").cloned().unwrap_or(Value::Null),
        "invalidatedPaths": ordered, "reusedFiles": source_files.len().saturating_sub(ordered.len()), "sourceObservation": source_observation.unwrap_or(Value::Null)
    }))
}

fn normalize_digest(value: &str) -> String {
    if value.starts_with("xxh128:") { value.to_owned() } else { format!("xxh128:{value}") }
}

fn stable_explicit_file(root: &Path, path: &str) -> Result<Option<graph::FileRecord>, BlueprintError> {
    let absolute = root.join(path);
    let read = match stable_read_with_limit(&absolute, MAX_SOURCE_FILE_BYTES) {
        Ok(read) => read,
        Err(StableReadError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(BlueprintError::new("blueprint_incremental_read_failed", format!("{path}: {error}"))),
    };
    if read.unstable { return Err(BlueprintError::new("blueprint_incremental_unstable", format!("source changed during stable read: {path}"))); }
    let text = String::from_utf8_lossy(&read.bytes).into_owned();
    Ok(Some(graph::FileRecord {
        path: path.to_owned(), absolute_path: absolute, size: read.bytes.len() as u64, bytes: read.bytes,
        content_hash: read.content_digest.clone(), semantic_content_hash: read.content_digest,
        text: Some(text),
    }))
}

fn stored_file_records(root: &Path, current: &GraphGeneration) -> Vec<graph::FileRecord> {
    let mut records = Vec::new();
    for node in &current.nodes {
        if node.kind != "file" { continue; }
        let Some(path) = node.path.clone() else { continue; };
        if records.iter().any(|file: &graph::FileRecord| file.path == path) { continue; }
        let digest = node.evidence.first().and_then(|evidence| evidence.get("contentHash")).and_then(Value::as_str).unwrap_or("").to_owned();
        records.push(graph::FileRecord {
            absolute_path: root.join(&path), path, bytes: Vec::new(), text: None, size: 0,
            semantic_content_hash: digest.clone(), content_hash: digest,
        });
    }
    records
}

fn stored_file_facts(current: &GraphGeneration, path: &str) -> Option<graph::FileFacts> {
    let file = current.nodes.iter().find(|node| node.kind == "file" && node.path.as_deref() == Some(path))?.clone();
    let nodes = current.nodes.iter().filter(|node| node.path.as_deref() == Some(path) && node.kind != "file").cloned().collect();
    let edges = current.edges.iter().filter(|edge| edge.evidence.iter().any(|evidence| evidence.get("path").and_then(Value::as_str) == Some(path))).cloned().collect();
    let report = current.files.iter().find(|report| report.path == path).cloned().unwrap_or_else(|| graph::FileReport {
        path: path.to_owned(), language: None, provider: "native-rust".into(), precision: graph::PrecisionTier::Lexical,
        parse_status: "unknown".into(), error_node_count: 0, error: None,
    });
    let content_digest = file.evidence.first().and_then(|evidence| evidence.get("contentHash")).and_then(Value::as_str)?.to_owned();
    Some(graph::FileFacts { file, nodes, edges, report, content_digest, size: 0 })
}

fn affected_reference_closure(current: &GraphGeneration, changed_path: &str) -> std::collections::BTreeSet<String> {
    let mut affected = std::collections::BTreeSet::new();
    let mut frontier = std::collections::VecDeque::from([changed_path.to_owned()]);
    while let Some(target_path) = frontier.pop_front() {
        for edge in &current.edges {
            let source_path = edge.evidence.iter().find_map(|e| e.get("path").and_then(Value::as_str));
            let target = edge.target.as_deref().and_then(|id| id.strip_prefix("file:")).or_else(|| {
                current.nodes.iter().find(|node| Some(node.id.as_str()) == edge.target.as_deref()).and_then(|node| node.path.as_deref())
            });
            let mut neighbors = Vec::new();
            if target == Some(target_path.as_str()) { if let Some(source) = source_path { neighbors.push(source); } }
            if source_path == Some(target_path.as_str()) { if let Some(target) = target { neighbors.push(target); } }
            for neighbor in neighbors.into_iter().filter(|path| !path.is_empty() && *path != target_path) {
                if affected.insert(neighbor.to_owned()) { frontier.push_back(neighbor.to_owned()); }
            }
        }
    }
    affected
}

fn unresolved_reference_dependents<'a, I>(
    current: &GraphGeneration,
    rebuilt: I,
    scan_files: &[graph::FileRecord],
) -> std::collections::BTreeSet<String>
where
    I: IntoIterator<Item = &'a graph::FileFacts>,
{
    let rebuilt = rebuilt.into_iter().collect::<Vec<_>>();
    let introduced = rebuilt.iter().flat_map(|facts| facts.nodes.iter().filter_map(|node| node.name.as_deref().map(str::to_owned))).collect::<HashSet<_>>();
    let introduced_paths = rebuilt.iter().filter_map(|facts| facts.file.path.clone()).collect::<HashSet<_>>();
    let file_map = scan_files.iter().map(|file| (file.path.clone(), file)).collect::<std::collections::BTreeMap<_, _>>();
    current.edges.iter().filter_map(|edge| {
        if edge.target.is_some() { return None; }
        let source_path = edge.evidence.iter().find_map(|evidence| evidence.get("path").and_then(Value::as_str))?;
        let reference = edge.evidence.iter().find_map(|evidence| evidence.get("callName").or_else(|| evidence.get("specifier")).and_then(Value::as_str))?;
        let becomes_resolved = match edge.kind.as_str() {
            "CALLS" | "CALL" => introduced.contains(reference),
            "IMPORTS" | "IMPORT" => crate::module_resolution::resolve_import_in_files(source_path, reference, &file_map)
                .is_some_and(|target| introduced_paths.contains(&target)),
            _ => false,
        };
        becomes_resolved.then(|| source_path.to_owned())
    }).collect()
}

fn provider_batches_for_path(
    root: &Path,
    path: &str,
    scan_files: &[graph::FileRecord],
) -> Result<Vec<FactBatch>, BlueprintError> {
    let Some(file) = scan_files.iter().find(|file| file.path == path) else { return Ok(Vec::new()); };
    let one_file = [file.clone()];
    let file_map = scan_files.iter().map(|file| (file.path.clone(), file)).collect::<std::collections::BTreeMap<_, _>>();
    let context = crate::providers::ProviderContext { repo_root: root, files: &one_file, file_map: &file_map };
    let mut batches = Vec::new();
    for descriptor in crate::providers::registry() {
        let output = (descriptor.run)(&context);
        let nodes = output.nodes.into_iter().filter(|node| node.path.as_deref() == Some(path)).collect::<Vec<_>>();
        let edges = output.edges.into_iter().filter(|edge| edge.evidence.iter().any(|evidence| evidence.get("path").and_then(Value::as_str) == Some(path))).collect::<Vec<_>>();
        if nodes.is_empty() && edges.is_empty() { continue; }
        let provider_version = nodes.iter()
            .flat_map(|node| node.evidence.iter())
            .chain(edges.iter().flat_map(|edge| edge.evidence.iter()))
            .find_map(|evidence| evidence.get("providerVersion").and_then(Value::as_str))
            .unwrap_or(graph::PROVIDER_VERSION);
        batches.push(FactBatch {
            provider_id: descriptor.id.to_owned(),
            provider_version: provider_version.to_owned(),
            nodes: nodes.into_iter().filter_map(|node| serde_json::to_value(node).ok()).collect(),
            edges: edges.into_iter().filter_map(|edge| serde_json::to_value(edge).ok()).collect(),
            dependencies: Vec::new(),
        });
    }
    Ok(batches)
}

fn file_delta_from_graph(
    path: &str,
    event_kind: EventKind,
    facts: Option<graph::FileFacts>,
    file: Option<&graph::FileRecord>,
    provider_batches: Vec<FactBatch>,
    source_clock: i64,
    source_hash: &str,
    config_digest: Option<String>,
    observation: Option<crate::git_source_observation::GitSourceObservation>,
    request: &BlueprintRequest,
) -> Result<FileDelta, BlueprintError> {
    let mut delta = FileDelta {
        path: path.to_owned(), event_kind, source_clock: Some(source_clock),
        content_digest: file.map(|file| file.content_hash.clone()), size: file.map(|file| file.size as i64),
        source_hash: Some(source_hash.to_owned()), config_digest,
        source_observation: Some(json!({"sourceClock": request.input.get("sourceClock"), "eventKind": request.input.get("eventKind"), "paths": request.input.get("paths"), "head": observation.as_ref().map(|value| value.head.clone()), "dirty": observation.as_ref().map(|value| value.dirty), "statusDigest": observation.as_ref().map(|value| value.status_digest.clone())})),
        ..FileDelta::default()
    };
    if let Some(facts) = facts {
        let mut nodes = Vec::with_capacity(facts.nodes.len() + 1);
        nodes.push(serde_json::to_value(facts.file).map_err(|error| BlueprintError::new("blueprint_delta_failed", error.to_string()))?);
        nodes.extend(facts.nodes.into_iter().filter_map(|node| serde_json::to_value(node).ok()));
        let dependencies = facts.edges.iter().filter_map(|edge| {
            let target = edge.target.as_deref()?.strip_prefix("file:")?;
            let reason = match edge.kind.as_str() {
                "IMPORTS" | "IMPORT" => "import",
                "CALLS" | "CALL" => "call",
                "ROUTES" | "ROUTE" => "route",
                "SCHEMA" | "SCHEMAS" => "schema",
                "CONFIG" | "CONFIGURES" => "config",
                "MANIFEST" | "MANIFESTS" => "manifest",
                _ => "schema",
            };
            Some((target.to_owned(), path.to_owned(), reason.to_owned()))
        }).collect();
        delta.content_digest = Some(facts.content_digest);
        delta.file_report = Some(serde_json::to_value(facts.report).map_err(|error| BlueprintError::new("blueprint_delta_failed", error.to_string()))?);
        // This batch is the merged replacement for one source path. An empty
        // provider id deliberately selects all prior owners, including
        // lexical/tree-sitter rows created by complete construction, so a
        // repair cannot leave stale facts from another provider behind.
        delta.fact_batches.push(FactBatch { provider_id: String::new(), provider_version: graph::PROVIDER_VERSION.into(), nodes, edges: facts.edges.into_iter().filter_map(|edge| serde_json::to_value(edge).ok()).collect(), dependencies });
        delta.fact_batches.extend(provider_batches);
    }
    Ok(delta)
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum VerifiedConstructionReason { GraphMissing, UnrecoverableCorruption }

impl VerifiedConstructionReason {
    fn code(self) -> &'static str { match self {
        Self::GraphMissing => "graph_missing",
        Self::UnrecoverableCorruption => "unrecoverable_corruption",
    }}
}

/// Owner-only construction authorization. Caller-provided reason/flags are
/// ignored; filesystem, SQLite & generation evidence decide eligibility.
pub(crate) fn verified_construction_reason(db_path: &Path) -> Result<Option<VerifiedConstructionReason>, BlueprintError> {
    if !db_path.is_file() { return Ok(Some(VerifiedConstructionReason::GraphMissing)); }
    match store::open_store_read_only(db_path).and_then(|connection| {
        let version = store::current_schema_version(&connection)?;
        if version > crate::migrations::SCHEMA_VERSION {
            return Err(StoreError::Migration(crate::migrations::MigrationError::UnsupportedVersion(version as i64)));
        }
        drop(connection);
        if version < crate::migrations::SCHEMA_VERSION {
            // Writable open takes the exact pre-migration backup before
            // applying a supported schema migration. A readable generation
            // is therefore preserved while its store schema catches up.
            store::open_store(Some(db_path))?;
        }
        let connection = store::open_store_read_only(db_path)?;
        store::load_generation(&connection)
    }) {
        Ok(Some(generation)) => {
            let graph_schema = generation.schema_version.unwrap_or(graph::GRAPH_SCHEMA_VERSION);
            let (provider, provider_version) = generation.provider.as_ref().map(|value| (
                value.get("id").and_then(Value::as_str).unwrap_or("native-rust"),
                value.get("version").and_then(Value::as_str).unwrap_or(graph::PROVIDER_VERSION),
            )).unwrap_or(("native-rust", graph::PROVIDER_VERSION));
            if graph_schema != graph::GRAPH_SCHEMA_VERSION || provider != "native-rust" || provider_version != graph::PROVIDER_VERSION {
                return Err(BlueprintError::new(
                    "blueprint_generation_incompatible",
                    format!("persisted graph schema/provider {graph_schema}/{provider}@{provider_version} is incompatible with supported {}/native-rust@{}", graph::GRAPH_SCHEMA_VERSION, graph::PROVIDER_VERSION),
                ));
            }
            Ok(None)
        },
        Ok(None) => Ok(Some(VerifiedConstructionReason::UnrecoverableCorruption)),
        Err(StoreError::Migration(crate::migrations::MigrationError::UnsupportedVersion(version)))
            if version > crate::migrations::SCHEMA_VERSION as i64 =>
                Err(BlueprintError::new("blueprint_schema_unsupported", format!("persisted schema version {version} is newer than supported version {}", crate::migrations::SCHEMA_VERSION))),
        Err(StoreError::Sqlite(error)) if error.to_string().contains("file is not a database") =>
            Ok(Some(VerifiedConstructionReason::UnrecoverableCorruption)),
        Err(error) => Err(store_error(error)),
    }
}

fn source_path_delta(current: &GraphGeneration, files: &[graph::FileRecord]) -> Vec<String> {
    let before = current.nodes.iter().filter(|node| node.kind == "file").filter_map(|node| {
        let path = node.path.as_deref()?;
        let hash = node.evidence.first()?.get("contentHash")?.as_str()?;
        let hash = hash.strip_prefix("xxh128:").unwrap_or(hash);
        Some((path, hash))
    }).collect::<std::collections::HashMap<_, _>>();
    let after = files.iter().map(|file| (file.path.as_str(), file.content_hash.strip_prefix("xxh128:").unwrap_or(&file.content_hash))).collect::<std::collections::HashMap<_, _>>();
    let mut paths = before.keys().chain(after.keys()).copied().collect::<Vec<_>>();
    paths.sort_unstable();
    paths.dedup();
    paths.into_iter().filter(|path| before.get(path) != after.get(path)).map(str::to_owned).collect()
}

fn build_and_publish(request: &BlueprintRequest, context: &RequestContext, root: &Path, db_path: &Path, verified_reason: VerifiedConstructionReason) -> Result<Value, BlueprintError> {
    context.check()?;
    record_construction_event(root, request, verified_reason, "started", None, db_path)?;
    // Keep graph construction synchronous so no detached worker can outlive
    // this call; its checkpoints observe request cancellation & deadline.
    let graph = match graph::build_generation_with_cancellation(root, &GraphOptions::default(), &context.cancellation) {
        Ok(graph) => graph,
        Err(error) => {
            let mapped = match error {
                graph::GraphError::Cancelled if context.cancellation.deadline_expired() => BlueprintError::deadline(),
                graph::GraphError::Cancelled => BlueprintError::cancelled(),
                error => BlueprintError::new("blueprint_build_failed", error.to_string()),
            };
            let _ = record_construction_event(root, request, verified_reason, "failed", None, db_path);
            return Err(mapped);
        }
    };
    context.check()?;
    let generation_id = graph.generation_id.clone();
    quarantine_unusable_store(db_path, request, verified_reason)?;
    let mut connection = open_store(db_path)?;
    context.check()?;
    // Gap 3 (lane STORE2): persist head/dirty with the generation at build
    // time, matching legacy `identityFromStore`'s `envelope.sourceObservation`
    // (`{ head, dirty }`), rather than leaving snapshot/changes callers to
    // recompute git state live at query time.
    //
    // Lane WIRE2 (NCL-02, git-source-observation.mjs row): this used to call
    // `lib_application_snapshots::current_git_identity`, a second,
    // independently drifted git observer (different porcelain exclusion set,
    // no statusDigest) -- exactly the split the legacy module's header
    // comment warns against. Build time and freshness-receipt time must run
    // the identical observer, so this now calls the canonical
    // `git_source_observation` port directly and also persists its
    // `statusDigest`, the bounded worktree fingerprint freshness comparisons
    // are built on.
    let git_identity = crate::git_source_observation::git_source_observation(&root.to_string_lossy());
    let observation = json!({
        "sourceClock": request.input.get("sourceClock").cloned().unwrap_or(Value::Null),
        "eventKind": request.input.get("eventKind").cloned().unwrap_or(Value::Null),
        "paths": request.input.get("paths").cloned().unwrap_or_else(|| json!([])),
        "head": git_identity.as_ref().map(|observation| observation.head.clone()),
        "dirty": git_identity.as_ref().map(|observation| observation.dirty),
        "statusDigest": git_identity.as_ref().map(|observation| observation.status_digest.clone()),
    });
    store::save_generation(&mut connection, &to_store_generation(&graph, observation.clone()))
        .map_err(store_error)?;
    // Gap 2 (lane STORE2): populate `generation_leaf` from the just-written
    // `files` table so snapshot/changes leaf identity comes from the same
    // table legacy's `identityFromStore` reads (`generation_leaf WHERE
    // kind='file'`), instead of native callers reading `files` directly.
    {
        let mut statement = connection
            .prepare("SELECT path, content_hash FROM files WHERE content_hash IS NOT NULL ORDER BY path")
            .map_err(|error| BlueprintError::new("blueprint_build_failed", error.to_string()))?;
        let rows = statement
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
            .map_err(|error| BlueprintError::new("blueprint_build_failed", error.to_string()))?;
        let mut leaf_files = Vec::new();
        for row in rows {
            let (path, content_hash) = row.map_err(|error| BlueprintError::new("blueprint_build_failed", error.to_string()))?;
            leaf_files.push(crate::merkle_ledger::LedgerFile { path, content_digest: content_hash });
        }
        drop(statement);
        crate::merkle_ledger::compute_full_ledger(&connection, &leaf_files)
            .map_err(|error| BlueprintError::new("blueprint_build_failed", error.to_string()))?;
    }
    // The legacy build publishes generated human docs after its graph/store
    // commit. Native builds have the same side effect through the Rust port;
    // `lib_generated_docs` reads the just-published generation directly when
    // retired JSON projection files are absent. A docs conflict is a typed
    // successful result with fallback output, matching the legacy contract.
    drop(connection);
    record_construction_event(root, request, verified_reason, "succeeded", Some(&generation_id), db_path)?;
    context.check()?;
    let docs_options = crate::lib_generated_docs::GenerateDocsOptions {
        no_readme_link: request
            .input
            .get("noReadmeLink")
            .or_else(|| request.input.get("no-readme-link"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
    };
    let docs_result = crate::lib_generated_docs::generate_docs(root, docs_options)
        .map_err(|error| BlueprintError::new("blueprint_docs_failed", error.to_string()))?;
    let docs_value = json!({
        "mode": docs_result.mode,
        "conflicts": docs_result.conflicts,
        "wrote": docs_result.wrote,
        "fallback": docs_result.fallback,
        "readme": docs_result.readme.map(|mode| json!({"mode": match mode {
            crate::lib_generated_docs::ReadmePointerMode::Absent => "absent",
            crate::lib_generated_docs::ReadmePointerMode::Created => "created",
            crate::lib_generated_docs::ReadmePointerMode::Updated => "updated",
        }})),
        "retired": docs_result.retired,
    });
    context.check()?;
    bounded_generation_response(request, json!({
        "schemaVersion": 1,
        "operation": request.method.as_str(),
        "state": "fresh",
        "generationId": generation_id,
        "repoRoot": root.to_string_lossy(),
        "storePath": db_path.to_string_lossy(),
        "sourceHash": graph.source_hash,
        "complete": graph.complete,
        "truncationReasons": graph.truncation_reasons,
        "counts": {"nodes": graph.nodes.len(), "edges": graph.edges.len(), "files": graph.files.len()},
        "sourceClock": request.input.get("sourceClock").cloned().unwrap_or(Value::Null),
        "eventKind": request.input.get("eventKind").cloned().unwrap_or(Value::Null),
        "sourceObservation": observation,
        "docsResult": docs_value,
    }))
}

fn quarantine_unusable_store(db_path: &Path, request: &BlueprintRequest, reason: VerifiedConstructionReason) -> Result<(), BlueprintError> {
    if !matches!(reason, VerifiedConstructionReason::UnrecoverableCorruption) || !db_path.exists() { return Ok(()); }
    let caller = request.request_id.chars().map(|value| if value.is_ascii_alphanumeric() || value == '-' { value } else { '_' }).collect::<String>();
    let backup = db_path.with_extension(format!("{}.{}.bak", reason.code(), caller));
    let mut moved = Vec::new();
    let mut targets = vec![(db_path.to_path_buf(), backup.clone())];
    for suffix in ["-wal", "-shm"] {
        let sidecar = PathBuf::from(format!("{}{}", db_path.to_string_lossy(), suffix));
        if sidecar.exists() { targets.push((sidecar, PathBuf::from(format!("{}{}", backup.to_string_lossy(), suffix)))); }
    }
    for (source, target) in &targets {
        if let Err(error) = fs::rename(source, target) {
            for (original, restored) in moved.into_iter().rev() { let _ = fs::rename(restored, original); }
            return Err(BlueprintError::new("blueprint_store_quarantine_failed", error.to_string()));
        }
        moved.push((source.clone(), target.clone()));
    }
    Ok(())
}

fn record_construction_event(root: &Path, request: &BlueprintRequest, reason: VerifiedConstructionReason, phase: &str, generation: Option<&str>, db_path: &Path) -> Result<(), BlueprintError> {
    let path = root.join(".agent").join("graph").join("full-constructions.jsonl");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| BlueprintError::new("blueprint_construction_receipt_failed", error.to_string()))?;
    }
    let evidence = match reason {
        VerifiedConstructionReason::GraphMissing => json!({"storePath":db_path,"storeExisted":false}),
        VerifiedConstructionReason::UnrecoverableCorruption => json!({"storePath":db_path,"storeExisted":true,"generationReadable":false}),
    };
    let record = json!({"schemaVersion":1,"phase":phase,"caller":{"requestId":request.request_id,"operation":request.method.as_str()},"verifiedReason":reason.code(),"evidence":evidence,"resultingGeneration":generation});
    let mut file = fs::OpenOptions::new().create(true).append(true).open(&path)
        .map_err(|error| BlueprintError::new("blueprint_construction_receipt_failed", error.to_string()))?;
    writeln!(file, "{}", record).map_err(|error| BlueprintError::new("blueprint_construction_receipt_failed", error.to_string()))
}

// Detailed omissions remain in the durable generation. Project only as many
// as fit on wire, accounting for the actual response envelope & UTF-8 bytes.
fn bounded_generation_response(request: &BlueprintRequest, mut value: Value) -> Result<Value, BlueprintError> {
    let count = value["truncationReasons"].as_array().map_or(0, Vec::len);
    value["truncationReasonCount"] = json!(count);
    value["truncationReasonsOmitted"] = json!(0);
    loop {
        let response = crate::api::BlueprintResponse::success(
            request.request_id.clone(), request.generation.clone(), value.clone());
        match response.validate(crate::api::Bounds::default()) {
            Ok(()) => return Ok(value),
            Err(error) => {
                let Some(reasons) = value["truncationReasons"].as_array_mut() else { return Err(error); };
                if reasons.pop().is_none() { return Err(error); }
                let omitted = count - reasons.len();
                value["truncationReasonsOmitted"] = json!(omitted);
                value["omissions"] = json!([{"reason": "response_projection", "field": "truncationReasons", "count": omitted}]);
            }
        }
    }
}

fn status(request: &BlueprintRequest, context: &RequestContext, root: &Path, db_path: &Path) -> Result<Value, BlueprintError> {
    context.check()?;
    if !db_path.exists() {
        return Ok(status_value(request, root, db_path, "missing", None, None, None));
    }
    let (generation, source_observation) = match load_current_with_observation(db_path) {
        Ok(value) => value,
        Err(error) if error.code == "blueprint_store_corrupt" => {
            return Ok(status_value(request, root, db_path, "corrupt", None, Some(error.message), None));
        }
        Err(error) => return Err(error),
    };
    context.check()?;
    let current = graph::scan_repository_with_cancellation(root, &graph::ScanOptions::default(), &context.cancellation)
        .map_err(|error| match error {
            graph::GraphError::Cancelled if context.cancellation.deadline_expired() => BlueprintError::deadline(),
            graph::GraphError::Cancelled => BlueprintError::cancelled(),
            error => BlueprintError::new("blueprint_status_failed", error.to_string()),
        })?;
    context.check()?;
    let state = if !current.traversal_truncated && !current.file_limit_reached
        && generation.source_hash == graph::source_hash_for_files(&current.files) { "fresh" } else { "stale" };
    let indexed_paths = generation.files.iter().map(|file| file.path.as_str()).collect::<HashSet<_>>();
    Ok(status_value(request, root, db_path, state, Some(&generation), None, Some(freshness_receipt(root, &generation.generation_id, source_observation.as_ref(), Some(&indexed_paths)))) )
}

fn status_value(request: &BlueprintRequest, root: &Path, db_path: &Path, state: &str, generation: Option<&GraphGeneration>, detail: Option<String>, freshness: Option<Value>) -> Value {
    json!({
        "schemaVersion": 1,
        "operation": request.method.as_str(),
        "state": state,
        "fresh": state == "fresh",
        "generationId": generation.map(|g| g.generation_id.clone()),
        "sourceHash": generation.map(|g| g.source_hash.clone()),
        "repoRoot": root.to_string_lossy(),
        "storePath": db_path.to_string_lossy(),
        "detail": detail,
        "complete": generation.map(|g| g.complete),
        "truncationReasons": generation.map(|g| g.truncation_reasons.clone()).unwrap_or_default(),
        "freshnessReceipt": freshness,
    })
}

fn doctor(request: &BlueprintRequest, root: &Path) -> Result<Value, BlueprintError> {
    let out_dir = request.input.get("outDir").and_then(Value::as_str).unwrap_or(".agent");
    let diagnostics = crate::lib_operations_doctor::collect_map_diagnostics(root, out_dir);
    Ok(json!({
        "schemaVersion": diagnostics.schema_version,
        "operation": request.method.as_str(),
        "state": diagnostics.state,
        "errors": diagnostics.errors,
        "warnings": diagnostics.warnings,
        "reasons": diagnostics.reasons.iter().map(|reason| json!({"code": reason.code, "severity": reason.severity, "message": reason.message})).collect::<Vec<_>>(),
        "repoRoot": root.to_string_lossy(),
        "outDir": out_dir,
    }))
}

fn repair(request: &BlueprintRequest, root: &Path) -> Result<Value, BlueprintError> {
    let out_dir = request.input.get("outDir").and_then(Value::as_str).unwrap_or(".agent");
    let diagnostics = crate::lib_operations_doctor::collect_map_diagnostics(root, out_dir);
    let reasons = diagnostics.reasons.iter().map(|reason| crate::lib_operations_repair::DoctorReason {
        code: reason.code.clone(), severity: reason.severity.clone(),
    }).collect::<Vec<_>>();
    let plan = crate::lib_operations_repair::build_repair_plan(&root.to_string_lossy(), out_dir, &diagnostics.state, &reasons);
    Ok(json!({
        "schemaVersion": plan.schema_version,
        "operation": request.method.as_str(),
        "root": plan.root,
        "outDir": out_dir,
        "graphState": plan.graph_state,
        "actions": plan.actions.iter().map(|action| json!({"id": action.id, "kind": action.kind, "command": action.command, "reversible": action.reversible, "reason": action.reason})).collect::<Vec<_>>(),
    }))
}

fn architecture_changes(request: &BlueprintRequest, context: &RequestContext, root: &Path, db_path: &Path) -> Result<Value, BlueprintError> {
    context.check()?;
    let connection = store::open_store_read_only(db_path).map_err(store_error)?;
    let (generation, source_observation) = load_current_with_observation(db_path)?;
    ensure_generation(request, &generation)?;
    let limit = request.input.get("limit").and_then(Value::as_u64).unwrap_or(100);
    let snapshot = request.input.get("snapshot").and_then(Value::as_str);
    let since_generation = request.input.get("sinceGeneration").and_then(Value::as_str);
    let head = request.input.get("head").and_then(Value::as_str).unwrap_or("HEAD");
    let treeish_pair = match request.input.get("treeish") {
        Some(Value::String(base)) => Some((base.as_str(), head)),
        Some(Value::Object(map)) => {
            let base = map.get("base").or_else(|| map.get("from")).and_then(Value::as_str);
            let treeish_head = map.get("head").or_else(|| map.get("to")).and_then(Value::as_str).unwrap_or(head);
            base.map(|value| (value, treeish_head))
        }
        _ => None,
    };
    let mut value = crate::lib_application_snapshots::changes_since_reference(
        &connection, root, snapshot, since_generation, treeish_pair, limit,
    )?;
    if let Value::Object(object) = &mut value {
        object.insert("view".into(), json!("changes"));
        object.insert("generationId".into(), json!(generation.generation_id.clone()));
        let indexed_paths = generation.files.iter().map(|file| file.path.as_str()).collect::<HashSet<_>>();
        object.insert("freshnessReceipt".into(), freshness_receipt(root, &generation.generation_id, source_observation.as_ref(), Some(&indexed_paths)));
    }
    context.check()?;
    Ok(value)
}

/// Build query-time freshness evidence from the same persisted source
/// observation captured at publication. Freshness is advisory state; graph
/// generation pinning remains an independent fail-closed check.
fn freshness_receipt(root: &Path, generation_id: &str, source_observation: Option<&Value>, indexed_paths: Option<&HashSet<&str>>) -> Value {
    let indexed_revision = source_observation.and_then(|value| value.get("head")).and_then(Value::as_str).map(str::to_owned);
    let indexed_fingerprint = source_observation.and_then(|value| value.get("statusDigest")).and_then(Value::as_str).map(str::to_owned);
    let current = crate::freshness_observation::observe_current_vcs_state(root);
    let basis = crate::freshness::GenerationFreshnessBasis { indexed_revision: indexed_revision.clone(), indexed_worktree_fingerprint: indexed_fingerprint.clone() };
    let changed_basis = basis.clone();
    let changed_current = current.clone();
    let indexed = indexed_paths;
    let receipt = crate::freshness_receipt::build_freshness_receipt(
        Some(generation_id.to_owned()),
        None,
        basis,
        current,
        || crate::freshness_observation::changed_paths_for_freshness(root, &changed_basis, &changed_current),
        |path| indexed.map_or(true, |paths| paths.contains(path)),
    );
    serde_json::to_value(receipt).unwrap_or_else(|_| json!({"schema":"BlueprintFreshnessReceiptV1","generationId":generation_id,"freshness":"unavailable"}))
}

fn open_store(path: &Path) -> Result<rusqlite::Connection, BlueprintError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| BlueprintError::new("blueprint_store_unavailable", error.to_string()))?;
    }
    store::open_store(Some(path)).map_err(store_error)
}

fn load_current(path: &Path) -> Result<GraphGeneration, BlueprintError> {
    load_current_with_observation(path).map(|(generation, _)| generation)
}

fn load_current_with_observation(path: &Path) -> Result<(GraphGeneration, Option<Value>), BlueprintError> {
    if !path.exists() { return Err(BlueprintError::new("blueprint_store_missing", "no persisted Blueprint generation exists")); }
    let connection = store::open_store_read_only(path).map_err(store_error)?;
    let generation = store::load_generation(&connection).map_err(store_error)?
        .ok_or_else(|| BlueprintError::new("blueprint_store_missing", "no persisted Blueprint generation exists"))?;
    let observation = generation.source_observation.clone();
    from_store_generation(generation).map(|graph| (graph, observation))
}

fn ensure_generation(request: &BlueprintRequest, generation: &GraphGeneration) -> Result<(), BlueprintError> {
    if let Some(expected) = request.generation.as_deref().or_else(|| request.input.get("generation").and_then(Value::as_str)) {
        if expected != generation.generation_id {
            return Err(BlueprintError::generation_mismatch(expected, generation.generation_id.clone()));
        }
    }
    Ok(())
}

fn to_store_generation(graph: &GraphGeneration, source_observation: Value) -> Generation {
    Generation {
        schema_version: Some(graph.schema_version),
        provider: Some(json!({"id": graph.provider, "version": graph.provider_version})),
        manifest: Some(json!({"generationId": graph.generation_id, "sourceHash": graph.source_hash, "configDigest": crate::graph::config_digest_for_generation(graph), "complete": graph.complete, "truncationReasons": graph.truncation_reasons})),
        repo_root: Some(json!(graph.repo_root)),
        augmentation: None,
        source_observation: Some(source_observation),
        nodes: graph.nodes.iter().filter_map(|value| serde_json::to_value(value).ok()).collect(),
        edges: graph.edges.iter().filter_map(|value| serde_json::to_value(value).ok()).collect(),
        file_reports: graph.files.iter().filter_map(|value| serde_json::to_value(value).ok()).collect(),
        documents: None,
        claims: None,
        claim_code_edges: None,
        document_supersession: None,
        extra: Default::default(),
    }
}

fn from_store_generation(value: Generation) -> Result<GraphGeneration, BlueprintError> {
    let manifest = value.manifest.as_ref().ok_or_else(|| BlueprintError::new("blueprint_store_corrupt", "persisted generation manifest is missing"))?;
    let generation_id = manifest.get("generationId").and_then(Value::as_str).ok_or_else(|| BlueprintError::new("blueprint_store_corrupt", "persisted generation identity is missing"))?.to_owned();
    let provider = value.provider.as_ref().and_then(|v| v.get("id")).and_then(Value::as_str).unwrap_or("native-rust").to_owned();
    let provider_version = value.provider.as_ref().and_then(|v| v.get("version")).and_then(Value::as_str).unwrap_or("native-rust-1").to_owned();
    Ok(GraphGeneration {
        schema_version: value.schema_version.unwrap_or(graph::GRAPH_SCHEMA_VERSION),
        provider,
        provider_version,
        generation_id,
        source_hash: manifest.get("sourceHash").and_then(Value::as_str).unwrap_or("").to_owned(),
        repo_root: value.repo_root.as_ref().and_then(Value::as_str).unwrap_or("").to_owned(),
        complete: manifest.get("complete").and_then(Value::as_bool).unwrap_or(true),
        nodes: value.nodes.into_iter().map(graph_node_from_store).collect::<Result<_, _>>()?,
        edges: value.edges.into_iter().map(graph_edge_from_store).collect::<Result<_, _>>()?,
        files: value.file_reports.into_iter().map(serde_json::from_value).collect::<Result<_, _>>().map_err(|e| BlueprintError::new("blueprint_store_corrupt", e.to_string()))?,
        truncation_reasons: manifest.get("truncationReasons").and_then(Value::as_array).map(|v| v.iter().filter_map(Value::as_str).map(str::to_owned).collect()).unwrap_or_default(),
    })
}

fn graph_node_from_store(value: Value) -> Result<crate::model::GraphNode, BlueprintError> {
    let Value::Object(mut object) = value else {
        return Err(BlueprintError::new("blueprint_store_corrupt", "persisted graph node is not an object"));
    };
    object.retain(|key, _| matches!(key.as_str(), "id" | "kind" | "path" | "name" | "generationId" | "evidence"));
    serde_json::from_value(Value::Object(object))
        .map_err(|error| BlueprintError::new("blueprint_store_corrupt", error.to_string()))
}

fn graph_edge_from_store(value: Value) -> Result<crate::model::GraphEdge, BlueprintError> {
    let Value::Object(mut object) = value else {
        return Err(BlueprintError::new("blueprint_store_corrupt", "persisted graph edge is not an object"));
    };
    object.retain(|key, _| matches!(key.as_str(), "id" | "kind" | "source" | "target" | "generationId" | "evidence"));
    serde_json::from_value(Value::Object(object))
        .map_err(|error| BlueprintError::new("blueprint_store_corrupt", error.to_string()))
}

fn store_error(error: StoreError) -> BlueprintError {
    let code = match &error {
        StoreError::GenerationNotFound | StoreError::GenerationMismatch { .. } => "blueprint_store_missing",
        StoreError::Sqlite(_) | StoreError::Migration(_) | StoreError::Json(_) | StoreError::InvalidGeneration(_) | StoreError::Path(_) => "blueprint_store_corrupt",
    };
    BlueprintError::new(code, error.to_string())
}

#[cfg(test)]
mod response_projection_tests {
    use super::*;

    #[test]
    fn oversized_omission_projection_preserves_coverage_and_accounts_for_every_reason() {
        let request = BlueprintRequest::new("projection", Operation::Build, "D:/repo");
        let reasons: Vec<String> = (0..169).map(|index| format!("unsupported_file_bytes:{index}/{}", "long-path/".repeat(30))).collect();
        let original = json!({"complete": false, "truncationReasons": reasons});
        let projected = bounded_generation_response(&request, original.clone()).unwrap();
        let retained = projected["truncationReasons"].as_array().unwrap();
        let omitted = projected["truncationReasonsOmitted"].as_u64().unwrap() as usize;
        assert!(omitted > 0);
        assert_eq!(retained.len() + omitted, 169);
        assert_eq!(retained, &original["truncationReasons"].as_array().unwrap()[..retained.len()]);
        assert_eq!(projected["complete"], false);
        assert_eq!(projected["omissions"][0]["count"], omitted);
        crate::api::BlueprintResponse::success(request.request_id, None, projected)
            .validate(crate::api::Bounds::default()).unwrap();
        assert_eq!(original["truncationReasons"].as_array().unwrap().len(), 169);
    }

    #[test]
    fn construction_guard_verifies_each_permitted_reason() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("missing.db");
        assert_eq!(verified_construction_reason(&missing).unwrap().unwrap().code(), "graph_missing");

        let corrupt = root.path().join("corrupt.db");
        std::fs::write(&corrupt, b"not sqlite").unwrap();
        assert_eq!(verified_construction_reason(&corrupt).unwrap().unwrap().code(), "unrecoverable_corruption");

        let incompatible = root.path().join("incompatible.db");
        let connection = rusqlite::Connection::open(&incompatible).unwrap();
        connection.execute_batch("CREATE TABLE meta(key TEXT PRIMARY KEY,value TEXT NOT NULL); INSERT INTO meta VALUES('schema_version','21');").unwrap();
        drop(connection);
        let error = verified_construction_reason(&incompatible).unwrap_err();
        assert_eq!(error.code, "blueprint_schema_unsupported");
    }
}
