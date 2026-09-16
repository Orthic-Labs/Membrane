//! Resident Adapt application service. MCP is inspection-only; local operator
//! commands keep the existing reviewed Cortex admission boundary. No DB path
//! can be selected by a transport and no call starts a daemon.
use crate::{store::TasteDeliveryInventoryV1, MemoryStore};
use membrane_adapt::{delivery::*, insights::IssueState, learner, scope::ScopeDimensions};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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
        // The lifecycle gate names the governed transitions a reviewer could
        // request through the trusted operator surface. Naming them here is
        // data only — inspection never performs them.
        let available_actions: Vec<Value> = [
            IssueState::Observed,
            IssueState::Recurring,
            IssueState::Confirmed,
            IssueState::MitigationProposed,
            IssueState::Mitigated,
            IssueState::Reopened,
            IssueState::Obsolete,
            IssueState::Dismissed,
        ]
        .into_iter()
        .filter(|target| issue.state.lifecycle.can_transition_to(*target))
        .map(|target| json!({"action":"transition","target_state":target}))
        .collect();
        let receipt_count = issue.state.receipts.len();
        let receipts: Vec<_> = issue
            .state
            .receipts
            .iter()
            .rev()
            .take(16)
            .map(|receipt| {
                json!({"transition":receipt.transition,"at":receipt.at,
                    "actor":receipt.actor,"new_status":receipt.new_status,
                    "receipt_id":receipt.receipt_id})
            })
            .collect();
        items.push(json!({
            "id":id,"schema_version":issue.schema_version,"issue_id":issue.issue_id,
            "scope":&scope,"family":issue.payload.family,
            "description":issue.payload.canonical_description,
            "honesty_limit":issue.payload.honesty_limit,
            "lifecycle":lifecycle,
            "lifecycle_state":issue.state.lifecycle,
            "recurrence":{
                "signature":issue.payload.recurrence_signature,
                "count":issue.state.recurrence_count,
                "first_seen":issue.state.first_seen,
                "last_seen":issue.state.last_seen,
                "after_mitigation":issue.state.recurrence_after_mitigation,
            },
            "applicability":issue.payload.applicability,
            "authority_class":issue.payload.authority_class,
            "influence_class":issue.payload.influence_class,
            "confidence":issue.payload.confidence,
            "evidence_quality":issue.payload.evidence_quality,
            "candidate_mechanisms":issue.payload.candidate_mechanisms,
            "mitigation_links":issue.state.mitigation_links,
            "evidence_refs":issue.payload.episode_refs,
            "evidence_digests":issue.payload.evidence_digests.iter().take(32).collect::<Vec<_>>(),
            "semantic_validator_receipt_id":issue.payload.semantic_validator_receipt_id,
            "available_actions":available_actions,
            "receipt_count":receipt_count,
            "receipts":receipts,
            "updated_at":issue.state.updated_at,
            "payload_sha256":issue.payload_sha256}));
    }
    let truncated = items.len() > limit;
    items.truncate(limit);
    Ok(
        json!({"contract":"adapt.insights-inspection.v1","inspection_only":true,"items":items,"truncated":truncated}),
    )
}

