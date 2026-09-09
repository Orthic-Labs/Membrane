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
    let (method, root, input) = cli_request(args)?;
    let request_id = format!(
        "blueprint-cli-{}-{}",
        std::process::id(),
        crate::time::now_millis()
    );
    let mut request = BlueprintRequest::new(request_id, method, root.to_string_lossy());
    request.input = input;
    let response = dispatch_native(request, CancellationToken::new());
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

fn cli_request(args: &[String]) -> Result<(Operation, PathBuf, Value), String> {
    let Some(verb) = args.first() else {
        return Err("native Blueprint operation is required".into());
    };
    let method = Operation::parse(verb)
        .ok_or_else(|| format!("unsupported native Blueprint operation: {verb}"))?;
    let mut root = std::env::current_dir().map_err(|error| format!("resolve repository root: {error}"))?;
    let mut input = json!({"repoRoot": root.to_string_lossy()});
    let mut positional = Vec::new();
    let mut index = 1;
    while index < args.len() {
        let key = &args[index];
        if key == "--repo-root" {
            index += 1;
            root = PathBuf::from(args.get(index).ok_or("--repo-root requires a path")?);
        } else if let Some(field) = key.strip_prefix("--") {
            index += 1;
            let value = args.get(index).ok_or_else(|| format!("{key} requires a value"))?;
            match field {
                "query" => {
                    input["query"] = Value::String(value.clone());
                    input["task"] = Value::String(value.clone());
                }
                "task" => input["task"] = Value::String(value.clone()),
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
    Ok((method, root, input))
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
        let (_, _, input) = cli_request(&[
            "impact".into(), "--repo-root".into(), root, "--node".into(), "exact".into(),
            "--limit".into(), "7".into(), "--depth".into(), "2".into(),
            "--budget".into(), "4096".into(), "--max-paths".into(), "9".into(),
        ]).unwrap();
        assert_eq!(input["nodeId"], "exact");
        assert_eq!(input["maxCandidates"], 7);
        assert_eq!(input["maxSeeds"], 7);
        assert_eq!(input["maxDepth"], 2);
        assert_eq!(input["maxBytes"], 4096);
        assert_eq!(input["maxPaths"], 9);
    }
}
