//! Native-only MCP operation owner.  This is intentionally a thin adapter:
//! MCP owns framing while Membrane owns all state and authority decisions.

use crate::{
    authorization::{self, AuthorizationRequest},
    checkpoint::CheckpointV1,
    feedback, scratchpad,
    store::ApprovedProposalAdmissionV1,
    DiagnosticsService, MemoryStore,
};
use cortex_store::{TemporalFact, TemporalFactQuery};
use membrane_federation::blueprint_client::{
    BlueprintBounds, BlueprintClient, BlueprintClientError, UnixBlueprintTransport,
};
use membrane_mcp::NativeMcpExecutor;
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddrV4, TcpStream},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

const MAX_OPERATION_BYTES: usize = 64 * 1024;

pub struct RuntimeMcpExecutor {
    store: MemoryStore,
    diagnostics: Mutex<DiagnosticsService>,
}

impl RuntimeMcpExecutor {
    pub fn for_hub(store: MemoryStore) -> Result<Self, String> {
        Ok(Self {
            store,
            diagnostics: Mutex::new(
                DiagnosticsService::production_service().map_err(|error| error.to_string())?,
            ),
        })
    }

    #[cfg(test)]
    fn with_store(store: MemoryStore, diagnostics: DiagnosticsService) -> Self {
        Self {
            store,
            diagnostics: Mutex::new(diagnostics),
        }
    }
}

fn success(operation: &str, data: Value) -> Value {
    json!({"schemaVersion":1,"operation":operation,"errorVersion":1,"result":{"kind":"success","data":data}})
}
fn error(operation: &str, code: &str, message: impl AsRef<str>) -> Value {
    json!({"schemaVersion":1,"operation":operation,"errorVersion":1,"result":{"kind":"error","code":code,"message":message.as_ref(),"retryable":false}})
}

fn blueprint_failure(operation: &str, failure: BlueprintClientError) -> Value {
    let code = match &failure {
        BlueprintClientError::Unavailable(_) => "blueprint_unavailable",
        BlueprintClientError::Timeout => "provider_timeout",
        BlueprintClientError::Cancelled => "provider_cancelled",
        BlueprintClientError::Malformed(_) => "blueprint_malformed",
        BlueprintClientError::Oversized(_) => "blueprint_oversized",
        BlueprintClientError::GenerationMismatch { .. } => "blueprint_stale",
        BlueprintClientError::Remote { code, .. }
            if matches!(
                code.as_str(),
                "root_not_enrolled" | "graph_missing" | "not_configured"
            ) =>
        {
            code.as_str()
        }
        BlueprintClientError::Remote { code, .. }
            if matches!(code.as_str(), "stale_blocked" | "generation_mismatch") =>
        {
            "blueprint_stale"
        }
        BlueprintClientError::Remote { .. } => "blueprint_remote_error",
    };
    error(operation, code, failure.to_string())
}
fn caller<'a>(arguments: &'a Value, operation: &str) -> Result<(&'a str, &'a str, &'a str), Value> {
    let Some(caller) = arguments.get("caller") else {
        return Err(error(operation, "caller_required", "caller is required"));
    };
    let root = caller
        .get("root")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty());
    let repository = caller
        .get("repositoryId")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty());
    let scope = caller
        .get("scopeId")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty());
    match (root, repository, scope) {
        (Some(root), Some(repository), Some(scope)) => Ok((root, repository, scope)),
        _ => Err(error(
            operation,
            "caller_required",
            "caller root, repositoryId, and scopeId are required",
        )),
    }
}

/// §15 AuthorizationGateV1 action vocabulary: the JS surface's `authorize(args, action)`
/// action names, mapped per native tool (operation-dependent tools follow the JS mapping,
/// e.g. `working_context load` is a read, `save`/`close` are writes).
fn native_action_for(name: &str, arguments: &Value) -> &'static str {
    match name {
        "membrane_context" | "membrane_blueprint" => "context",
        "membrane_source_read" | "membrane_push_prepare" | "membrane_push_resolve" => "source_read",
        "membrane_checkpoint_save" => "checkpoint",
        "membrane_checkpoint_load" => "checkpoint_load",
        "membrane_working_context" => {
            if arguments.get("operation").and_then(Value::as_str) == Some("load") {
                "working_context_load"
            } else {
                "checkpoint"
            }
        }
        "membrane_temporal_fact" => {
            if arguments.get("operation").and_then(Value::as_str) == Some("query") {
                "temporal_fact_query"
            } else {
                "checkpoint"
            }
        }
        "membrane_scratchpad" => {
            if arguments.get("operation").and_then(Value::as_str) == Some("load") {
                "scratchpad_load"
            } else {
                "checkpoint"
            }
        }
        "membrane_feedback" => "feedback",
        "membrane_knowledge_propose" => "proposal",
        "membrane_diagnostic_workspace"
            if arguments.get("operation").and_then(Value::as_str) == Some("status") => "context",
        "membrane_diagnostic_snapshot"
            if matches!(
                arguments.get("operation").and_then(Value::as_str),
                Some("get" | "explain" | "delta")
            ) => "context",
        _ => "checkpoint",
    }
}

/// One shared-module gate pass for a repository-scoped native request. The declared target
/// is the caller's own repository unless the arguments name one explicitly (granted-child
/// reach); `taskGrantLevel` rides the envelope when a caller carries explicit task authority.
fn authorize_native_request(
    arguments: &Value,
    name: &str,
    root: &str,
    repository: &str,
    scope: &str,
) -> Result<(), authorization::AuthorizationDenial> {
    authorization::authorize(&AuthorizationRequest {
        caller_root: root,
        caller_repository_id: repository,
        caller_scope_id: scope,
        caller_scope_descriptor: arguments.pointer("/caller/scopeDescriptor"),
        target_repository: arguments
            .get("repository")
            .or_else(|| arguments.get("repoId"))
            .and_then(Value::as_str)
            .unwrap_or(repository),
        task_grant_level: arguments.get("taskGrantLevel").and_then(Value::as_str),
        action: native_action_for(name, arguments),
    })
    .map(|_| ())
}

fn bounded(value: &Value, operation: &str, code: &str) -> Result<Vec<u8>, Value> {
    let bytes = serde_json::to_vec(value)
        .map_err(|_| error(operation, code, "payload is not serializable"))?;
    if bytes.len() > MAX_OPERATION_BYTES {
        return Err(error(operation, code, "payload exceeds 65536 bytes"));
    }
    Ok(bytes)
}

fn source_path(root: &str, source_ref: &str) -> Result<PathBuf, &'static str> {
    let reference = crate::ledger::identifier::WorktreeDocRef::parse(source_ref)
        .map_err(|_| "source_read_scope_denied")?;
    let relative = Path::new(reference.relative_path());
    if relative
        .components()
        .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err("source_read_scope_denied");
    }
    let root = Path::new(root)
        .canonicalize()
        .map_err(|_| "source_read_scope_denied")?;
    let path = root
        .join(relative)
        .canonicalize()
        .map_err(|_| "source_read_unavailable")?;
    path.starts_with(&root)
        .then_some(path)
        .ok_or("source_read_scope_denied")
}

fn blueprint_endpoint() -> Result<PathBuf, String> {
    if let Some(endpoint) = std::env::var_os("BLUEPRINT_DAEMON_ENDPOINT") {
        return Ok(PathBuf::from(endpoint));
    }
    #[cfg(windows)]
    {
        use sha2::{Digest, Sha256};
        let profile =
            std::env::var("USERPROFILE").map_err(|_| "USERPROFILE unavailable".to_owned())?;
        let suffix = hex::encode(Sha256::digest(profile.as_bytes()));
        Ok(PathBuf::from(format!(
            r"\\.\pipe\membrane-blueprint-{}",
            &suffix[..16]
        )))
    }
    #[cfg(not(windows))]
    {
        let home = std::env::var_os("HOME").ok_or_else(|| "HOME unavailable".to_owned())?;
        Ok(PathBuf::from(home).join(".blueprint/blueprint.sock"))
    }
}