/// Read-only projection over the daemon-owned Cortex proposal queue
/// (ADP-034/ADP-074). Returns typed unavailable when the queue schema has
/// never been installed in this store rather than an empty success — an
/// absent queue is not the same fact as an empty one. Emission text is
/// truncated for inspection; the full emission digest is always included.
pub fn inspect_proposals(store: &MemoryStore, scope: &str, limit: usize) -> Result<Value, String> {
    if scope.trim().is_empty() || limit > 32 {
        return Err("invalid proposal inspection bounds".into());
    }
    let scope = crate::scope::normalize_scope(scope);
    let conn = store.db().lock_events();
    let present = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='membrane_knowledge_proposal'",
            [],
            |_| Ok(()),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .is_some();
    if !present {
        return Ok(json!({"contract":"adapt.proposal-inspection.v1","inspection_only":true,
            "available":false,"reason":"proposal_queue_schema_absent","items":[],"truncated":false}));
    }
    let mut stmt = conn
        .prepare(
            "SELECT p.proposal_id,p.state,p.created_at,p.decided_at,p.reviewer,p.emission_json,p.emission_sha256,
                    a.state,a.attempts,a.last_error
             FROM membrane_knowledge_proposal p
             LEFT JOIN cortex_proposal_admission_v1 a USING(proposal_id)
             WHERE p.scope_id=?1 ORDER BY p.created_at DESC,p.proposal_id LIMIT ?2",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params![scope, (limit + 1) as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, Option<i64>>(8)?,
                row.get::<_, Option<String>>(9)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut items = Vec::new();
    for row in rows {
        let (id, state, created, decided, reviewer, emission, emission_sha, admission, attempts, last_error) =
            row.map_err(|e| e.to_string())?;
        let emission: Value = serde_json::from_str(&emission).unwrap_or(Value::Null);
        let full_text = emission
            .get("text")
            .or_else(|| emission.get("content"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let full_text_chars = full_text.chars().count();
        let text: String = full_text.chars().take(512).collect();
        items.push(json!({
            "proposal_id":id,"review_state":state,"created_at":created,
            "decided_at":decided,"reviewer":reviewer,
            "emission_sha256":emission_sha,
            "kind":emission.get("kind"),"producer":emission.get("producer"),
            "epistemic_class":emission.get("epistemicClass"),
            "adapt_produced":emission.get("producer").and_then(Value::as_str)
                .is_some_and(|producer| producer.starts_with("adapt")),
            "text_excerpt":text,"text_truncated":full_text_chars > 512,
            "admission_state":admission,"admission_attempts":attempts,
            "admission_blocked":last_error.is_some(),
            "last_error":last_error,
        }));
    }
    let truncated = items.len() > limit;
    items.truncate(limit);
    Ok(json!({"contract":"adapt.proposal-inspection.v1","inspection_only":true,
        "available":true,"items":items,"truncated":truncated}))
}

/// Locate the workspace/state root the daemon wrote sidecars under. The
/// event DB path is authoritative; `WORKSPACE_ROOT` is the documented
/// fallback for stores whose path sits outside the conventional
/// `<workspace>/tools/...` layout. An in-memory store has no root — the
/// learner lane then reports typed unobserved rather than guessing.
fn learner_workspace_root(store: &MemoryStore) -> Option<PathBuf> {
    store
        .db()
        .event_db_path()
        .and_then(|path| {
            path.ancestors()
                .find(|ancestor| {
                    ancestor
                        .file_name()
                        .is_some_and(|name| name == "tools")
                })
                .and_then(Path::parent)
                .map(Path::to_path_buf)
        })
        .or_else(|| {
            std::env::var_os("WORKSPACE_ROOT")
                .map(PathBuf::from)
                .filter(|value| !value.as_os_str().is_empty())
        })
}

/// Bounded tail read of a JSONL sidecar. Oversized/unreadable files are
/// reported, never silently treated as empty.
fn tail_jsonl(path: &Path, max_bytes: u64) -> Result<Vec<Value>, String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let len = file.metadata().map_err(|e| e.to_string())?.len();
    let start = len.saturating_sub(max_bytes);
    file.seek(SeekFrom::Start(start)).map_err(|e| e.to_string())?;
    let mut buffer = String::new();
    file.read_to_string(&mut buffer)
        .map_err(|e| e.to_string())?;
    let mut lines: Vec<&str> = buffer.lines().collect();
    if start > 0 {
        // First entry may be a partial line at the seek boundary.
        lines.remove(0);
    }
    Ok(lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect())
}

/// ADP-075: daemon-backed learner-lane projection built only from observable
/// bindings — the daemon's durable observation sidecar, the proposal sink
/// queue and its drain cursor, and the tray-written input snapshot. Nothing
/// here is a health claim: absent sidecars stay typed absent.
fn learner_lane(store: &MemoryStore, provider_configured: bool) -> Value {
    let Some(root) = learner_workspace_root(store) else {
        return json!({"supported":true,"observable":false,
            "reason":"workspace_root_unavailable_for_daemon_sidecars",
            "provider_endpoint_configured":provider_configured});
    };
    let observations_path = std::env::var_os(crate::background_review::OBSERVATIONS_PATH_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            root.join(crate::background_review::DEFAULT_OBSERVATIONS_RELATIVE_PATH)
        });
    let proposals_path = std::env::var_os(crate::background_review::BACKGROUND_REVIEW_PROPOSALS_PATH_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            root.join(crate::background_review::DEFAULT_PROPOSALS_RELATIVE_PATH)
        });
    let snapshot_path = crate::background_review_input::input_path(&root);

    let snapshot = std::fs::metadata(&snapshot_path).ok();
    let snapshot_modified_ms = snapshot
        .as_ref()
        .and_then(|meta| meta.modified().ok())
        .and_then(|at| {
            at.duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|d| d.as_millis() as u64)
        });

    let observations = if observations_path.is_file() {
        let oversized = std::fs::metadata(&observations_path)
            .map(|meta| meta.len() > 256 * 1024)
            .unwrap_or(false);
        match tail_jsonl(&observations_path, 256 * 1024) {
            Ok(records) => {
                let last = records.last().and_then(|record| {
                    let observation = record.get("observation")?;
                    Some(json!({
                        "observation_id":record.get("observationId"),
                        "job_kind":observation.get("kind"),
                        "status":observation.get("status"),
                        "reason":observation.get("reason"),
                        "observed_at_ms":observation.get("observedAtUnixMs"),
                        "hub_active":observation.get("hubActive"),
                        "foreground_active":observation.get("foregroundActive"),
                    }))
                });
                json!({"present":true,"records_seen":records.len(),
                    "records_truncated":oversized,
                    "last":last})
            }
            Err(error) => json!({"present":true,"readable":false,"reason":error}),
        }
    } else {
        json!({"present":false})
    };

    let (queued, drained) = if proposals_path.is_file() {
        let queued = std::fs::read_to_string(&proposals_path)
            .ok()
            .map(|body| body.lines().filter(|line| !line.trim().is_empty()).count());
        let cursor_path = {
            let mut name = proposals_path.as_os_str().to_owned();
            name.push(".cursor");
            PathBuf::from(name)
        };
        let drained = std::fs::read_to_string(&cursor_path)
            .ok()
            .and_then(|raw| raw.trim().parse::<usize>().ok());
        (queued, drained)
    } else {
        (None, None)
    };
    let pending_records = match (queued, drained) {
        (Some(q), Some(d)) => Some(q.saturating_sub(d)),
        (Some(q), None) => Some(q),
        _ => None,
    };

    // Factual lane state: derived only from what the daemon durably recorded.
    let last_obs = observations.get("last");
    let state = if snapshot.is_none()
        && !observations["present"].as_bool().unwrap_or(false)
        && queued.is_none()
    {
        // No snapshot, no observations, no proposals: genuinely empty lane.
        "empty"
    } else if last_obs.is_none() {
        if snapshot.is_some() {
            "awaiting_first_run"
        } else {
            "no_daemon_observations"
        }
    } else {
        match last_obs
            .and_then(|o| o.get("status"))
            .and_then(Value::as_str)
            .unwrap_or("")
        {
            "deferred" => "deferred",
            "failed" => "failed",
            "cancelled" => "cancelled",
            "started" => "running",
            "completed" => "completed",
            _ => "unobserved",
        }
    };

    json!({"supported":true,"observable":true,"state":state,
        "provider_endpoint_configured":provider_configured,
        "first_party_analyzer":learner::LEARNER_ANALYZER_ID,
        "input_snapshot":{"path":snapshot_path,"present":snapshot.is_some(),
            "last_modified_ms":snapshot_modified_ms},
        "observations":observations,
        "proposal_sink":{"path":proposals_path,
            "present":queued.is_some(),"queued_records":queued,
            "drained_through":drained,"pending_records":pending_records}})
}

