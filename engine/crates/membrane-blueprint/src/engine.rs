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
use serde_json::{json, Value};
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
            Operation::Build | Operation::Refresh => {
                if request.generation.is_some() || request.input.get("generation").and_then(Value::as_str).is_some() {
                    let current = load_current(&db_path)?;
                    ensure_generation(request, &current)?;
                }
                build_and_publish(request, context, &root, &db_path)
            }
            Operation::Status | Operation::DbStatus => status(request, context, &root, &db_path),
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

fn build_and_publish(request: &BlueprintRequest, context: &RequestContext, root: &Path, db_path: &Path) -> Result<Value, BlueprintError> {
    context.check()?;
    // `graph::build_generation` has no cooperative cancellation seam.  Keep
    // the operation synchronous so no detached worker can outlive this call;
    // cancellation/deadline is enforced at every safe pre-save boundary.
    let graph = graph::build_generation(root, &GraphOptions::default())
        .map_err(|error| BlueprintError::new("blueprint_build_failed", error.to_string()))?;
    context.check()?;
    let generation_id = graph.generation_id.clone();
    let mut connection = open_store(db_path)?;
    context.check()?;
    let observation = json!({
        "sourceClock": request.input.get("sourceClock").cloned().unwrap_or(Value::Null),
        "eventKind": request.input.get("eventKind").cloned().unwrap_or(Value::Null),
        "paths": request.input.get("paths").cloned().unwrap_or_else(|| json!([])),
    });
    store::save_generation(&mut connection, &to_store_generation(&graph, observation.clone()))
        .map_err(store_error)?;
    Ok(json!({
        "schemaVersion": 1,
        "operation": request.method.as_str(),
        "state": "fresh",
        "generationId": generation_id,
        "repoRoot": root.to_string_lossy(),
        "storePath": db_path.to_string_lossy(),
        "sourceHash": graph.source_hash,
        "complete": graph.complete,
        "counts": {"nodes": graph.nodes.len(), "edges": graph.edges.len(), "files": graph.files.len()},
        "sourceClock": request.input.get("sourceClock").cloned().unwrap_or(Value::Null),
        "eventKind": request.input.get("eventKind").cloned().unwrap_or(Value::Null),
        "sourceObservation": observation,
    }))
}

fn status(request: &BlueprintRequest, context: &RequestContext, root: &Path, db_path: &Path) -> Result<Value, BlueprintError> {
    context.check()?;
    if !db_path.exists() {
        return Ok(status_value(request, root, db_path, "missing", None, None));
    }
    let generation = match load_current(db_path) {
        Ok(value) => value,
        Err(error) if error.code == "blueprint_store_corrupt" => {
            return Ok(status_value(request, root, db_path, "corrupt", None, Some(error.message)));
        }
        Err(error) => return Err(error),
    };
    context.check()?;
    let current = graph::build_generation(root, &GraphOptions::default())
        .map_err(|error| BlueprintError::new("blueprint_status_failed", error.to_string()))?;
    context.check()?;
    let state = if generation.source_hash == current.source_hash { "fresh" } else { "stale" };
    Ok(status_value(request, root, db_path, state, Some(&generation), None))
}

fn status_value(request: &BlueprintRequest, root: &Path, db_path: &Path, state: &str, generation: Option<&GraphGeneration>, detail: Option<String>) -> Value {
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
    })
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
