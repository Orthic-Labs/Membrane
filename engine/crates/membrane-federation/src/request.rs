//! Strict federation ingress validation and immutable request normalization.

use crate::root::{normalize_anchor_path, resolve_canonical_root, RootError, RootPathSource};
use membrane_protocol::{DeadlineBudget, FederationRequestV1, FEDERATION_REQUEST_SCHEMA_VERSION};
use serde_json::Value;
use std::collections::BTreeMap;

/// Stable field-level validation categories.  They are safe to expose at an
/// ingress boundary because they contain no request or filesystem content.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ValidationCode {
    #[error("schema version is unsupported")]
    SchemaVersion,
    #[error("required value is missing")]
    Missing,
    #[error("repository root is invalid")]
    InvalidRoot,
    #[error("repository identity does not match canonical root")]
    RepositoryIdentity,
    #[error("anchor is invalid")]
    InvalidAnchor,
    #[error("budget must be positive")]
    InvalidBudget,
    #[error("manifest digest is invalid")]
    InvalidManifestDigest,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("{field}: {code}")]
pub struct RequestValidationError {
    pub field: &'static str,
    pub code: ValidationCode,
}

impl RequestValidationError {
    pub const fn new(field: &'static str, code: ValidationCode) -> Self {
        Self { field, code }
    }

    pub const fn field_path(&self) -> &'static str {
        self.field
    }
}

/// An anchor retained as intent, never as resolved repository content.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NormalizedAnchor {
    pub original: String,
    /// Relative lexical path for file-like anchors, or normalized symbolic
    /// intent.  It is safe to pass to a later owner for resolution.
    pub value: String,
}

/// Validated request owned by the federation coordinator.
///
/// Construction is the only normalization boundary.  The request owns all
/// strings and collections, so no caller can mutate the serialized input via
/// an alias after validation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NormalizedFederationRequest {
    pub schema_version: u32,
    pub request_id: String,
    pub trace_id: String,
    pub task: String,
    pub repository_root: String,
    pub repository_id: String,
    pub worktree_root: String,
    pub client: String,
    pub session_id: String,
    pub nonce: Option<String>,
    pub deadline_ms: u64,
    pub max_tokens: u32,
    pub deadline: DeadlineBudget,
    pub anchors: Vec<NormalizedAnchor>,
    pub scope_grant_id: Option<String>,
    pub manifest_digest: Option<String>,
    pub release_generation: Option<String>,
    pub blueprint_generation: Option<String>,
    pub skills_generation: Option<String>,
    pub extensions: BTreeMap<String, Value>,
}

impl NormalizedFederationRequest {
    pub fn normalize<S: RootPathSource>(
        request: &FederationRequestV1,
        source: &S,
    ) -> Result<Self, RequestValidationError> {
        normalize_request(request, source)
    }

    pub fn validate<S: RootPathSource>(
        request: &FederationRequestV1,
        source: &S,
    ) -> Result<Self, RequestValidationError> {
        Self::normalize(request, source)
    }
}