/// Live projection over the daemon-owned proposal queue. Table absence is
/// typed unavailable, not an empty queue.
fn review_queue(store: &MemoryStore, scope: &str) -> Value {
    let conn = store.db().lock_events();
    let present = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='membrane_knowledge_proposal'",
            [],
            |_| Ok(()),
        )
        .optional()
        .map(|row| row.is_some())
        .unwrap_or(false);
    if !present {
        return json!({"available":false,"reason":"proposal_queue_schema_absent"});
    }
    let mut review_states: BTreeMap<String, i64> = BTreeMap::new();
    if let Ok(mut stmt) =
        conn.prepare("SELECT state,COUNT(*) FROM membrane_knowledge_proposal WHERE scope_id=?1 GROUP BY state")
    {
        if let Ok(rows) = stmt.query_map(rusqlite::params![scope], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        }) {
            for row in rows.flatten() {
                review_states.insert(row.0, row.1);
            }
        }
    }
    let mut admission_states: BTreeMap<String, i64> = BTreeMap::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT a.state,COUNT(*) FROM cortex_proposal_admission_v1 a
         JOIN membrane_knowledge_proposal p USING(proposal_id)
         WHERE p.scope_id=?1 GROUP BY a.state",
    ) {
        if let Ok(rows) = stmt.query_map(rusqlite::params![scope], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        }) {
            for row in rows.flatten() {
                admission_states.insert(row.0, row.1);
            }
        }
    }
    let adapt_pending: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM membrane_knowledge_proposal
             WHERE scope_id=?1 AND state='pending' AND emission_json LIKE '%\"producer\":\"adapt%'",
            rusqlite::params![scope],
            |row| row.get(0),
        )
        .unwrap_or(0);
    json!({"available":true,"review_states":review_states,
        "pending":review_states.get("pending").copied().unwrap_or(0),
        "adapt_pending":adapt_pending,"admission_states":admission_states})
}

