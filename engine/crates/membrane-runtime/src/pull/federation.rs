//! Membrane federation gateway — Rust shell.
//!
//! Per dispatch §G3A + §G5: the authenticated local Membrane gateway
//! is the SOLE owner of provider fan-out and admission. Clients (Claude,
//! Codex, MCP) submit (task, repo_root, client, session, max_tokens, anchors,
//! scope_grant_id, remainingContextCeiling); the gateway invokes provider adapters in
//! parallel, runs the deterministic in-process admission, and emits a
//! content-free ContextPacket v1 plus per-candidate ContextReceipt v2.
//!
//! This Rust module is the native dispatcher and planner-envelope adapter.
//!
//! Provider payload formats and SQLite details never enter client
//! adapters. Cortex durable storage is never modified. Bearer tokens
//! are passed via the standard `MEMBRANE_API_TOKEN_FILE` env, never in
//! argv or stdout. ScopeGrant enforcement happens in native source bindings.

use super::federation_sources::RuntimeReleaseSource;
use super::{federation_sources, native_federation};
use crate::pull::planner::{plan, ContextCandidateSetV1, PlannerInput};
use membrane_protocol::{PublicationFenceStatusV1, PublicationFenceV1};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

fn federation_session_id(session: Option<String>) -> String {
    session
        .unwrap_or_else(|| crate::store::opaque_correlation_token("anonymous-session", "session"))
}

#[derive(Debug)]
enum NativeRouteError {
    Internal(String),
    RequestTime(crate::push::selection::PacketReductionRequestError),
}

impl From<String> for NativeRouteError {
    fn from(error: String) -> Self {
        Self::Internal(error)
    }
}

/// Run native federation end-to-end and emit its final planner envelope.
#[allow(clippy::too_many_arguments)]
pub fn run_federate(
    task: String,
    repo: PathBuf,
    max_tokens: usize,
    packet_char_budget_override: Option<usize>,
    packet_char_budget_model: Option<String>,
    client: String,
    session: Option<String>,
    anchors: Vec<String>,
    scope_grant_id: Option<String>,
    federation_script: Option<PathBuf>,
    accepted_receipt_versions: Vec<u32>,
) -> Result<(), String> {
    // Kept for V1 CLI call compatibility; native execution never consults it.
    let _ = federation_script;
    let root = repo
        .canonicalize()
        .map_err(|error| format!("resolve repository root: {error}"))?;
    let session_id = federation_session_id(session);
    let release_generation = RuntimeReleaseSource::generation()?;
    let request = native_request(
        &task,
        &root,
        max_tokens,
        2_000,
        release_generation,
        &client,
        &session_id,
        anchors,
        scope_grant_id.clone(),
        None,
    );
    let started = Instant::now();
    let (response, native_metrics, freshness) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("create native federation runtime: {error}"))?
        .block_on(async {
            let bindings = federation_sources::NativeSourceBindings::for_repository(
                &root,
                scope_grant_id.as_deref(),
            )?;
            let native = native_federation::NativeFederation::new(bindings)?;
            let response = native
                .federate(&request, tokio_util::sync::CancellationToken::new())
                .await?;
            let freshness = native
                .freshness_snapshot()
                .ok_or_else(|| "native freshness verdict unavailable".to_owned())?;
            Ok::<_, String>((response, native.metrics_snapshot(), freshness))
        })?;
    let ccs = native_response_to_ccs(&response, &request, &freshness);
    let native_receipts = collect_native_receipts(&response);
    let mut payload = envelope_from_ccs(
        &serde_json::to_string(&ccs)
            .map_err(|error| format!("serialize native candidates: {error}"))?,
        EnvelopeInput {
            max_tokens,
            packet_char_budget_override,
            packet_char_budget_model,
            accepted_receipt_versions: if accepted_receipt_versions.is_empty() {
                vec![2]
            } else {
                accepted_receipt_versions
            },
            scope_grant_present: scope_grant_id.is_some(),
            // The CLI binds no post-fusion grant snapshot to compare against,
            // so no fence verdict can be supplied here; enforcement stays on
            // callers that re-validate (the resident route via the engine).
            scope_grant_fence: None,
            gateway_process_ms: started.elapsed().as_secs_f64() * 1000.0,
        },
    )?;
    if let Some(fields) = payload.as_object_mut() {
        fields.insert("transport".to_owned(), Value::String("native".to_owned()));
        fields.insert(
            "federationMetrics".to_owned(),
            serde_json::json!(native_metrics),
        );
        merge_native_receipts(fields, native_receipts);
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&payload).map_err(|e| format!("serialize: {e}"))?
    );
    Ok(())
}

