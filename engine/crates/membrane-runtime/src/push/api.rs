//! Same operation owner for native MCP and authenticated resident HTTP.
use serde_json::{json, Value};
use super::{delivery, recovery::{self, RecoveryError, RecoveryScope, RecoveryStore, Selector}};

fn failure(operation: &str, code: &str) -> Value {
    json!({"schemaVersion":1,"operation":operation,"errorVersion":1,
        "result":{"kind":"error","code":code,"message":code,"retryable":false}})
}
pub fn execute(operation: &str, arguments: &Value) -> Value {
    let result = (|| -> Result<Value, String> {
        let caller = arguments.get("caller").ok_or("caller_required")?;
        let root = caller.get("root").and_then(Value::as_str).ok_or("caller_required")?;
        let repository = caller.get("repositoryId").and_then(Value::as_str).ok_or("caller_required")?;
        let session = caller.get("scopeId").and_then(Value::as_str).ok_or("caller_required")?;
        if arguments.get("repository").and_then(Value::as_str) != Some(repository) {
            return Err("caller_scope_binding_denied".into());
        }
        crate::authorization::authorize(&crate::authorization::AuthorizationRequest {
            caller_root:root, caller_repository_id:repository, caller_scope_id:session,
            caller_scope_descriptor:caller.get("scopeDescriptor"), target_repository:repository,
            task_grant_level:arguments.get("taskGrantLevel").and_then(Value::as_str), action:"source_read",
        }).map_err(|denial| denial.code().to_owned())?;
        let scope = RecoveryScope::new(std::path::Path::new(root), session).map_err(|e| e.to_string())?;
        let store = RecoveryStore::configured();
        let data = match operation {
            "membrane_push_prepare" => {
                let request = serde_json::from_value(arguments.get("request").cloned().ok_or("push_request_required")?)
                    .map_err(|_| "push_invalid_request")?;
                serde_json::to_value(delivery::prepare(&store, &scope, request).map_err(|e| e.to_string())?).map_err(|_| "push_serialization_failed")?
            }
            "membrane_push_resolve" => match arguments.get("operation").and_then(Value::as_str).unwrap_or("resolve") {
                "probe" => delivery::resolver_probe(&store, &scope).map_err(|e| e.to_string())?,
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
                    serde_json::to_value(store.resolve(&scope, handle, &selector, max, recovery::now_ms()).map_err(|e| e.to_string())?).map_err(|_| "push_serialization_failed")?
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
        Ok(data) => json!({"schemaVersion":1,"operation":operation,"errorVersion":1,"result":{"kind":"success","data":data}}),
        Err(code) => failure(operation, &code),
    }
}
pub fn http_response(operation: &str, body: &str) -> (u16, String) {
    let args: Value = match serde_json::from_str(body) {
        Ok(value) => value, Err(_) => return (400, json!({"error":"push_invalid_request"}).to_string()),
    };
    let result = execute(operation, &args);
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
