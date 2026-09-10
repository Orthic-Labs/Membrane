//! Typed bounded-owner binding, framing & no-replay policy. Hosts inject process effects.
use crate::{CallOptions, ClientError, CompatibilityRequirement, KnownCandidate};
use serde_json::{Map, Value};
use std::{path::PathBuf, sync::Arc};
pub use membrane_protocol::explicit::{ExplicitOperation, ExplicitOwnerBindingV1, ExplicitOwnerMode};
use membrane_protocol::explicit::{ExplicitRequestV1, ExplicitResponseV1, EXPLICIT_MAX_BYTES};

/// Host must launch this exact executable with these argv, close stdin after input,
/// bound stdout/stderr, & terminate/reap its process tree on deadline/cancellation.
/// No shell, Hub startup, health synthesis, retry, or caller-selected executable.
pub struct ExplicitInvocation {
    pub executable: PathBuf,
    pub arguments: [&'static str; 2],
    pub input: Vec<u8>,
    pub max_output_bytes: usize,
}

#[derive(Debug)]
pub struct ExplicitTransportFailure {
    pub error: ClientError,
    /// True once any action input was sent; uncertainty then cannot be replayed.
    pub action_dispatched: bool,
}

pub type ExplicitOwnerTransport = dyn Fn(&ExplicitInvocation, &CallOptions)
    -> Result<Vec<u8>, ExplicitTransportFailure> + Send + Sync;

pub struct InstalledExplicitClient {
    candidate: KnownCandidate,
    binding: ExplicitOwnerBindingV1,
    transport: Arc<ExplicitOwnerTransport>,
}

/// Authenticated response from native ADP-076 comparison. Decision & receipt
/// remain JSON so client crate does not duplicate membrane-adapt domain types.
#[derive(Debug, Clone)]
pub struct AdaptComparisonResponse {
    pub decision: Value,
    pub receipt: Value,
}

fn incompatible(message: &str) -> ClientError { ClientError::Incompatible { message: message.into() } }

pub fn verify_explicit_binding(candidate: &KnownCandidate, binding: &ExplicitOwnerBindingV1,
    requirement: &CompatibilityRequirement) -> Result<(), ClientError> {
    if binding.schema_version != requirement.schema_version || binding.protocol_version != requirement.protocol_version
        || binding.mode != ExplicitOwnerMode::BoundedExplicit
        || crate::binding::comparable_stable_root(&binding.stable_install_root)
            != crate::binding::comparable_stable_root(&candidate.stable_install_root)
        || binding.installation_id.trim().is_empty() || binding.cortex_store_id.trim().is_empty()
        || binding.release_generation.trim().is_empty() || binding.embedder_dim == 0
        || requirement.require_native_only && !binding.native_only
        || requirement.required_runtime_origin.as_deref().is_some_and(|origin| origin != "installed")
        || candidate.expected_installation_id.as_deref().is_some_and(|id| id != binding.installation_id)
        || candidate.expected_startup_generation.is_some_and(|epoch| epoch != binding.startup_generation)
        || requirement.installation_id.as_deref().is_some_and(|id| id != binding.installation_id)
        || requirement.cortex_store_id.as_deref().is_some_and(|id| id != binding.cortex_store_id)
        || requirement.release_generation.as_deref().is_some_and(|id| id != binding.release_generation)
        || requirement.required_subsystems.iter().any(|v| !binding.subsystems.contains(v))
        || requirement.required_capabilities.iter().any(|v| !binding.capabilities.contains(v)) {
        return Err(incompatible("explicit owner identity or compatibility mismatch"));
    }
    Ok(())
}

fn after_dispatch(operation: ExplicitOperation, failure: ClientError) -> ClientError {
    if operation.may_mutate() {
        ClientError::CommitUnknown { message: format!("bounded operation outcome unknown: {failure}"), receipt_id: None }
    } else { failure }
}

fn invoke(candidate: &KnownCandidate, transport: &ExplicitOwnerTransport, request: ExplicitRequestV1,
    options: &CallOptions) -> Result<ExplicitResponseV1, ClientError> {
    options.check()?;
    let input = serde_json::to_vec(&request).map_err(|e| ClientError::InvalidRequest { message: e.to_string() })?;
    if input.len() > EXPLICIT_MAX_BYTES { return Err(ClientError::InvalidRequest { message: "explicit request too large".into() }); }
    let invocation = ExplicitInvocation {
        executable: PathBuf::from(&candidate.stable_install_root).join(if cfg!(windows) { "membrane.exe" } else { "membrane" }),
        arguments: ["cli", "explicit-call"], input, max_output_bytes: EXPLICIT_MAX_BYTES,
    };
    let output = transport(&invocation, options).map_err(|failure| {
        if failure.action_dispatched { after_dispatch(request.operation, failure.error) } else { failure.error }
    })?;
    options.check().map_err(|error| after_dispatch(request.operation, error))?;
    if output.len() > EXPLICIT_MAX_BYTES {
        return Err(after_dispatch(request.operation, ClientError::protocol("response_oversized", "explicit response too large")));
    }
    let response: ExplicitResponseV1 = serde_json::from_slice(&output)
        .map_err(|_| after_dispatch(request.operation, ClientError::protocol("response_malformed", "invalid explicit response")))?;
    if response.schema_version != 1 || !(100..=599).contains(&response.status) {
        return Err(after_dispatch(request.operation, incompatible("explicit response version/status unsupported")));
    }
    if request.expected_binding.as_ref().is_some_and(|binding| binding != &response.binding) {
        // A typed refusal proves that owner rejected the fence before execution.
        if response.status == 409 && response.data.get("code").and_then(Value::as_str) == Some("corrupt_or_rotation") {
            return Err(ClientError::CorruptOrRotation { message: "installed owner changed before dispatch".into() });
        }
        return Err(after_dispatch(request.operation, incompatible("explicit response identity changed")));
    }
    if response.status >= 400 {
        let failure = ClientError::from_json_error(&response.data);
        return Err(if response.status >= 500 { after_dispatch(request.operation, failure) } else { failure });
    }
    Ok(response)
}

impl InstalledExplicitClient {
    pub fn connect(candidate: KnownCandidate, requirement: &CompatibilityRequirement,
        transport: Arc<ExplicitOwnerTransport>, options: &CallOptions) -> Result<Self, ClientError> {
        let response = invoke(&candidate, transport.as_ref(), ExplicitRequestV1 {
            schema_version: 1, operation: ExplicitOperation::Binding, expected_binding: None, request: Map::new(),
        }, options)?;
        verify_explicit_binding(&candidate, &response.binding, requirement)?;
        Ok(Self { candidate, binding: response.binding, transport })
    }
    pub fn binding(&self) -> &ExplicitOwnerBindingV1 { &self.binding }
    pub fn call(&self, operation: ExplicitOperation, request: Map<String, Value>, options: &CallOptions) -> Result<Value, ClientError> {
        let response = invoke(&self.candidate, self.transport.as_ref(), ExplicitRequestV1 {
            schema_version: 1, operation, expected_binding: Some(self.binding.clone()), request,
        }, options)?;
        if response.data.get("error").is_some() || response.data.get("kind").and_then(Value::as_str) == Some("error") {
            return Err(ClientError::from_json_error(&response.data));
        }
        Ok(response.data)
    }
    /// Invoke native ADP-076 comparison through the bound installed owner.
    pub fn compare_adapt(&self, request: Map<String, Value>, options: &CallOptions) -> Result<AdaptComparisonResponse, ClientError> {
        let data = self.call(ExplicitOperation::AdaptCompare, request, options)?;
        let object = data.as_object().ok_or_else(|| ClientError::protocol("response_malformed", "ADP-076 response is not an object"))?;
        let decision = object.get("decision").cloned().ok_or_else(|| ClientError::protocol("response_malformed", "ADP-076 response lacks decision"))?;
        let receipt = object.get("receipt").cloned().ok_or_else(|| ClientError::protocol("response_malformed", "ADP-076 response lacks receipt"))?;
        Ok(AdaptComparisonResponse { decision, receipt })
    }
    pub(crate) fn memory_call(&self, route: &str, request: &Map<String, Value>, options: &CallOptions) -> Result<Value, ClientError> {
        use ExplicitOperation as Op;
        let operation = match route {
            "/activity" if request.keys().all(|key| key == "limit") => Op::ActivityRead,
            "/activity" => Op::Activity, "/delete" => Op::Delete, "/federate" => Op::Federate,
            "/get" => Op::Get, "/list" => Op::List, "/metrics" => Op::Metrics,
            "/put" => Op::Put, "/recall" => Op::Recall, "/remember" => Op::Remember,
            "/remember_consolidated" => Op::RememberConsolidated, "/scopes" => Op::Scopes,
            "/search" => Op::Search, "/use" => Op::Use,
            _ => return Err(ClientError::InvalidRequest { message: "unsupported explicit memory operation".into() }),
        };
        self.call(operation, request.clone(), options)
    }
}
