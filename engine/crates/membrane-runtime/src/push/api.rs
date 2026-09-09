//! Same operation owner for native MCP and authenticated resident HTTP.
use serde_json::{json, Value};
use super::{delivery, recovery::{self, RecoveryError, RecoveryScope, RecoveryStore, Selector}};
use membrane_federation::deadline::{Deadline, SystemClock};
use tokio_util::sync::CancellationToken;

const MAX_PUSH_TEXT_BYTES: usize = 1 * 1024 * 1024;

fn failure(operation: &str, code: &str) -> Value {
    json!({"schemaVersion":1,"operation":operation,"errorVersion":1,
        "result":{"kind":"error","code":code,"message":code,"retryable":false}})
}
pub fn execute(operation: &str, arguments: &Value) -> Value {
    execute_with_control(operation, arguments,
        Deadline::after(&SystemClock, std::time::Duration::from_secs(120)),
        &CancellationToken::new())
}

pub fn execute_with_control(operation: &str, arguments: &Value,
    deadline: Deadline, cancellation: &CancellationToken) -> Value {
    let ceiling = if arguments.get("remainingContextCeiling").is_some() {
        let request_session = arguments.pointer("/sessionId").and_then(Value::as_str).filter(|id| !id.trim().is_empty());
        let task_id = arguments.pointer("/taskId").and_then(Value::as_str).filter(|id| !id.trim().is_empty());
        match (request_session, task_id) {
            (Some(request_session), Some(task_id)) => crate::push::selection::parse_request_time_h8(arguments, request_session, task_id).ok(),
            _ => None,
        }
    } else { None };
    let result = (|| -> Result<Value, String> {
        if cancellation.is_cancelled() || deadline.is_exhausted(&SystemClock) {
            return Err(RecoveryError::Cancelled.to_string());
        }
        if serde_json::to_vec(arguments).map_err(|_| "push_invalid_request")?.len()
            > membrane_protocol::explicit::EXPLICIT_MAX_BYTES
        {
            return Err("push_resource_limit".into());
        }
        if operation == "membrane_push_prepare"
            && arguments.pointer("/request/text").and_then(Value::as_str)
                .is_some_and(|text| text.len() > MAX_PUSH_TEXT_BYTES)
        {
            return Err("push_input_limit".into());
        }
        let caller = arguments.get("caller").ok_or("caller_required")?;
        let root = caller.get("root").and_then(Value::as_str).ok_or("caller_required")?;
        let repository = caller.get("repositoryId").and_then(Value::as_str).ok_or("caller_required")?;
        let session = caller.get("scopeId").and_then(Value::as_str).ok_or("caller_required")?;
        let resolve_operation = arguments.get("operation").and_then(Value::as_str).unwrap_or("resolve");
        let probe_identity_supplied = operation == "membrane_push_resolve"
            && resolve_operation == "probe"
            && (arguments.get("taskId").is_some() || arguments.get("sessionId").is_some());
        let requires_identity = operation == "membrane_push_prepare"
            || (operation == "membrane_push_resolve" && resolve_operation != "probe")
            || probe_identity_supplied;
        let task_id = if requires_identity {
            let task_id = arguments.get("taskId").and_then(Value::as_str).filter(|id| !id.trim().is_empty()).ok_or("push_task_required")?;
            let request_session = arguments.get("sessionId").and_then(Value::as_str).filter(|id| !id.trim().is_empty()).ok_or("push_session_required")?;
            // When a request-time ceiling (H8) is supplied, its own identity/
            // freshness validation names the actual defect (an H8 whose bound
            // session/task/staleness does not hold) more precisely than the
            // raw scope-binding comparison below. Check it first so a request
            // carrying an invalid H8 is refused as `push_h8_invalid` rather
            // than the coarser `push_session_binding_denied`, which is
            // reserved for identity mismatches on requests that carry no H8
            // at all.
            if arguments.get("remainingContextCeiling").is_some() && ceiling.is_none() {
                return Err("push_h8_invalid".into());
            }
            if request_session != session { return Err("push_session_binding_denied".into()); }
            Some(task_id)
        } else { None };
        if arguments.get("repository").and_then(Value::as_str) != Some(repository) {
            return Err("caller_scope_binding_denied".into());
        }
        crate::authorization::authorize(&crate::authorization::AuthorizationRequest {
            caller_root:root, caller_repository_id:repository, caller_scope_id:session,
            caller_scope_descriptor:caller.get("scopeDescriptor"), target_repository:repository,
            task_grant_level:arguments.get("taskGrantLevel").and_then(Value::as_str), action:"source_read",
        }).map_err(|denial| denial.code().to_owned())?;
        let scope = match task_id {
            Some(task_id) => RecoveryScope::new_for_task(std::path::Path::new(root), task_id, session),
            None => RecoveryScope::new(std::path::Path::new(root), session),
        }.map_err(|e| e.to_string())?;
        let store = RecoveryStore::configured();
        let data = match operation {
            "membrane_push_prepare" => {
                let request = serde_json::from_value(arguments.get("request").cloned().ok_or("push_request_required")?)
                    .map_err(|_| "push_invalid_request")?;
                serde_json::to_value(delivery::prepare_with_control(&store, &scope, request, deadline, cancellation).map_err(|e| e.to_string())?).map_err(|_| "push_serialization_failed")?
            }
            "membrane_push_resolve" => match arguments.get("operation").and_then(Value::as_str).unwrap_or("resolve") {
                "probe" => {
                    if cancellation.is_cancelled() || deadline.is_exhausted(&SystemClock) { return Err(RecoveryError::Cancelled.to_string()); }
                    delivery::resolver_probe(&store, &scope).map_err(|e| e.to_string())?
                },
                "resolve" => {
                    let handle = arguments.get("handle").or_else(|| arguments.get("anchor")).and_then(Value::as_str).ok_or("push_handle_required")?;
                    let selector: Selector = match arguments.get("selector") {
                        Some(value) => serde_json::from_value(value.clone()).map_err(|_| "push_invalid_selector")?,
                        None => Selector::Whole,
                    };
                    let max = match arguments.get("maxBytes") {
                        Some(value) => value.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or("push_invalid_limit")?,
                        None => 16 * 1024,
                    };
                    serde_json::to_value(store.resolve_with_control(&scope, handle, &selector, max, recovery::now_ms(), deadline, cancellation).map_err(|e| e.to_string())?).map_err(|_| "push_serialization_failed")?
                }
                _ => return Err("push_invalid_operation".into()),
            },
            _ => return Err("push_unknown_operation".into()),
        };
        // Native transport is bounded to 128 KiB including JSON-RPC. Leave
        // headroom for both MCP content and structuredContent serialization.
        if serde_json::to_vec(&data).map_err(|_| "push_serialization_failed")?.len() > 48 * 1024 {
            return Err(RecoveryError::Limit.to_string());
        }
        Ok(data)
    })();
    match result {
        Ok(data) => {
            let envelope = json!({"schemaVersion":1,"operation":operation,"errorVersion":1,"result":{"kind":"success","data":data}});
            if let Some(ceiling) = ceiling {
                match crate::push::egress::fit_native_response(envelope, &ceiling) {
                    Ok(fitted) => fitted,
                    Err(error) => failure(operation, &error.to_string()),
                }
            } else if membrane_mcp::tool_result(envelope.clone()).to_string().len() > 120 * 1024 {
                failure(operation,"push_resource_limit")
            } else { envelope }
        },
        Err(code) => failure(operation, &code),
    }
}
pub fn http_response(operation: &str, body: &str) -> (u16, String) {
    http_response_with_control(operation, body,
        Deadline::after(&SystemClock, std::time::Duration::from_secs(120)),
        &CancellationToken::new())
}
pub fn http_response_with_control(operation: &str, body: &str,
    deadline: Deadline, cancellation: &CancellationToken) -> (u16, String) {
    let args: Value = match serde_json::from_str(body) {
        Ok(value) => value, Err(_) => return (400, json!({"error":"push_invalid_request"}).to_string()),
    };
    let result = execute_with_control(operation, &args, deadline, cancellation);
    let status = if result.pointer("/result/kind").and_then(Value::as_str) == Some("success") { 200 }
    else {
        match result.pointer("/result/code").and_then(Value::as_str).unwrap_or("") {
            "push_artifact_not_found" | "push_selector_miss" => 404,
            "push_artifact_expired" | "push_artifact_invalidated" => 410,
            "push_resource_limit" => 413,
            "push_artifact_corrupt" => 409,
            "push_store_unavailable" => 503,
            code if code.contains("denied") => 403,
            _ => 400,
        }
    };
    (status, result.to_string())
}
