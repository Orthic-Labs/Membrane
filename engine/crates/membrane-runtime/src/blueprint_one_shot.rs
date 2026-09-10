//! Holder-independent native Blueprint execution.
//!
//! Explicit CLI and MCP work dispatch directly to Blueprint's bounded
//! in-process operation. This module never discovers an endpoint or starts
//! an interpreter-backed child.

use membrane_blueprint::{
    native_blueprint_operation, BlueprintApi, BlueprintRequest, BlueprintResponse,
    CancellationToken, OneShotExecutor, Operation,
};
use serde_json::{json, Value};
use std::path::PathBuf;

/// Execute one explicit request without acquiring resident authority.
pub(crate) fn dispatch_native(
    request: BlueprintRequest,
    cancellation: CancellationToken,
) -> BlueprintResponse {
    OneShotExecutor::new(native_blueprint_operation()).dispatch(request, cancellation)
}

/// Run a native Blueprint CLI verb from current repository root.
pub(crate) fn run_cli(args: &[String]) -> Result<(), String> {
    let (method, root, input, cancel_before_dispatch, deadline_ms) = cli_request_with_deadline(args)?;
    let request_id = format!(
        "blueprint-cli-{}-{}",
        std::process::id(),
        crate::time::now_millis()
    );
    let mut request = BlueprintRequest::new(request_id, method, root.to_string_lossy());
    request.deadline_ms = deadline_ms;
    request.input = input;
    let cancellation = CancellationToken::new();
    if cancel_before_dispatch {
        cancellation.cancel();
    }
    let response = dispatch_native(request, cancellation);
    if response.ok {
        println!(
            "{}",
            serde_json::to_string(&response.result)
                .map_err(|error| format!("encode Blueprint response: {error}"))?
        );
        return Ok(());
    }
    let error = response
        .error
        .map(|error| format!("{}: {}", error.code, error.message))
        .unwrap_or_else(|| "native Blueprint request failed".into());
    Err(error)
}