/// Resident `/federate` entrypoint. Production routes call this native path;
/// legacy worker framing remains isolated for shadow qualification only.
pub fn native_route_response(body: &str) -> (u16, String) {
    let value: Value = match serde_json::from_str(body) {
        Ok(value) => value,
        Err(_) => return (400, "{\"error\":\"invalid JSON body\"}".to_owned()),
    };
    let Some(task) = value.get("task").and_then(Value::as_str) else {
        return (400, "{\"error\":\"task required\"}".to_owned());
    };
    let Some(repo_text) = value.get("repo").and_then(Value::as_str) else {
        return (400, "{\"error\":\"repo required\"}".to_owned());
    };
    let root = match PathBuf::from(repo_text).canonicalize() {
        Ok(path) if path.is_dir() => path,
        _ => {
            return (
                400,
                "{\"error\":\"repo must be an existing directory\"}".to_owned(),
            )
        }
    };
    let max_tokens = value
        .get("maxTokens")
        .and_then(Value::as_u64)
        .map_or(4096, |n| n.clamp(1, 1_000_000) as usize);
    let deadline_ms = value
        .get("maxWaitMs")
        .and_then(Value::as_u64)
        .unwrap_or(2_000)
        .clamp(1, 2_000);
    let client = value
        .get("client")
        .and_then(Value::as_str)
        .unwrap_or("claude")
        .to_owned();
    let session = federation_session_id(
        value
            .get("session")
            .and_then(Value::as_str)
            .map(str::to_owned),
    );
    let anchors = value
        .get("anchors")
        .and_then(Value::as_str)
        .map(|text| {
            text.split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let scope_grant_id = value
        .get("scopeGrantId")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let sufficiency_contract = value.get("sufficiencyContract").cloned();
    let ceiling = match crate::push::selection::parse_request_time_h8(&value, &session, task) {
        Ok(ceiling) => ceiling,
        Err(error) => {
            return request_time_refusal(crate::push::selection::PacketReductionRequestError::H8(
                error,
            ))
        }
    };
    let started = Instant::now();
    let result = (|| -> Result<Value, NativeRouteError> {
        let release_generation = RuntimeReleaseSource::generation()?;
        let request = native_request_with_h8(
            task,
            &root,
            max_tokens,
            deadline_ms,
            release_generation,
            &client,
            &session,
            anchors,
            scope_grant_id.clone(),
            sufficiency_contract,
            &ceiling,
        );
        let bindings = federation_sources::NativeSourceBindings::for_repository(
            &root,
            scope_grant_id.as_deref(),
        )?;
        let native = native_federation::NativeFederation::new(bindings)?;
        let response = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| format!("create native federation runtime: {error}"))?
            .block_on(native.federate(&request, tokio_util::sync::CancellationToken::new()))?;
        let native_metrics = native.metrics_snapshot();
        let freshness = native
            .freshness_snapshot()
            .ok_or_else(|| "native freshness verdict unavailable".to_owned())?;
        let ccs = native_response_to_ccs(&response, &request, &freshness);
        let native_receipts = collect_native_receipts(&response);
        let publication_fence = response
            .extensions
            .get("publicationFence")
            .map(|receipt| {
                serde_json::from_value::<PublicationFenceV1>(receipt.clone())
                    .map_err(|error| format!("publication fence receipt invalid: {error}"))
            })
            .transpose()?;
        let mut payload = envelope_from_ccs(
            &serde_json::to_string(&ccs).map_err(|error| error.to_string())?,
            EnvelopeInput {
                max_tokens,
                packet_char_budget_override: value
                    .get("packetCharBudget")
                    .and_then(Value::as_u64)
                    .map(|n| n as usize),
                packet_char_budget_model: value
                    .get("packetCharBudgetModel")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                accepted_receipt_versions: vec![2],
                scope_grant_present: scope_grant_id.is_some(),
                scope_grant_fence: publication_fence,
                gateway_process_ms: started.elapsed().as_secs_f64() * 1000.0,
            },
        )?;
        let packet = payload
            .get("packet")
            .cloned()
            .ok_or_else(|| "federation envelope omitted packet".to_owned())
            .and_then(|packet| {
                serde_json::from_value::<cortex_core::planner::ContextPacketV1>(packet)
                    .map_err(|error| format!("federation packet is invalid: {error}"))
            })?;
        let push_policy = push_policy_for_request(&value, task);
        let selection = crate::push::selection::select_packet_for_h8_with_policy(
            &packet,
            &ceiling,
            &push_policy,
        )
        .map_err(NativeRouteError::RequestTime)?;
        let selected_content = selection.selected_representation.content.clone();
        let fields = payload
            .as_object_mut()
            .ok_or_else(|| "federation envelope is not an object".to_owned())?;
        fields.insert("packet".to_owned(), selected_content);
        fields.insert("transport".to_owned(), Value::String("native".to_owned()));
        fields.insert(
            "federationMetrics".to_owned(),
            serde_json::json!(native_metrics),
        );
        merge_native_receipts(fields, native_receipts);
        fields.insert(
            "packetReduction".to_owned(),
            serde_json::to_value(selection)
                .map_err(|error| format!("serialize packet reduction selection: {error}"))?,
        );
        Ok(payload)
    })();
    match result {
        Ok(payload) => serde_json::to_string(&payload)
            .map(|body| (200, body))
            .unwrap_or_else(|_| {
                (
                    500,
                    "{\"error\":\"federation envelope serialization failed\"}".to_owned(),
                )
            }),
        Err(NativeRouteError::RequestTime(error)) => request_time_refusal(error),
        Err(NativeRouteError::Internal(error)) => {
            (502, serde_json::json!({"error": error}).to_string())
        }
    }
}

/// Select control vs query-aware `reduced_1` Push from the same `/federate`
/// request body the planner already sent. Control remains the default arm:
/// query-aware is reachable only when the request explicitly opts in via
/// `pushPolicy: "queryAware"`, carrying the request's own `task` as the
/// query-admitted metadata. Membrane never derives this from task prose on
/// its own — the opt-in is an explicit planner signal, not an inference.
fn push_policy_for_request(body: &Value, task: &str) -> crate::push::prep::PushPolicy {
    let opts_into_query_aware = body
        .get("pushPolicy")
        .and_then(Value::as_str)
        .is_some_and(|policy| policy.eq_ignore_ascii_case("queryAware"));
    if opts_into_query_aware && !task.trim().is_empty() {
        crate::push::prep::PushPolicy::query_aware(task.to_owned(), true, true)
    } else {
        crate::push::prep::PushPolicy::Control
    }
}

fn request_time_refusal(
    error: crate::push::selection::PacketReductionRequestError,
) -> (u16, String) {
    (
        400,
        serde_json::json!({
            "error": "request_time_selection_refused",
            "kind": error.kind(),
            "reason": error.to_string(),
        })
        .to_string(),
    )
}

fn collect_native_receipts(response: &membrane_protocol::FederationResponseV1) -> Value {
    let mut receipts = serde_json::Map::new();
    for key in ["fusionReceipt", "correctiveRetrieval", "publicationFence"] {
        if let Some(value) = response.extensions.get(key) {
            receipts.insert(key.to_owned(), value.clone());
        }
    }
    Value::Object(receipts)
}

fn merge_native_receipts(fields: &mut serde_json::Map<String, Value>, receipts: Value) {
    if let Value::Object(receipts) = receipts {
        for (key, value) in receipts {
            fields.insert(key, value);
        }
    }
}

fn native_request(
    task: &str,
    root: &Path,
    max_tokens: usize,
    deadline_ms: u64,
    release_generation: String,
    client: &str,
    session: &str,
    anchors: Vec<String>,
    scope_grant_id: Option<String>,
    sufficiency_contract: Option<Value>,
) -> membrane_protocol::FederationRequestV1 {
    let repository_root = root.to_string_lossy().into_owned();
    let request_id = crate::store::opaque_correlation_token(task, "federation");
    let mut extensions = std::collections::BTreeMap::from([
        (
            "repositoryId".to_owned(),
            serde_json::json!(membrane_federation::root::canonical_repository_id(root)),
        ),
        (
            "worktreeRoot".to_owned(),
            serde_json::json!(repository_root),
        ),
    ]);
    if let Some(contract) = sufficiency_contract {
        extensions.insert("sufficiencyContract".to_owned(), contract);
    }
    membrane_protocol::FederationRequestV1 {
        schema_version: membrane_protocol::FEDERATION_REQUEST_SCHEMA_VERSION,
        request_id,
        trace_id: String::new(),
        task: task.to_owned(),
        repository_root: repository_root.clone(),
        client: client.to_owned(),
        session_id: session.to_owned(),
        deadline_ms,
        max_tokens: max_tokens.min(u32::MAX as usize) as u32,
        anchors,
        scope_grant_id,
        manifest_digest: None,
        release_generation: Some(release_generation),
        blueprint_generation: None,
        skills_generation: None,
        extensions,
    }
}

#[allow(clippy::too_many_arguments)]
fn native_request_with_h8(
    task: &str,
    root: &Path,
    max_tokens: usize,
    deadline_ms: u64,
    release_generation: String,
    client: &str,
    session: &str,
    anchors: Vec<String>,
    scope_grant_id: Option<String>,
    sufficiency_contract: Option<Value>,
    ceiling: &membrane_protocol::host_observation::RemainingContextCeilingV1,
) -> membrane_protocol::FederationRequestV1 {
    let mut request = native_request(
        task,
        root,
        max_tokens,
        deadline_ms,
        release_generation,
        client,
        session,
        anchors,
        scope_grant_id,
        sufficiency_contract,
    );
    request.extensions.insert(
        "remainingContextCeiling".to_owned(),
        serde_json::to_value(ceiling).expect("RemainingContextCeilingV1 is serializable"),
    );
    request
}

fn native_response_to_ccs(
    response: &membrane_protocol::FederationResponseV1,
    request: &membrane_protocol::FederationRequestV1,
    freshness: &membrane_protocol::FreshnessSnapshotV1,
) -> Value {
    let candidates =
        serde_json::to_value(&response.candidates).unwrap_or_else(|_| Value::Array(Vec::new()));
    let omissions = response
        .omissions
        .iter()
        .enumerate()
        .map(|(index, omission)| {
            serde_json::json!({
                "id": omission.candidate_id.clone().unwrap_or_else(|| format!("omission:{index}")),
                "layer": Value::Null,
                "reason": omission.reason.as_str(),
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "schemaVersion": 1,
        "traceId": request.trace_id,
        "indexedAt": freshness.snapshot_id.clone().unwrap_or_else(|| request.release_generation.clone().unwrap_or_default()),
        "task": request.task,
        "mode": "native",
        "provider": "federation",
        "freshness": {
            "revision": freshness.generation.clone().or_else(|| request.release_generation.clone()).unwrap_or_default(),
            "indexedAt": freshness.snapshot_id.clone().unwrap_or_default(),
            "stale": freshness.stale,
        },
        "providerCeiling": {"maxCandidates": 256, "maxEstimatedTokens": request.max_tokens},
        "candidates": candidates,
        "omissions": omissions,
    })
}

/// Planner-side half of one federation cycle, shared verbatim by the CLI
/// (`run_federate`) and the resident `/federate` route: parse the gateway's
/// CCS line, surface fail-closed aborts, run the in-process planner, and
/// assemble the client envelope.
pub struct EnvelopeInput {
    pub max_tokens: usize,
    pub packet_char_budget_override: Option<usize>,
    pub packet_char_budget_model: Option<String>,
    pub accepted_receipt_versions: Vec<u32>,
    pub scope_grant_present: bool,
    /// Publication fence input (pending §17.2). The caller re-validated the
    /// bound scope grant after fusion and passes the typed verdict here:
    /// `None` is honest only when no grant was ever bound (scope-free
    /// request); `Some(tripped)` refuses packet emission fail-closed.
    pub scope_grant_fence: Option<PublicationFenceV1>,
    /// Wall time spent obtaining the CCS (process spawn or worker roundtrip).
    pub gateway_process_ms: f64,
}

/// Publication fence for the runtime packet seam (pending §17.2).
///
/// Grant identity, policy epoch and revocation are re-checked after fusion,
/// immediately before packet emission. A tripped fence publishes typed
/// `policy_changed` and refuses to emit the packet authorized under the
/// superseded grant; `None` means no grant was bound and the fence is a
/// no-op, never a silent bypass.
pub fn fence_packet_emission(
    fence: Option<PublicationFenceV1>,
) -> Result<Option<PublicationFenceV1>, String> {
    match fence {
        None => Ok(None),
        Some(fence) => {
            if matches!(fence.status, PublicationFenceStatusV1::PolicyChanged) {
                Err(format!(
                    "publication fenced after fusion: policy_changed ({:?}); \
                     the stale-authorized packet is not emitted",
                    fence.change
                ))
            } else {
                Ok(Some(fence))
            }
        }
    }
}

pub fn envelope_from_ccs(stdout: &str, input: EnvelopeInput) -> Result<Value, String> {
    // The gateway may emit a fail-closed envelope (exit 2) when a
    // ScopeGrant is rejected. Detect that envelope and surface it
    // before attempting strict CCS deserialization.
    let parse_started = Instant::now();
    let mut raw_value: Value = match serde_json::from_str(stdout) {
        Ok(v) => v,
        Err(e) => {
            return Err(format!(
                "federation gateway returned non-JSON payload: {e}; first 200 bytes: {}",
                stdout.chars().take(200).collect::<String>()
            ));
        }
    };
    if raw_value.get("_membrane").is_some() {
        if let Some(abort_reason) = raw_value
            .get("_membrane")
            .and_then(|v| v.get("abortReason"))
            .and_then(|v| v.as_str())
        {
            return Err(format!(
                "federation aborted by gateway: abortReason={abort_reason}; abortDetail={}",
                raw_value
                    .get("_membrane")
                    .and_then(|v| v.get("abortDetail"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("(none)")
            ));
        }
    }
    // Publication fence (pending §17.2): the caller re-validated the bound
    // grant after fusion. A tripped fence refuses packet emission here — the
    // last admission boundary before the packet reaches a client.
    fence_packet_emission(input.scope_grant_fence)?;
    let observability = gateway_observability(&raw_value);
    let source_resolution_receipts =
        crate::source_resolution::gate_source_resolutions(&mut raw_value);
    let ccs: ContextCandidateSetV1 = match serde_json::from_value(raw_value) {
        Ok(v) => v,
        Err(e) => {
            return Err(format!(
                "federation gateway returned non-CCS payload: {e}; first 200 bytes: {}",
                stdout.chars().take(200).collect::<String>()
            ));
        }
    };
    let rust_parse_ms = parse_started.elapsed().as_secs_f64() * 1000.0;
    let planner_input = PlannerInput {
        candidate_set: ccs,
        max_tokens: input.max_tokens,
        packet_char_budget_override: input.packet_char_budget_override,
        packet_char_budget_model: input.packet_char_budget_model,
        accepted_receipt_versions: input.accepted_receipt_versions,
        trace_id_override: None,
        scope_grant_present: input.scope_grant_present,
    };
    let planner_started = Instant::now();
    let out = match plan(&planner_input) {
        Ok(o) => o,
        Err(e) => return Err(format!("planner rejected federation CSS: {e}")),
    };
    let rust_planner_ms = planner_started.elapsed().as_secs_f64() * 1000.0;
    let mut payload = serde_json::json!({
        "packet": out.packet,
        "receipts": out.receipts,
        "providerStatus": out.provider_status,
        "fallbackMode": out.fallback_mode,
        "degradationReason": out.degradation_reason,
        "sourceGeneration": out.source_generation,
        "expectedReleaseGeneration": out.expected_release_generation,
        "observedReleaseGeneration": out.observed_release_generation,
        "releaseGenerationStatus": out.release_generation_status,
        "structuredEvent": out.structured_event,
        "sourceResolutionReceipts": source_resolution_receipts,
    });
    if let Some(packet) = payload.get("packet") {
        payload["cachePrefixDiagnostic"] =
            serde_json::to_value(crate::cache_prefix::diagnose_cache_prefix(packet, None))
                .expect("cache prefix diagnostic serializes");
    }
    if let (Some(payload_fields), Some(observability_fields)) =
        (payload.as_object_mut(), observability.as_object())
    {
        payload_fields.extend(observability_fields.clone());
        let stages = payload_fields
            .entry("stageElapsedMs".to_string())
            .or_insert_with(|| serde_json::json!({}));
        if let Some(stage_fields) = stages.as_object_mut() {
            stage_fields.insert(
                "gateway_process".to_string(),
                input.gateway_process_ms.into(),
            );
            stage_fields.insert("rust_parse".to_string(), rust_parse_ms.into());
            stage_fields.insert("rust_planner".to_string(), rust_planner_ms.into());
        }
    }
    Ok(payload)
}

/// Membrane Cortex durable-memory candidate provider. Pure in-process
/// read of eligible MemoryEntry rows normalised into ContextCandidateSet v1
/// records (Layer 7, sourceKind "memory", trustClass "agent_verified").
#[allow(clippy::too_many_arguments)]
pub fn run_memory_candidates(
    task: String,
    repo: PathBuf,
    scope: Option<String>,
    max_candidates: usize,
    _scope_grant_id: Option<String>,
) -> Result<(), String> {
    let canonical_repo = repo
        .canonicalize()
        .map_err(|e| format!("resolve repo: {e}"))?;
    let workspace = canonical_repo
        .parent()
        .ok_or_else(|| "repo has no parent".to_string())?
        .to_path_buf();
    let db_path = db_path_for(&workspace);
    let db = crate::MemDb::open(&db_path)
        .map_err(|e| format!("open cortex db at {}: {e}", db_path.display()))?;
    let store = crate::MemoryStore::try_open(db).map_err(|e| format!("open MemoryStore: {e}"))?;

    let scope_id = scope.clone().unwrap_or_else(|| "D--Claude".to_string());
    // The CLI always has a real, already-canonicalized repo root in hand (canonicalize()
    // above already succeeded or this function would have returned), so this call site
    // always has a genuine freshness signal available — unlike the resident-serve HTTP
    // route, which does not (see the /memory-candidates handler in serve.rs).
    let payload = memory_candidates_payload(
        &store,
        &task,
        &scope_id,
        max_candidates,
        Some(&canonical_repo),
    );
    println!(
        "{}",
        serde_json::to_string_pretty(&payload).map_err(|e| format!("serialize: {e}"))?
    );
    Ok(())
}

/// Testable core: build the Cortex ContextCandidateSet from REAL relevance-ranked memories.
///
/// Uses `recall_scored` (the same full-corpus hybrid retriever that backs live `context_for`),
/// not an arbitrary `entries(max)` slice — so results are relevant. Emits `text` = a bounded
/// word-boundary content preview (the old code emitted `text = e.id`, i.e. a useless slug), and a
/// real content hash. Feedback-rail vetoes are applied via `gate_history_for` so a memory the agent
/// marked `contradicted` never surfaces here either.
pub fn memory_candidates_payload(
    store: &crate::MemoryStore,
    task: &str,
    scope_id: &str,
    max_candidates: usize,
    repo_root: Option<&Path>,
) -> serde_json::Value {
    memory_candidates_payload_for_descriptor(
        store,
        task,
        &crate::scope::ScopeDescriptorV1::filesystem(scope_id),
        max_candidates,
        repo_root,
    )
    .unwrap_or_else(|error| serde_json::json!({"error": error, "candidates": []}))
}

/// Reason code for a memory candidate that scored well enough to be considered but was cut by
/// the caller's `max_candidates` ceiling. Mirrors the `Omission.reason` convention in
/// `memory_provider.rs::reasons`, scoped locally since this function's admission model (a single
/// recall pass, not `consider_entries`/`partition`) has no other omission source to report.
const OMISSION_REASON_CEILING_TRUNCATED: &str = "ceiling_truncated";

/// Descriptor-aware candidate surface. Virtual scope ancestry is exact and opaque; a legacy
/// string is deliberately represented as a filesystem descriptor by the compatibility wrapper.
pub fn memory_candidates_payload_for_descriptor(
    store: &crate::MemoryStore,
    task: &str,
    descriptor: &crate::scope::ScopeDescriptorV1,
    max_candidates: usize,
    repo_root: Option<&Path>,
) -> Result<serde_json::Value, String> {
    // Canonicalize whatever the caller sent (raw filesystem path, slug, or `global`) into the full
    // visibility chain: self + ancestor scopes that hold rows + global. Before 2026-07-16 this
    // passed the raw string into recall (clients send paths like `D:\Claude`), so project-scoped
    // rows never matched and the rich path recalled from the global corpus only (Sol audit P0).
    let scope_started = Instant::now();
    let scopes = descriptor
        .resolve_chain(&store.scopes())
        .map_err(|error| format!("invalid scope descriptor: {error}"))?;
    let scope_ms = scope_started.elapsed().as_secs_f64() * 1000.0;
    // Shared recall owns one-hop augmentation, its bounded graph lane, and effectiveness vetoes.
    // Keeping those policies here as well would double-expand candidates and split behavior across
    // live recall, replay, and federation.
    //
    // F11: request one MORE than the ceiling so a real ceiling-truncation can be told apart from
    // "nothing else scored" without changing what gets served — the first `max_candidates` hits of
    // an N+1 request are the same top-N a request for exactly N would have returned (ranking is
    // deterministic; see `recall_scored`'s doc comment), so this changes zero user-visible output.
    let probe_limit = max_candidates.saturating_add(1);
    let (mut hits, mut stage_elapsed) = store.recall_scored_timed(task, probe_limit, &scopes);
    stage_elapsed.recall_ms += scope_ms;
    let dropped_by_ceiling = if hits.len() > max_candidates {
        hits.split_off(max_candidates)
    } else {
        Vec::new()
    };
    let rank_started = Instant::now();
    let candidates: Vec<serde_json::Value> = hits
        .iter()
        .map(|(e, score)| {
            let preview = memory_preview(&e.content);
            serde_json::json!({
                "id": format!("memory:role:{}", e.id),
                "layer": 7,
                "sourceKind": "memory",
                "sourceRef": e.scope_id.clone(),
                "sourceHash": sha256_hex(&e.content),
                "trustClass": "agent_verified",
                "instructionPolicy": "data_only",
                "providerScore": score.clamp(0.0, 1.0),
                // `structural` is the key the planner's memory-relevance gate reads
                // (planner.rs: structural<=0 && lexical<0.85 -> memory_low_relevance). Emit the
                // cosine relevance as structural so real hits clear the gate.
                "scoreComponents": {"structural": score.clamp(0.0, 1.0), "relevance": score.clamp(0.0, 1.0)},
                "estimatedTokens": std::cmp::max(1, preview.chars().count() / 4),
                "protected": false,
                "exact": false,
                "recoverable": true,
                "resolver": format!("cortex get {}", e.id),
                "text": preview,
            })
        })
        .collect();
    let omissions: Vec<serde_json::Value> = dropped_by_ceiling
        .iter()
        .map(|(e, _score)| {
            serde_json::json!({
                "id": format!("memory:role:{}", e.id),
                "layer": 7,
                "reason": OMISSION_REASON_CEILING_TRUNCATED,
            })
        })
        .collect();
    stage_elapsed.rank_ms += rank_started.elapsed().as_secs_f64() * 1000.0;

    // F11: reuse the freshness verdict machinery `/freshness` already exposes rather than
    // inventing a second staleness concept. `stable` is the verdict's own truth-in-observation
    // signal (the epoch sandwich did or did not hold across the read); its negation is `stale`.
    // When no repo root is available at this call site at all, that is itself an unverifiable
    // condition — express it honestly as `stale: true`, never fall back to `false`.
    let stale = repo_root.is_none_or(|root| {
        !crate::freshness::evaluate_repository_freshness(store, root.to_path_buf()).stable
    });

    let indexed_at = iso_now();
    Ok(serde_json::json!({
        "schemaVersion": 1,
        "traceId": new_trace_id(),
        "indexedAt": indexed_at,
        "task": task,
        "mode": "verify",
        "provider": "cortex",
        "freshness": {
            "revision": cortex_revision(),
            "indexedAt": indexed_at,
            "stale": stale,
        },
        "providerCeiling": {
            "maxCandidates": max_candidates,
            "maxEstimatedTokens": 4096,
        },
        "candidates": candidates,
        "omissions": omissions,
        "scope": scopes.first().cloned().unwrap_or_default(),
        "_membrane": {
            "stageElapsedMs": {
                "embed": stage_elapsed.embed_ms,
                "recall": stage_elapsed.recall_ms,
                "rank": stage_elapsed.rank_ms,
            }
        },
    }))
}

/// Bounded, word-boundary content preview for a memory candidate's delivered text.
fn memory_preview(content: &str) -> String {
    const CAP: usize = 200;
    let normalized = content.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= CAP {
        return normalized;
    }
    let truncated: String = normalized.chars().take(CAP).collect();
    let cut = truncated.rfind(' ').unwrap_or(truncated.len());
    format!("{}…", &truncated[..cut])
}

fn sha256_hex(s: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(s.as_bytes()))
}

pub(crate) fn db_path_for(workspace: &Path) -> PathBuf {
    std::env::var("CORTEX_DB")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            let home = if cfg!(windows) {
                std::env::var_os("USERPROFILE").map(PathBuf::from)
            } else {
                std::env::var_os("HOME").map(PathBuf::from)
            };
            home.map(|p| p.join(".claude").join("cortex").join("cortex.db"))
        })
        .unwrap_or_else(|| {
            workspace
                .join("tools")
                .join(".cache")
                .join("cortex")
                .join("cortex.db")
        })
}

fn new_trace_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("rc-mem-{ts:x}")
}

fn iso_now() -> String {
    crate::time::now_iso()
}

fn cortex_revision() -> String {
    std::env::var("MEMBRANE_REVISION").unwrap_or_else(|_| "membrane-0.1.1-federation".to_string())
}

fn gateway_observability(raw: &Value) -> Value {
    let mut fields = serde_json::Map::new();
    if let Some(membrane) = raw.get("_membrane") {
        for field in [
            "providerCounts",
            "providerWarnings",
            "providerElapsedMs",
            "providerStageElapsedMs",
            "serviceGeneration",
            "firstAfterIdle",
            "idleGapMs",
            "stageElapsedMs",
        ] {
            if let Some(value) = membrane.get(field) {
                fields.insert(field.to_string(), value.clone());
            }
        }
    }
    if let Some(graph_state) = raw
        .get("freshness")
        .and_then(|freshness| freshness.get("graphState"))
    {
        fields.insert("graphState".to_string(), graph_state.clone());
    }
    Value::Object(fields)
}

#[cfg(test)]
mod observability_tests {
    use super::gateway_observability;

    #[test]
    fn preserves_content_free_gateway_observability_for_clients() {
        let raw = serde_json::json!({
            "freshness": {"graphState": "dirty_overlay"},
            "_membrane": {
                "providerCounts": {"git": 2},
                "providerWarnings": [],
                "providerElapsedMs": {"git": 1.25},
                "providerStageElapsedMs": {"cortex": {"embed": 2.5, "recall": 3.5}},
                "stageElapsedMs": {"freshness": 2.0, "provider_fanout": 3.0},
                "idleGapMs": 300001,
                "serviceGeneration": "svc-test",
                "firstAfterIdle": true
            }
        });

        assert_eq!(
            gateway_observability(&raw),
            serde_json::json!({
                "providerCounts": {"git": 2},
                "providerWarnings": [],
                "providerElapsedMs": {"git": 1.25},
                "providerStageElapsedMs": {"cortex": {"embed": 2.5, "recall": 3.5}},
                "stageElapsedMs": {"freshness": 2.0, "provider_fanout": 3.0},
                "idleGapMs": 300001,
                "graphState": "dirty_overlay",
                "serviceGeneration": "svc-test",
                "firstAfterIdle": true
            })
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_request_forwards_explicit_sufficiency_contract_only() {
        let contract = serde_json::json!({
            "schemaVersion": 1,
            "policy": "membrane-sufficiency-v1",
            "requirements": [{
                "id": "repository-evidence",
                "evidenceClass": "repository_file",
                "acceptableProviders": ["blueprint"],
                "minimumCandidates": 1
            }],
            "maxCorrectiveStages": 1
        });
        let with_contract = native_request(
            "task",
            Path::new(r"C:\repo"),
            100,
            1000,
            "release".to_owned(),
            "test",
            "session",
            Vec::new(),
            None,
            Some(contract.clone()),
        );
        assert_eq!(
            with_contract.extensions.get("sufficiencyContract"),
            Some(&contract)
        );

        let without_contract = native_request(
            "task",
            Path::new(r"C:\repo"),
            100,
            1000,
            "release".to_owned(),
            "test",
            "session",
            Vec::new(),
            None,
            None,
        );
        assert!(!without_contract
            .extensions
            .contains_key("sufficiencyContract"));
    }

    #[test]
    fn native_request_forwards_same_request_h8_ceiling() {
        let ceiling = membrane_protocol::host_observation::RemainingContextCeilingV1 {
            schema_version:
                membrane_protocol::host_observation::REMAINING_CONTEXT_CEILING_SCHEMA_VERSION,
            ceiling_id: "ceiling-1".to_owned(),
            session_id: "session-1".to_owned(),
            task_id: membrane_protocol::host_observation::ObservedFieldV1::complete(
                "task-1".to_owned(),
            ),
            requested_at_unix_ms: 1_700_000_000_000,
            remaining_tokens: membrane_protocol::host_observation::TokenEstimateV1::complete(
                membrane_protocol::host_observation::EstimatorBasisV1::new("test", "v1"),
                128,
            ),
            provenance_receipt:
                membrane_protocol::host_observation::HostObservationProvenanceV1::new(
                    "receipt-1",
                    "test-host",
                    1_700_000_000_000,
                    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                ),
        };
        let request = native_request_with_h8(
            "task",
            Path::new(r"C:\repo"),
            100,
            1000,
            "release".to_owned(),
            "test",
            "session-1",
            Vec::new(),
            None,
            None,
            &ceiling,
        );
        assert_eq!(
            request.extensions.get("remainingContextCeiling"),
            Some(&serde_json::to_value(ceiling).unwrap())
        );
    }

    #[test] #[rustfmt::skip] fn federation_emits_content_free_cache_prefix_diagnostic() {
        let mut source: Value = serde_json::from_str(include_str!("../../../../../schemas/registry/context-candidate-set.v1.golden.json")).unwrap(); source["generationId"] = serde_json::json!("gen-current"); source["candidates"][0]["sourceKind"] = serde_json::json!("graph");
        source["candidates"][0]["sourceResolution"] = serde_json::json!({"schemaVersion":1,"candidateId":"cand-blueprint-types","provider":"blueprint-treesitter","status":"resolved","expectedHash":"sha256:0000000000000000000000000000000000000000000000000000000000000011","resolvedHash":"sha256:0000000000000000000000000000000000000000000000000000000000000011","expectedGeneration":"gen-current","resolvedGeneration":"gen-current","expectedPath":"engine/crates/membrane-protocol/src/types.rs:1-60","resolvedPath":"engine/crates/membrane-protocol/src/types.rs:1-60","resolver":"source_read"}); let ccs = serde_json::to_string(&source).unwrap();
        let payload = envelope_from_ccs(
            &ccs,
            EnvelopeInput {
                max_tokens: 4096,
                packet_char_budget_override: None,
                packet_char_budget_model: None,
                accepted_receipt_versions: vec![2],
                scope_grant_present: false,
                scope_grant_fence: None,
                gateway_process_ms: 0.0,
            },
        )
        .expect("golden CCS plans");
        let diagnostic = &payload["cachePrefixDiagnostic"];
        assert_eq!(diagnostic["schemaVersion"], 1);
        let serialized = serde_json::to_string(diagnostic).unwrap();
        assert!(!serialized.contains("ScopeGrantV1"));
        assert!(!serialized.contains("single source of truth"));
        assert!(diagnostic["blockDigests"].is_array());
        assert_eq!(payload["sourceResolutionReceipts"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn missing_session_uses_canonical_opaque_identity() {
        let generated = federation_session_id(None);
        assert!(generated.starts_with("session-"));
        assert_eq!(generated.len(), 40);
        assert!(generated[8..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')));
        assert_ne!(generated, "anonymous-session");
        assert_eq!(
            federation_session_id(Some("scope-grant-session".to_string())),
            "scope-grant-session"
        );
    }

    #[test]
    fn memory_candidates_rank_topical_and_emit_content_preview() {
        let store = crate::MemoryStore::new();
        let _ = store.remember(
            "Always answer briefly and tersely, cutting all filler and preamble.",
            vec![],
        );
        let _ = store.remember(
            "The nginx container is dockerized; diff the confs before any rebuild.",
            vec![],
        );
        let _ = store.remember(
            "Vast.ai GPU rental uses the vastai CLI and an API key from the env.",
            vec![],
        );
        let payload = memory_candidates_payload(
            &store,
            "answer briefly and tersely please",
            "global",
            5,
            None,
        );
        let cands = payload["candidates"].as_array().expect("candidates array");
        assert!(!cands.is_empty(), "expected memory candidates");
        let top = &cands[0];
        let text = top["text"].as_str().unwrap().to_lowercase();
        // Relevance: the brief memory wins for a brief/terse query.
        assert!(
            text.contains("briefly") || text.contains("tersely"),
            "top candidate text should be the brief memory content, got: {text}"
        );
        // text is CONTENT, not an id/slug (the bug this fixes).
        assert!(!text.starts_with("memory:role:") && !text.starts_with("mem-"));
        assert!(top["resolver"].as_str().unwrap().starts_with("cortex get "));
    }

    #[test]
    fn memory_preview_truncates_on_word_boundary() {
        let long = "word ".repeat(100);
        let p = memory_preview(&long);
        assert!(p.chars().count() <= 201, "preview must be capped");
        assert!(p.ends_with('…'));
        assert!(
            !p.contains("wor…"),
            "must cut on a word boundary, not mid-word"
        );
    }

    /// Minimal, hermetic git repo fixture for the F11 freshness tests below — explicit identity
    /// flags so this works in any sandbox regardless of global git config.
    fn init_git_repo(dir: &Path) {
        let run = |args: &[&str]| {
            let status = Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(args)
                .env_remove("GIT_DIR")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .expect("git must be available to run this test");
            assert!(status.success(), "git {args:?} failed in {}", dir.display());
        };
        run(&["init", "--quiet"]);
        run(&[
            "-c",
            "user.email=federation-test@example.com",
            "-c",
            "user.name=federation-test",
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "init",
        ]);
    }

    /// F11 — Git presence alone is not repository-truth evidence. Blueprint owns current
    /// repository truth, so a coherent Git repository without a Blueprint observation must fail
    /// closed as stale.
    #[test]
    fn git_repository_without_blueprint_evidence_is_stale() {
        let repo = tempfile::tempdir().unwrap();
        init_git_repo(repo.path());
        let store = crate::MemoryStore::new();
        let payload = memory_candidates_payload(&store, "any task", "global", 5, Some(repo.path()));
        assert_eq!(
            payload["freshness"]["stale"], true,
            "Git presence without Blueprint evidence must report stale=true, payload={payload}"
        );
    }

    /// F11 — the inverse: a directory that is not a git repository at all makes the freshness
    /// epoch unreadable (Indeterminate/unstable), and having no repo root at all is itself an
    /// unverifiable condition. Both must honestly report `stale: true` — never fall back to the
    /// old hardcoded `false`.
    #[test]
    fn stale_is_true_when_the_freshness_signal_cannot_be_verified() {
        let store = crate::MemoryStore::new();

        let non_repo = tempfile::tempdir().unwrap();
        let broken =
            memory_candidates_payload(&store, "any task", "global", 5, Some(non_repo.path()));
        assert_eq!(broken["freshness"]["stale"], true);

        let unknown = memory_candidates_payload(&store, "any task", "global", 5, None);
        assert_eq!(unknown["freshness"]["stale"], true);
    }

    /// F11 — candidates pushed past the caller's `max_candidates` ceiling must be recorded as
    /// omissions, not silently dropped, while the served candidate list is still capped exactly
    /// as before.
    #[test]
    fn omissions_reports_candidates_dropped_by_the_ceiling_truncation() {
        let store = crate::MemoryStore::new();
        for n in 0..5 {
            let _ = store.remember(
                &format!("ceiling truncation fixture memory entry number {n}"),
                vec![],
            );
        }
        let payload = memory_candidates_payload(
            &store,
            "ceiling truncation fixture memory entry",
            "global",
            2,
            None,
        );
        let candidates = payload["candidates"].as_array().expect("candidates array");
        assert_eq!(
            candidates.len(),
            2,
            "the ceiling must still cap what is served, payload={payload}"
        );
        let omissions = payload["omissions"].as_array().expect("omissions array");
        assert!(
            !omissions.is_empty(),
            "entries pushed past the ceiling must be recorded as omissions, not silently \
             dropped: {payload}"
        );
        assert!(omissions
            .iter()
            .all(|omission| omission["reason"] == "ceiling_truncated"));
    }
}