struct HubTransportExecutor {
    port: u16,
    installation_id: String,
    cortex_store_id: String,
    release_generation: String,
    session_id: String,
    token: String,
}

struct UnavailableHubTransportExecutor {
    failure: String,
}

/// A short excerpt of a response body for an error message. Bounded so a
/// large or binary payload cannot flood a log line. A bare "malformed" made
/// an empty read, a truncated body, and a wrong listener indistinguishable.
fn preview(bytes: &[u8]) -> String {
    const LIMIT: usize = 200;
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(LIMIT)])
        .replace(['\r', '\n'], " ");
    if bytes.len() > LIMIT {
        format!("{text}... ({} bytes total)", bytes.len())
    } else if text.trim().is_empty() {
        "<empty>".to_owned()
    } else {
        text
    }
}

fn hub_inactive_message(failure: impl AsRef<str>) -> String {
    let failure = failure.as_ref();
    if failure.contains("membrane_unavailable { hub_inactive }") {
        failure.to_owned()
    } else {
        format!("membrane_unavailable {{ hub_inactive }}: {failure}")
    }
}

impl HubTransportExecutor {
    fn active() -> Result<Self, String> {
        let exe = std::env::current_exe().map_err(|failure| failure.to_string())?;
        let runtime = crate::service::runtime_from_exe(&exe)?;
        let workspace = runtime
            .db
            .ancestors()
            .nth(4)
            .ok_or_else(|| "membrane_unavailable { hub_inactive }".to_owned())?;
        let paths = crate::installation_identity::InstallationPaths::for_workspace(workspace);
        if !paths.identity.is_file() || !runtime.token.is_file() {
            return Err("membrane_unavailable { hub_inactive }".into());
        }
        let identity =
            crate::installation_identity::load_or_create_installation(&paths.identity, &[])
                .map_err(|_| "membrane_unavailable { hub_inactive }".to_owned())?;
        let session_id = identity
            .current_service_instance_id
            .ok_or_else(|| "membrane_unavailable { hub_inactive }".to_owned())?;
        let token = std::fs::read_to_string(&runtime.token)
            .map_err(|_| "membrane_unavailable { hub_inactive }".to_owned())?;
        let token = token.trim().to_owned();
        if token.is_empty() {
            return Err("membrane_unavailable { hub_inactive }".into());
        }
        let mut executor = Self {
            port: runtime.port,
            installation_id: identity.installation_id,
            cortex_store_id: String::new(),
            release_generation: String::new(),
            session_id,
            token,
        };
        let health = executor.health()?;
        if health.get("installationId").and_then(Value::as_str)
            != Some(executor.installation_id.as_str())
        {
            return Err("Hub health installationId does not match active binding".to_owned());
        }
        executor.cortex_store_id = health
            .get("cortexStoreId")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "Hub health omitted cortexStoreId".to_owned())?
            .to_owned();
        executor.release_generation = health
            .get("releaseGeneration")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "Hub health omitted releaseGeneration".to_owned())?
            .to_owned();
        Ok(executor)
    }

    fn health(&self) -> Result<Value, String> {
        let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, self.port);
        let mut stream = TcpStream::connect_timeout(&address.into(), Duration::from_secs(2))
            .map_err(|error| format!("Hub health unavailable: {error}"))?;
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .map_err(|error| error.to_string())?;
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .map_err(|error| error.to_string())?;
        let host = format!("127.0.0.1:{}", self.port);
        let request = format!("GET /health HTTP/1.0\r\nHost: {host}\r\nConnection: close\r\n\r\n");
        stream
            .write_all(request.as_bytes())
            .map_err(|error| error.to_string())?;
        let mut response = Vec::new();
        stream
            .take((MAX_OPERATION_BYTES * 2) as u64)
            .read_to_end(&mut response)
            .map_err(|error| error.to_string())?;
        let split = response
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .ok_or_else(|| {
                format!(
                    "Hub health response malformed: no header terminator in {} byte(s): {}",
                    response.len(),
                    preview(&response)
                )
            })?;
        let head = std::str::from_utf8(&response[..split]).map_err(|error| error.to_string())?;
        let status = head.lines().next().unwrap_or("");
        if !status.contains(" 200 ") {
            return Err(format!(
                "Hub health unavailable: hub answered {status}: {}",
                preview(&response[split + 4..])
            ));
        }
        serde_json::from_slice(&response[split + 4..]).map_err(|error| error.to_string())
    }

    fn post(&self, name: &str, arguments: &Value) -> Result<Value, String> {
        let payload = json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":arguments}}).to_string();
        let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, self.port);
        let mut stream = TcpStream::connect_timeout(&address.into(), Duration::from_secs(2))
            .map_err(|_| "membrane_unavailable { hub_inactive }".to_owned())?;
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| e.to_string())?;
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .map_err(|e| e.to_string())?;
        let host = format!("127.0.0.1:{}", self.port);
        let request = format!(
            "POST /mcp HTTP/1.0\r\nHost: {host}\r\nOrigin: http://{host}\r\nContent-Type: application/json\r\nAuthorization: Bearer {}\r\nx-membrane-installation-id: {}\r\nx-membrane-cortex-store-id: {}\r\nx-membrane-release-generation: {}\r\nx-membrane-session: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            self.token, self.installation_id, self.cortex_store_id, self.release_generation,
            self.session_id, payload.len(), payload,
        );
        stream
            .write_all(request.as_bytes())
            .map_err(|e| e.to_string())?;
        let mut response = Vec::new();
        stream
            .take((MAX_OPERATION_BYTES * 2) as u64)
            .read_to_end(&mut response)
            .map_err(|e| e.to_string())?;
        let split = response
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .ok_or_else(|| {
                format!(
                    "Hub MCP response malformed: no header terminator in {} byte(s): {}",
                    response.len(),
                    preview(&response)
                )
            })?;
        let head = std::str::from_utf8(&response[..split]).map_err(|e| e.to_string())?;
        let status = head.lines().next().unwrap_or("");
        if !status.contains(" 200 ") {
            return Err(format!(
                "membrane_unavailable {{ hub_inactive }}: hub answered {status} for {name}: {}",
                preview(&response[split + 4..])
            ));
        }
        let body: Value =
            serde_json::from_slice(&response[split + 4..]).map_err(|e| e.to_string())?;
        body.pointer("/result/structuredContent")
            .cloned()
            .ok_or_else(|| "Hub MCP response omitted structuredContent".to_owned())
    }
}

impl NativeMcpExecutor for HubTransportExecutor {
    fn execute(&self, name: &str, arguments: &Value) -> Value {
        self.post(name, arguments).unwrap_or_else(|failure| {
            error(name, "membrane_unavailable", hub_inactive_message(failure))
        })
    }
}

impl NativeMcpExecutor for UnavailableHubTransportExecutor {
    fn execute(&self, name: &str, _arguments: &Value) -> Value {
        error(
            name,
            "membrane_unavailable",
            hub_inactive_message(&self.failure),
        )
    }
}
fn parse<T: serde::de::DeserializeOwned>(
    operation: &str,
    value: Option<&Value>,
    code: &str,
) -> Result<T, Value> {
    value
        .cloned()
        .ok_or_else(|| error(operation, code, "required operation payload missing"))
        .and_then(|value| {
            serde_json::from_value(value)
                .map_err(|_| error(operation, code, "operation payload is invalid"))
        })
}