/// Parse a native Blueprint CLI verb and its arguments into a dispatchable
/// request, additionally returning the deadline (in
/// milliseconds) this invocation should run with. An explicit `--deadline-ms`
/// always wins. Absent an override, `Operation::Refresh` uses the same
/// extended cap the resident watcher path already applies to refresh work
/// (`membrane-blueprint/src/service.rs::execute_refresh_event`) rather than
/// the short interactive-query default: an explicit CLI refresh performs the
/// same real graph rebuild/publish as a watcher-triggered refresh, so the
/// short default (tuned for point queries like `status`/`search`) starves it
/// before it can finish (LC-02: installed refresh against a real repository
/// observed at ~2.1s wall time against a 2000ms/`DEFAULT_DEADLINE_MS`
/// deadline, returning `deadline_exceeded` every time).
fn cli_request_with_deadline(args: &[String]) -> Result<(Operation, PathBuf, Value, bool, u64), String> {
    let Some(verb) = args.first() else {
        return Err("native Blueprint operation is required".into());
    };
    let method = Operation::parse(verb)
        .ok_or_else(|| format!("unsupported native Blueprint operation: {verb}"))?;
    let mut root = std::env::current_dir().map_err(|error| format!("resolve repository root: {error}"))?;
    let mut input = json!({"repoRoot": root.to_string_lossy()});
    let mut positional = Vec::new();
    let mut cancel_before_dispatch = false;
    let mut deadline_ms: Option<u64> = None;
    let mut index = 1;
    while index < args.len() {
        let key = &args[index];
        if key == "--repo-root" {
            index += 1;
            root = PathBuf::from(args.get(index).ok_or("--repo-root requires a path")?);
        } else if key == "--deadline-ms" {
            index += 1;
            let value = args.get(index).ok_or("--deadline-ms requires a value")?;
            deadline_ms = Some(parse_cli_u64("deadline-ms", value)?);
        } else if key == "--cancel-before-dispatch" {
            cancel_before_dispatch = true;
        } else if key == "--allow-stale" {
            input["allowStale"] = Value::Bool(true);
        } else if key == "--dry-run" {
            input["dryRun"] = Value::Bool(true);
        } else if key == "--offline" {
            input["offline"] = Value::Bool(true);
        } else if key == "--update-checks-disabled" {
            input["updateChecksDisabled"] = Value::Bool(true);
        } else if let Some(field) = key.strip_prefix("--") {
            index += 1;
            let value = args.get(index).ok_or_else(|| format!("{key} requires a value"))?;
            match field {
                "query" => {
                    input["query"] = Value::String(value.clone());
                    input["task"] = Value::String(value.clone());
                }
                "task" => input["task"] = Value::String(value.clone()),
                "generation" | "baseline-generation" => {
                    input[if field == "generation" { "generation" } else { "baselineGeneration" }] = Value::String(value.clone());
                }
                "node" => input["nodeId"] = Value::String(value.clone()),
                "seed" | "target" | "from" | "to" | "direction" => {
                    input[field] = Value::String(value.clone());
                }
                "limit" => {
                    let value = parse_cli_u64(field, value)?;
                    input["maxCandidates"] = Value::from(value);
                    input["maxSeeds"] = Value::from(value);
                }
                "depth" => input["maxDepth"] = Value::from(parse_cli_u64(field, value)?),
                "budget" => input["maxBytes"] = Value::from(parse_cli_u64(field, value)?),
                "max-paths" => input["maxPaths"] = Value::from(parse_cli_u64(field, value)?),
                // `blueprint init` (Operation::Init; commands.mjs case "init").
                "host" | "scope" | "mcp" | "watch" | "hooks" | "policy" => {
                    input[field] = Value::String(value.clone());
                }
                // `blueprint update` (Operation::Update; commands.mjs case "update").
                "channel" | "artifact-dir" | "current-version" => {
                    input[match field {
                        "artifact-dir" => "artifactDir",
                        "current-version" => "currentVersion",
                        other => other,
                    }] = Value::String(value.clone());
                }
                "manifest" => {
                    let raw = std::fs::read_to_string(value)
                        .map_err(|error| format!("--manifest: read {value}: {error}"))?;
                    input["manifest"] = serde_json::from_str(&raw)
                        .map_err(|error| format!("--manifest: parse {value}: {error}"))?;
                }
                "rollback" => {
                    let raw = std::fs::read_to_string(value)
                        .map_err(|error| format!("--rollback: read {value}: {error}"))?;
                    input["rollback"] = serde_json::from_str(&raw)
                        .map_err(|error| format!("--rollback: parse {value}: {error}"))?;
                }
                _ => return Err(format!("unsupported native Blueprint option: {key}")),
            }
        } else {
            positional.push(key.clone());
        }
        index += 1;
    }
    let root = root
        .canonicalize()
        .map_err(|error| format!("canonicalize repository root: {error}"))?;
    input["repoRoot"] = Value::String(root.to_string_lossy().into_owned());
    if let Some(value) = positional.first().cloned() {
        match method {
            Operation::Search => {
                input["query"] = Value::String(value.clone());
                input["task"] = Value::String(value);
            }
            Operation::Recall | Operation::Architecture => input["task"] = Value::String(value),
            Operation::Resolve => input["nodeId"] = Value::String(value),
            Operation::Expand | Operation::Impact => {
                input["seed"] = Value::String(value.clone());
                input["target"] = Value::String(value);
            }
            _ => input["args"] = Value::Array(positional.into_iter().map(Value::String).collect()),
        }
    }
    let deadline_ms = deadline_ms.unwrap_or_else(|| default_deadline_ms(method));
    Ok((method, root, input, cancel_before_dispatch, deadline_ms))
}

/// Default deadline (ms) for a native CLI operation absent an explicit
/// `--deadline-ms`. Point queries (`status`, `search`, `recall`, ...) keep
/// the protocol's short interactive default. `Build` performs a complete
/// graph scan, while `Refresh` performs the same
/// real rebuild-and-publish graph work as the resident watcher's refresh
/// path, which already runs under `model::MAX_DEADLINE_MS` rather than the
/// interactive default (see `membrane-blueprint/src/service.rs`,
/// `execute_refresh_event`) — an explicit CLI refresh gets the same cap for
/// the same reason.
fn default_deadline_ms(method: Operation) -> u64 {
    match method {
        // Build performs a complete source scan and durable graph publish;
        // match resident startup's bounded build cap instead of starving the
        // first explicit Hub-off build with the point-query default.
        Operation::Build => membrane_blueprint::model::MAX_BUILD_DEADLINE_MS,
        Operation::Refresh => membrane_blueprint::model::MAX_DEADLINE_MS,
        _ => membrane_blueprint::model::DEFAULT_DEADLINE_MS,
    }
}

