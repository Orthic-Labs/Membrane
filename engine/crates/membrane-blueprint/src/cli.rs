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
    architecture_orientation_view(repo_root, task_or_symbol, None, deadline_ms)
}

/// Same as [`architecture_orientation`], with the legacy `view` selector
/// (`flows`/`liveness`/`processes`/`contracts`/`signatures`/`orientation`/
/// `projection`/`changes`) threaded through when supplied (lane WIRE1;
/// see `crate::architecture_views::dispatch_view`). `None` or `"summary"`
/// keeps the existing BM03 task-orientation default.
pub fn architecture_orientation_view(
    repo_root: impl Into<String>,
    task_or_symbol: impl Into<String>,
    view: Option<&str>,
    deadline_ms: Option<u64>,
) -> Result<Value, BlueprintError> {
    let mut request = BlueprintRequest::new("cli-architecture", Operation::Architecture, repo_root);
    if let Some(deadline_ms) = deadline_ms {
        request.deadline_ms = deadline_ms;
    }
    if let Value::Object(map) = &mut request.input {
        map.insert("task".into(), Value::from(task_or_symbol.into()));
        if let Some(view) = view {
            map.insert("view".into(), Value::from(view));
        }
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

/// Read one named snapshot's identity (`blueprint snapshot_get`). Ported by
/// lane STORE1 (r5 windows closure) from `blueprint/src/graph/snapshots.mjs`'s
/// `getSnapshot` via `Operation::SnapshotGet`.
pub fn snapshot_get(repo_root: impl Into<String>, name: impl Into<String>, deadline_ms: Option<u64>) -> Result<Value, BlueprintError> {
    let input = serde_json::json!({"snapshot": name.into()});
    run_query("cli-snapshot-get", Operation::SnapshotGet, repo_root, input, deadline_ms)
}

/// List every named snapshot for this repository (`blueprint snapshot_list`).
/// Ported by lane STORE1 from `snapshots.mjs`'s `listSnapshots` via
/// `Operation::SnapshotList`.
pub fn snapshot_list(repo_root: impl Into<String>, deadline_ms: Option<u64>) -> Result<Value, BlueprintError> {
    run_query("cli-snapshot-list", Operation::SnapshotList, repo_root, serde_json::json!({}), deadline_ms)
}

/// Compute changes since a named snapshot, a generation id, or a git treeish
/// pair (`blueprint changes`). Ported by lane STORE1 from `snapshots.mjs`'s
/// `changesSinceReference` via `Operation::Changes`. Exactly one of
/// `snapshot`, `since_generation`, or `treeish` should be set, matching the
/// legacy `change_reference_required` contract.
pub fn changes(
    repo_root: impl Into<String>,
    snapshot: Option<&str>,
    since_generation: Option<&str>,
    treeish: Option<(&str, &str)>,
    limit: Option<u64>,
    deadline_ms: Option<u64>,
) -> Result<Value, BlueprintError> {
    let mut input = serde_json::json!({});
    if let Some(snapshot) = snapshot { input["snapshot"] = Value::from(snapshot); }
    if let Some(since_generation) = since_generation { input["sinceGeneration"] = Value::from(since_generation); }
    if let Some((base, head)) = treeish { input["treeish"] = serde_json::json!({"base": base, "head": head}); }
    if let Some(limit) = limit { input["limit"] = Value::from(limit); }
    run_query("cli-changes", Operation::Changes, repo_root, input, deadline_ms)
}

/// Route one bounded query across an explicit set of federated repositories
/// (`blueprint federate`). Ported by lane STORE1 from
/// `blueprint/src/lib/federation/index.mjs`'s `routeFederatedQuery` via
/// `Operation::Federate`. `repositories` is `{repoId, repoRoot, generation?}`
/// per entry; `operation` is one of `search`, `recall`, `impact`,
/// `architecture`.
pub fn federate(
    repo_root: impl Into<String>,
    repositories: Value,
    operation: impl Into<String>,
    query: Value,
    allowed_repo_ids: Option<Value>,
    deadline_ms: Option<u64>,
) -> Result<Value, BlueprintError> {
    let mut input = serde_json::json!({
        "repositories": repositories,
        "operation": operation.into(),
        "query": query,
    });
    if let Some(allowed) = allowed_repo_ids {
        input["allowedRepoIds"] = allowed;
    }
    run_query("cli-federate", Operation::Federate, repo_root, input, deadline_ms)
}

/// Create (or idempotently confirm) one named snapshot of the current
/// generation (`blueprint snapshot create`). Legacy exposes this only
/// through `blueprint.mjs`'s CLI `snapshot create` action, never through
/// `service.mjs`'s operation surface (no `Operation` variant exists for it),
/// so this bypasses `Operation` dispatch and writes the store directly,
/// matching the legacy CLI-only entry point.
pub fn snapshot_create(repo_root: impl Into<String>, name: impl Into<String>) -> Result<Value, BlueprintError> {
    let repo_root = repo_root.into();
    let db_path = std::path::Path::new(&repo_root).join(".agent").join("graph").join("graph.db");
    let mut connection = crate::store::open_store(Some(&db_path)).map_err(|error| BlueprintError::new("blueprint_store_unavailable", error.to_string()))?;
    crate::lib_application_snapshots::create_snapshot(&mut connection, &name.into(), std::path::Path::new(&repo_root))
}

// --- Native doctor / repair / support-bundle dispatch (lane V2, r5 closure) ---
//
// Parity target: `blueprint/scripts/cli/commands.mjs` `case "service"` (the
// `doctor`/`repair`/`support-bundle` paths) and
// `blueprint/src/lib/operations/{doctor,repair,support-bundle}.mjs`. Legacy
// diagnostics key off filesystem artifacts (`.agent/map.json`,
// `.agent/stale.json`); the native engine's durable state instead lives in
// the SQLite store at `.agent/graph/graph.db` (see
// `crate::engine::store_path`), so this dispatch checks native store
// readiness (open + `load_generation`) in place of the legacy map.json
// read, while reusing the legacy-shaped reason-code ladder
// (`missing_map`/`corrupt_map`/`mcp_config_launchable`) from
// `crate::lib_operations_doctor` and the ordered repair-plan builder from
// `crate::lib_operations_repair` unchanged. This keeps one native store
// owner (`engine::NativeBlueprintOperation`) and adds no second graph/store
// path.

/// Exit-code ladder a hosting binary should use for `blueprint doctor`,
/// mirroring the legacy CLI's `EXIT` constants for these paths.
pub mod doctor_exit {
    pub const OK: i32 = 0;
    pub const CONFIRMATION_REQUIRED: i32 = 3;
    pub const BROKEN: i32 = 1;
}

fn store_db_path(root: &std::path::Path) -> std::path::PathBuf {
    root.join(".agent").join("graph").join("graph.db")
}

/// Native store readiness, in place of legacy's `map.json`/`stale.json`
/// presence check. Returns `(state, errors, warnings, reasons)` using the
/// same state ladder (`missing|corrupt|degraded|broken`) and reason codes
/// (`missing_map`/`corrupt_map`) so downstream repair-plan/support-bundle
/// consumers are unaffected by the artifact-format change.
fn native_store_diagnostics(root: &std::path::Path) -> (String, Vec<String>, Vec<String>, Vec<Value>) {
    let db_path = store_db_path(root);
    if !db_path.exists() {
        return (
            "missing".to_string(),
            vec!["blueprint native store is not present; run build/refresh first".to_string()],
            vec![],
            vec![serde_json::json!({"code": "missing_map", "severity": "blocker", "message": "Blueprint native store is not present; planner cannot retrieve candidates."})],
        );
    }
    match crate::store::open_store_read_only(&db_path) {
        Ok(connection) => match crate::store::load_generation(&connection) {
            Ok(Some(_generation)) => ("degraded".to_string(), vec![], vec![], vec![]),
            Ok(None) => (
                "missing".to_string(),
                vec!["blueprint native store has no persisted generation; run build/refresh first".to_string()],
                vec![],
                vec![serde_json::json!({"code": "missing_map", "severity": "blocker", "message": "Blueprint native store has no persisted generation; planner cannot retrieve candidates."})],
            ),
            Err(error) => (
                "corrupt".to_string(),
                vec![error.to_string()],
                vec![],
                vec![serde_json::json!({"code": "corrupt_map", "severity": "blocker", "message": "Blueprint native store could not be read."})],
            ),
        },
        Err(error) => (
            "corrupt".to_string(),
            vec![error.to_string()],
            vec![],
            vec![serde_json::json!({"code": "corrupt_map", "severity": "blocker", "message": "Blueprint native store could not be opened."})],
        ),
    }
}

/// Native `blueprint doctor [--full] [--json]`. Mirrors
/// `collectDoctorDiagnostics(root, ".agent", { full })`'s typed state ladder
/// and reason codes, sourced from the native store instead of `map.json`
/// (see module docs above), plus the unchanged MCP-config launchability
/// probe (`crate::lib_operations_doctor::mcp_config_launchable`).
pub fn doctor(repo_root: impl Into<String>, full: bool) -> Value {
    let repo_root = repo_root.into();
    let root = std::path::Path::new(&repo_root);
    let generated_at = chrono_now_iso();
    let (mut state, mut errors, mut warnings, mut reasons) = native_store_diagnostics(root);

    // Only run the MCP launchability + fold-in-errors ladder when the store
    // itself is not already missing/corrupt, matching legacy's early return
    // for those two states.
    if state != "missing" && state != "corrupt" {
        match crate::lib_operations_doctor::mcp_config_launchable(root) {
            Some(crate::lib_operations_doctor::McpLaunchable::Pass { command, args }) => {
                reasons.push(serde_json::json!({"code": "mcp_config_launchable", "severity": "info", "status": "pass", "command": command, "args": args}));
            }
            Some(crate::lib_operations_doctor::McpLaunchable::Warning { message }) => {
                warnings.push(message.clone());
                reasons.push(serde_json::json!({"code": "mcp_config_launchable", "severity": "warning", "message": message}));
            }
            Some(crate::lib_operations_doctor::McpLaunchable::Fail { command, args, exit_code }) => {
                let message = format!(
                    "MCP server config for blueprint is not launchable: spawned command \"{command}\" with args {args:?} and it exited early with exit code {exit_code:?} before the {} ms liveness window; expected the server to stay alive waiting on stdio.",
                    crate::lib_operations_doctor::MCP_LIVENESS_MS
                );
                errors.push(message.clone());
                reasons.push(serde_json::json!({"code": "mcp_config_launchable", "severity": "blocker", "command": command, "args": args, "exitCode": exit_code, "message": message}));
            }
            None => {}
        }
        state = if !errors.is_empty() { "broken".to_string() } else { "degraded".to_string() };
    }

    serde_json::json!({
        "schemaVersion": 1,
        "state": state,
        "generatedAt": generated_at,
        // Keep legacy artifact names while exposing the native store name;
        // consumers can migrate from map.json without losing a stable key.
        "artifacts": {"map": ".agent/map.json", "graph": ".agent/graph/graph.db", "store": ".agent/graph/graph.db"},
        "errors": errors,
        "warnings": warnings,
        "reasons": reasons,
        "completion": if full { serde_json::json!({"checked": true}) } else { Value::Null },
    })
}

fn chrono_now_iso() -> String {
    // No chrono dependency in this crate; a millisecond-precision RFC3339
    // string built from SystemTime is sufficient for a generatedAt marker
    // (no downstream parity check compares this value to legacy output).
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let secs = now.as_secs();
    let millis = now.subsec_millis();
    let days = secs / 86_400;
    let time_of_day = secs % 86_400;
    let (h, m, s) = (time_of_day / 3600, (time_of_day % 3600) / 60, time_of_day % 60);
    // Civil-from-days (Howard Hinnant's algorithm) to avoid a chrono dependency.
    let z = days as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mth = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mth <= 2 { y + 1 } else { y };
    format!("{y:04}-{mth:02}-{d:02}T{h:02}:{m:02}:{s:02}.{millis:03}Z")
}

fn doctor_repair_reasons(doctor_json: &Value) -> Vec<crate::lib_operations_repair::DoctorReason> {
    doctor_json
        .get("reasons")
        .and_then(Value::as_array)
        .map(|reasons| {
            reasons
                .iter()
                .filter_map(|r| {
                    let code = r.get("code")?.as_str()?.to_string();
                    let severity = r.get("severity")?.as_str()?.to_string();
                    Some(crate::lib_operations_repair::DoctorReason { code, severity })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Native `blueprint doctor --repair-plan --json`. Mirrors
/// `buildRepairPlan({ root, outDir, graphState, reasons })`, driven by this
/// module's own [`doctor`] output.
pub fn repair_plan(repo_root: impl Into<String>) -> Value {
    let repo_root = repo_root.into();
    let diagnostics = doctor(repo_root.clone(), false);
    let graph_state = diagnostics.get("state").and_then(Value::as_str).unwrap_or("unknown").to_string();
    let reasons = doctor_repair_reasons(&diagnostics);
    let plan = crate::lib_operations_repair::build_repair_plan(&repo_root, ".agent", &graph_state, &reasons);
    serde_json::json!({
        "schemaVersion": plan.schema_version,
        "root": plan.root,
        "graphState": plan.graph_state,
        "actions": plan.actions.iter().map(|a| serde_json::json!({
            "id": a.id,
            "kind": a.kind,
            "command": a.command,
            "reversible": a.reversible,
            "details": {"reason": a.reason},
        })).collect::<Vec<_>>(),
    })
}

/// Native `blueprint doctor --repair-plan --apply-repair [--yes] --json`.
/// Without `yes`, mirrors legacy's `confirmation_required` refusal (exit
/// code 3, see [`doctor_exit::CONFIRMATION_REQUIRED`]). With `yes`, executes
/// each actionable plan step through the same native operations the rest of
/// this module already exposes (`rebuild-graph` -> [`manual_refresh`]);
/// steps with no native equivalent yet (`reconcile-watcher`,
/// `regenerate-docs`) are reported as `skipped` rather than silently
/// dropped or falsely claimed applied.
pub fn apply_repair(repo_root: impl Into<String>, yes: bool) -> Result<Value, BlueprintError> {
    if !yes {
        return Err(BlueprintError::new("confirmation_required", "apply-repair requires explicit confirmation; pass yes"));
    }
    let repo_root = repo_root.into();
    let plan = repair_plan(repo_root.clone());
    let actions = plan.get("actions").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut applied = Vec::new();
    for action_value in actions {
        let id = action_value.get("id").and_then(Value::as_str).unwrap_or("").to_string();
        match id.as_str() {
            "rebuild-graph" => {
                // A corrupt store file (not just a missing/stale one) fails
                // the native store's own open/migrate path outright -- it
                // cannot be repaired in place. Quarantine the unreadable
                // file (never silently delete it: it stays on disk as
                // evidence) so the rebuild below can create a fresh store,
                // matching this action's own `reversible: false` marking.
                let db_path = std::path::Path::new(&repo_root).join(".agent").join("graph").join("graph.db");
                if db_path.exists() && crate::store::open_store_read_only(&db_path).and_then(|c| crate::store::load_generation(&c)).is_err() {
                    let quarantine = db_path.with_extension(format!("db.corrupt-{}", chrono_now_iso().replace([':', '.'], "-")));
                    let _ = std::fs::rename(&db_path, &quarantine);
                }
                match manual_refresh(repo_root.clone(), None) {
                    Ok(_result) => applied.push(serde_json::json!({"id": id, "status": "applied"})),
                    Err(error) => applied.push(serde_json::json!({"id": id, "status": "failed", "error": {"code": error.code, "message": error.message}})),
                }
            }
            "no-op" => applied.push(serde_json::json!({"id": id, "status": "applied"})),
            _ => applied.push(serde_json::json!({"id": id, "status": "skipped", "reason": "no native operation ported for this repair action yet"})),
        }
    }
    Ok(serde_json::json!({"schemaVersion": plan.get("schemaVersion").cloned().unwrap_or(Value::from(1)), "root": repo_root, "applied": applied}))
}

/// Native `blueprint service support-bundle`. Assembles the current
/// `doctor`/`repair-plan` output plus store presence into the legacy
/// allowlisted, redacted bundle via
/// [`crate::lib_operations_support_bundle::build_support_bundle`].
pub fn support_bundle(repo_root: impl Into<String>, destination: Option<&std::path::Path>) -> Result<Value, BlueprintError> {
    let repo_root = repo_root.into();
    let root = std::path::Path::new(&repo_root);
    let doctor_value = doctor(repo_root.clone(), false);
    let repair_value = repair_plan(repo_root.clone());
    let bundle_root = destination.map(std::path::Path::to_path_buf).unwrap_or_else(|| root.join(".agent").join("support-bundle"));
    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_default();
    let records = crate::lib_operations_support_bundle::SupportBundleRecords {
        package_channel: None,
        installation: None,
        service_status: None,
        repository_status: None,
        doctor: Some(doctor_value),
        repair_plan: Some(repair_value),
        watchman_log: None,
        service_log: None,
    };
    let platform_arch = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    let result = crate::lib_operations_support_bundle::build_support_bundle(root, &bundle_root, &home, &chrono_now_iso(), &platform_arch, &records)
        .map_err(|error| BlueprintError::new("blueprint_support_bundle_failed", error.to_string()))?;
    Ok(serde_json::json!({
        "path": result.path.to_string_lossy(),
        "files": result.files,
        "checksums": result.checksums.into_iter().map(|(name, hash)| serde_json::json!({"name": name, "sha256": hash})).collect::<Vec<_>>(),
    }))
}

/// Native `blueprint docs` (CLI parity, `commands.mjs:175` case `"docs"`).
/// Legacy calls `service.documentTruth({...common, limit})`, the exact same
/// operation the MCP-facing path already dispatches through
/// `Operation::DocumentTruth`; this is a thin CLI-shaped wrapper adding no
/// second document-truth path.
pub fn docs(repo_root: impl Into<String>, limit: Option<u64>, deadline_ms: Option<u64>) -> Result<Value, BlueprintError> {
    let repo_root = repo_root.into();
    // A read-only SQLite open reports an OS error when graph.db is absent;
    // docs contract exposes this as typed `blueprint_store_missing`.
    if !std::path::Path::new(&repo_root).join(".agent").join("graph").join("graph.db").exists() {
        return Err(BlueprintError::new("blueprint_store_missing", "no persisted Blueprint generation exists"));
    }
    let mut input = serde_json::json!({});
    if let Some(limit) = limit {
        input["limit"] = Value::from(limit);
    }
    let mut result = run_query("cli-docs", Operation::DocumentTruth, repo_root, input, deadline_ms)?;
    // The legacy projection advertises its stable kind marker. Keep this
    // adapter-level marker here because `Operation::DocumentTruth` is also
    // consumed by non-CLI callers whose response contract predates docs.
    if let Value::Object(object) = &mut result {
        object.insert("kind".into(), Value::String("document-truth-grounding".into()));
    }
    Ok(result)
}

/// Native `blueprint languages` (CLI parity, `commands.mjs:453` case
/// `"languages"`). Legacy forwards to `language-registry.mjs`'s
/// `languagesJson()`, pure static catalog data with no repository/store
/// dependency; delegates to `crate::lib_cli_languages::languages_json`.
pub fn languages() -> Value {
    crate::lib_cli_languages::languages_json()
}

/// Native `blueprint rules [check|baseline|explain]` (CLI parity,
/// `commands.mjs:458` case `"rules"`). Delegates to
/// `crate::lib_cli_rules::run`, returning the legacy typed error code/exit
/// class on failure via `BlueprintError`.
pub fn rules(repo_root: impl Into<String>, subcommand: Option<&str>) -> Result<Value, BlueprintError> {
    let repo_root = repo_root.into();
    crate::lib_cli_rules::run(std::path::Path::new(&repo_root), subcommand)
        .map_err(|error| BlueprintError::new(error.code(), error.message()))
}

/// Native `blueprint mcp [print|install]` (CLI parity, `commands.mjs:482`
/// case `"mcp"`). Legacy's only subcommand, `mcp serve`, stands up the
/// superseded legacy JS server (see `crate::lib_cli_mcp` module docs for
/// why that is intentionally not ported); this exposes the native stdio
/// server declaration instead, printing it or installing it into the
/// allowlisted JSON host config files via `crate::lib_cli_mcp::run`.
pub fn mcp(repo_root: impl Into<String>, subcommand: Option<&str>) -> Result<Value, BlueprintError> {
    let repo_root = repo_root.into();
    crate::lib_cli_mcp::run(std::path::Path::new(&repo_root), subcommand)
        .map_err(|error| BlueprintError::new(error.code(), format!("{error:?}")))
}

/// Native `blueprint uninstall` (CLI parity, `commands.mjs:217` case
/// `"uninstall"`). Delegates to `crate::lib_cli_uninstall::run`, which
/// already returns the exact legacy `uninstallInit` JSON result shape
/// (`ok`, `action`, `restored`, `idempotent`, typed `error`), so this
/// wrapper returns it unchanged rather than re-wrapping it as a
/// `BlueprintError` on failure — legacy's `uninstall` CLI case prints the
/// result either way and maps `result.ok` to the process exit code.
pub fn uninstall(repo_root: impl Into<String>) -> Value {
    let repo_root = repo_root.into();
    crate::lib_cli_uninstall::run(std::path::Path::new(&repo_root))
}