impl NativeMcpExecutor for RuntimeMcpExecutor {
    fn execute(&self, name: &str, arguments: &Value) -> Value {
        let diagnostic = name.starts_with("membrane_diagnostic_");
        // Only these repository-independent diagnostic views remain outside
        // AuthorizationGateV1: fence evaluation, capabilities, and provider
        // list/status. Workspace status and snapshot get/explain/delta are
        // repository-scoped reads and are gated too.
        let diagnostic_read_only = matches!(
            (name, arguments.get("operation").and_then(Value::as_str)),
            ("membrane_diagnostic_fence", _)
                | ("membrane_diagnostic_capabilities", _)
                | ("membrane_diagnostic_provider", Some("list" | "status"))
        );
        let gated_diagnostic = diagnostic && !diagnostic_read_only;
        let (root, repository, scope) = if diagnostic {
            let repository = arguments
                .get("repoId")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("diagnostics");
            let scope = arguments
                .get("worktreeId")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("diagnostics");
            (
                arguments
                    .get("projectRoot")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
                repository,
                scope,
            )
        } else {
            match caller(arguments, name) {
                Ok(caller) => caller,
                Err(result) => return result,
            }
        };
        let diagnostic_caller = if gated_diagnostic && arguments.get("caller").is_some() {
            match caller(arguments, name) {
                Ok(caller) => Some(caller),
                Err(_) => {
                    return error(
                        name,
                        "authorization_denied",
                        "caller_scope_binding_denied: caller envelope is invalid",
                    )
                }
            }
        } else {
            None
        };
        if !diagnostic && arguments.get("repository").and_then(Value::as_str) != Some(repository) {
            let code = match name {
                "membrane_source_read" => "source_read_scope_denied",
                "membrane_blueprint" => "blueprint_caller_scope_binding_denied",
                "membrane_checkpoint_save" | "membrane_checkpoint_load" => {
                    "checkpoint_scope_denied"
                }
                "membrane_working_context" => "working_context_scope_denied",
                "membrane_temporal_fact" => "temporal_fact_scope_denied",
                "membrane_scratchpad" => "scratchpad_scope_denied",
                "membrane_knowledge_propose" => "proposal_scope_denied",
                _ => "context_envelope_invalid",
            };
            return error(name, code, "repository must match caller repositoryId");
        };
        // §15 AuthorizationGateV1: every repository-scoped native request passes the shared
        // monotone-authority module BEFORE retrieval scoring and before admission. Diagnostics
        // use the caller envelope for the gate and retain repoId/worktreeId only as the
        // verified target service key; bearer transport authenticates the channel, never scope.
        if gated_diagnostic {
            let task_grant = arguments.get("taskGrantLevel").and_then(Value::as_str);
            let action = native_action_for(name, arguments);
            let authorization = if let Some((caller_root, caller_repository, caller_scope)) = diagnostic_caller {
                let project_root = arguments
                    .get("projectRoot")
                    .and_then(Value::as_str)
                    .unwrap_or(caller_root);
                authorization::authorize_diagnostic(
                    &AuthorizationRequest {
                        caller_root,
                        caller_repository_id: caller_repository,
                        caller_scope_id: caller_scope,
                        caller_scope_descriptor: arguments.pointer("/caller/scopeDescriptor"),
                        target_repository: repository,
                        task_grant_level: task_grant,
                        action,
                    },
                    project_root,
                )
            } else {
                authorization::authorize_diagnostic_identity(
                    repository,
                    arguments.get("projectRoot").and_then(Value::as_str),
                    task_grant,
                    action,
                )
            };
            if let Err(denial) = authorization {
                return error(name, "authorization_denied", format!("{}: {}", denial.code(), denial));
            }
        } else if !diagnostic {
            if let Err(denial) = authorize_native_request(arguments, name, root, repository, scope)
            {
                return error(name, denial.code(), denial.to_string());
            }
        }
        match name {
            "membrane_push_prepare" | "membrane_push_resolve" => crate::push::api::execute(name, arguments),
            "membrane_context" => {
                let task = arguments
                    .get("task")
                    .and_then(Value::as_str)
                    .filter(|s| !s.trim().is_empty());
                let Some(task) = task else {
                    return error(name, "context_envelope_invalid", "task is required");
                };
                let budget = arguments
                    .get("budget")
                    .and_then(Value::as_u64)
                    .unwrap_or(5)
                    .min(50) as usize;
                // Frozen interface 2: the planner-authored sufficiency contract travels
                // VERBATIM into the resident federate request — never parsed, never
                // rewritten here. Absent means "not supplied", and the body stays absent so
                // federation evaluates sufficiency as not_evaluated.
                let sufficiency_contract = arguments.get("sufficiencyContract").cloned();
                let mut body = json!({
                    "task": task,
                    "repo": root,
                    "maxTokens": budget.saturating_mul(1024),
                    "client": "membrane-native",
                    "session": scope,
                    "scopeGrantId": arguments.get("scopeGrantId").and_then(Value::as_str),
                });
                if let Some(contract) = sufficiency_contract {
                    body["sufficiencyContract"] = contract;
                }
                // The runtime refuses every context request without this
                // (RequestTimeH8Error::Missing) and reads it from the request
                // body, but the executor never forwarded it, so no client could
                // satisfy the requirement no matter what it sent. Carried
                // verbatim like the sufficiency contract: the host observed it,
                // and Membrane never derives or defaults an observation.
                if let Some(ceiling) = arguments.get("remainingContextCeiling").cloned() {
                    body["remainingContextCeiling"] = ceiling;
                }
                let request = match serde_json::to_string(&body)
                    .map_err(|_| error(name, "context_envelope_invalid", "request is invalid"))
                {
                    Ok(request) => request,
                    Err(result) => return result,
                };
                let (status, payload) =
                    match crate::pull::federation::native_route_response(&request) {
                        (200, payload) => ("ok", payload),
                        (status, payload) => ("unavailable", payload),
                    };
                let federated: Value = match serde_json::from_str(&payload).map_err(|_| {
                    error(name, "context_unavailable", "federation payload is invalid")
                }) {
                    Ok(federated) => federated,
                    Err(result) => return result,
                };
                if status != "ok" {
                    // Refusals carry `kind` and `reason` beside `error`. Keeping
                    // only `error` reduced every failure to a bare code such as
                    // "request_time_selection_refused", which names the gate but
                    // not the cause, leaving the caller nothing to act on.
                    let code = federated
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("federation is unavailable");
                    let detail = match (
                        federated.get("kind").and_then(Value::as_str),
                        federated.get("reason").and_then(Value::as_str),
                    ) {
                        (Some(kind), Some(reason)) => format!("{code}: {kind}: {reason}"),
                        (None, Some(reason)) | (Some(reason), None) => format!("{code}: {reason}"),
                        (None, None) => code.to_owned(),
                    };
                    return error(name, "context_unavailable", detail);
                }
                let packet = federated.get("packet").cloned().unwrap_or(Value::Null);
                let candidates = federated
                    .pointer("/packet/blocks")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let receipts = federated.get("receipts").cloned().unwrap_or(Value::Null);
                let degradation_reason = federated
                    .get("degradationReason")
                    .filter(|value| !value.is_null())
                    .cloned()
                    .unwrap_or_else(|| json!("none"));
                success(
                    name,
                    json!({
                        "repositoryId":repository,
                        "scopeId":scope,
                        "status":status,
                        "packet":packet,
                        "candidates":candidates,
                        "receipts":receipts,
                        "packetReduction":federated.get("packetReduction").and_then(|v| v.get("selectionReceipt")).cloned(),
                        "degradationReason":degradation_reason,
                        "sufficiencyEvaluated":arguments.get("sufficiencyContract").is_some(),
                    }),
                )
            }
            "membrane_source_read" => {
                let source_ref = arguments
                    .get("sourceRef")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let anchor = arguments
                    .get("anchorId")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let expected = arguments
                    .get("expectedContentHash")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if source_ref.is_empty() || anchor.is_empty() || expected.is_empty() {
                    return error(
                        name,
                        "source_read_envelope_invalid",
                        "sourceRef, anchorId, and expectedContentHash are required",
                    );
                }
                let path = match source_path(root, source_ref) {
                    Ok(path) => path,
                    Err(code) => {
                        return error(
                            name,
                            code,
                            "source reference is unavailable or outside caller root",
                        )
                    }
                };
                let markdown = match std::fs::read_to_string(path) {
                    Ok(value) => value,
                    Err(_) => {
                        return error(name, "source_read_unavailable", "source is unreadable")
                    }
                };
                match crate::ledger::outline::read_section(
                    source_ref, &markdown, anchor, expected, 12_000,
                ) {
                    Ok(read) => success(
                        name,
                        json!({"ok":true,"contentSha256":read.content_hash,"section":read,"sourceRef":source_ref}),
                    ),
                    Err(crate::ledger::outline::DocReadError::SourceChanged) => error(
                        name,
                        "source_read_hash_mismatch",
                        "expectedContentHash did not match live document content",
                    ),
                    Err(crate::ledger::outline::DocReadError::Relocated) => error(
                        name,
                        "source_read_anchor_missing",
                        "anchorId does not exist in live document",
                    ),
                    Err(crate::ledger::outline::DocReadError::Deny) => error(
                        name,
                        "source_read_scope_denied",
                        "source reference is outside caller root",
                    ),
                    Err(crate::ledger::outline::DocReadError::SourceMissing) => {
                        error(name, "source_read_unavailable", "source is unavailable")
                    }
                }
            }
            "membrane_blueprint" => {
                let operation = arguments
                    .get("operation")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let (method, mut input) = match operation {
                    "architecture" => (
                        "architecture",
                        json!({"repoRoot":root,"budget":arguments.get("budget").and_then(Value::as_u64).unwrap_or(2000)}),
                    ),
                    "symbol" => (
                        "resolve",
                        json!({"repoRoot":root,"nodeId":arguments.get("node").and_then(Value::as_str).unwrap_or("")}),
                    ),
                    "reference" | "references" => (
                        "expand",
                        json!({"repoRoot":root,"anchor":arguments.get("node").and_then(Value::as_str).unwrap_or(""),"direction":"both","depth":arguments.get("depth").and_then(Value::as_u64).unwrap_or(1),"budget":arguments.get("budget").and_then(Value::as_u64).unwrap_or(2000)}),
                    ),
                    "impact" => (
                        "impact",
                        json!({"repoRoot":root,"anchor":arguments.get("node").and_then(Value::as_str).unwrap_or(""),"depth":arguments.get("depth").and_then(Value::as_u64).unwrap_or(3),"budget":arguments.get("budget").and_then(Value::as_u64).unwrap_or(2000)}),
                    ),
                    "changes" | "snapshot_get" | "snapshot_list" | "changes_since" => {
                        (operation, json!({"repoRoot":root}))
                    }
                    _ => {
                        return error(
                            name,
                            "blueprint_envelope_invalid",
                            "unsupported Blueprint operation",
                        )
                    }
                };
                if let Some(items) = arguments.get("items") {
                    input["items"] = items.clone();
                }
                if let Some(node) = arguments.get("node") {
                    input["node"] = node.clone();
                }
                let endpoint = match blueprint_endpoint() {
                    Ok(value) => value,
                    Err(message) => return error(name, "blueprint_unavailable", message),
                };
                let expected_generation = match arguments.get("generationId") {
                    None => None,
                    Some(Value::String(value)) if !value.trim().is_empty() => {
                        Some(value.trim().to_owned())
                    }
                    Some(_) => {
                        return error(
                            name,
                            "blueprint_envelope_invalid",
                            "generationId must be a non-empty string",
                        )
                    }
                };
                let client = BlueprintClient::new(Arc::new(UnixBlueprintTransport::new(endpoint)));
                let request_id = format!(
                    "mcp-blueprint-{}-{}",
                    std::process::id(),
                    crate::time::now_millis()
                );
                match client.execute_wire(
                    &request_id,
                    repository,
                    method,
                    input,
                    expected_generation.as_deref(),
                    BlueprintBounds::default(),
                    Duration::from_secs(2),
                ) {
                    Ok(payload) => success(name, payload),
                    Err(failure) => blueprint_failure(name, failure),
                }
            }
            "membrane_knowledge_propose" => self.propose(name, arguments, repository, scope),
            "membrane_checkpoint_save" => {
                let checkpoint: CheckpointV1 = match parse(
                    name,
                    arguments.get("checkpoint"),
                    "checkpoint_envelope_invalid",
                ) {
                    Ok(value) => value,
                    Err(result) => return result,
                };
                if checkpoint.repository_id != repository || checkpoint.scope_id != scope {
                    return error(
                        name,
                        "checkpoint_scope_denied",
                        "checkpoint identity must match caller",
                    );
                }
                match self.store.save_checkpoint(&checkpoint) {
                    Ok(()) => success(
                        name,
                        json!({"id":checkpoint.checkpoint_id,"status":"saved"}),
                    ),
                    Err(e) => error(name, "checkpoint_unavailable", e.to_string()),
                }
            }
            "membrane_checkpoint_load" => {
                let id = arguments
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|s| !s.trim().is_empty());
                let Some(id) = id else {
                    return error(name, "checkpoint_envelope_invalid", "id is required");
                };
                let as_of = arguments
                    .get("asOfMs")
                    .and_then(Value::as_i64)
                    .unwrap_or_else(|| crate::time::now_millis() as i64);
                match self.store.load_checkpoint(id, as_of) {
                    Ok(checkpoint)
                        if checkpoint.repository_id == repository
                            && checkpoint.scope_id == scope =>
                    {
                        success(name, json!({"checkpoint":checkpoint}))
                    }
                    Ok(_) => error(
                        name,
                        "checkpoint_scope_denied",
                        "checkpoint is outside caller scope",
                    ),
                    Err(e) => error(name, "checkpoint_unavailable", e.to_string()),
                }
            }
            "membrane_working_context" => self.working_context(name, arguments, repository, scope),
            "membrane_temporal_fact" => {
                let operation = arguments
                    .get("operation")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if operation == "record" {
                    let fact: TemporalFact = match parse(
                        name,
                        arguments.get("fact"),
                        "temporal_fact_envelope_invalid",
                    ) {
                        Ok(value) => value,
                        Err(result) => return result,
                    };
                    if fact.scope_id != scope {
                        return error(
                            name,
                            "temporal_fact_scope_denied",
                            "fact scope must match caller",
                        );
                    }
                    match self.store.temporal_facts().record(fact, true) {
                        Ok(receipt) => success(name, json!({"receipt":receipt})),
                        Err(e) => error(name, "temporal_fact_invalid", e),
                    }
                } else if operation == "query" {
                    let query = TemporalFactQuery {
                        scope_chain: vec![scope.to_owned()],
                        subject: arguments
                            .get("subject")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned(),
                        predicate: arguments
                            .get("predicate")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned(),
                        as_of: arguments
                            .get("asOf")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned(),
                    };
                    match self.store.temporal_facts().query(query) {
                        Ok(facts) => success(name, json!({"facts":facts})),
                        Err(e) => error(name, "temporal_fact_query_invalid", e),
                    }
                } else {
                    error(
                        name,
                        "temporal_fact_envelope_invalid",
                        "operation must be record or query",
                    )
                }
            }
            "membrane_scratchpad" => {
                let mut scoped = arguments.clone();
                if let Some(session) = arguments.get("sessionId").and_then(Value::as_str) {
                    scoped["sessionId"] = Value::String(format!("{repository}:{scope}:{session}"));
                }
                scratchpad::handle(&scoped)
            }
            "membrane_feedback" => {
                let outcome = match arguments
                    .get("outcome")
                    .and_then(Value::as_str)
                    .and_then(|value| feedback::parse_outcome(value).ok())
                {
                    Some(value) => value,
                    None => return error(name, "feedback_invalid", "outcome is invalid"),
                };
                let receipt = arguments
                    .get("receiptId")
                    .and_then(Value::as_str)
                    .filter(|s| !s.trim().is_empty());
                let verdict = arguments
                    .get("verdictRef")
                    .and_then(Value::as_str)
                    .filter(|s| !s.trim().is_empty());
                let Some(receipt) = receipt else {
                    return error(name, "feedback_invalid", "receiptId is required");
                };
                let source = if verdict.is_some() {
                    feedback::FeedbackSource::CitedVerdict
                } else {
                    feedback::FeedbackSource::Advisory
                };
                let record = feedback::FeedbackRecord {
                    trace_id: receipt.to_owned(),
                    candidate_id: receipt.to_owned(),
                    content_sha256: receipt.to_owned(),
                    outcome,
                    source: source.clone(),
                    verdict_ref: verdict.map(str::to_owned),
                    scope_id: scope.to_owned(),
                };
                match self.store.record_feedback(&record) {
                    Ok(()) => {
                        let verified = matches!(source, feedback::FeedbackSource::CitedVerdict);
                        let feedback_id = crate::digest::digest_str(&format!(
                            "{scope}:{receipt}:{}",
                            feedback::outcome_str(outcome)
                        ));
                        let status = if verified {
                            "persisted"
                        } else {
                            "accepted_advisory"
                        };
                        success(
                            name,
                            json!({"status":status,"durable":true,"feedbackId":feedback_id,"receiptId":receipt,"outcome":feedback::outcome_str(outcome),"source":if verified {"cited_verdict"} else {"advisory"},"verified":verified,"lifecycleReceipt":{"schema":"membrane.lifecycle-receipt.v1","operation":"feedback","status":status,"durableId":feedback_id,"eventId":feedback_id,"readbackDigest":crate::digest::digest_str(receipt),"recordedAt":crate::time::now_iso()},"provenance":{"repositoryId":repository,"scopeId":scope}}),
                        )
                    }
                    Err(e) => error(name, "feedback_invalid", e),
                }
            }
            diagnostic if diagnostic.starts_with("membrane_diagnostic_") => {
                self.diagnostic(diagnostic, arguments, repository, scope)
            }
            _ => error(name, "unknown_tool", "unknown native MCP operation"),
        }
    }
}

