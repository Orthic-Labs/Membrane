//! Storage-free native Blueprint API boundary.
//!
//! The boundary owns protocol validation, caps, identity binding, deadline,
//! cancellation, and typed failure mapping.  Graph/storage implementations
//! sit behind [`BlueprintOperation`]; callers only exchange values.

use crate::model::{RepositoryScope, PROTOCOL_VERSION, DEFAULT_CANDIDATE_CAP,
    DEFAULT_DEADLINE_MS, DEFAULT_PATH_CAP, MAX_BUILD_DEADLINE_MS, MAX_CANDIDATE_CAP,
    MAX_DAEMON_FRAME_BYTES, MAX_ONE_SHOT_REQUEST_BYTES, MAX_PATH_CAP, MAX_PATH_LENGTH, MAX_RESPONSE_BYTES,
    MAX_DEADLINE_MS, MIN_DEADLINE_MS};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;
use std::collections::BTreeSet;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::time::{Duration, Instant};

pub use crate::model::{FreshnessBinding, FreshnessState, GenerationBinding, GraphEdge, GraphNode, Operation};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    pub max_request_bytes: usize,
    pub max_frame_bytes: usize,
    pub max_response_bytes: usize,
    pub max_candidates: usize,
    pub max_paths: usize,
}

pub type BlueprintBounds = Bounds;
pub type BlueprintWireRequest = BlueprintRequest;
pub type BlueprintWireResponse = BlueprintResponse;

impl Default for Bounds {
    fn default() -> Self { Self { max_request_bytes: MAX_ONE_SHOT_REQUEST_BYTES, max_frame_bytes: MAX_DAEMON_FRAME_BYTES, max_response_bytes: MAX_RESPONSE_BYTES, max_candidates: DEFAULT_CANDIDATE_CAP, max_paths: DEFAULT_PATH_CAP } }
}

impl Bounds {
    pub fn one_shot() -> Self { Self { max_frame_bytes: MAX_ONE_SHOT_REQUEST_BYTES, ..Self::default() } }
    pub fn daemon() -> Self { Self::default() }

