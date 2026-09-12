//! Resident Adapt application service. MCP is inspection-only; local operator
//! commands keep the existing reviewed Cortex admission boundary. No DB path
//! can be selected by a transport and no call starts a daemon.
use crate::{store::TasteDeliveryInventoryV1, MemoryStore};
use membrane_adapt::{delivery::*, scope::ScopeDimensions};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const OPERATOR_PATH: &str = "/v1/adapt/operator";
pub const OBSERVATION_PATH: &str = "/v1/adapt/observations";
pub const MAX_INPUT_BYTES: usize = 1_048_576;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdaptHostContextV1 {
    pub client: Option<String>,
    pub model: Option<String>,
    pub machine: Option<String>,
    #[serde(default)]
    pub dimensions: BTreeMap<String, String>,
}

impl AdaptHostContextV1 {
    pub fn dimensions(&self) -> Result<ScopeDimensions, String> {
        let mut raw = self.dimensions.clone();
        // Identity keys cannot be smuggled in using normalized aliases.
        let normalized = ScopeDimensions::normalize(&raw).map_err(|e| e.to_string())?;
        for (key, value) in [("client", &self.client), ("model", &self.model)] {
            if let Some(value) = value {
                if normalized.get(key).is_some_and(|v| v != value) {
                    return Err(format!("conflicting host {key} identity"));
                }
                raw.retain(|k, _| k.trim().to_lowercase() != key);
                raw.insert(key.into(), value.clone());
            }
        }
        ScopeDimensions::normalize(&raw).map_err(|e| e.to_string())
    }
}

/// Shared selection boundary. Inspection never persists an exposure receipt.
pub fn select(
    store: &MemoryStore,
    context: &PreferenceDeliveryContextV1,
) -> Result<(TasteDeliveryInventoryV1, PreferenceDeliveryPlanV1), String> {
    if context.allowed_scopes.is_empty()
        || context.max_total_records > 50
        || context.max_core_records > 4
        || context.max_scoped_records > 32
        || context.max_rendered_chars > 65536
    {
        return Err("invalid Adapt selection bounds".into());
    }
    if context.dimensions.get("user").is_some() || context.dimensions.get("org").is_some() {
        return Err("authenticated user/org applicability binding unavailable".into());
    }
    let mut inventory = store.taste_delivery_inventory()?;
    // Source visibility is stricter than applicability: don't return receipts,
    // identifiers or rules from another user's/org's/repository's scope.
    inventory.candidates.retain(|c| {
        let dimension_identity_matches = ["user", "org", "repo"].iter().all(|key| {
            c.scope_dimensions.get(key).is_none_or(|v| {
                context
                    .dimensions
                    .get(key)
                    .is_some_and(|actual| actual.eq_ignore_ascii_case(v))
            })
        });
        let scope_visible = context.allowed_scopes.contains(&c.scope)
            || (c.scope.starts_with("dimensions:")
                && c.scope_dimensions.matches(&context.dimensions));
        dimension_identity_matches && scope_visible
    });
    let plan = select_delivery_candidates(&inventory.candidates, context);
    Ok((inventory, plan))
}

pub fn inspect_preferences(
    store: &MemoryStore,
    scope: &str,
    dimensions: ScopeDimensions,
    machine: Option<String>,
    model: Option<String>,
    limit: usize,
) -> Result<Value, String> {
    if scope.trim().is_empty() || limit > 32 {
        return Err("invalid inspection scope/limit".into());
    }
    let scope = crate::scope::normalize_scope(scope);
    if dimensions
        .get("repo")
        .is_some_and(|repo| crate::scope::normalize_scope(repo) != scope)
    {
        return Err("repository dimension exceeds inspection scope".into());
    }
    let context = PreferenceDeliveryContextV1 {
        allowed_scopes: crate::scope_chain(&scope, &store.scopes()),
        client: dimensions.get("client").unwrap_or("unknown").into(),
        dimensions,
        machine,
        model,
        max_core_records: 4,
        max_scoped_records: 32,
        max_total_records: limit,
        max_rendered_chars: 65536,
        timestamp: crate::time::now_iso(),
        session_id: String::new(),
        trace_id: String::new(),
        request_id: "inspection".into(),
    };
    let (inventory, plan) = select(store, &context)?;
    let records: Vec<_> = plan
        .delivered
        .iter()
        .map(|p| {
            json!({
                "record_id": p.record_id, "rule": p.rule,
                "scope": inventory.scope_for_record(&p.record_id), "receipt": p.receipt,
            })
        })
        .collect();
    Ok(
        json!({"contract":"adapt.inspection.v1", "inspection_only":true,
        "exposure_recorded":false, "records":records, "decisions":plan.receipts}),
    )
}

