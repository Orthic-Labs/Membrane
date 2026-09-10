//! Bounded one-shot Blueprint CLI entry points.
//!
//! Native, explicit callers (manual refresh, a CLI binary, a bounded script)
//! use this module to invoke the canonical native Blueprint operation
//! without depending on Hub or a resident service holder. It opens no second
//! graph/store/planner: every call here delegates to
//! [`crate::engine::native_blueprint_operation`], the single owner of
//! repository graph construction, publication, and query dispatch.
//!
//! The module is exported from the crate root so native callers can use the
//! same operation owner without reaching into the graph or store modules.

use crate::api::{BlueprintApi, BlueprintError, BlueprintRequest, BlueprintResponse, CancellationToken};
use crate::engine::native_blueprint_operation;
use crate::model::Operation;
use serde_json::Value;

/// Run one bounded, explicit Blueprint operation against `repo_root` and
/// return its raw response. Deadline defaults to the protocol default when
/// `deadline_ms` is `None`. Cancellation is fresh per call: callers that need
/// cooperative cancellation should use [`crate::api::BlueprintOperation`]
/// directly with their own [`CancellationToken`].
pub fn run_one_shot(
    request_id: impl Into<String>,
    method: Operation,
    repo_root: impl Into<String>,
    deadline_ms: Option<u64>,
) -> BlueprintResponse {
    let mut request = BlueprintRequest::new(request_id, method, repo_root);
    if let Some(deadline_ms) = deadline_ms {
        request.deadline_ms = deadline_ms;
    }
    dispatch(request)
}

fn dispatch(request: BlueprintRequest) -> BlueprintResponse {
    native_blueprint_operation().dispatch(request, CancellationToken::new())
}

fn unwrap_result(response: BlueprintResponse, failure_code: &'static str, failure_message: &'static str) -> Result<Value, BlueprintError> {
    if response.ok {
        response.result.ok_or_else(|| BlueprintError::malformed("response missing result"))
    } else {
        Err(response.error.unwrap_or_else(|| BlueprintError::new(failure_code, failure_message)))
    }
}

/// Manual refresh: explicit rebuild-and-publish, callable from idle,
/// mid-build, watcher-disabled and Hub-off states alike (LC-02). Freshness is
/// always reported from the published generation this call produces, never
/// from an enqueue timestamp — this function performs the build/save
/// synchronously and only returns after `native_blueprint_operation` has
/// published the generation.
pub fn manual_refresh(repo_root: impl Into<String>, deadline_ms: Option<u64>) -> Result<Value, BlueprintError> {
    let response = run_one_shot("cli-manual-refresh", Operation::Refresh, repo_root, deadline_ms);
    unwrap_result(response, "blueprint_cli_refresh_failed", "manual refresh failed")
}

/// Bounded task/symbol/file/node orientation lookup (BM03). This wrapper
/// adds no second analysis path: it forwards to the existing Architecture
/// operation (`crate::query::execute_query`, reached through
/// `native_blueprint_operation`) and returns its typed result unchanged,
/// including every section's disposition/returned_count/total_known_count/
/// truncated/reason fields.
pub fn architecture_orientation(
    repo_root: impl Into<String>,
    task_or_symbol: impl Into<String>,
    deadline_ms: Option<u64>,
) -> Result<Value, BlueprintError> {
    let mut request = BlueprintRequest::new("cli-architecture", Operation::Architecture, repo_root);
    if let Some(deadline_ms) = deadline_ms {
        request.deadline_ms = deadline_ms;
    }
    if let Value::Object(map) = &mut request.input {
        map.insert("task".into(), Value::from(task_or_symbol.into()));
    }
    let response = dispatch(request);
    unwrap_result(response, "blueprint_cli_architecture_failed", "architecture orientation failed")
}

/// Explicit repository status, independent of any Hub/resident holder.
pub fn status(repo_root: impl Into<String>, deadline_ms: Option<u64>) -> Result<Value, BlueprintError> {
    let response = run_one_shot("cli-status", Operation::Status, repo_root, deadline_ms);
    unwrap_result(response, "blueprint_cli_status_failed", "status failed")
}

/// Run a bounded query through the canonical native operation. `input` may
/// contain operation-specific fields (`query`, `seed`, `target`, `direction`,
/// and limits); this wrapper only binds the repository root and never performs
/// retrieval or ranking itself.
pub fn run_query(
    request_id: impl Into<String>,
    method: Operation,
    repo_root: impl Into<String>,
    mut input: Value,
    deadline_ms: Option<u64>,
) -> Result<Value, BlueprintError> {
    let Value::Object(object) = &mut input else {
        return Err(BlueprintError::invalid("query input must be an object"));
    };
    object.insert("repoRoot".into(), Value::from(repo_root.into()));
    let mut request = BlueprintRequest::new(request_id, method, "");
    request.input = input;
    if let Some(deadline_ms) = deadline_ms {
        request.deadline_ms = deadline_ms;
    }
    unwrap_result(dispatch(request), "blueprint_query_failed", "Blueprint query failed")
}

/// Source-grounded lexical search over the published native generation.
pub fn search(
    repo_root: impl Into<String>,
    query_text: impl Into<String>,
    limit: Option<usize>,
    deadline_ms: Option<u64>,
) -> Result<Value, BlueprintError> {
    let mut input = serde_json::json!({"query": query_text.into()});
    if let Some(limit) = limit { input["maxCandidates"] = Value::from(limit); }
    run_query("cli-search", Operation::Search, repo_root, input, deadline_ms)
}

/// Resolve a source-bound seed and return the bounded recall projection.
pub fn recall(
    repo_root: impl Into<String>,
    seed: impl Into<String>,
    generation: Option<String>,
    deadline_ms: Option<u64>,
) -> Result<Value, BlueprintError> {
    let mut input = serde_json::json!({"seed": seed.into()});
    if let Some(generation) = generation { input["generation"] = Value::from(generation); }
    run_query("cli-recall", Operation::Recall, repo_root, input, deadline_ms)
}

/// Expand a source-bound seed through the published native graph.
pub fn expand(
    repo_root: impl Into<String>,
    seed: impl Into<String>,
    direction: Option<&str>,
    max_depth: Option<usize>,
    deadline_ms: Option<u64>,
) -> Result<Value, BlueprintError> {
    let mut input = serde_json::json!({"seed": seed.into()});
    if let Some(direction) = direction { input["direction"] = Value::from(direction); }
    if let Some(max_depth) = max_depth { input["maxDepth"] = Value::from(max_depth); }
    run_query("cli-expand", Operation::Expand, repo_root, input, deadline_ms)
}
