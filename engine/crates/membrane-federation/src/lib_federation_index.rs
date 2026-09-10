//! Native port of `blueprint/src/lib/federation/index.mjs`.
//!
//! Scoped federation: repositories retain independent generation/identity
//! spaces. Named groups are configuration only; cross-repo traversal crosses
//! exact contract bridges and never same-name similarity joins. This is a
//! faithful behavioral port -- same validation order, same error codes, same
//! output shape (field names kept camelCase-equivalent via serde_json::Value
//! to match the legacy envelope byte-for-byte at the JSON level) -- built on
//! top of `membrane_blueprint::contract_registry::stitch_contract_traces`,
//! which is itself the native port of `graph/contract-registry.mjs`'s
//! `stitchContractTraces` that the legacy federation module calls.
//!
//! `routeFederatedQuery` in the legacy module is async and calls a
//! caller-supplied `querySlice` async function per repository. This port
//! represents that as a synchronous closure (`FnMut(&Value, &str, &Value) ->
//! Result<Value, Value>`) rather than threading an async runtime through a
//! narrow federation-composition seam -- the composition/validation logic
//! (the part with actual behavior to prove parity on) is unchanged; only the
//! I/O-boundary calling convention is simplified.

use crate::blueprint_client::{BlueprintClient, BlueprintClientError, BlueprintQuery};
use membrane_blueprint::contract_registry::{Contract, ContractRegistry, stitch_contract_traces};
use membrane_blueprint::{BlueprintApi, BlueprintRequest, BlueprintResponse, Bounds, CancellationToken as NativeCancellation, Operation};
use membrane_provider_sdk::BlueprintResult;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Component, Path};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct FederationError {
    pub code: &'static str,
    pub message: String,
}

impl FederationError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
}

impl std::fmt::Display for FederationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for FederationError {}

/// Native Blueprint owner used by federation adapters.
///
/// Recall and Resolve intentionally go through [`BlueprintClient`], which is
/// the request-bound source boundary used by Pull. Federate uses the same
/// injected [`BlueprintApi`] directly because its result is a scoped slice
/// envelope rather than a single `BlueprintResult`. No transport, process, or
/// graph storage is owned here.
pub struct NativeBlueprintFederator {
    api: Arc<dyn BlueprintApi>,
    client: BlueprintClient,
}

impl std::fmt::Debug for NativeBlueprintFederator {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("NativeBlueprintFederator").field("api", &"injected").finish()
    }
}

impl NativeBlueprintFederator {
    pub fn new(api: Arc<dyn BlueprintApi>) -> Self {
        Self { client: BlueprintClient::new(api.clone()), api }
    }

    pub fn from_operation(operation: Arc<dyn membrane_blueprint::BlueprintOperation>) -> Self {
        Self::new(Arc::new(membrane_blueprint::OneShotExecutor::new(operation)))
    }

