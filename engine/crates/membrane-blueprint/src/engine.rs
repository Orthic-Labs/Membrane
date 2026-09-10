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
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs;
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
                if request.method == Operation::Refresh {
                    match incremental_refresh(request, context, &root, &db_path)? {
                        Some(value) => Ok(value),
                        None => build_and_publish(request, context, &root, &db_path),
                    }
                } else {
                    build_and_publish(request, context, &root, &db_path)
                }
            }
            Operation::Status | Operation::DbStatus => status(request, context, &root, &db_path)
                .and_then(|value| bounded_generation_response(request, value)),
            Operation::FindingsGet => {
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

/// Apply one watcher-shaped code-file refresh in place. Returning `None`
/// means the event is outside the narrow structural lane and must use the
/// complete graph rebuild, preserving provider/document semantics.
fn incremental_refresh(
    request: &BlueprintRequest,
    context: &RequestContext,
    root: &Path,
    db_path: &Path,
) -> Result<Option<Value>, BlueprintError> {
    let Some(paths) = request.input.get("paths").and_then(Value::as_array) else { return Ok(None); };
    if paths.len() != 1 { return Ok(None); }
    let Some(path) = paths[0].as_str().map(|value| value.replace('\\', "/")) else { return Ok(None); };
    if path.is_empty() || path.starts_with('/') || path.contains("..") || path.starts_with(".agent/") { return Ok(None); }
    let event_kind = match request.input.get("eventKind").and_then(Value::as_str).unwrap_or("modify").to_ascii_lowercase().as_str() {
        "create" => EventKind::Create,
        "modify" | "changed" => EventKind::Modify,
        "delete" => EventKind::Delete,
        // Rename needs a second file's facts and is intentionally handled by
        // the complete builder until both sides can be admitted atomically.
        _ => return Ok(None),
    };
    if !graph::is_code_path(&path) { return Ok(None); }
    if !db_path.exists() { return Ok(None); }
    let current = load_current_with_observation(db_path)?;
    if !current.0.complete { return Ok(None); }
    context.check()?;

    // A bounded scan verifies this event did not hide additional changes or a
    // traversal gap. It performs no parsing/provider work; only the eligible
    // file is then converted into facts.
    let scan = graph::scan_repository_with_cancellation(root, &graph::ScanOptions::default(), &context.cancellation)
        .map_err(|error| match error {
            graph::GraphError::Cancelled if context.cancellation.deadline_expired() => BlueprintError::deadline(),
            graph::GraphError::Cancelled => BlueprintError::cancelled(),
            error => BlueprintError::new("blueprint_refresh_scan_failed", error.to_string()),
        })?;
    if scan.traversal_truncated || scan.file_limit_reached { return Ok(None); }
    if !only_path_changed(&current.0, &scan.files, &path, event_kind) { return Ok(None); }
    let facts = match event_kind {
        EventKind::Delete => None,
        _ => match graph::build_file_facts(root, &path, &context.cancellation) {
            Ok(value) => value,
            Err(graph::GraphError::Cancelled) if context.cancellation.deadline_expired() => return Err(BlueprintError::deadline()),
            Err(graph::GraphError::Cancelled) => return Err(BlueprintError::cancelled()),
            Err(_) => None,
        },
    };
    if !matches!(event_kind, EventKind::Delete) && facts.is_none() { return Ok(None); }

    let observation = crate::git_source_observation::git_source_observation(&root.to_string_lossy());
    let source_clock = request.input.get("sourceClock").and_then(Value::as_u64).and_then(|clock| i64::try_from(clock).ok());
    let Some(source_clock) = source_clock else { return Ok(None); };
    let mut delta = FileDelta {
        path: path.clone(), event_kind, source_clock: Some(source_clock),
        source_hash: Some(graph::source_hash_for_files(&scan.files)),
        source_observation: Some(json!({
            "sourceClock": request.input.get("sourceClock").cloned().unwrap_or(Value::Null),
            "eventKind": request.input.get("eventKind").cloned().unwrap_or(Value::Null),
            "paths": request.input.get("paths").cloned().unwrap_or_else(|| json!([])),
            "head": observation.as_ref().map(|value| value.head.clone()),
            "dirty": observation.as_ref().map(|value| value.dirty),
            "statusDigest": observation.as_ref().map(|value| value.status_digest.clone()),
        })),
        ..FileDelta::default()
    };
    if let Some(facts) = facts {
        let provider = graph::PROVIDER_VERSION;
        let mut nodes = Vec::with_capacity(facts.nodes.len() + 1);
        nodes.push(serde_json::to_value(facts.file).map_err(|error| BlueprintError::new("blueprint_delta_failed", error.to_string()))?);
        nodes.extend(facts.nodes.into_iter().filter_map(|node| serde_json::to_value(node).ok()));
        delta.content_digest = Some(facts.content_digest);
        delta.size = Some(facts.size);
        delta.file_report = Some(serde_json::to_value(&facts.report).map_err(|error| BlueprintError::new("blueprint_delta_failed", error.to_string()))?);
        delta.fact_batches.push(FactBatch {
            provider_id: "native-rust".into(), provider_version: provider.into(), nodes,
            edges: facts.edges.into_iter().filter_map(|edge| serde_json::to_value(edge).ok()).collect(),
            dependencies: Vec::new(),
        });
    }
    context.check()?;
    let mut connection = open_store(db_path)?;
    delta_store::apply_file_delta(&mut connection, &delta, ApplyOptions::default())
        .map_err(|error| BlueprintError::new("blueprint_delta_failed", error.to_string()))?;
    context.check()?;
    let (generation, source_observation) = load_current_with_observation(db_path)?;
    bounded_generation_response(request, json!({
        "schemaVersion": 1, "operation": request.method.as_str(), "state": "fresh",
        "refreshMode": "incremental", "generationId": generation.generation_id,
        "repoRoot": root.to_string_lossy(), "storePath": db_path.to_string_lossy(),
        "sourceHash": generation.source_hash, "complete": generation.complete,
        "truncationReasons": generation.truncation_reasons,
        "counts": {"nodes": generation.nodes.len(), "edges": generation.edges.len(), "files": generation.files.len()},
        "sourceClock": request.input.get("sourceClock").cloned().unwrap_or(Value::Null),
        "eventKind": request.input.get("eventKind").cloned().unwrap_or(Value::Null),
        "sourceObservation": source_observation.unwrap_or(Value::Null),
    })).map(Some)
}

fn only_path_changed(current: &GraphGeneration, files: &[graph::FileRecord], path: &str, event_kind: EventKind) -> bool {
    let mut before = current.nodes.iter().filter(|node| node.kind == "file").filter_map(|node| {
        let path = node.path.clone()?;
        let hash = node.evidence.first().and_then(|value| value.get("contentHash")).and_then(Value::as_str)?;
        let digest = hash.strip_prefix("xxh128:").unwrap_or(hash).to_owned();
        Some((path, digest))
    }).collect::<std::collections::BTreeMap<_, _>>();
    let after = files.iter().map(|file| (file.path.clone(), file.content_hash.strip_prefix("xxh128:").unwrap_or(&file.content_hash).to_owned())).collect::<std::collections::BTreeMap<_, _>>();
    let all = before.keys().chain(after.keys()).cloned().collect::<std::collections::BTreeSet<_>>();
    let changed = all.into_iter().filter(|candidate| before.remove(candidate) != after.get(candidate).cloned()).collect::<Vec<_>>();
    changed.len() == 1 && changed[0] == path && ((event_kind == EventKind::Delete && !after.contains_key(path)) || (event_kind != EventKind::Delete && after.contains_key(path)))
}

fn build_and_publish(request: &BlueprintRequest, context: &RequestContext, root: &Path, db_path: &Path) -> Result<Value, BlueprintError> {
    context.check()?;
    // Keep graph construction synchronous so no detached worker can outlive
    // this call; its checkpoints observe request cancellation & deadline.
    let graph = graph::build_generation_with_cancellation(root, &GraphOptions::default(), &context.cancellation)
        .map_err(|error| match error {
            graph::GraphError::Cancelled if context.cancellation.deadline_expired() => BlueprintError::deadline(),
            graph::GraphError::Cancelled => BlueprintError::cancelled(),
            error => BlueprintError::new("blueprint_build_failed", error.to_string()),
        })?;
    context.check()?;
    let generation_id = graph.generation_id.clone();
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
    let current = graph::build_generation_with_cancellation(root, &GraphOptions::default(), &context.cancellation)
        .map_err(|error| match error {
            graph::GraphError::Cancelled if context.cancellation.deadline_expired() => BlueprintError::deadline(),
            graph::GraphError::Cancelled => BlueprintError::cancelled(),
            error => BlueprintError::new("blueprint_status_failed", error.to_string()),
        })?;
    context.check()?;
    let state = if generation.source_hash == current.source_hash { "fresh" } else { "stale" };
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
        manifest: Some(json!({"generationId": graph.generation_id, "sourceHash": graph.source_hash, "complete": graph.complete, "truncationReasons": graph.truncation_reasons})),
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
}