pub fn status(store: &MemoryStore, scope: &str, session: Option<&str>) -> Result<Value, String> {
    let scope = crate::scope::normalize_scope(scope);
    if scope.trim().is_empty() {
        return Err("scope required".into());
    }
    let observations = store
        .db()
        .reference_events(&scope, "adapt.detector_coverage", 128)?;
    let emissions = store
        .db()
        .reference_events(&scope, "adapt.packet_emitted", 1)?;
    let acknowledgements = store
        .db()
        .reference_events(&scope, "adapt.host_acknowledgement", 1)?;
    let comparisons = store.db().reference_events(&scope, "adapt.comparison", 1)?;
    let outcome_joins = store
        .db()
        .reference_events(&scope, "adapt.outcome_join", 128)?;
    let latest = |page: &cortex_store::reference_events::ReferenceEventPage| {
        page.events.first().map(|e| json!({
        "receipt_id":e.event_id,"recorded_at_ms":if e.recorded_at_ms == 0 {None}else{Some(e.recorded_at_ms)},"content_sha256":e.content_hash}))
    };
    let env_configured =
        std::env::var(crate::background_review::BACKGROUND_SEMANTIC_PROVIDER_ENDPOINT_ENV)
            .is_ok_and(|v| !v.trim().is_empty());
    let queue = review_queue(store, &scope);
    let queue_available = queue["available"].as_bool().unwrap_or(false);
    let pending_count = if queue_available {
        queue["pending"].clone()
    } else {
        Value::Null
    };
    // Distinguish a missing evidence stream from an empty-but-observed one.
    let stream_state = if observations.available {
        "available"
    } else {
        "unavailable"
    };
    let workload = if observations.events.is_empty() {
        "empty"
    } else {
        "observed"
    };
    let last_coverage = observations.events.first();
    let coverage_state = last_coverage
        .and_then(|event| event.payload.get("state"))
        .and_then(Value::as_str);
    let coverage_missing = last_coverage
        .and_then(|event| event.payload.get("missing_fields"))
        .cloned()
        .unwrap_or(json!([]));
    let missing_outcome_joins = observations
        .events
        .len()
        .saturating_sub(outcome_joins.events.len());
    // Configuration is not connectivity, and last activity is not current health.
    Ok(
        json!({"contract":"adapt.live-status.v1", "installation_id":store.installation_id(),
        "cortex_store_id":store.cortex_store_id(),"release_generation":crate::release_identity::release_generation(),
        "scope":scope, "session_id":session, "qualified":false,
        "lanes":{
            "explicit_taste":{"supported":true,"reachable":true,"qualified":false},
            "automatic_taste":{"supported":true,"configured":env_configured,"enabled":null,"reachable":null,"reason":"semantic_provider_health_unavailable"},
            "insights":{"supported":true,"evidence_stream":stream_state,"workload":workload,
                "coverage_windows_seen":observations.events.len(),"coverage_truncated":observations.truncated,
                "outcome_joins_seen":outcome_joins.events.len(),"missing_outcome_joins":missing_outcome_joins,
                "last_coverage_state":coverage_state,"last_coverage_missing_fields":coverage_missing,
                "last_receipt":latest(&observations),"producer_reachable":null,
                "reason":if observations.events.is_empty(){"producer_progress_unavailable"}else{"host_submitted_window_only"}},
            "review":{"supported":true,"surface":"local_operator","queue":queue,
                "pending_count":pending_count,
                "reason":if queue_available{"live_queue_projection"}else{"review_queue_projection_unavailable"}},
            "background_learner":learner_lane(store, env_configured),
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

// ---------------------------------------------------------------------------
// ADP-035: first-party semantic provider for `adapt_behavioral_review`.
//
// The provider executes `membrane_adapt::learner` — deterministic behavioral
// learner semantics over the request's already-validated event window — and
// returns proposal-only values. It holds no store, no sink, and no cursor:
// `execute_background_semantic_review` remains the only path onward, and the
// cursor advances only after the proposal sink accepts the records.
// ---------------------------------------------------------------------------

/// Result-construction helper shared by the refusal and proposal paths.
fn adapt_review_result(
    request: &membrane_protocol::background_review::BackgroundSemanticReviewRequestV1,
    observed_at_unix_ms: u64,
    curation_proposals: Vec<Value>,
    next_cursor: Option<membrane_protocol::background_review::BackgroundReviewCursorV1>,
    status: membrane_protocol::background_review::BackgroundSemanticReviewStatusV1,
) -> membrane_protocol::background_review::BackgroundSemanticReviewResultV1 {
    // The receipt binds this result to the exact request bytes analyzed; no
    // upstream host receipt exists because no external producer ran.
    let receipt_digest =
        membrane_protocol::digest_str(&membrane_protocol::canonical_json_of(request));
    let provenance_receipt = membrane_protocol::HostObservationProvenanceV1::new(
        format!("{}-{}", learner::LEARNER_ANALYZER_ID, request.job_id),
        format!(
            "{}@{}/{}",
            learner::LEARNER_ANALYZER_ID,
            learner::LEARNER_ANALYZER_VERSION,
            request.job_id
        ),
        observed_at_unix_ms,
        receipt_digest,
    );
    membrane_protocol::background_review::BackgroundSemanticReviewResultV1 {
        schema_version:
            membrane_protocol::background_review::BackgroundSemanticReviewResultV1::SCHEMA_VERSION,
        job_id: request.job_id.clone(),
        job_kind: request.job_kind,
        session_id: request.session_id.clone(),
        task_id: request.task_id.clone(),
        turn_id: request.turn_id.clone(),
        curation_proposals,
        memory_candidates: Vec::new(),
        next_cursor,
        model: None,
        provider: Some(format!(
            "{}@{}",
            learner::LEARNER_ANALYZER_ID,
            learner::LEARNER_ANALYZER_VERSION
        )),
        usage: None,
        provenance_receipt,
        status,
    }
}

/// First-party provider that runs the Adapt behavioral learner for
/// [`BackgroundReviewJobKindV1::AdaptBehavioralReview`]. Other job kinds are
/// refused with a typed blocked result: an Adapt learner must not impersonate
/// a Cortex semantic-dream or memory-extraction provider.
#[derive(Debug, Default)]
pub struct AdaptBehavioralReviewProvider {
    now_unix_ms: Option<u64>,
}

impl AdaptBehavioralReviewProvider {
    pub fn new() -> Self {
        Self::default()
    }

    /// Pin the clock for deterministic tests.
    pub fn with_now_unix_ms(mut self, now_unix_ms: u64) -> Self {
        self.now_unix_ms = Some(now_unix_ms);
        self
    }

    fn now(&self) -> u64 {
        self.now_unix_ms.unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .try_into()
                .unwrap_or(u64::MAX)
        })
    }
}

impl crate::background_review::BackgroundSemanticReviewProvider
    for AdaptBehavioralReviewProvider
{
    fn execute(
        &self,
        request: &membrane_protocol::background_review::BackgroundSemanticReviewRequestV1,
    ) -> Result<
        membrane_protocol::background_review::BackgroundSemanticReviewResultV1,
        crate::background_review::BackgroundSemanticReviewProviderError,
    > {
        use membrane_protocol::background_review::BackgroundSemanticReviewStatusV1 as Status;
        use membrane_protocol::BackgroundReviewReasonV1 as Reason;
        let blocked = |request: &membrane_protocol::background_review::BackgroundSemanticReviewRequestV1,
                       reason: Reason| {
            adapt_review_result(
                request,
                self.now(),
                Vec::new(),
                None,
                Status::Blocked { reason },
            )
        };
        if request.job_kind != membrane_protocol::BackgroundReviewJobKindV1::AdaptBehavioralReview {
            return Ok(blocked(request, Reason::InvalidJob));
        }
        // Fail closed at the provider boundary too; never trust that the
        // caller already validated the frame.
        if request.validate().is_err() {
            return Ok(blocked(request, Reason::InvalidJob));
        }
        // One scope binding per window. A mixed-scope window would make the
        // proposal's scope claim ambiguous, so it is refused rather than
        // guessed.
        let scope_ids = request
            .events
            .iter()
            .map(|event| event.scope_id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        if scope_ids.len() > 1 {
            return Ok(blocked(request, Reason::InvalidJob));
        }
        let scope_id = scope_ids.iter().next().copied().unwrap_or("").to_string();
        let input = learner::AdaptLearnerInputV1 {
            job_id: request.job_id.clone(),
            session_id: request.session_id.clone(),
            scope_id,
            cursor_last_seq: request.cursor.last_seq,
            events: request
                .events
                .iter()
                .map(|event| learner::LearnerEventV1 {
                    event_id: event.event_id.clone(),
                    seq: event.seq,
                    event_type: event.event_type.clone(),
                    scope_id: event.scope_id.clone(),
                    content_hash: event.content_hash.clone(),
                    occurred_at_ms: event.occurred_at_ms,
                    payload: event.payload.clone(),
                })
                .collect(),
        };
        let outcome = learner::run_adapt_behavioral_review(&input);
        match outcome.status {
            learner::AdaptLearnerStatusV1::Unavailable { reason } => {
                let reason = match reason {
                    learner::LearnerUnavailableReason::EmptyWindow => {
                        Reason::CursorInputUnavailable
                    }
                    learner::LearnerUnavailableReason::InvalidInput => Reason::InvalidJob,
                };
                Ok(blocked(request, reason))
            }
            learner::AdaptLearnerStatusV1::NoFindings => {
                // The protocol forbids cursor advancement without proposals:
                // a clean window is re-examined next run, never skipped.
                Ok(adapt_review_result(
                    request,
                    self.now(),
                    Vec::new(),
                    None,
                    Status::Proposals,
                ))
            }
            learner::AdaptLearnerStatusV1::Proposals => {
                let proposals = outcome
                    .proposals
                    .iter()
                    .map(serde_json::to_value)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| {
                        crate::background_review::BackgroundSemanticReviewProviderError::Encode(
                            error.to_string(),
                        )
                    })?;
                Ok(adapt_review_result(
                    request,
                    self.now(),
                    proposals,
                    Some(
                        membrane_protocol::background_review::BackgroundReviewCursorV1 {
                            session_id: request.session_id.clone(),
                            last_seq: outcome.consumed_through_seq,
                        },
                    ),
                    Status::Proposals,
                ))
            }
        }
    }
}

/// Dispatching first-party provider for a default deployment: the Adapt
/// behavioral learner serves `adapt_behavioral_review`; Cortex's deterministic
/// analyzer serves the Cortex-owned kinds. Wiring this type keeps the honest
/// refusal boundary — a deterministic duplicate/assertion analyzer never
/// impersonates a behavioral learner, and vice versa.
pub struct AdaptFirstPartySemanticReviewProvider {
    adapt: AdaptBehavioralReviewProvider,
    cortex: crate::background_review::DeterministicFirstPartySemanticReviewProvider,
}

impl std::fmt::Debug for AdaptFirstPartySemanticReviewProvider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AdaptFirstPartySemanticReviewProvider")
            .finish_non_exhaustive()
    }
}

impl AdaptFirstPartySemanticReviewProvider {
    pub fn new() -> Self {
        Self {
            adapt: AdaptBehavioralReviewProvider::new(),
            cortex:
                crate::background_review::DeterministicFirstPartySemanticReviewProvider::new(),
        }
    }
}

impl Default for AdaptFirstPartySemanticReviewProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::background_review::BackgroundSemanticReviewProvider
    for AdaptFirstPartySemanticReviewProvider
{
    fn execute(
        &self,
        request: &membrane_protocol::background_review::BackgroundSemanticReviewRequestV1,
    ) -> Result<
        membrane_protocol::background_review::BackgroundSemanticReviewResultV1,
        crate::background_review::BackgroundSemanticReviewProviderError,
    > {
        match request.job_kind {
            membrane_protocol::BackgroundReviewJobKindV1::AdaptBehavioralReview => {
                self.adapt.execute(request)
            }
            _ => self.cortex.execute(request),
        }
    }
}
