//! Thin, transport-injected client for canonical Membrane v1 operation envelopes.
//! This crate owns no policy, sockets, retries, or daemon lifecycle.

use serde_json::{Map, Value};
use std::collections::BTreeSet;

pub mod binding;
pub mod error;
pub mod handshake;
pub mod memory_backend;
pub mod records;
pub use binding::{
    bind_candidate, classify_known_candidate, ensure_action, CanonicalBinding, DiscoveryKind,
    DiscoveryOutcome, EnsureAction, KnownCandidate,
};
pub use error::ClientError;
pub use handshake::{CompatibilityRequirement, ServiceIdentity};
pub use memory_backend::{
    CallOptions, CancellationToken, MemoryBackendCall, MemoryBackendClient, MemoryTransport,
};
pub use records::{FullRecord, MemoryEntry, MemoryListRow, MemoryTier};

pub const ENVELOPE_VERSION: u64 = 1;
pub const ERROR_VERSION: u64 = 1;
pub const CONTEXT_OPERATION: &str = "membrane_context";
pub const SOURCE_READ_OPERATION: &str = "membrane_source_read";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    pub details: Map<String, Value>,
}

impl ProtocolError {
    fn new(code: &str, message: &str) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retryable: false,
            details: Map::new(),
        }
    }
}

pub type ProtocolResult<T> = Result<T, ProtocolError>;
pub type Transport = dyn Fn(&str, &Map<String, Value>) -> ProtocolResult<Value> + Send + Sync;

pub struct MembraneClient<T: ?Sized = Transport> {
    transport: Box<T>,
}

impl<T: ?Sized + Fn(&str, &Map<String, Value>) -> ProtocolResult<Value> + Send + Sync>
    MembraneClient<T>
{
    pub fn new(transport: Box<T>) -> Self {
        Self { transport }
    }

    pub fn context(&self, request: &Map<String, Value>) -> ProtocolResult<Map<String, Value>> {
        self.call(CONTEXT_OPERATION, request)
    }

    pub fn source_read(&self, request: &Map<String, Value>) -> ProtocolResult<Map<String, Value>> {
        self.call(SOURCE_READ_OPERATION, request)
    }

    pub fn call(
        &self,
        operation: &str,
        request: &Map<String, Value>,
    ) -> ProtocolResult<Map<String, Value>> {
        if error_codes(operation).is_none() {
            return Err(ProtocolError::new(
                "operation_unsupported",
                "unsupported operation",
            ));
        }
        let response = (self.transport)(operation, request)?;
        validate_response(operation, response)
    }
}

fn object(value: Value, label: &str) -> ProtocolResult<Map<String, Value>> {
    value.as_object().cloned().ok_or_else(|| {
        ProtocolError::new(
            "protocol_envelope_invalid",
            &format!("{label} must be an object"),
        )
    })
}

fn closed(value: &Map<String, Value>, allowed: &[&str], label: &str) -> ProtocolResult<()> {
    let allowed: BTreeSet<_> = allowed.iter().copied().collect();
    if value.keys().any(|key| !allowed.contains(key.as_str())) {
        return Err(ProtocolError::new(
            "protocol_envelope_invalid",
            &format!("{label} has unknown fields"),
        ));
    }
    Ok(())
}

fn validate_response(operation: &str, response: Value) -> ProtocolResult<Map<String, Value>> {
    let response = object(response, "response")?;
    closed(
        &response,
        &["schemaVersion", "operation", "errorVersion", "result"],
        "response",
    )?;
    if response.get("schemaVersion").and_then(Value::as_u64) != Some(ENVELOPE_VERSION)
        || response.get("errorVersion").and_then(Value::as_u64) != Some(ERROR_VERSION)
    {
        return Err(ProtocolError::new(
            "protocol_version_unsupported",
            "unsupported operation envelope version",
        ));
    }
    if response.get("operation").and_then(Value::as_str) != Some(operation) {
        return Err(ProtocolError::new(
            "protocol_envelope_invalid",
            "response operation does not match request",
        ));
    }
    let result = object(
        response.get("result").cloned().unwrap_or(Value::Null),
        "result",
    )?;
    match result.get("kind").and_then(Value::as_str) {
        Some("success") => {
            closed(&result, &["kind", "data"], "success result")?;
            object(
                result.get("data").cloned().unwrap_or(Value::Null),
                "result.data",
            )
        }
        Some("error") => {
            closed(
                &result,
                &["kind", "code", "message", "retryable", "details"],
                "error result",
            )?;
            let code = result
                .get("code")
                .and_then(Value::as_str)
                .filter(|code| error_codes(operation).is_some_and(|codes| codes.contains(code)));
            let message = result
                .get("message")
                .and_then(Value::as_str)
                .filter(|message| !message.is_empty());
            let retryable = result.get("retryable").and_then(Value::as_bool);
            let details = match result.get("details") {
                Some(value) => object(value.clone(), "error details")?,
                None => Map::new(),
            };
            match (code, message, retryable) {
                (Some(code), Some(message), Some(retryable)) => Err(ProtocolError {
                    code: code.into(),
                    message: message.into(),
                    retryable,
                    details,
                }),
                _ => Err(ProtocolError::new(
                    "protocol_envelope_invalid",
                    "unknown or malformed typed error",
                )),
            }
        }
        _ => Err(ProtocolError::new(
            "protocol_envelope_invalid",
            "result kind must be success or error",
        )),
    }
}

fn error_codes(operation: &str) -> Option<&'static [&'static str]> {
    match operation {
        CONTEXT_OPERATION => Some(&[
            "context_unavailable",
            "context_scope_denied",
            "context_workspace_no_repos",
            "context_workspace_abstained",
            "context_deadline_exceeded",
            "context_caller_scope_binding_denied",
            "context_caller_not_authorized",
            "context_cross_root_denied",
            "context_target_denied",
            "context_envelope_invalid",
        ]),
        SOURCE_READ_OPERATION => Some(&[
            "source_read_unavailable",
            "source_read_hash_mismatch",
            "source_read_anchor_missing",
            "source_read_scope_denied",
            "source_read_envelope_invalid",
        ]),
        _ => None,
    }
}