pub fn inspect_issues(store: &MemoryStore, scope: &str, limit: usize) -> Result<Value, String> {
    if scope.trim().is_empty() || limit > 32 {
        return Err("invalid issue inspection bounds".into());
    }
    let scope = crate::scope::normalize_scope(scope);
    let conn = store.db().lock();
    let mut stmt = conn.prepare("SELECT id,content,lifecycle_state FROM memories WHERE artifact_family='adapt' AND record_type='insight_issue' AND scope_id=?1 ORDER BY id LIMIT ?2")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params![scope, (limit + 1) as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut items = Vec::new();
    for row in rows {
        let (id, content, lifecycle) = row.map_err(|e| e.to_string())?;
        let issue: membrane_adapt::insights::sealed_issue::SealedInsightIssueV1 =
            serde_json::from_str(&content).map_err(|e| e.to_string())?;
        issue
            .verify()
            .map_err(|e| format!("invalid stored Insight seal: {e:?}"))?;
        items.push(json!({"id":id,"family":issue.payload.family,"description":issue.payload.canonical_description,
            "honesty_limit":issue.payload.honesty_limit,"lifecycle":lifecycle,
            "evidence_refs":issue.payload.episode_refs,"payload_sha256":issue.payload_sha256}));
    }
    let truncated = items.len() > limit;
    items.truncate(limit);
    Ok(
        json!({"contract":"adapt.insights-inspection.v1","inspection_only":true,"items":items,"truncated":truncated}),
    )
}

pub fn status(store: &MemoryStore, scope: &str, session: Option<&str>) -> Result<Value, String> {
    let scope = crate::scope::normalize_scope(scope);
    if scope.trim().is_empty() {
        return Err("scope required".into());
    }
    let observations = store
        .db()
        .reference_events(&scope, "adapt.detector_coverage", 1)?;
    let emissions = store
        .db()
        .reference_events(&scope, "adapt.packet_emitted", 1)?;
    let acknowledgements = store
        .db()
        .reference_events(&scope, "adapt.host_acknowledgement", 1)?;
    let comparisons = store.db().reference_events(&scope, "adapt.comparison", 1)?;
    let latest = |page: &cortex_store::reference_events::ReferenceEventPage| {
        page.events.first().map(|e| json!({
        "receipt_id":e.event_id,"recorded_at_ms":if e.recorded_at_ms == 0 {None}else{Some(e.recorded_at_ms)},"content_sha256":e.content_hash}))
    };
    let env_configured =
        std::env::var(crate::background_review::BACKGROUND_SEMANTIC_PROVIDER_ENDPOINT_ENV)
            .is_ok_and(|v| !v.trim().is_empty());
    // Configuration is not connectivity, and last activity is not current health.
    Ok(
        json!({"contract":"adapt.live-status.v1", "installation_id":store.installation_id(),
        "cortex_store_id":store.cortex_store_id(),"release_generation":crate::release_identity::release_generation(),
        "scope":scope, "session_id":session, "qualified":false,
        "lanes":{
            "explicit_taste":{"supported":true,"reachable":true,"qualified":false},
            "automatic_taste":{"supported":true,"configured":env_configured,"enabled":null,"reachable":null,"reason":"semantic_provider_health_unavailable"},
            "insights":{"supported":true,"last_receipt":latest(&observations),"producer_reachable":null,
                "reason":if observations.events.is_empty(){"producer_progress_unavailable"}else{"host_submitted_window_only"}},
            "review":{"supported":true,"surface":"local_operator","pending_count":null,"reason":"review_queue_projection_unavailable"},
            "admission":{"supported":true,"reachable":true,"owner":"cortex"},
            "delivery":{"supported":true,"last_emission":latest(&emissions),"last_host_acknowledgement":latest(&acknowledgements)},
            "effectiveness":{"supported":true,"last_comparison":latest(&comparisons),"qualified":false,"reason":"exact_outcome_join_required"}
        },"observed_at_ms":crate::time::now_millis()}),
    )
}

/// Evidence-only receipt in Cortex's existing append-only event store, not a
/// second Adapt truth database. Reuse of an id with different meaning fails.
pub(crate) fn journal(
    store: &MemoryStore,
    scope: &str,
    kind: &str,
    id: &str,
    payload: Value,
) -> Result<Value, String> {
    if id.trim().is_empty() || id.len() > 512 {
        return Err("invalid Adapt receipt identity".into());
    }
    let key = membrane_adapt::canonical::sha256_canonical(&json!([scope, kind, id]));
    let session_id = format!("adapt:{key}");
    let hash = membrane_adapt::canonical::sha256_canonical(&payload);
    let events = cortex_store::AbsorbedStore::new(store.db().clone()).map_err(|e| e.to_string())?;
    if let Some(existing) = events
        .events_range(&session_id, 1, 2)
        .map_err(|e| e.to_string())?
        .first()
    {
        if existing.content_hash != hash || existing.payload != payload {
            return Err("Adapt receipt identity conflict".into());
        }
        return Ok(json!({"receipt_id":existing.event_id,"content_sha256":hash,"replayed":true}));
    }
    let event = cortex_store::SessionEvent {
        schema_version: 1,
        session_id,
        seq: 1,
        event_id: format!("adapt:{key}"),
        event_type: kind.into(),
        payload,
        scope_id: scope.into(),
        authority: "A0".into(),
        influence_class: "reference".into(),
        lifecycle: "active".into(),
        retention: "local_audit".into(),
        provenance: vec![cortex_store::ProvenanceRef {
            source: "membrane.adapt.service".into(),
            source_event_ids: vec![id.into()],
            producer: Some("adapt_native".into()),
        }],
        content_hash: hash.clone(),
        occurred_at_ms: 0,
        recorded_at_ms: 0,
    };
    // Timestamp is deliberately absent (0) from this immutable receipt. The
    // enclosing service observation stamps time; retries never alter bytes.
    events.append_event(&event).map_err(|e| e.to_string())?;
    Ok(json!({"receipt_id":event.event_id,"content_sha256":hash,"replayed":false}))
}

pub fn operator_response(store: &MemoryStore, body: &str) -> (u16, String) {
    let result = (|| {
        if body.len() > MAX_INPUT_BYTES {
            return Err("Adapt request too large".into());
        }
        let command: crate::cli::AdaptCmd =
            serde_json::from_str(body).map_err(|e| format!("invalid Adapt command: {e}"))?;
        if !command.requires_canonical_store() {
            return Err("offline operation does not require canonical storage".into());
        }
        let deployed = crate::cli::current_deployed_runtime();
        crate::cli::execute_adapt_command(command, Some(store), deployed.as_ref())
    })();
    match result {
        Ok(data) => (200, data.to_string()),
        Err(e) => (
            400,
            json!({"error":"adapt_operation_refused","detail":e}).to_string(),
        ),
    }
}

/// Native signed/local-reviewed Taste admission boundary.  Adapt callers do
/// not route these records through generic Cortex proposal APIs: verification
/// and canonical-pool CAS stay in `try_put_verified_adapt_taste_manifest`.
pub fn admit_verified_taste_manifest(
    store: &MemoryStore,
    manifest: &membrane_adapt::manifest::PreferenceManifestV1,
    trust: Option<&membrane_adapt::proposal::SemanticAdjudicatorTrustStoreV1>,
) -> Result<crate::store::MemoryBatchReceipt, String> {
    store
        .try_put_verified_adapt_taste_manifest(manifest, trust)
        .map_err(|error| error.to_string())
}