    pub fn recall(
        &self,
        query: &BlueprintQuery,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> Result<BlueprintResult, FederationError> {
        let result = self.client.query_with_cancellation(query, cancellation).map_err(client_error)?;
        validate_typed_source_refs(&result, &query.repository_root)?;
        Ok(result)
    }

    pub fn query(
        &self,
        query: &BlueprintQuery,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> Result<BlueprintResult, FederationError> {
        self.recall(query, cancellation)
    }

    pub fn resolve(
        &self,
        query: &BlueprintQuery,
        symbol: &str,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> Result<BlueprintResult, FederationError> {
        let result = self.client.resolve_symbol_with_cancellation(query, symbol, cancellation).map_err(client_error)?;
        validate_typed_source_refs(&result, &query.repository_root)?;
        Ok(result)
    }

    pub fn resolve_symbol(
        &self,
        query: &BlueprintQuery,
        symbol: &str,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> Result<BlueprintResult, FederationError> {
        self.resolve(query, symbol, cancellation)
    }

    /// Execute one native Blueprint operation for each explicitly selected
    /// repository and compose isolated result slices. A failure, stale
    /// generation, malformed candidate, or cancellation degrades only that
    /// repository's slice and remains visible as an omission.
    pub fn federate(
        &self,
        request_id: &str,
        group: Option<(&str, &[Value])>,
        repositories: &[Value],
        allowed_repo_ids: &[String],
        operation: &str,
        input: &Value,
        deadline: Duration,
        cancellation: NativeCancellation,
    ) -> Result<Value, FederationError> {
        route_native_federated_query(
            self.api.clone(), request_id, group, repositories, allowed_repo_ids,
            operation, input, deadline, cancellation,
        )
    }
}

fn client_error(error: BlueprintClientError) -> FederationError {
    FederationError::new(error.code(), error.to_string())
}

fn validate_typed_source_refs(result: &BlueprintResult, repository_root: &str) -> Result<(), FederationError> {
    for candidate in &result.candidates {
        if !source_ref_is_bound(repository_root, &candidate.source_ref) {
            return Err(FederationError::new(
                "blueprint_source_unbound",
                format!("candidate {} is outside its repository source scope", candidate.id),
            ));
        }
    }
    Ok(())
}

/// Mirrors `defineFederationGroup({ name, repositories })`.
pub fn define_federation_group(name: &str, repositories: &[Value]) -> Result<Value, FederationError> {
    let group_name = name.trim();
    if group_name.is_empty() {
        return Err(FederationError::new("federation_group_name_invalid", "federation group requires a non-empty name"));
    }
    if repositories.is_empty() || repositories.len() > 16 {
        return Err(FederationError::new("federation_bounds_invalid", "federation group requires 1 to 16 repositories"));
    }
    let mut ids = BTreeSet::new();
    let mut normalized = Vec::new();
    for repository in repositories {
        let repo_id = repository.get("repoId").and_then(Value::as_str);
        let Some(repo_id) = repo_id else {
            return Err(FederationError::new("repository_duplicate", "each federated repository needs one unique repoId"));
        };
        if ids.contains(repo_id) {
            return Err(FederationError::new("repository_duplicate", "each federated repository needs one unique repoId"));
        }
        ids.insert(repo_id.to_owned());
        normalized.push(repository.clone());
    }
    Ok(json!({
        "schemaVersion": 1,
        "name": group_name,
        "repositories": normalized,
    }))
}

fn contract_from_json(value: &Value) -> Contract {
    Contract {
        contract_id: value.get("contractId").and_then(Value::as_str).unwrap_or_default().to_owned(),
        contract_key: value.get("contractKey").and_then(Value::as_str).unwrap_or_default().to_owned(),
        repo_id: value.get("repoId").and_then(Value::as_str).map(str::to_owned),
        kind: value.get("kind").and_then(Value::as_str).unwrap_or_default().to_owned(),
        address: value.get("address").and_then(Value::as_str).unwrap_or_default().to_owned(),
        schema: value.get("schema").cloned().filter(|v| !v.is_null()),
        roles: value.get("roles").and_then(Value::as_array).map(|a| a.iter().filter_map(|r| r.as_str().map(str::to_owned)).collect()).unwrap_or_default(),
        node_id: value.get("nodeId").and_then(Value::as_str).unwrap_or_default().to_owned(),
        portable_id: value.get("portableId").and_then(Value::as_str).map(str::to_owned),
        evidence: value.get("evidence").cloned().unwrap_or(Value::Null),
    }
}

fn bridge_to_json(bridge: &membrane_blueprint::contract_registry::Bridge) -> Value {
    json!({
        "id": bridge.id,
        "contractKey": bridge.contract_key,
        "kind": bridge.kind,
        "address": bridge.address,
        "consumer": { "repoId": bridge.consumer_repo_id, "nodeId": bridge.consumer_node_id, "generationId": bridge.consumer_generation_id },
        "provider": { "repoId": bridge.provider_repo_id, "nodeId": bridge.provider_node_id, "generationId": bridge.provider_generation_id },
        "evidence": bridge.evidence,
    })
}

fn trace_to_json(trace: &membrane_blueprint::contract_registry::Trace) -> Value {
    json!({
        "id": trace.id,
        "contractKey": trace.contract_key,
        "steps": trace.steps.iter().map(|s| json!({"repoId": s.repo_id, "nodeId": s.node_id, "role": s.role, "generationId": s.generation_id})).collect::<Vec<_>>(),
        "evidence": trace.evidence,
    })
}

/// Mirrors `composeFederatedSlices(slices, { groupName })`.
pub fn compose_federated_slices(slices: &[Value], group_name: Option<&str>) -> Result<Value, FederationError> {
    let mut repos = Vec::new();
    let mut seen = BTreeSet::new();
    for slice in slices {
        let repo_id = slice.get("repoId").and_then(Value::as_str);
        let generation_id = slice.get("generationId").and_then(Value::as_str);
        let (Some(repo_id), Some(generation_id)) = (repo_id, generation_id) else {
            return Err(FederationError::new("slice_incomplete", "each federated slice needs repoId and generationId"));
        };
        if let Some(existing) = repos.iter().find(|(r, _): &&(String, String)| r == repo_id) {
            if existing.1 != generation_id {
                return Err(FederationError::new("generation_ambiguity", format!("repo {repo_id} contributed two generations")));
            }
        }
        if seen.contains(repo_id) {
            return Err(FederationError::new("repository_duplicate", format!("repo {repo_id} contributed more than one slice")));
        }
        seen.insert(repo_id.to_owned());
        repos.push((repo_id.to_owned(), generation_id.to_owned()));
    }

    let repos_json: Vec<Value> = slices
        .iter()
        .map(|slice| {
            json!({
                "repoId": slice.get("repoId"),
                "repoRoot": slice.get("repoRoot").cloned().unwrap_or(Value::Null),
                "generationId": slice.get("generationId"),
                "receiptId": slice.get("receiptId").cloned().unwrap_or(Value::Null),
                "resultCount": slice.get("results").and_then(Value::as_array).map(|a| a.len()).unwrap_or(0),
            })
        })
        .collect();

    let mut registries: Vec<ContractRegistry> = Vec::new();
    for slice in slices {
        let contracts = slice.get("contracts").and_then(Value::as_array);
        if let Some(contracts) = contracts {
            if contracts.is_empty() {
                continue;
            }
            registries.push(ContractRegistry {
                schema_version: 1,
                repo_id: slice.get("repoId").and_then(Value::as_str).map(str::to_owned),
                generation_id: slice.get("generationId").and_then(Value::as_str).map(str::to_owned),
                contracts: contracts.iter().map(contract_from_json).collect(),
            });
        }
    }
    let stitching = stitch_contract_traces(&registries);

    Ok(json!({
        "schemaVersion": 1,
        "kind": "federated",
        "groupName": group_name,
        "repos": repos_json,
        "plannerAuthority": "external",
        "selection": "unranked_repository_slices",
        "slices": slices.iter().map(|slice| json!({
            "repoId": slice.get("repoId"),
            "repoRoot": slice.get("repoRoot").cloned().unwrap_or(Value::Null),
            "generationId": slice.get("generationId"),
            "receiptId": slice.get("receiptId").cloned().unwrap_or(Value::Null),
            "results": slice.get("results").cloned().unwrap_or(json!([])),
            "omissions": slice.get("omissions").cloned().unwrap_or(json!([])),
            "contracts": slice.get("contracts").cloned().unwrap_or(json!([])),
        })).collect::<Vec<_>>(),
        "results": slices.iter().map(|slice| json!({
            "repoId": slice.get("repoId"),
            "generationId": slice.get("generationId"),
            "results": slice.get("results").cloned().unwrap_or(json!([])),
        })).collect::<Vec<_>>(),
        "contractBridges": stitching.bridges.iter().map(bridge_to_json).collect::<Vec<_>>(),
        "traces": stitching.traces.iter().map(trace_to_json).collect::<Vec<_>>(),
    }))
}

/// Mirrors `isRepoAllowed(slice, allowedRepoIds)`.
pub fn is_repo_allowed(slice: &Value, allowed_repo_ids: &[String]) -> bool {
    slice.get("repoId").and_then(Value::as_str).map(|id| allowed_repo_ids.iter().any(|a| a == id)).unwrap_or(false)
}

const FEDERATED_OPERATIONS: &[&str] = &["search", "recall", "impact", "architecture"];

/// Mirrors `routeFederatedQuery({ group, repositories, allowedRepoIds, operation, input, querySlice })`.
///
/// `query_slice` is called once per selected repository with `(repository, operation, input)`
/// and must return either a result `Value` (on success) or an error `Value` carrying
/// `code`/`message` fields (on failure) -- mirroring the legacy try/catch per-slice fallback.
pub fn route_federated_query<F>(
    group: Option<(&str, &[Value])>,
    repositories: &[Value],
    allowed_repo_ids: &[String],
    operation: &str,
    input: &Value,
    mut query_slice: F,
) -> Result<Value, FederationError>
where
    F: FnMut(&Value, &str, &Value) -> Result<Value, Value>,
{
    let normalized_group = match group {
        Some((name, repos)) => Some(define_federation_group(name, repos)?),
        None => None,
    };
    let selected: Vec<Value> = match &normalized_group {
        Some(g) => g.get("repositories").and_then(Value::as_array).cloned().unwrap_or_default(),
        None => repositories.to_vec(),
    };
    if selected.is_empty() || selected.len() > 16 {
        return Err(FederationError::new("federation_bounds_invalid", "federation requires 1 to 16 explicit repositories"));
    }
    if !FEDERATED_OPERATIONS.contains(&operation) {
        return Err(FederationError::new("federation_operation_invalid", format!("unsupported federated operation {operation}")));
    }
    let mut ids = BTreeSet::new();
    for repository in &selected {
        let repo_id = repository.get("repoId").and_then(Value::as_str);
        let Some(repo_id) = repo_id else {
            return Err(FederationError::new("repository_duplicate", "each federated repository needs one unique repoId"));
        };
        if ids.contains(repo_id) {
            return Err(FederationError::new("repository_duplicate", "each federated repository needs one unique repoId"));
        }
        ids.insert(repo_id.to_owned());
        if !is_repo_allowed(repository, allowed_repo_ids) {
            return Err(FederationError::new("repository_not_allowed", format!("repo {repo_id} is outside the explicit federation allowlist")));
        }
    }

    let mut slices = Vec::new();
    for repository in &selected {
        let repo_id = repository.get("repoId").and_then(Value::as_str).unwrap_or_default();
        match query_slice(repository, operation, input) {
            Ok(result) => {
                let repo_root = result.get("repoRoot").cloned().unwrap_or_else(|| repository.get("repoRoot").cloned().unwrap_or(Value::Null));
                let generation_id = result.get("generationId").cloned()
                    .or_else(|| repository.get("generation").cloned())
                    .filter(|value| value.as_str().is_some_and(|generation| !generation.is_empty()))
                    .unwrap_or_else(|| json!("unavailable"));
                let receipt_id = result.get("freshnessReceipt").and_then(|r| r.get("receiptId")).cloned().unwrap_or(Value::Null);
                let contracts = result.get("contracts").cloned().unwrap_or(json!([]));
                let omissions = result.get("omissions").cloned().unwrap_or_else(|| json!([]));
                slices.push(json!({
                    "repoId": repo_id,
                    "repoRoot": repo_root,
                    "generationId": generation_id,
                    "receiptId": receipt_id,
                    "results": [result],
                    "omissions": if omissions.is_array() { omissions } else { json!([{"reason":"repository_query_failed","code":"blueprint_malformed","message":"omissions must be an array"}]) },
                    "contracts": contracts,
                }));
            }
            Err(error) => {
                let code = error.get("code").and_then(Value::as_str).unwrap_or("internal_error");
                let message = error.get("message").and_then(Value::as_str).unwrap_or("").to_owned();
                slices.push(json!({
                    "repoId": repo_id,
                    "repoRoot": repository.get("repoRoot").cloned().unwrap_or(Value::Null),
                    "generationId": repository.get("generation").cloned().unwrap_or(json!("unavailable")),
                    "receiptId": Value::Null,
                    "results": [],
                    "omissions": [{"reason": "repository_query_failed", "code": code, "message": message}],
                    "contracts": [],
                }));
            }
        }
    }
    compose_federated_slices(&slices, normalized_group.as_ref().and_then(|g| g.get("name")).and_then(Value::as_str))
}

/// Native counterpart to [`route_federated_query`]. Each repository receives
/// its own Blueprint request and identity binding; no repository's graph is
/// merged into another repository's candidate or generation space.
pub fn route_native_federated_query(
    api: Arc<dyn BlueprintApi>,
    request_id: &str,
    group: Option<(&str, &[Value])>,
    repositories: &[Value],
    allowed_repo_ids: &[String],
    operation: &str,
    input: &Value,
    deadline: Duration,
    cancellation: NativeCancellation,
) -> Result<Value, FederationError> {
    let normalized_group = match group {
        Some((name, repos)) => Some(define_federation_group(name, repos)?),
        None => None,
    };
    let selected: Vec<Value> = normalized_group
        .as_ref()
        .and_then(|group| group.get("repositories"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_else(|| repositories.to_vec());
    if selected.is_empty() || selected.len() > 16 {
        return Err(FederationError::new(
            "federation_bounds_invalid",
            "federation requires 1 to 16 explicit repositories",
        ));
    }
    if !FEDERATED_OPERATIONS.contains(&operation) {
        return Err(FederationError::new(
            "federation_operation_invalid",
            format!("unsupported federated operation {operation}"),
        ));
    }
    let native_operation = Operation::parse(operation).ok_or_else(|| {
        FederationError::new(
            "federation_operation_invalid",
            format!("unsupported federated operation {operation}"),
        )
    })?;
    let mut ids = BTreeSet::new();
    for repository in &selected {
        let Some(repo_id) = repository.get("repoId").and_then(Value::as_str).filter(|id| !id.is_empty()) else {
            return Err(FederationError::new(
                "repository_duplicate",
                "each federated repository needs one unique repoId",
            ));
        };
        if !ids.insert(repo_id.to_owned()) {
            return Err(FederationError::new(
                "repository_duplicate",
                "each federated repository needs one unique repoId",
            ));
        }
        if !is_repo_allowed(repository, allowed_repo_ids) {
            return Err(FederationError::new(
                "repository_not_allowed",
                format!("repo {repo_id} is outside the explicit federation allowlist"),
            ));
        }
    }

    let mut slices = Vec::with_capacity(selected.len());
    for repository in &selected {
        let repo_id = repository.get("repoId").and_then(Value::as_str).unwrap_or_default();
        let repo_root = repository.get("repoRoot").and_then(Value::as_str).unwrap_or_default();
        let expected_generation = repository
            .get("generation")
            .or_else(|| repository.get("generationId"))
            .and_then(Value::as_str)
            .filter(|generation| !generation.is_empty());
        let fallback_generation = expected_generation.unwrap_or("unavailable").to_owned();

        let mut child_input = input.clone();
        if let Some(object) = child_input.as_object_mut() {
            object.insert("repoRoot".into(), Value::String(repo_root.to_owned()));
            object.insert("repoId".into(), Value::String(repo_id.to_owned()));
            if let Some(generation) = expected_generation {
                object.insert("generation".into(), Value::String(generation.to_owned()));
            }
        }
        let mut child_request = BlueprintRequest::new(
            format!("{}-federate-{repo_id}", if request_id.trim().is_empty() { "blueprint" } else { request_id }),
            native_operation,
            repo_root,
        );
        child_request.repo_id = Some(repo_id.to_owned());
        child_request.generation = expected_generation.map(str::to_owned);
        child_request.deadline_ms = deadline.as_millis().clamp(10, 30_000) as u64;
        child_request.input = child_input;

        let slice = if cancellation.is_cancelled() {
            native_failure_slice(repository, &fallback_generation, "request_cancelled", "request cancelled")
        } else {
            let response = api.dispatch(child_request, cancellation.clone());
            native_response_slice(repository, &fallback_generation, expected_generation, response)
        };
        slices.push(slice);
    }
    compose_federated_slices(
        &slices,
        normalized_group
            .as_ref()
            .and_then(|group| group.get("name"))
            .and_then(Value::as_str),
    )
}

fn native_response_slice(
    repository: &Value,
    fallback_generation: &str,
    expected_generation: Option<&str>,
    response: BlueprintResponse,
) -> Value {
    let repo_id = repository.get("repoId").cloned().unwrap_or(Value::Null);
    let repo_root = repository.get("repoRoot").cloned().unwrap_or(Value::Null);
    let mut base = json!({
        "repoId": repo_id,
        "repoRoot": repo_root,
        "generationId": fallback_generation,
        "receiptId": Value::Null,
        "results": [],
        "omissions": [],
        "contracts": [],
    });
    if let Err(error) = response.validate(Bounds::default()) {
        add_slice_omission(&mut base, "repository_query_failed", &error.code, &error.message);
        return base;
    }
    if !response.ok {
        if let Some(error) = response.error {
            add_slice_omission(&mut base, "repository_query_failed", &error.code, &error.message);
        } else {
            add_slice_omission(&mut base, "repository_query_failed", "internal_error", "native Blueprint request failed");
        }
        return base;
    }
    let Some(mut result) = response.result else {
        add_slice_omission(&mut base, "repository_query_failed", "blueprint_malformed", "successful response has no result");
        return base;
    };
    let observed_generation = response
        .generation
        .clone()
        .or_else(|| result.get("generationId").and_then(Value::as_str).map(str::to_owned));
    if let Some(expected) = expected_generation {
        if observed_generation.as_deref() != Some(expected) {
            add_slice_omission(
                &mut base,
                "generation_mismatch",
                "generation_mismatch",
                "native Blueprint response generation does not match repository binding",
            );
            return base;
        }
    }
    if let Some(generation) = observed_generation {
        base["generationId"] = Value::String(generation);
    }
    if let Err(reason) = validate_native_candidates(&result, repository.get("repoRoot").and_then(Value::as_str).unwrap_or_default()) {
        add_slice_omission(&mut base, "source_candidate_rejected", "blueprint_malformed", reason);
        return base;
    }
    let mut omissions = result.get("omissions").cloned().unwrap_or_else(|| json!([]));
    if let Some(omissions_array) = omissions.as_array_mut() {
        let state = result.get("state").and_then(Value::as_str);
        let stale = result.pointer("/freshness/stale").and_then(Value::as_bool).unwrap_or(false)
            || matches!(state, Some("stale" | "partial" | "ambiguous" | "suppressed"));
        if stale && omissions_array.is_empty() {
            omissions_array.push(json!({"reason": if result.pointer("/freshness/stale").and_then(Value::as_bool).unwrap_or(false) { "stale_generation" } else { state.unwrap_or("blueprint_incomplete") }}));
        }
    }
    base["omissions"] = if omissions.is_array() { omissions } else { json!([{"reason":"blueprint_malformed","detail":"omissions must be an array"}]) };
    if let Some(receipt_id) = result.pointer("/freshnessReceipt/receiptId").cloned() {
        base["receiptId"] = receipt_id;
    }
    base["results"] = json!([result]);
    base
}

fn native_failure_slice(repository: &Value, generation: &str, code: &str, message: &str) -> Value {
    json!({
        "repoId": repository.get("repoId").cloned().unwrap_or(Value::Null),
        "repoRoot": repository.get("repoRoot").cloned().unwrap_or(Value::Null),
        "generationId": generation,
        "receiptId": Value::Null,
        "results": [],
        "omissions": [{"reason":"repository_query_failed", "code":code, "message":message}],
        "contracts": [],
    })
}

fn add_slice_omission(slice: &mut Value, reason: &str, code: &str, message: &str) {
    if let Some(omissions) = slice.get_mut("omissions").and_then(Value::as_array_mut) {
        omissions.push(json!({"reason": reason, "code": code, "message": message}));
    }
}

fn validate_native_candidates(result: &Value, repo_root: &str) -> Result<(), &'static str> {
    let source_bound_set = result.get("candidateSet").is_some();
    let candidates = result
        .get("candidateSet")
        .and_then(|set| set.get("candidates"))
        .or_else(|| result.get("candidates"));
    let Some(candidates) = candidates else { return Ok(()); };
    let Some(candidates) = candidates.as_array() else { return Err("candidates must be an array"); };
    for candidate in candidates {
        if let Some(source_ref) = candidate.get("sourceRef").and_then(Value::as_str) {
            if !source_ref_is_bound(repo_root, source_ref) {
                return Err("source-bound candidate escapes repository root");
            }
        } else if source_bound_set {
            return Err("source-bound candidate is missing sourceRef");
        }
    }
    Ok(())
}

fn source_ref_is_bound(repo_root: &str, source_ref: &str) -> bool {
    if source_ref.trim().is_empty() || source_ref.as_bytes().iter().any(|byte| *byte == 0) {
        return false;
    }
    let path = Path::new(source_ref);
    if !path.is_absolute() {
        return path.components().all(|component| !matches!(component, Component::ParentDir | Component::RootDir | Component::Prefix(_)));
    }
    let root = Path::new(repo_root);
    path.starts_with(root)
}