/// Normalize a serialized request before any provider or source scheduling.
/// Validation order is intentionally contract-stable.
pub fn normalize_request<S: RootPathSource>(
    request: &FederationRequestV1,
    source: &S,
) -> Result<NormalizedFederationRequest, RequestValidationError> {
    // 1. Schema/version.
    if request.schema_version != FEDERATION_REQUEST_SCHEMA_VERSION {
        return Err(RequestValidationError::new(
            "schemaVersion",
            ValidationCode::SchemaVersion,
        ));
    }

    // 2. Required identities.  Trim only at this boundary; all later values
    // are owned strings and cannot change underneath the coordinator.
    let request_id = required("requestId", &request.request_id)?;
    let task = required("task", &request.task)?;
    let requested_root = required("repositoryRoot", &request.repository_root)?;
    let client = required("client", &request.client)?;
    let session_id = required("sessionId", &request.session_id)?;
    let trace_id = request.trace_id.trim().to_owned();

    // 3. Canonical absolute root.  The injected source performs directory,
    // alias, traversal, and worktree confinement checks before it returns.
    let canonical = resolve_canonical_root(source, requested_root.as_str())
        .map_err(|error| root_error("repositoryRoot", error))?;

    // 4. Repository identity binding.  Optional transport extensions are
    // checked when present; absent identity is derived by the root owner.
    validate_identity_extension(
        &request.extensions,
        &["repositoryId", "repository_id"],
        &canonical.repository_id,
        ValidationCode::RepositoryIdentity,
    )?;
    validate_worktree_extension(&request.extensions, &canonical.worktree_root)?;

    let nonce = optional_string(&request.extensions, "nonce")?;

    // 5. Anchor normalization is lexical only: no path exists/read is done.
    let mut anchors = Vec::with_capacity(request.anchors.len());
    for (index, anchor) in request.anchors.iter().enumerate() {
        let original = anchor.trim();
        if original.is_empty() {
            return Err(RequestValidationError::new(
                "anchors",
                ValidationCode::InvalidAnchor,
            ));
        }
        let value = normalize_anchor_path(&canonical.path, original).map_err(|_| {
            let _ = index; // keep index available for debugger-facing callers
            RequestValidationError::new("anchors", ValidationCode::InvalidAnchor)
        })?;
        anchors.push(NormalizedAnchor {
            original: original.to_owned(),
            value,
        });
    }

    // 6. Deadline/token budget.
    if request.deadline_ms == 0 || request.max_tokens == 0 {
        return Err(RequestValidationError::new(
            "deadlineMs",
            ValidationCode::InvalidBudget,
        ));
    }
    let deadline = DeadlineBudget::from_millis(request.deadline_ms);

    // 7. Manifest digest syntax is checked last by contract.
    let manifest_digest = request.manifest_digest.as_deref().map(str::trim);
    if manifest_digest.is_some_and(|digest| !valid_digest(digest)) {
        return Err(RequestValidationError::new(
            "manifestDigest",
            ValidationCode::InvalidManifestDigest,
        ));
    }

    Ok(NormalizedFederationRequest {
        schema_version: request.schema_version,
        request_id,
        trace_id,
        task,
        repository_root: canonical.path.to_string_lossy().into_owned(),
        repository_id: canonical.repository_id,
        worktree_root: canonical.worktree_root.to_string_lossy().into_owned(),
        client,
        session_id,
        nonce,
        deadline_ms: request.deadline_ms,
        max_tokens: request.max_tokens,
        deadline,
        anchors,
        scope_grant_id: request
            .scope_grant_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned),
        manifest_digest: manifest_digest.map(str::to_ascii_lowercase),
        release_generation: request
            .release_generation
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned),
        blueprint_generation: request
            .blueprint_generation
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned),
        skills_generation: request
            .skills_generation
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned),
        extensions: request.extensions.clone(),
    })
}

fn required(field: &'static str, value: &str) -> Result<String, RequestValidationError> {
    let value = value.trim();
    (!value.is_empty())
        .then(|| value.to_owned())
        .ok_or_else(|| RequestValidationError::new(field, ValidationCode::Missing))
}

fn root_error(field: &'static str, error: RootError) -> RequestValidationError {
    let code = match error {
        RootError::MissingIdentity | RootError::InvalidIdentity | RootError::OutsideWorktree => {
            ValidationCode::RepositoryIdentity
        }
        RootError::NotAbsolute
        | RootError::Unavailable
        | RootError::NotDirectory
        | RootError::Aliased => ValidationCode::InvalidRoot,
    };
    RequestValidationError::new(field, code)
}

fn validate_identity_extension(
    extensions: &BTreeMap<String, Value>,
    keys: &[&'static str],
    expected: &str,
    code: ValidationCode,
) -> Result<(), RequestValidationError> {
    for &key in keys {
        if let Some(value) = extensions.get(key) {
            let Some(value) = value.as_str().map(str::trim).filter(|v| !v.is_empty()) else {
                return Err(RequestValidationError::new(key, code));
            };
            if value != expected {
                return Err(RequestValidationError::new(key, code));
            }
        }
    }
    Ok(())
}

fn validate_worktree_extension(
    extensions: &BTreeMap<String, Value>,
    expected: &std::path::Path,
) -> Result<(), RequestValidationError> {
    for key in ["worktreeRoot", "worktreePath"] {
        if let Some(value) = extensions.get(key) {
            let Some(value) = value.as_str().map(str::trim).filter(|v| !v.is_empty()) else {
                return Err(RequestValidationError::new(
                    key,
                    ValidationCode::RepositoryIdentity,
                ));
            };
            if value.replace('\\', "/") != expected.to_string_lossy().replace('\\', "/") {
                return Err(RequestValidationError::new(
                    key,
                    ValidationCode::RepositoryIdentity,
                ));
            }
        }
    }
    Ok(())
}

fn optional_string(
    extensions: &BTreeMap<String, Value>,
    key: &'static str,
) -> Result<Option<String>, RequestValidationError> {
    extensions
        .get(key)
        .map(|value| {
            value
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| RequestValidationError::new(key, ValidationCode::Missing))
        })
        .transpose()
}

fn valid_digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
}