impl RuntimeMcpExecutor {
    /// §16.1 review transition: operate on the NAMED proposal row. When
    /// `review.proposalId` is present the emission arguments carry no new
    /// text — the row's stored emission is loaded and re-admitted; when a
    /// review names an unknown id, that is a typed review failure, never a
    /// new quarantine row.
    fn propose(&self, name: &str, arguments: &Value, repository: &str, scope: &str) -> Value {
        let named_review = arguments
            .get("review")
            .and_then(|review| review.get("proposalId"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        let emission = match (&named_review, arguments.get("emission")) {
            (Some(_), _) => serde_json::json!({"text": "named-proposal-review"}),
            (None, Some(emission)) => emission.clone(),
            (None, None) => {
                return error(
                    name,
                    "proposal_emission_text_required",
                    "emission is required",
                )
            }
        };
        let bytes = match bounded(&emission, name, "proposal_payload_too_large") {
            Ok(bytes) => bytes,
            Err(result) => return result,
        };
        let text = emission
            .get("text")
            .or_else(|| emission.get("content"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if text.is_empty() {
            return error(
                name,
                "proposal_emission_text_required",
                "emission text is required",
            );
        }
        // This table is a proposal quarantine only. No row can become Cortex
        // durable truth, Taste, or Insights without separate qualified review.
        let proposal_id = match &named_review {
            Some(named) => named.clone(),
            None => crate::digest::digest_bytes(
                format!("{repository}\0{scope}\0{}", String::from_utf8_lossy(&bytes)).as_bytes(),
            ),
        };
        // New-emission rows serialize the caller's payload; named reviews load
        // the row's stored emission below, before any admission.
        let mut emission_json: String = match serde_json::to_string(&emission) {
            Ok(value) => value,
            Err(_) => return error(name, "proposal_payload_too_large", "emission is invalid"),
        };
        let mut emission_sha: String = crate::digest::digest_str(&emission_json);
        let Some(event_db) = self.store.db().event_db_path() else {
            return error(
                name,
                "proposal_binding_unresolvable",
                "Hub event store is unavailable",
            );
        };
        let db = match rusqlite::Connection::open(event_db) {
            Ok(value) => value,
            Err(failure) => {
                return error(name, "proposal_binding_unresolvable", failure.to_string())
            }
        };
        if let Err(failure) = db.execute_batch(
            "CREATE TABLE IF NOT EXISTS membrane_knowledge_proposal(\
             proposal_id TEXT PRIMARY KEY,repository_id TEXT NOT NULL,scope_id TEXT NOT NULL,\
             emission_json TEXT NOT NULL,emission_sha256 TEXT NOT NULL,\
             state TEXT NOT NULL CHECK(state IN ('pending','approved','rejected')),\
             created_at TEXT NOT NULL,decided_at TEXT,reviewer TEXT) STRICT;",
        ) {
            return error(name, "proposal_durable_write_failed", failure.to_string());
        }
        let existing_row: Option<(String, String)> = if named_review.is_some() {
            let row = match db
                .query_row(
                    "SELECT repository_id,scope_id,emission_json,emission_sha256 FROM membrane_knowledge_proposal WHERE proposal_id=?1",
                    params![proposal_id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                        ))
                    },
                )
                .optional()
                .map_err(|failure| {
                    error(name, "proposal_durable_write_failed", failure.to_string())
                }) {
                Ok(row) => row,
                Err(result) => return result,
            };
            let Some((row_repository, row_scope, row_emission_json, row_emission_sha)) = row else {
                return error(
                    name,
                    "proposal_review_unknown",
                    "reviewed proposalId does not exist",
                );
            };
            if row_repository != repository || row_scope != scope {
                return error(
                    name,
                    "proposal_scope_denied",
                    "reviewed proposal belongs to a different repository or scope",
                );
            }
            // The admission payload is the row's stored emission — never the
            // review carrier's.
            emission_json = row_emission_json;
            emission_sha = row_emission_sha;
            Some((row_repository, row_scope))
        } else {
            None
        };
        if existing_row.is_none() {
            if let Err(failure) = db.execute(
                "INSERT OR IGNORE INTO membrane_knowledge_proposal(proposal_id,repository_id,scope_id,emission_json,emission_sha256,state,created_at) VALUES(?1,?2,?3,?4,?5,'pending',?6)",
                params![proposal_id, repository, scope, emission_json, emission_sha, crate::time::now_iso()],
            ) {
                return error(name, "proposal_durable_write_failed", failure.to_string());
            }
            let readback: Option<(String, String, String)> = db.query_row(
                "SELECT repository_id,scope_id,emission_sha256 FROM membrane_knowledge_proposal WHERE proposal_id=?1",
                params![proposal_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            ).optional().unwrap_or(None);
            if readback.as_ref()
                != Some(&(
                    repository.to_owned(),
                    scope.to_owned(),
                    emission_sha.clone(),
                ))
            {
                return error(
                    name,
                    "proposal_durable_write_failed",
                    "proposal readback mismatch",
                );
            }
        }
        // §16.1: `approved`/`rejected` are already legal states of this table; without a
        // transition the `approved` state would be a silent promise of promotion that nothing
        // consumes. The out-of-band reviewer marks the decision here through the one governed
        // transition (only pending rows move, only via review), and ONLY `approved` is then
        // consumed into Cortex admission — `rejected` and `pending` stay quarantine-only.
        let review_decision = arguments
            .get("review")
            .and_then(|review| review.get("decision"))
            .and_then(Value::as_str);
        if named_review.is_some() && review_decision.is_none() {
            return error(
                name,
                "proposal_review_invalid",
                "review.decision must be approve or reject",
            );
        }
        let review_note = arguments
            .get("review")
            .and_then(|review| review.get("note"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_owned();
        let admission = match review_decision {
            Some("approve") => {
                let reviewer = arguments
                    .get("review")
                    .and_then(|review| review.get("reviewer"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim();
                if reviewer.is_empty() {
                    return error(
                        name,
                        "proposal_review_invalid",
                        "review.reviewer is required",
                    );
                }
                let decided = match db
                    .execute(
                        "UPDATE membrane_knowledge_proposal SET state='approved', decided_at=?2, reviewer=?3 WHERE proposal_id=?1 AND state='pending'",
                        params![proposal_id, crate::time::now_iso(), reviewer],
                    )
                    .map_err(|failure| {
                        error(name, "proposal_durable_write_failed", failure.to_string())
                    }) {
                    Ok(decided) => decided,
                    Err(result) => return result,
                };
                if decided != 1 {
                    let state: String = db
                        .query_row(
                            "SELECT state FROM membrane_knowledge_proposal WHERE proposal_id=?1",
                            params![proposal_id],
                            |row| row.get(0),
                        )
                        .unwrap_or_default();
                    return match state.as_str() {
                        "approved" | "rejected" => error(
                            name,
                            "proposal_already_decided",
                            format!("proposal is already {state}"),
                        ),
                        _ => error(
                            name,
                            "proposal_durable_write_failed",
                            "proposal review transition failed",
                        ),
                    };
                }
                // Frozen interface 1 — the admission side. Exactly one outcome per the §14
                // contract: the proposal reaches Cortex admission, or `approved` is
                // unreachable because the admission write failed — a failed admission is
                // reverted to `pending` (review not yet consumed), never left approved and
                // silently unconsumed.
                match self
                    .store
                    .admit_approved_proposal(&proposal_id, &emission_json)
                {
                    Ok(admission) => Some(admission),
                    Err(failure) => {
                        if let Err(revert) = db.execute(
                            "UPDATE membrane_knowledge_proposal SET state='pending', decided_at=NULL, reviewer=NULL WHERE proposal_id=?1 AND state='approved'",
                            params![proposal_id],
                        ) {
                            return error(
                                name,
                                "proposal_admission_failed",
                                format!(
                                    "{} (admission revert also failed: {revert})",
                                    failure
                                ),
                            );
                        }
                        return error(name, "proposal_admission_failed", failure.to_string());
                    }
                }
            }
            Some("reject") => {
                let reviewer = arguments
                    .get("review")
                    .and_then(|review| review.get("reviewer"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim();
                if reviewer.is_empty() {
                    return error(
                        name,
                        "proposal_review_invalid",
                        "review.reviewer is required",
                    );
                }
                let decided = match db
                    .execute(
                        "UPDATE membrane_knowledge_proposal SET state='rejected', decided_at=?2, reviewer=?3 WHERE proposal_id=?1 AND state='pending'",
                        params![proposal_id, crate::time::now_iso(), reviewer],
                    )
                    .map_err(|failure| {
                        error(name, "proposal_durable_write_failed", failure.to_string())
                    }) {
                    Ok(decided) => decided,
                    Err(result) => return result,
                };
                if decided != 1 {
                    let state: String = db
                        .query_row(
                            "SELECT state FROM membrane_knowledge_proposal WHERE proposal_id=?1",
                            params![proposal_id],
                            |row| row.get(0),
                        )
                        .unwrap_or_default();
                    return match state.as_str() {
                        "approved" | "rejected" => error(
                            name,
                            "proposal_already_decided",
                            format!("proposal is already {state}"),
                        ),
                        _ => error(
                            name,
                            "proposal_durable_write_failed",
                            "proposal review transition failed",
                        ),
                    };
                }
                None
            }
            Some(_) => {
                return error(
                    name,
                    "proposal_review_invalid",
                    "review.decision must be approve or reject",
                )
            }
            None => None,
        };
        if let Some(review_decision) = review_decision {
            if let Some(decision) = admission {
                let event_id =
                    crate::digest::digest_str(&format!("knowledge_review:{proposal_id}"));
                let (status, provenance_status) = match &decision {
                    ApprovedProposalAdmissionV1::Admitted { memory_id } => {
                        ("approved", json!({"memoryId":memory_id}))
                    }
                    ApprovedProposalAdmissionV1::Duplicate { existing_id } => {
                        ("duplicate", json!({"existingId":existing_id}))
                    }
                    ApprovedProposalAdmissionV1::Conflict { existing_id } => {
                        ("conflict", json!({"existingId":existing_id}))
                    }
                };
                return success(
                    name,
                    json!({
                        "status":"reviewed","durable":true,"proposalId":proposal_id,"durableId":proposal_id,
                        "reviewState":"approved","reviewDecision":review_decision,
                        "admission":{
                            "schema":"membrane.approved-proposal-admission.v1",
                            "outcome":status,
                            "provenance":provenance_status,
                        },
                        "lifecycleReceipt":{"schema":"membrane.lifecycle-receipt.v1",
                        "operation":"knowledge_review","status":status,"durableId":proposal_id,
                        "eventId":event_id,"readbackDigest":emission_sha,"recordedAt":crate::time::now_iso()},
                        "provenance":{"repositoryId":repository,"scopeId":scope,"reviewNote":review_note}
                    }),
                );
            }
            let event_id = crate::digest::digest_str(&format!("knowledge_review:{proposal_id}"));
            return success(
                name,
                json!({
                    "status":"reviewed","durable":true,"proposalId":proposal_id,"durableId":proposal_id,
                    "reviewState":"rejected","reviewDecision":review_decision,
                    "lifecycleReceipt":{"schema":"membrane.lifecycle-receipt.v1",
                    "operation":"knowledge_review","status":"rejected","durableId":proposal_id,
                    "eventId":event_id,"readbackDigest":emission_sha,"recordedAt":crate::time::now_iso()},
                    "provenance":{"repositoryId":repository,"scopeId":scope,"reviewNote":review_note}
                }),
            );
        }
        let event_id = crate::digest::digest_str(&format!("knowledge_propose:{proposal_id}"));
        success(
            name,
            json!({
                "status":"needs_review","durable":true,"proposalId":proposal_id,"durableId":proposal_id,
                "reviewState":"pending","lifecycleReceipt":{"schema":"membrane.lifecycle-receipt.v1",
                "operation":"knowledge_propose","status":"needs_review","durableId":proposal_id,
                "eventId":event_id,"readbackDigest":emission_sha,"recordedAt":crate::time::now_iso()},
                "provenance":{"repositoryId":repository,"scopeId":scope,"authority":"proposal_only"}
            }),
        )
    }

    fn working_context(
        &self,
        name: &str,
        arguments: &Value,
        repository: &str,
        scope: &str,
    ) -> Value {
        let operation = arguments
            .get("operation")
            .and_then(Value::as_str)
            .unwrap_or("");
        if !matches!(operation, "save" | "load" | "close") {
            return error(
                name,
                "working_context_operation_invalid",
                "operation must be save, load, or close",
            );
        }
        let session = arguments
            .get("sessionId")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let task = arguments
            .get("taskId")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let context_id = arguments
            .get("contextId")
            .or_else(|| {
                arguments
                    .get("context")
                    .and_then(|value| value.get("contextId"))
            })
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if session.is_empty() || task.is_empty() {
            return error(
                name,
                "working_context_scope_required",
                "sessionId and taskId are required",
            );
        }
        if operation != "load" && context_id.is_empty() {
            return error(name, "working_context_id_required", "contextId is required");
        }
        let Some(event_db) = self.store.db().event_db_path() else {
            return error(
                name,
                "working_context_envelope_invalid",
                "Hub event store is unavailable",
            );
        };
        let mut db = match rusqlite::Connection::open(event_db) {
            Ok(value) => value,
            Err(failure) => {
                return error(
                    name,
                    "working_context_envelope_invalid",
                    failure.to_string(),
                )
            }
        };
        if let Err(failure) = db.execute_batch(
            "CREATE TABLE IF NOT EXISTS membrane_working_context(\
             context_id TEXT PRIMARY KEY,repository_id TEXT NOT NULL,scope_id TEXT NOT NULL,\
             session_id TEXT NOT NULL,task_id TEXT NOT NULL,payload_json TEXT NOT NULL,\
             payload_sha256 TEXT NOT NULL,expires_at TEXT NOT NULL,state TEXT NOT NULL,\
             created_at TEXT NOT NULL,closed_at TEXT) STRICT;",
        ) {
            return error(
                name,
                "working_context_envelope_invalid",
                failure.to_string(),
            );
        }
        match operation {
            "save" => {
                let Some(context) = arguments.get("context") else {
                    return error(
                        name,
                        "working_context_envelope_invalid",
                        "context is required",
                    );
                };
                if let Err(result) = bounded(context, name, "working_context_payload_too_large") {
                    return result;
                }
                let payload = match serde_json::to_string(context) {
                    Ok(value) => value,
                    Err(_) => {
                        return error(
                            name,
                            "working_context_envelope_invalid",
                            "context is invalid",
                        )
                    }
                };
                let digest = crate::digest::digest_str(&payload);
                let tx = match db.transaction() {
                    Ok(value) => value,
                    Err(failure) => {
                        return error(
                            name,
                            "working_context_envelope_invalid",
                            failure.to_string(),
                        )
                    }
                };
                if let Err(failure) = tx.execute(
                    "INSERT INTO membrane_working_context(context_id,repository_id,scope_id,session_id,task_id,payload_json,payload_sha256,expires_at,state,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,strftime('%Y-%m-%dT%H:%M:%fZ','now','+24 hours'),'active',strftime('%Y-%m-%dT%H:%M:%fZ','now')) ON CONFLICT(context_id) DO UPDATE SET payload_json=excluded.payload_json,payload_sha256=excluded.payload_sha256,expires_at=excluded.expires_at WHERE repository_id=excluded.repository_id AND scope_id=excluded.scope_id AND session_id=excluded.session_id AND task_id=excluded.task_id AND state='active'",
                    params![context_id,repository,scope,session,task,payload,digest],
                ) { return error(name, "working_context_scope_denied", failure.to_string()); }
                if tx.commit().is_err() {
                    return error(
                        name,
                        "working_context_envelope_invalid",
                        "working context commit failed",
                    );
                }
                success(
                    name,
                    json!({"status":"saved","operation":"save","contextId":context_id,"context":context}),
                )
            }
            "load" => {
                let mut statement = match db.prepare("SELECT context_id,payload_json FROM membrane_working_context WHERE repository_id=?1 AND scope_id=?2 AND session_id=?3 AND task_id=?4 AND state='active' AND expires_at>strftime('%Y-%m-%dT%H:%M:%fZ','now') ORDER BY created_at,context_id LIMIT 256") { Ok(value) => value, Err(failure) => return error(name, "working_context_envelope_invalid", failure.to_string()) };
                let rows = statement.query_map(params![repository, scope, session, task], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                });
                let contexts = match rows {
                    Ok(rows) => rows
                        .filter_map(Result::ok)
                        .filter_map(|(id, payload)| {
                            serde_json::from_str::<Value>(&payload)
                                .ok()
                                .map(|mut value| {
                                    if value.get("contextId").is_none() {
                                        value["contextId"] = Value::String(id);
                                    }
                                    value
                                })
                        })
                        .collect::<Vec<_>>(),
                    Err(failure) => {
                        return error(
                            name,
                            "working_context_envelope_invalid",
                            failure.to_string(),
                        )
                    }
                };
                success(
                    name,
                    json!({"status":"loaded","operation":"load","contexts":contexts}),
                )
            }
            _ => {
                let changed = db.execute("UPDATE membrane_working_context SET state='closed',closed_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE context_id=?1 AND repository_id=?2 AND scope_id=?3 AND session_id=?4 AND task_id=?5 AND state='active'", params![context_id,repository,scope,session,task]).unwrap_or(0);
                success(
                    name,
                    json!({"status":"closed","operation":"close","contextId":context_id,"closed":changed == 1}),
                )
            }
        }
    }

    fn diagnostic(&self, name: &str, arguments: &Value, repository: &str, scope: &str) -> Value {
        let operation = arguments
            .get("operation")
            .and_then(Value::as_str)
            .unwrap_or("status");
        let mut service = match self.diagnostics.lock() {
            Ok(value) => value,
            Err(_) => {
                return error(
                    name,
                    "diagnostic_unavailable",
                    "diagnostic service lock unavailable",
                )
            }
        };
        let worktree = arguments
            .get("worktreeId")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .unwrap_or(scope);
        let result = (|| -> Result<Value, crate::LiveDiagnosticsServiceError> {
            match name {
                "membrane_diagnostic_workspace" => match operation {
                    "open" => service.workspace_open(
                        repository,
                        worktree,
                        arguments.get("projectRoot").and_then(Value::as_str),
                    ),
                    "close" => service.workspace_close(repository, worktree),
                    "status" => service.workspace_status(repository, worktree),
                    "reconcile" => {
                        let manifest = arguments
                            .get("manifestDigest")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        let hashes = serde_json::from_value::<
                            Vec<membrane_protocol::diagnostics::ChangedFileHashV1>,
                        >(
                            arguments
                                .get("hashes")
                                .cloned()
                                .unwrap_or_else(|| json!([])),
                        )
                        .map_err(|failure| {
                            crate::LiveDiagnosticsServiceError::Provider(failure.to_string())
                        })?;
                        service
                            .workspace_reconcile(repository, worktree, manifest, &hashes)
                            .map(|classification| json!({"classification":classification}))
                    }
                    _ => Err(crate::LiveDiagnosticsServiceError::Provider(
                        "invalid diagnostic workspace operation".into(),
                    )),
                },
                "membrane_diagnostic_mutation" => match operation {
                    "begin" => service.mutation_begin(repository, worktree),
                    "abort" => service.mutation_abort(repository, worktree),
                    "seal" | "registerObserved" => {
                        let epoch = serde_json::from_value::<
                            membrane_protocol::diagnostics::WorkspaceEpochV1,
                        >(
                            arguments.get("epoch").cloned().unwrap_or(Value::Null)
                        )
                        .map_err(|failure| {
                            crate::LiveDiagnosticsServiceError::Provider(failure.to_string())
                        })?;
                        if operation == "seal" {
                            service.mutation_seal(repository, worktree, epoch)
                        } else {
                            service.mutation_register_observed(repository, worktree, epoch)
                        }
                    }
                    _ => Err(crate::LiveDiagnosticsServiceError::Provider(
                        "invalid diagnostic mutation operation".into(),
                    )),
                },
                "membrane_diagnostic_capabilities" => Ok(service.capabilities()),
                "membrane_diagnostic_provider" => {
                    let key = arguments
                        .get("keyDigest")
                        .or_else(|| arguments.get("provider"))
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    match operation {
                        "list" => Ok(service.provider_list()),
                        "status" => service.provider_status(key),
                        "restart" => service.provider_restart(key),
                        _ => Err(crate::LiveDiagnosticsServiceError::Provider(
                            "invalid diagnostic provider operation".into(),
                        )),
                    }
                }
                "membrane_diagnostic_snapshot" => {
                    match operation {
                        "await" => {
                            let required_capabilities = serde_json::from_value(
                                arguments
                                    .get("requiredCapabilities")
                                    .cloned()
                                    .unwrap_or_else(|| json!([])),
                            )
                            .map_err(|failure| {
                                crate::LiveDiagnosticsServiceError::Provider(format!(
                                    "invalid requiredCapabilities: {failure}"
                                ))
                            })?;
                            let max_cost = arguments
                                .get("maxCost")
                                .cloned()
                                .map(serde_json::from_value)
                                .transpose()
                                .map_err(|failure| {
                                    crate::LiveDiagnosticsServiceError::Provider(format!(
                                        "invalid maxCost: {failure}"
                                    ))
                                })?;
                            let request = crate::SnapshotAwaitRequest {
                        repo_id: repository.to_owned(), worktree_id: worktree.to_owned(),
                        policy_profile_name: arguments.get("policyProfileName").and_then(Value::as_str).unwrap_or(crate::live_diagnostics_service::DEFAULT_POLICY_PROFILE_NAME).to_owned(),
                        required_capabilities, max_cost,
                        deadline_ms: arguments.get("deadlineMs").and_then(Value::as_u64),
                    };
                            service.snapshot_await(&request).and_then(|value| {
                                serde_json::to_value(value).map_err(|failure| {
                                    crate::LiveDiagnosticsServiceError::Provider(
                                        failure.to_string(),
                                    )
                                })
                            })
                        }
                        "get" => service.snapshot_get(repository, worktree),
                        "explain" => service.snapshot_explain(repository, worktree),
                        "delta" => service.snapshot_delta(repository, worktree),
                        _ => Err(crate::LiveDiagnosticsServiceError::Provider(
                            "invalid diagnostic snapshot operation".into(),
                        )),
                    }
                }
                "membrane_diagnostic_fence" => {
                    let snapshot = serde_json::from_value::<
                        membrane_protocol::diagnostics::DiagnosticEvidenceSnapshotV1,
                    >(
                        arguments.get("snapshot").cloned().unwrap_or(Value::Null)
                    )
                    .map_err(|failure| {
                        crate::LiveDiagnosticsServiceError::Provider(failure.to_string())
                    })?;
                    let epoch =
                        serde_json::from_value::<membrane_protocol::diagnostics::WorkspaceEpochV1>(
                            arguments
                                .get("expectedEpoch")
                                .cloned()
                                .unwrap_or(Value::Null),
                        )
                        .map_err(|failure| {
                            crate::LiveDiagnosticsServiceError::Provider(failure.to_string())
                        })?;
                    let policy = serde_json::from_value::<
                        membrane_protocol::diagnostics::GatePolicyProfileV1,
                    >(
                        arguments.get("policy").cloned().unwrap_or(Value::Null)
                    )
                    .map_err(|failure| {
                        crate::LiveDiagnosticsServiceError::Provider(failure.to_string())
                    })?;
                    serde_json::to_value(DiagnosticsService::evaluate_fence(
                        &snapshot, &epoch, &policy,
                    ))
                    .map_err(|failure| {
                        crate::LiveDiagnosticsServiceError::Provider(failure.to_string())
                    })
                }
                "membrane_diagnostic_baseline" => {
                    let baseline = arguments.get("name").and_then(Value::as_str).unwrap_or("");
                    match operation {
                        "capture" => service.baseline_capture(repository, worktree, baseline),
                        "update" => service.baseline_update(repository, worktree, baseline),
                        _ => Err(crate::LiveDiagnosticsServiceError::Provider(
                            "invalid diagnostic baseline operation".into(),
                        )),
                    }
                }
                _ => Err(crate::LiveDiagnosticsServiceError::Provider(
                    "unknown diagnostic operation".to_owned(),
                )),
            }
        })();
        match result {
            Ok(data) => success(name, data),
            Err(e) => error(name, e.code(), e.to_string()),
        }
    }
}

static NATIVE_EXECUTOR: OnceLock<Arc<dyn NativeMcpExecutor>> = OnceLock::new();

/// Install exactly one Hub-owned executor. Only active Hub opens Cortex.
pub fn install_native_mcp_executor_for_hub(store: MemoryStore) -> Result<(), String> {
    if NATIVE_EXECUTOR.get().is_some() {
        return Ok(());
    }
    let executor: Arc<dyn NativeMcpExecutor> = Arc::new(RuntimeMcpExecutor::for_hub(store)?);
    membrane_mcp::install_executor(executor.clone())
        .map_err(|_| "native MCP executor already owned by another runtime".to_owned())?;
    let _ = NATIVE_EXECUTOR.set(executor);
    Ok(())
}

/// Install stateless stdio transport to active Hub. This process never opens
/// Cortex, Blueprint storage, or a second runtime.
pub fn install_native_mcp_transport() -> Result<(), String> {
    if NATIVE_EXECUTOR.get().is_some() {
        return Ok(());
    }
    let executor: Arc<dyn NativeMcpExecutor> = match HubTransportExecutor::active() {
        Ok(active) => Arc::new(active),
        Err(failure) => Arc::new(UnavailableHubTransportExecutor { failure }),
    };
    membrane_mcp::install_executor(executor.clone())
        .map_err(|_| "native MCP executor already owned by another runtime".to_owned())?;
    let _ = NATIVE_EXECUTOR.set(executor);
    Ok(())
}

#[cfg(test)]
mod hub_transport_tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn hub_off_blueprint_is_membrane_unavailable_not_blueprint_unavailable() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("reserve port");
        let port = listener.local_addr().expect("reserved address").port();
        drop(listener);
        let executor = HubTransportExecutor {
            port,
            installation_id: "installation-test".into(),
            cortex_store_id: "store-test".into(),
            release_generation: "release-test".into(),
            session_id: "session-test".into(),
            token: "token-test".into(),
        };
        let response = executor.execute("membrane_blueprint", &json!({}));
        assert_eq!(
            response.pointer("/result/code").and_then(Value::as_str),
            Some("membrane_unavailable")
        );
        assert!(response
            .pointer("/result/message")
            .and_then(Value::as_str)
            .is_some_and(|message| message.contains("hub_inactive")));
    }

    #[test]
    fn missing_hub_binding_installs_typed_unavailable_executor() {
        let executor = UnavailableHubTransportExecutor {
            failure: "identity binding missing".into(),
        };
        let response = executor.execute("membrane_context", &json!({}));
        assert_eq!(
            response.pointer("/result/code").and_then(Value::as_str),
            Some("membrane_unavailable")
        );
        assert!(response
            .pointer("/result/message")
            .and_then(Value::as_str)
            .is_some_and(|message| message.contains("hub_inactive")));
    }
}
