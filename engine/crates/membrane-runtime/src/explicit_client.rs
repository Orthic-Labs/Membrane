//! One request to the installed owner; no socket, resident startup, or replay.
use membrane_protocol::explicit::{ExplicitOperation, ExplicitOwnerBindingV1, ExplicitOwnerMode,
    ExplicitRequestV1, ExplicitResponseV1, EXPLICIT_MAX_BYTES};
use serde_json::{json, Value};
use std::io::{Read, Write};

fn owner_binding(store: &crate::MemoryStore) -> Result<ExplicitOwnerBindingV1, String> {
    let runtime = crate::service::runtime_from_installed_exe(
        &std::env::current_exe().map_err(|e| e.to_string())?)?;
    let paths = crate::installation_identity::InstallationPaths::defaults_for_workspace(&runtime.workspace_root);
    let identity: crate::installation_identity::InstallationIdentity =
        serde_json::from_slice(&std::fs::read(&paths.identity).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if identity.schema_version != 2 || identity.installation_id != store.installation_id() {
        return Err("installed owner identity changed or is incompatible".into());
    }
    crate::installation_identity::assert_installation_not_quarantined(&paths.mirror, &identity.installation_id)
        .map_err(|e| e.to_string())?;
    Ok(ExplicitOwnerBindingV1 {
        schema_version: 1, mode: ExplicitOwnerMode::BoundedExplicit,
        installation_id: identity.installation_id, cortex_store_id: store.cortex_store_id(),
        release_generation: crate::release_identity::release_generation().to_string(),
        startup_generation: identity.startup_generation,
        stable_install_root: runtime.stable_current.ok_or("installed current missing")?.to_string_lossy().into_owned(),
        protocol_version: 1, native_only: true,
        subsystems: ["pull", "push", "cortex", "blueprint", "ledger", "adapt"].map(str::to_owned).to_vec(),
        capabilities: ["memory", "diagnostics", "explicit-call"].map(str::to_owned).to_vec(),
        embedder_dim: store.embedder_dim(),
    })
}

fn dispatch(store: &crate::MemoryStore, request: &ExplicitRequestV1) -> (u16, Value) {
    if request.operation == ExplicitOperation::Binding { return (200, json!({})); }
    if request.operation == ExplicitOperation::Diagnostic {
        let name = request.request.get("name").and_then(Value::as_str).unwrap_or("");
        let arguments = request.request.get("arguments").cloned().unwrap_or(json!({}));
        if !name.starts_with("membrane_diagnostic_") || membrane_mcp::validate_arguments(name, &arguments).is_err() {
            return (400, json!({"code":"invalid_request","error":"unknown diagnostic operation"}));
        }
        return (200, crate::mcp_executor::execute_installed_diagnostic(name, &arguments));
    }
    crate::serve::explicit_memory_response(store, request.operation, &request.request)
}

fn dispatch_bound(store: &crate::MemoryStore, binding: &ExplicitOwnerBindingV1,
    request: &ExplicitRequestV1) -> (u16, Value) {
    if request.operation != ExplicitOperation::Binding && request.expected_binding.as_ref() != Some(binding) {
        (409, json!({"code":"corrupt_or_rotation","error":"installed owner binding changed before dispatch"}))
    } else { dispatch(store, request) }
}

pub(crate) fn run() -> Result<(), String> {
    let mut bytes = Vec::new();
    std::io::stdin().take(EXPLICIT_MAX_BYTES as u64 + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() > EXPLICIT_MAX_BYTES { return Err("explicit request exceeds byte limit".into()); }
    let request: ExplicitRequestV1 = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if request.schema_version != 1 { return Err("explicit request version unsupported".into()); }
    let started = std::time::Instant::now();
    eprintln!("{}", json!({"event":"explicit_client_started","operation":request.operation}));
    let outcome = run_request(&request);
    match &outcome {
        Ok(status) => eprintln!("{}", json!({"event":"explicit_client_finished","operation":request.operation,"status":status,"elapsedMs":started.elapsed().as_millis()})),
        Err(failure) => eprintln!("{}", json!({"event":"explicit_client_failed","operation":request.operation,"error":failure,"elapsedMs":started.elapsed().as_millis()})),
    }
    outcome.map(|_| ())
}

fn run_request(request: &ExplicitRequestV1) -> Result<u16, String> {
    let store = crate::service::open_installed_store()?;
    let binding = owner_binding(&store)?;
    let (status, data) = dispatch_bound(&store, &binding, request);
    let response = ExplicitResponseV1 { schema_version: 1, binding, status, data };
    let bytes = serde_json::to_vec(&response).map_err(|e| e.to_string())?;
    if bytes.len() > EXPLICIT_MAX_BYTES { return Err("explicit response exceeds byte limit".into()); }
    std::io::stdout().write_all(&bytes).and_then(|_| std::io::stdout().write_all(b"\n")).map_err(|e| e.to_string())?;
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(store: &crate::MemoryStore) -> ExplicitOwnerBindingV1 {
        ExplicitOwnerBindingV1 { schema_version: 1, mode: ExplicitOwnerMode::BoundedExplicit,
            installation_id: store.installation_id().into(), cortex_store_id: store.cortex_store_id(),
            release_generation: "fixture".into(), startup_generation: 2,
            stable_install_root: "fixture/current".into(), protocol_version: 1, native_only: true,
            subsystems: vec![], capabilities: vec![], embedder_dim: store.embedder_dim() }
    }
    #[test]
    fn explicit_owner_fence_prevents_write_and_valid_binding_uses_canonical_store() {
        let store = crate::MemoryStore::new(); let binding = fixture(&store);
        let mut request = ExplicitRequestV1 { schema_version: 1, operation: ExplicitOperation::Put,
            expected_binding: None, request: json!({"name":"explicit-proof","content":"proof bytes","scope":"proposed","tier":"semantic"}).as_object().unwrap().clone() };
        assert_eq!(dispatch_bound(&store, &binding, &request).0, 409);
        assert!(store.entries(10).is_empty());
        let mut changed = binding.clone(); changed.release_generation = "old".into();
        request.expected_binding = Some(changed);
        assert_eq!(dispatch_bound(&store, &binding, &request).0, 409);
        assert!(store.entries(10).is_empty());
        request.expected_binding = Some(binding.clone());
        let (status, data) = dispatch_bound(&store, &binding, &request);
        assert_eq!(status, 200, "{data}");
        assert_eq!(store.entries(10).len(), 1);
        request.operation = ExplicitOperation::List;
        request.request = json!({"scope":"proposed"}).as_object().unwrap().clone();
        let (status, rows) = dispatch_bound(&store, &binding, &request);
        assert_eq!(status, 200); assert_eq!(rows.as_array().unwrap().len(), 1);
    }
    #[test]
    fn explicit_diagnostics_cannot_tunnel_arbitrary_tools() {
        let store = crate::MemoryStore::new(); let binding = fixture(&store);
        let request = ExplicitRequestV1 { schema_version: 1, operation: ExplicitOperation::Diagnostic,
            expected_binding: Some(binding.clone()), request: json!({"name":"membrane_knowledge_propose","arguments":{}}).as_object().unwrap().clone() };
        assert_eq!(dispatch_bound(&store, &binding, &request).0, 400);
        assert!(store.entries(10).is_empty());
    }
}