fn parse_cli_u64(field: &str, value: &str) -> Result<u64, String> {
    value
        .parse()
        .map_err(|_| format!("--{field} requires an unsigned integer"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_query_options_use_native_query_fields() {
        let root = std::env::current_dir().unwrap().to_string_lossy().into_owned();
        let (_, _, input, cancel_before_dispatch, deadline_ms) = cli_request_with_deadline(&[
            "impact".into(), "--repo-root".into(), root, "--node".into(), "exact".into(),
            "--limit".into(), "7".into(), "--depth".into(), "2".into(),
            "--budget".into(), "4096".into(), "--max-paths".into(), "9".into(),
        ]).unwrap();
        assert!(!cancel_before_dispatch);
        assert_eq!(input["nodeId"], "exact");
        assert_eq!(input["maxCandidates"], 7);
        assert_eq!(input["maxSeeds"], 7);
        assert_eq!(input["maxDepth"], 2);
        assert_eq!(input["maxBytes"], 4096);
        assert_eq!(input["maxPaths"], 9);
        assert_eq!(deadline_ms, membrane_blueprint::model::DEFAULT_DEADLINE_MS);
    }

    #[test]
    fn cli_cancel_before_dispatch_is_out_of_band_control() {
        let root = std::env::current_dir().unwrap().to_string_lossy().into_owned();
        let (_, _, input, cancel_before_dispatch, _deadline_ms) = cli_request_with_deadline(&[
            "recall".into(), "--repo-root".into(), root, "--task".into(), "exact".into(),
            "--cancel-before-dispatch".into(),
        ]).unwrap();
        assert!(cancel_before_dispatch);
        assert_eq!(input["task"], "exact");
        assert!(input.get("cancel-before-dispatch").is_none());
    }

    /// LC-02: an explicit CLI `refresh` performs the same real
    /// rebuild-and-publish graph work as the resident watcher's refresh path
    /// (`membrane-blueprint/src/service.rs::execute_refresh_event`), which
    /// already runs under the extended `MAX_DEADLINE_MS` cap rather than the
    /// short interactive-query default. Absent an explicit `--deadline-ms`,
    /// the CLI must apply the same extended cap — the installed CLI's prior
    /// default (`DEFAULT_DEADLINE_MS` == 2000ms) measured against this real
    /// repository returned `deadline_exceeded` at ~2.1s wall time on every
    /// observed run.
    #[test]
    fn cli_refresh_defaults_to_extended_deadline_not_interactive_default() {
        let root = std::env::current_dir().unwrap().to_string_lossy().into_owned();
        let (method, _, _, _, deadline_ms) =
            cli_request_with_deadline(&["refresh".into(), "--repo-root".into(), root]).unwrap();
        assert_eq!(method, Operation::Refresh);
        assert_eq!(deadline_ms, membrane_blueprint::model::MAX_DEADLINE_MS);
    }

    #[test]
    fn cli_build_defaults_to_bounded_build_deadline() {
        let root = std::env::current_dir().unwrap().to_string_lossy().into_owned();
        let (method, _, _, _, deadline_ms) =
            cli_request_with_deadline(&["build".into(), "--repo-root".into(), root]).unwrap();
        assert_eq!(method, Operation::Build);
        assert_eq!(deadline_ms, membrane_blueprint::model::MAX_BUILD_DEADLINE_MS);
    }

    /// An explicit `--deadline-ms` always overrides the operation default,
    /// for both the extended-default `refresh` path and the short-default
    /// interactive path.
    #[test]
    fn cli_explicit_deadline_ms_overrides_operation_default() {
        let root = std::env::current_dir().unwrap().to_string_lossy().into_owned();
        let (_, _, _, _, deadline_ms) = cli_request_with_deadline(&[
            "refresh".into(), "--repo-root".into(), root.clone(), "--deadline-ms".into(), "5000".into(),
        ]).unwrap();
        assert_eq!(deadline_ms, 5000);

        let (_, _, _, _, deadline_ms) = cli_request_with_deadline(&[
            "status".into(), "--repo-root".into(), root, "--deadline-ms".into(), "12000".into(),
        ]).unwrap();
        assert_eq!(deadline_ms, 12000);
    }
}