    pub fn bounded(self) -> Self {
        Self {
            max_request_bytes: self.max_request_bytes.clamp(1, MAX_ONE_SHOT_REQUEST_BYTES),
            max_frame_bytes: self.max_frame_bytes.clamp(1, MAX_ONE_SHOT_REQUEST_BYTES),
            max_response_bytes: self.max_response_bytes.clamp(1, MAX_RESPONSE_BYTES),
            max_candidates: self.max_candidates.clamp(1, MAX_CANDIDATE_CAP),
            max_paths: self.max_paths.clamp(1, MAX_PATH_CAP),
        }
    }
    pub fn validate(self) -> Result<Self, BlueprintError> {
        let bounded = self.bounded();
        if self.max_request_bytes != bounded.max_request_bytes || self.max_frame_bytes != bounded.max_frame_bytes || self.max_response_bytes != bounded.max_response_bytes || self.max_candidates != bounded.max_candidates || self.max_paths != bounded.max_paths {
            return Err(BlueprintError::invalid("bounds exceed native Blueprint caps"));
        }
        Ok(bounded)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BlueprintRequest {
    pub protocol_version: u32,
    pub request_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<String>,
    pub method: Operation,
    pub deadline_ms: u64,
    pub input: Value,
}

impl BlueprintRequest {
    pub fn new(request_id: impl Into<String>, method: Operation, repo_root: impl Into<String>) -> Self {
        Self { protocol_version: PROTOCOL_VERSION, request_id: request_id.into(), repo_id: None, generation: None, method, deadline_ms: DEFAULT_DEADLINE_MS, input: serde_json::json!({"repoRoot": repo_root.into()}) }
    }

    pub fn scope(&self) -> Result<RepositoryScope, BlueprintError> {
        let root = self.input.get("repoRoot").and_then(Value::as_str).filter(|v| !v.trim().is_empty()).ok_or_else(|| BlueprintError::missing("input.repoRoot"))?;
        Ok(RepositoryScope { repo_root: root.into(), repo_id: self.repo_id.clone().or_else(|| self.input.get("repoId").and_then(Value::as_str).map(str::to_owned)), worktree: self.input.get("worktree").and_then(Value::as_str).map(str::to_owned), generation: self.generation.clone().or_else(|| self.input.get("generation").and_then(Value::as_str).map(str::to_owned)), paths: self.input.get("paths").and_then(Value::as_array).map(|values| values.iter().filter_map(Value::as_str).map(str::to_owned).collect()) })
    }

    pub fn validate(&self, bounds: Bounds) -> Result<RequestContext, BlueprintError> {
        let bounds = bounds.validate()?;
        if self.protocol_version != PROTOCOL_VERSION { return Err(BlueprintError::protocol(self.protocol_version)); }
        if self.request_id.trim().is_empty() || self.request_id.len() > 256 { return Err(BlueprintError::invalid("requestId must be non-empty and at most 256 bytes")); }
        if !self.input.is_object() { return Err(BlueprintError::invalid("input must be an object")); }
        validate_input_bounds(&self.input, bounds)?;
        let scope = self.scope()?;
        if let (Some(top), Some(inner)) = (self.generation.as_deref(), self.input.get("generation").and_then(Value::as_str)) {
            if top != inner { return Err(BlueprintError::generation_mismatch(top, inner)); }
        }
        if let (Some(top), Some(inner)) = (self.repo_id.as_deref(), self.input.get("repoId").and_then(Value::as_str)) {
            if top != inner { return Err(BlueprintError::invalid("repoId does not match input.repoId")); }
        }
        Deadline::from_millis(self.deadline_ms, self.method)?;
        let encoded = serde_json::to_vec(self).map_err(|e| BlueprintError::malformed(e.to_string()))?;
        if encoded.len() > bounds.max_request_bytes || encoded.len() > bounds.max_frame_bytes { return Err(BlueprintError::oversized("request_frame")); }
        Ok(RequestContext { scope, deadline: Deadline::from_millis(self.deadline_ms, self.method)?, cancellation: CancellationToken::new(), bounds })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BlueprintResponse {
    pub protocol_version: u32,
    pub request_id: Option<String>,
    pub ok: bool,
    pub generation: Option<String>,
    pub result: Option<Value>,
    pub error: Option<BlueprintError>,
}

impl BlueprintResponse {
    pub fn success(request_id: impl Into<String>, generation: Option<String>, result: Value) -> Self { Self { protocol_version: PROTOCOL_VERSION, request_id: Some(request_id.into()), ok: true, generation, result: Some(result), error: None } }
    pub fn failure(request_id: Option<String>, error: BlueprintError) -> Self { Self { protocol_version: PROTOCOL_VERSION, request_id, ok: false, generation: None, result: None, error: Some(error) } }
    pub fn validate(&self, bounds: Bounds) -> Result<(), BlueprintError> {
        let bounds = bounds.validate()?;
        if self.protocol_version != PROTOCOL_VERSION { return Err(BlueprintError::protocol(self.protocol_version)); }
        if self.ok == self.error.is_some() || (self.ok && self.result.is_none()) || (!self.ok && self.result.is_some()) { return Err(BlueprintError::malformed("response success/error/result fields are inconsistent")); }
        if let Some(result) = self.result.as_ref() {
            validate_result_bounds(result, bounds)?;
        }
        let bytes = serde_json::to_vec(self).map_err(|e| BlueprintError::malformed(e.to_string()))?;
        if bytes.len() > bounds.max_response_bytes { return Err(BlueprintError::oversized("response_bytes")); }
        Ok(())
    }
}

fn validate_input_bounds(input: &Value, bounds: Bounds) -> Result<(), BlueprintError> {
    if let Some(paths) = input.get("paths") {
        validate_path_array(paths, bounds, "input.paths")?;
    }
    if let Some(candidates) = input.get("candidates") {
        validate_candidate_array(candidates, bounds)?;
    }
    if let Some(candidate_set) = input.get("candidateSet") {
        let candidates = candidate_set
            .get("candidates")
            .ok_or_else(|| BlueprintError::invalid("input.candidateSet.candidates is required"))?;
        validate_candidate_array(candidates, bounds)?;
    }
    Ok(())
}

fn validate_result_bounds(result: &Value, bounds: Bounds) -> Result<(), BlueprintError> {
    if let Some(paths) = result.get("paths") {
        validate_result_path_array(paths, bounds)?;
    }
    if let Some(recall_circuit) = result.get("recallCircuit") {
        if let Some(paths) = recall_circuit.get("paths") {
            validate_result_path_array(paths, bounds)?;
        }
    }
    if let Some(candidates) = result.get("candidates") {
        validate_candidate_array(candidates, bounds)?;
    }
    if let Some(candidate_set) = result.get("candidateSet") {
        let candidates = candidate_set
            .get("candidates")
            .ok_or_else(|| BlueprintError::malformed("candidateSet.candidates is required"))?;
        validate_candidate_array(candidates, bounds)?;
    }
    Ok(())
}

fn validate_result_path_array(value: &Value, bounds: Bounds) -> Result<(), BlueprintError> {
    let paths = value
        .as_array()
        .ok_or_else(|| BlueprintError::malformed("result.paths must be an array"))?;
    if paths.len() > bounds.max_paths {
        return Err(BlueprintError::oversized("path_count"));
    }
    for path in paths {
        let object = path
            .as_object()
            .ok_or_else(|| BlueprintError::malformed("result.paths entries must be objects"))?;
        for key in ["nodes", "edges"] {
            let value = object
                .get(key)
                .ok_or_else(|| BlueprintError::malformed(format!("result.paths entries require {key}")))?;
            if !value.is_array() {
                return Err(BlueprintError::malformed(format!("result.paths entries {key} must be arrays")));
            }
        }
        validate_embedded_paths(path, bounds)?;
    }
    Ok(())
}

fn validate_embedded_paths(value: &Value, bounds: Bounds) -> Result<(), BlueprintError> {
    match value {
        Value::Array(values) => {
            for value in values {
                validate_embedded_paths(value, bounds)?;
            }
        }
        Value::Object(object) => {
            for (key, value) in object {
                if matches!(key.as_str(), "path" | "sourceRef") {
                    if !(key == "path" && value.is_null()) {
                        let path = value
                            .as_str()
                            .ok_or_else(|| BlueprintError::malformed(format!("{key} must be a string")))?;
                        validate_path(path, "path_length")?;
                    }
                }
                validate_embedded_paths(value, bounds)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn validate_candidate_array(value: &Value, bounds: Bounds) -> Result<(), BlueprintError> {
    let candidates = value
        .as_array()
        .ok_or_else(|| BlueprintError::invalid("candidates must be an array"))?;
    if candidates.len() > bounds.max_candidates {
        return Err(BlueprintError::oversized("candidate_count"));
    }
    let mut paths = BTreeSet::new();
    for candidate in candidates {
        let Some(object) = candidate.as_object() else {
            return Err(BlueprintError::invalid("candidate must be an object"));
        };
        if let Some(source_ref) = object.get("sourceRef").or_else(|| object.get("path")) {
            let path = source_ref
                .as_str()
                .ok_or_else(|| BlueprintError::invalid("candidate path must be a string"))?;
            validate_path(path, "path_length")?;
            paths.insert(path.to_owned());
            if paths.len() > bounds.max_paths {
                return Err(BlueprintError::oversized("path_count"));
            }
        }
    }
    Ok(())
}

fn validate_path_array(value: &Value, bounds: Bounds, field: &'static str) -> Result<(), BlueprintError> {
    let paths = value
        .as_array()
        .ok_or_else(|| BlueprintError::invalid(format!("{field} must be an array")))?;
    let mut unique = BTreeSet::new();
    for path in paths {
        let path = path
            .as_str()
            .ok_or_else(|| BlueprintError::invalid(format!("{field} entries must be strings")))?;
        validate_path(path, "path_length")?;
        unique.insert(path.to_owned());
        if unique.len() > bounds.max_paths {
            return Err(BlueprintError::oversized("path_count"));
        }
    }
    Ok(())
}

fn validate_path(path: &str, what: &'static str) -> Result<(), BlueprintError> {
    if path.len() > MAX_PATH_LENGTH {
        return Err(BlueprintError::oversized(what));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BlueprintError {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
    #[serde(default)]
    pub retryable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remediation: Option<String>,
}

impl BlueprintError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self { Self { code: code.into(), message: message.into(), details: None, retryable: false, remediation: None } }
    pub fn invalid(message: impl Into<String>) -> Self { Self::new("invalid_request", message) }
    pub fn missing(field: impl Into<String>) -> Self { Self::new("required_field_missing", format!("required field missing: {}", field.into())) }
    pub fn malformed(message: impl Into<String>) -> Self { Self::new("blueprint_malformed", message) }
    pub fn oversized(what: &'static str) -> Self { Self::new("blueprint_oversized", format!("{what} exceeds configured bound")) }
    pub fn protocol(observed: u32) -> Self { Self::new("protocol_version_mismatch", format!("protocolVersion must equal {PROTOCOL_VERSION}, got {observed}")) }
    pub fn cancelled() -> Self { Self::new("request_cancelled", "request cancelled") }
    pub fn deadline() -> Self { Self::new("deadline_exceeded", "request deadline exceeded") }
    pub fn generation_mismatch(expected: impl Into<String>, observed: impl Into<String>) -> Self { let mut e = Self::new("generation_mismatch", "request generation does not match served generation"); e.details = Some(serde_json::json!({"expected":expected.into(),"observed":observed.into()})); e }
}

impl fmt::Display for BlueprintError { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{}: {}", self.code, self.message) } }
impl std::error::Error for BlueprintError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Deadline { started: Instant, duration: Duration }

impl Deadline {
    pub fn from_millis(value: u64, method: Operation) -> Result<Self, BlueprintError> {
        let max = if method.is_build() { MAX_BUILD_DEADLINE_MS } else { MAX_DEADLINE_MS };
        if !(MIN_DEADLINE_MS..=max).contains(&value) { return Err(BlueprintError::new("deadline_invalid", format!("deadlineMs must be an integer from {MIN_DEADLINE_MS} to {max}"))); }
        Ok(Self { started: Instant::now(), duration: Duration::from_millis(value) })
    }
    pub fn remaining(self) -> Duration { self.duration.saturating_sub(self.started.elapsed()) }
    pub fn expired(self) -> bool { self.remaining().is_zero() }
    pub fn duration(self) -> Duration { self.duration }
}

#[derive(Debug, Clone)]
pub struct CancellationToken { cancelled: Arc<AtomicBool>, deadline: Option<Instant> }
impl Default for CancellationToken { fn default() -> Self { Self { cancelled: Arc::new(AtomicBool::new(false)), deadline: None } } }
impl CancellationToken {
    pub fn new() -> Self { Self::default() }
    pub fn cancel(&self) { self.cancelled.store(true, Ordering::Release); }
    pub fn bind_deadline(&self, deadline: Deadline) -> Self {
        let bound = deadline.started + deadline.duration;
        Self { cancelled: Arc::clone(&self.cancelled), deadline: Some(self.deadline.map_or(bound, |current| current.min(bound))) }
    }
    pub fn is_cancelled(&self) -> bool { self.cancelled.load(Ordering::Acquire) || self.deadline_expired() }
    pub fn deadline_expired(&self) -> bool { !self.cancelled.load(Ordering::Acquire) && self.deadline.is_some_and(|deadline| Instant::now() >= deadline) }
}

#[derive(Debug, Clone)]
pub struct RequestContext { pub scope: RepositoryScope, pub deadline: Deadline, pub cancellation: CancellationToken, pub bounds: Bounds }
impl RequestContext { pub fn check(&self) -> Result<(), BlueprintError> { if self.cancellation.deadline_expired() { Err(BlueprintError::deadline()) } else if self.cancellation.is_cancelled() { Err(BlueprintError::cancelled()) } else if self.deadline.expired() { Err(BlueprintError::deadline()) } else { Ok(()) } } }

/// Implemented by native graph/service owners.  It owns no storage through
/// this boundary and is usable by one-shot as well as resident callers.
pub trait BlueprintOperation: Send + Sync {
    fn execute(&self, request: &BlueprintRequest, context: &RequestContext) -> Result<Value, BlueprintError>;
}

pub trait BlueprintApi: Send + Sync {
    fn dispatch(&self, request: BlueprintRequest, cancellation: CancellationToken) -> BlueprintResponse;
}

impl<T: BlueprintOperation + ?Sized> BlueprintApi for T {
    fn dispatch(&self, request: BlueprintRequest, cancellation: CancellationToken) -> BlueprintResponse {
        let mut response = match request.validate(Bounds::default()) {
            Ok(mut context) => { context.cancellation = cancellation.bind_deadline(context.deadline); match context.check().and_then(|_| self.execute(&request, &context)).and_then(|value| context.check().map(|_| value)) { Ok(value) => BlueprintResponse::success(request.request_id.clone(), request.generation.clone(), value), Err(error) => BlueprintResponse::failure(Some(request.request_id.clone()), error) } },
            Err(error) => BlueprintResponse::failure(Some(request.request_id.clone()), error),
        };
        if let Err(error) = response.validate(Bounds::default()) { response = BlueprintResponse::failure(response.request_id.clone(), error); }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(token: CancellationToken, deadline: Deadline) -> RequestContext {
        RequestContext {
            scope: RepositoryScope { repo_root: ".".into(), repo_id: None, worktree: None, generation: None, paths: None },
            deadline,
            cancellation: token,
            bounds: Bounds::default(),
        }
    }

    #[test]
    fn bound_deadline_is_observed_at_checkpoint_without_mutating_parent() {
        let parent = CancellationToken::new();
        let expired = Deadline { started: Instant::now() - Duration::from_millis(2), duration: Duration::from_millis(1) };
        let child = parent.bind_deadline(expired);
        let error = context(child.clone(), expired).check().unwrap_err();
        assert_eq!(error.code, "deadline_exceeded");
        assert!(child.deadline_expired());
        assert!(!parent.is_cancelled());
    }

    #[test]
    fn explicit_parent_cancellation_keeps_cancellation_classification() {
        let parent = CancellationToken::new();
        let deadline = Deadline { started: Instant::now(), duration: Duration::from_secs(30) };
        let child = parent.bind_deadline(deadline);
        parent.cancel();
        let error = context(child.clone(), deadline).check().unwrap_err();
        assert_eq!(error.code, "request_cancelled");
        assert!(!child.deadline_expired());
    }

    #[test]
    fn expired_child_does_not_poison_future_bound_request() {
        let parent = CancellationToken::new();
        let expired = Deadline { started: Instant::now() - Duration::from_millis(2), duration: Duration::from_millis(1) };
        assert!(parent.bind_deadline(expired).is_cancelled());
        let future = Deadline { started: Instant::now(), duration: Duration::from_secs(30) };
        let next = parent.bind_deadline(future);
        assert!(!next.is_cancelled());
        assert!(!parent.is_cancelled());
    }
}
