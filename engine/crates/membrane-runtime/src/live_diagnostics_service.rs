//! Host-facing Live Diagnostics operational capability — design
//! `docs/design/membrane-live-diagnostics-final-architecture.md` §12 (public
//! operational surface), §11 (host modes), §10 (semantic edit fence), and §14
//! (storage).
//!
//! [`DiagnosticsService`] owns per-workspace [`DiagnosticsSession`]s, the one
//! [`DiagnosticsSupervisor`] that supervises qualified engines, the
//! planner-supplied gate policy profiles (the planner owns policy; the service
//! only stores and applies it), named fence baselines, and an append-only
//! audit sink under the platform data root (`<data-root>/diagnostics/
//! audit.jsonl`). Events never clear the edit fence; only
//! [`DiagnosticsService::snapshot_await`] evaluating planner policy through
//! `membrane_protocol::diagnostics::evaluate_gate` does.
//!
//! This module never computes parent Membrane health: Phase 3 Hub lifecycle
//! and the Hub-status repair own that surface.

use crate::live_diagnostics::{
    AbsoluteDeadline, CapabilityKind, DiagnosticsProvider, DiagnosticsSession,
    DiagnosticsSupervisor, LiveDiagnosticsConfig, LiveDiagnosticsError, ProviderCapabilities,
    ReconcileClassification, SideEffectClass, WorkspaceEngineKey, LIVE_DIAGNOSTICS_SCHEMA_VERSION,
};
use axum::extract::{DefaultBodyLimit, Query, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use membrane_protocol::diagnostics::{
    evaluate_gate, CapabilityVocabulary, ChangedFileHashV1, ConvergenceClass, CostClass,
    DiagnosticEvidenceSnapshotV1, DiagnosticGateDecisionV1, GateOutcome, GatePolicyProfileV1,
    WorkspaceEpochOrigin, WorkspaceEpochV1,
};
use membrane_protocol::CanonicalSerialize;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tower_http::timeout::TimeoutLayer;

/// Schema version stamped on service-level responses.
pub const DIAGNOSTICS_SERVICE_SCHEMA_VERSION: &str = "LiveDiagnosticsServiceV1";

/// Schema version stamped on every audit line (design §14).
pub const DIAGNOSTICS_AUDIT_SCHEMA_VERSION: &str = "live-diagnostics-audit.v1";

/// The one seeded policy profile. The planner owns policy content; the
/// service stores the profile and fills `requiredCapabilities` per request.
pub const DEFAULT_POLICY_PROFILE_NAME: &str = "changed-files-zero";

/// Audit sink location relative to the platform data root (design §14).
pub const AUDIT_RELATIVE_PATH: &str = "diagnostics/audit.jsonl";

/// Largest accepted request body for the diagnostics routes. Evidence
/// snapshots legitimately carry large observation arrays; the limit stays
/// generous for a loopback control surface while still refusing runaways.
const MAX_DIAGNOSTICS_BODY_BYTES: usize = 4 * 1024 * 1024;

/// Parity with the resident workload timeout: a wedged provider must hit the
/// supervisor deadline long before this ceiling.
const DIAGNOSTICS_REQUEST_TIMEOUT: Duration = Duration::from_secs(90);

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Typed failures surfaced as `{error:{code,detail}}` omission envelopes with
/// stable machine-readable codes.
#[derive(Debug, thiserror::Error)]
pub enum LiveDiagnosticsServiceError {
    #[error("workspace {repo_id}/{worktree_id} is not open")]
    WorkspaceNotOpen {
        repo_id: String,
        worktree_id: String,
    },
    #[error("{0}")]
    EpochNotMonotonic(String),
    #[error("mutation boundary violated: {0}")]
    MutationBoundary(String),
    #[error("no sealed workspace epoch is available")]
    NoSealedEpoch,
    #[error("gate policy profile {profile_name:?} is not registered")]
    PolicyUnknown { profile_name: String },
    #[error("{0}")]
    Provider(String),
    #[error("fence for {repo_id}/{worktree_id} is not cleared by any decision")]
    FenceNotCleared {
        repo_id: String,
        worktree_id: String,
    },
    #[error(transparent)]
    Supervisor(#[from] LiveDiagnosticsError),
}

impl LiveDiagnosticsServiceError {
    /// Stable omission-envelope code for this failure.
    pub fn code(&self) -> &'static str {
        match self {
            Self::WorkspaceNotOpen { .. } => "workspace_not_open",
            Self::EpochNotMonotonic(_) => "epoch_not_monotonic",
            Self::MutationBoundary(_) => "mutation_boundary",
            Self::NoSealedEpoch => "no_sealed_epoch",
            Self::PolicyUnknown { .. } => "policy_unknown",
            Self::Provider(_) => "provider_error",
            Self::FenceNotCleared { .. } => "fence_not_cleared",
            Self::Supervisor(inner) => supervisor_error_code(inner),
        }
    }
}

fn supervisor_error_code(error: &LiveDiagnosticsError) -> &'static str {
    match error {
        LiveDiagnosticsError::Config(_) => "config_invalid",
        LiveDiagnosticsError::NoSealedEpoch => "no_sealed_epoch",
        LiveDiagnosticsError::MutationBoundary(_) => "mutation_boundary",
        LiveDiagnosticsError::EpochNotMonotonic(_) => "epoch_not_monotonic",
        LiveDiagnosticsError::NoQualifiedProvider(_) => "provider_error",
        LiveDiagnosticsError::Provider(_) => "provider_error",
    }
}

fn session_key(repo_id: &str, worktree_id: &str) -> (String, String) {
    (repo_id.to_string(), worktree_id.to_string())
}

fn workspace_not_open(repo_id: &str, worktree_id: &str) -> LiveDiagnosticsServiceError {
    LiveDiagnosticsServiceError::WorkspaceNotOpen {
        repo_id: repo_id.to_string(),
        worktree_id: worktree_id.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Audit sink (design §14)
// ---------------------------------------------------------------------------

/// Append-only JSON Lines audit sink. Persisted records: sealed workspace
/// epochs, gate decisions, reconciliation results, and named baselines. Every
/// write failure is skipped silently — auditing degrades before the service
/// ever panics or fails a request because the audit file is unavailable.
struct AuditSink {
    path: PathBuf,
}

impl AuditSink {
    fn under_data_root(data_root: &Path) -> Self {
        Self {
            path: data_root.join(AUDIT_RELATIVE_PATH),
        }
    }

    fn record(&self, kind: &str, payload: Value) {
        let entry = json!({
            "schemaVersion": DIAGNOSTICS_AUDIT_SCHEMA_VERSION,
            "kind": kind,
            "recordedAtUnixMs": now_unix_ms(),
            "record": payload,
        });
        let Ok(mut line) = serde_json::to_string(&entry) else {
            return;
        };
        line.push('\n');
        let Some(parent) = self.path.parent() else {
            return;
        };
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            use std::io::Write as _;
            let _ = file.write_all(line.as_bytes());
        }
    }
}

fn now_unix_ms() -> u64 {
    crate::time::now_millis() as u64
}

// ---------------------------------------------------------------------------
// Service state
// ---------------------------------------------------------------------------

/// Per-workspace entry pairing the fence session with the service-tracked
/// transactional-mutation flag (`DiagnosticsSession` exposes no accessor for
/// it, and the service mediates every `begin`/`seal` pair).
struct SessionEntry {
    session: DiagnosticsSession,
    open_mutation: bool,
}

impl SessionEntry {
    fn new() -> Self {
        Self {
            session: DiagnosticsSession::new(),
            open_mutation: false,
        }
    }
}

/// One named fence baseline: the cleared decision reference plus the manifest
/// digest of the exact bytes that decision describes (design §12
/// `baseline.capture`/`baseline.update`). `DiagnosticGateDecisionV1` carries
/// no standalone id, so the clearing snapshot id together with the policy
/// digest is the stable reference recorded here and in the audit.
struct NamedBaseline {
    name: String,
    decision_ref: String,
    policy_digest: String,
    manifest_digest: String,
    epoch_number: u64,
    captured_at_unix_ms: u64,
}

impl NamedBaseline {
    fn payload(&self, repo_id: &str, worktree_id: &str, action: &str) -> Value {
        json!({
            "action": action,
            "repoId": repo_id,
            "worktreeId": worktree_id,
            "name": self.name,
            "decisionRef": self.decision_ref,
            "policyDigest": self.policy_digest,
            "manifestDigest": self.manifest_digest,
            "epochNumber": self.epoch_number,
            "capturedAtUnixMs": self.captured_at_unix_ms,
        })
    }
}

/// Owns the §12 operational capability: sessions keyed by
/// `(repo_id, worktree_id)`, one supervisor with default configuration, the
/// seeded planner policy map, named baselines, and the audit sink.
pub struct DiagnosticsService {
    supervisor: DiagnosticsSupervisor,
    sessions: HashMap<(String, String), SessionEntry>,
    policies: HashMap<String, GatePolicyProfileV1>,
    baselines: HashMap<(String, String), HashMap<String, NamedBaseline>>,
    audit: AuditSink,
}

impl DiagnosticsService {
    /// Build a service whose audit sink lives under the platform data root
    /// (`crate::paths::data_root()` + [`AUDIT_RELATIVE_PATH`]).
    pub fn new() -> Result<Self, LiveDiagnosticsServiceError> {
        Self::with_data_root(crate::paths::data_root())
    }

    /// Build a service with the audit sink rooted at an explicit directory.
    /// Tests and alternative hosts pin their own root; production callers use
    /// [`DiagnosticsService::new`].
    pub fn with_data_root(data_root: PathBuf) -> Result<Self, LiveDiagnosticsServiceError> {
        let supervisor = DiagnosticsSupervisor::new(LiveDiagnosticsConfig::default())?;
        Ok(Self {
            supervisor,
            sessions: HashMap::new(),
            policies: seeded_policies(),
            baselines: HashMap::new(),
            audit: AuditSink::under_data_root(&data_root),
        })
    }

    /// Registration seam for qualified engine adapters, delegating to the
    /// supervisor. Factories are consumed lazily on first acquisition.
    pub fn register_provider(
        &mut self,
        key: WorkspaceEngineKey,
        capabilities: ProviderCapabilities,
        factory: Box<dyn FnOnce() -> Box<dyn DiagnosticsProvider> + Send>,
    ) {
        self.supervisor.register(key, capabilities, factory);
    }

    // -- workspace lifecycle ------------------------------------------------

    /// Open (or idempotently reopen) the diagnostics workspace session for
    /// one repo/worktree pair.
    pub fn workspace_open(
        &mut self,
        repo_id: &str,
        worktree_id: &str,
    ) -> Result<Value, LiveDiagnosticsServiceError> {
        let key = session_key(repo_id, worktree_id);
        let created = !self.sessions.contains_key(&key);
        if created {
            self.sessions.insert(key, SessionEntry::new());
        }
        let mut status = self.workspace_status(repo_id, worktree_id)?;
        if let Some(object) = status.as_object_mut() {
            object.insert("created".to_string(), Value::Bool(created));
        }
        Ok(status)
    }

    /// Close the workspace session. Closing drops the fence session and any
    /// open-mutation flag; sealed epochs and decisions remain in the audit.
    pub fn workspace_close(
        &mut self,
        repo_id: &str,
        worktree_id: &str,
    ) -> Result<Value, LiveDiagnosticsServiceError> {
        let key = session_key(repo_id, worktree_id);
        self.sessions
            .remove(&key)
            .ok_or_else(|| workspace_not_open(repo_id, worktree_id))?;
        Ok(json!({
            "schemaVersion": DIAGNOSTICS_SERVICE_SCHEMA_VERSION,
            "repoId": repo_id,
            "worktreeId": worktree_id,
            "closed": true,
        }))
    }

    /// Report one workspace session's fence state without mutating it.
    pub fn workspace_status(
        &self,
        repo_id: &str,
        worktree_id: &str,
    ) -> Result<Value, LiveDiagnosticsServiceError> {
        let entry = self
            .sessions
            .get(&session_key(repo_id, worktree_id))
            .ok_or_else(|| workspace_not_open(repo_id, worktree_id))?;
        Ok(workspace_status_value(repo_id, worktree_id, entry))
    }

    // -- mutation boundary --------------------------------------------------

    /// Open one coherent mutation batch (design §4.1 transactional mode).
    pub fn mutation_begin(
        &mut self,
        repo_id: &str,
        worktree_id: &str,
    ) -> Result<Value, LiveDiagnosticsServiceError> {
        let entry = self
            .sessions
            .get_mut(&session_key(repo_id, worktree_id))
            .ok_or_else(|| workspace_not_open(repo_id, worktree_id))?;
        entry.session.begin_mutation();
        entry.open_mutation = true;
        Ok(json!({
            "schemaVersion": DIAGNOSTICS_SERVICE_SCHEMA_VERSION,
            "repoId": repo_id,
            "worktreeId": worktree_id,
            "openMutation": true,
        }))
    }

    /// Seal the open mutation with its resulting workspace epoch. The epoch
    /// must identify the addressed workspace; sealing persists the sealed
    /// epoch to the audit and invalidates prior fence clearance.
    pub fn mutation_seal(
        &mut self,
        repo_id: &str,
        worktree_id: &str,
        epoch: WorkspaceEpochV1,
    ) -> Result<Value, LiveDiagnosticsServiceError> {
        validate_epoch_identity(repo_id, worktree_id, &epoch)?;
        let entry = self
            .sessions
            .get_mut(&session_key(repo_id, worktree_id))
            .ok_or_else(|| workspace_not_open(repo_id, worktree_id))?;
        entry.session.seal_mutation(epoch.clone())?;
        entry.open_mutation = false;
        self.audit.record(
            "epoch_sealed",
            json!({
                "repoId": repo_id,
                "worktreeId": worktree_id,
                "mode": origin_label(&epoch),
                "epoch": serde_json::to_value(&epoch).unwrap_or(Value::Null),
            }),
        );
        Ok(json!({
            "schemaVersion": DIAGNOSTICS_SERVICE_SCHEMA_VERSION,
            "repoId": repo_id,
            "worktreeId": worktree_id,
            "sealedEpoch": epoch.epoch,
            "parentEpoch": epoch.parent_epoch,
            "fenceCleared": false,
        }))
    }

    /// Register exact observed resulting bytes in `observed_hook` mode with
    /// the same monotonic validation, audit persistence, and clearance
    /// invalidation as a transactional seal (design §11).
    pub fn mutation_register_observed(
        &mut self,
        repo_id: &str,
        worktree_id: &str,
        epoch: WorkspaceEpochV1,
    ) -> Result<Value, LiveDiagnosticsServiceError> {
        validate_epoch_identity(repo_id, worktree_id, &epoch)?;
        let entry = self
            .sessions
            .get_mut(&session_key(repo_id, worktree_id))
            .ok_or_else(|| workspace_not_open(repo_id, worktree_id))?;
        entry.session.register_observed(epoch.clone())?;
        self.audit.record(
            "epoch_sealed",
            json!({
                "repoId": repo_id,
                "worktreeId": worktree_id,
                "mode": origin_label(&epoch),
                "epoch": serde_json::to_value(&epoch).unwrap_or(Value::Null),
            }),
        );
        Ok(json!({
            "schemaVersion": DIAGNOSTICS_SERVICE_SCHEMA_VERSION,
            "repoId": repo_id,
            "worktreeId": worktree_id,
            "observedEpoch": epoch.epoch,
            "parentEpoch": epoch.parent_epoch,
            "fenceCleared": false,
        }))
    }

    // -- reconciliation -----------------------------------------------------

    /// Classify current worktree bytes against the latest cleared epoch.
    /// Returns `"cleared"`, `"superseded"`, or `"unknown_conflict"`; every
    /// result is persisted to the audit (design §14).
    pub fn workspace_reconcile(
        &mut self,
        repo_id: &str,
        worktree_id: &str,
        manifest_digest: &str,
        hashes: &[ChangedFileHashV1],
    ) -> Result<&'static str, LiveDiagnosticsServiceError> {
        let entry = self
            .sessions
            .get_mut(&session_key(repo_id, worktree_id))
            .ok_or_else(|| workspace_not_open(repo_id, worktree_id))?;
        let pairs: Vec<(String, String)> = hashes
            .iter()
            .map(|hash| (hash.path.clone(), hash.hash.clone()))
            .collect();
        let classification = entry.session.reconcile(manifest_digest, &pairs);
        let label = classification_label(classification);
        self.audit.record(
            "reconcile",
            json!({
                "repoId": repo_id,
                "worktreeId": worktree_id,
                "classification": label,
                "manifestDigest": manifest_digest,
            }),
        );
        Ok(label)
    }

    // -- snapshots, fence, baselines ----------------------------------------

    /// Resolve the effective planner policy for one acquisition: the stored
    /// profile supplies identity and blocking codes; the request supplies the
    /// required capabilities. The digest is `sha256:` over the sorted-key
    /// canonical JSON serialization of the effective profile with
    /// `policyDigest` left empty during hashing, so it is reproducible from
    /// the profile alone.
    fn resolve_policy(
        &self,
        profile_name: &str,
        required_capabilities: &[CapabilityVocabulary],
    ) -> Result<GatePolicyProfileV1, LiveDiagnosticsServiceError> {
        let stored = self
            .policies
            .get(profile_name)
            .ok_or_else(|| LiveDiagnosticsServiceError::PolicyUnknown {
                profile_name: profile_name.to_string(),
            })?;
        let mut effective = stored.clone();
        effective.required_capabilities = required_capabilities.to_vec();
        effective.policy_digest = String::new();
        effective.policy_digest = effective.canonical_digest();
        Ok(effective)
    }

    /// Run every registered provider for the workspace within the request's
    /// cost ceiling and absolute deadline, assemble the exact-evidence
    /// snapshot, evaluate planner policy, and clear the fence only on
    /// `clean_exact`. Returns the gate decision.
    pub fn snapshot_await(
        &mut self,
        request: &SnapshotAwaitRequest,
    ) -> Result<DiagnosticGateDecisionV1, LiveDiagnosticsServiceError> {
        let deadline_ms = request
            .deadline_ms
            .unwrap_or(self.supervisor.config().default_deadline_ms);
        let deadline = AbsoluteDeadline::after(self.supervisor.now_monotonic_ms(), deadline_ms);
        let policy =
            self.resolve_policy(&request.policy_profile_name, &request.required_capabilities)?;
        let keys: Vec<WorkspaceEngineKey> = self
            .supervisor
            .registered_keys()
            .into_iter()
            .filter(|key| key.repo_id == request.repo_id && key.worktree_id == request.worktree_id)
            .cloned()
            .collect();
        let max_cost = request.max_cost.unwrap_or_default();
        let entry = self
            .sessions
            .get_mut(&session_key(&request.repo_id, &request.worktree_id))
            .ok_or_else(|| workspace_not_open(&request.repo_id, &request.worktree_id))?;
        let sealed = entry
            .session
            .latest_sealed()
            .cloned()
            .ok_or(LiveDiagnosticsServiceError::NoSealedEpoch)?;
        let decision = entry.session.acquire_snapshot(
            &mut self.supervisor,
            &keys,
            Vec::new(),
            &sealed,
            &policy,
            max_cost,
            deadline,
        )?;
        self.audit.record(
            "gate_decision",
            json!({
                "repoId": request.repo_id,
                "worktreeId": request.worktree_id,
                "policyProfile": request.policy_profile_name,
                "boundEpoch": sealed.epoch,
                "manifestDigest": sealed.source_manifest_digest,
                "decision": serde_json::to_value(&decision).unwrap_or(Value::Null),
            }),
        );
        Ok(decision)
    }

    /// Pure `fence.evaluate` passthrough over planner-owned policy. Takes no
    /// service state and mutates nothing: identical inputs always produce an
    /// identical decision (design §12 enforcement path).
    pub fn evaluate_fence(
        snapshot: &DiagnosticEvidenceSnapshotV1,
        expected_epoch: &WorkspaceEpochV1,
        policy: &GatePolicyProfileV1,
    ) -> DiagnosticGateDecisionV1 {
        evaluate_gate(snapshot, expected_epoch, policy)
    }

    /// Record the current cleared decision as a named baseline in memory and
    /// in the audit.
    pub fn baseline_capture(
        &mut self,
        repo_id: &str,
        worktree_id: &str,
        name: &str,
    ) -> Result<Value, LiveDiagnosticsServiceError> {
        self.record_baseline(repo_id, worktree_id, name, "capture")
    }

    /// Refresh a named baseline to the current cleared decision (upsert) in
    /// memory and in the audit.
    pub fn baseline_update(
        &mut self,
        repo_id: &str,
        worktree_id: &str,
        name: &str,
    ) -> Result<Value, LiveDiagnosticsServiceError> {
        self.record_baseline(repo_id, worktree_id, name, "update")
    }

    // -- provider lifecycle ---------------------------------------------------

    /// Targeted provider shutdown by workspace-engine-key digest. The current
    /// supervisor surface has no per-key shutdown, so this uses eviction
    /// semantics: matched instances past the idle threshold are shut down and
    /// lazily restart on next use; warm instances keep running until idle. An
    /// unknown digest is a typed `provider_error`.
    pub fn provider_restart(
        &mut self,
        key_digest: &str,
    ) -> Result<Value, LiveDiagnosticsServiceError> {
        let matched: Vec<WorkspaceEngineKey> = self
            .supervisor
            .registered_keys()
            .into_iter()
            .filter(|key| key.digest() == key_digest)
            .cloned()
            .collect();
        if matched.is_empty() {
            return Err(LiveDiagnosticsServiceError::Provider(format!(
                "no registered provider key matches digest {key_digest}"
            )));
        }
        let restarted = self.supervisor.evict_idle();
        Ok(json!({
            "schemaVersion": DIAGNOSTICS_SERVICE_SCHEMA_VERSION,
            "keyDigest": key_digest,
            "matched": true,
            "semantics": "evict_idle",
            "restarted": restarted
                .iter()
                .map(|key| json!({
                    "keyDigest": key.digest(),
                    "engineId": key.engine_id,
                }))
                .collect::<Vec<_>>(),
        }))
    }

    // -- reporting ------------------------------------------------------------

    /// Declared provider inventory, seeded policy profiles, and per-session
    /// fence state (§12 `diagnostics.capabilities`).
    pub fn capabilities(&self) -> Value {
        let mut keys: Vec<&WorkspaceEngineKey> = self.supervisor.registered_keys();
        keys.sort();
        let providers = keys
            .iter()
            .filter_map(|key| {
                let caps = self.supervisor.registry_capabilities(key)?;
                Some(json!({
                    "keyDigest": key.digest(),
                    "repoId": key.repo_id,
                    "worktreeId": key.worktree_id,
                    "engineId": key.engine_id,
                    "engineVersion": key.engine_version,
                    "providerId": caps.provider_id,
                    "version": caps.version,
                    "capabilities": caps
                        .capabilities
                        .iter()
                        .map(capability_kind_label)
                        .collect::<Vec<_>>(),
                    "sideEffectClass": side_effect_class_label(caps.side_effect_class),
                    "convergenceClass": convergence_class_label(caps.convergence_class),
                    "costClass": cost_class_label(caps.cost_class),
                }))
            })
            .collect::<Vec<_>>();
        let mut profiles: Vec<&str> = self.policies.keys().map(String::as_str).collect();
        profiles.sort();
        let sessions = self
            .sessions
            .iter()
            .map(|((repo_id, worktree_id), entry)| {
                json!({
                    "repoId": repo_id,
                    "worktreeId": worktree_id,
                    "fenceCleared": entry.session.cleared_decision().is_some(),
                })
            })
            .collect::<Vec<_>>();
        json!({
            "schemaVersion": DIAGNOSTICS_SERVICE_SCHEMA_VERSION,
            "engineSchemaVersion": LIVE_DIAGNOSTICS_SCHEMA_VERSION,
            "policyProfiles": profiles,
            "providers": providers,
            "sessions": sessions,
        })
    }

    /// Operational status summary: latest sealed epoch number, per-session
    /// cleared outcomes, active acquisitions, and the configuration summary.
    pub fn status(&self) -> Value {
        let mut latest_sealed: Option<u64> = None;
        let mut sessions = Vec::new();
        for ((repo_id, worktree_id), entry) in &self.sessions {
            if let Some(epoch) = entry.session.latest_sealed().map(|epoch| epoch.epoch) {
                latest_sealed = Some(match latest_sealed {
                    Some(current) => current.max(epoch),
                    None => epoch,
                });
            }
            sessions.push(workspace_status_value(repo_id, worktree_id, entry));
        }
        sessions.sort_by(|left, right| {
            (
                left["repoId"].as_str().unwrap_or_default(),
                left["worktreeId"].as_str().unwrap_or_default(),
            )
                .cmp(&(
                    right["repoId"].as_str().unwrap_or_default(),
                    right["worktreeId"].as_str().unwrap_or_default(),
                ))
        });
        let config = self.supervisor.config();
        json!({
            "schemaVersion": DIAGNOSTICS_SERVICE_SCHEMA_VERSION,
            "latestSealedEpoch": latest_sealed,
            "activeAcquires": self.supervisor.active_acquire_count(),
            "config": {
                "maxConcurrentAcquires": config.max_concurrent_acquires,
                "idleEvictAfterSecs": config.idle_evict_after_secs,
                "defaultDeadlineMs": config.default_deadline_ms,
            },
            "sessions": sessions,
        })
    }

    /// Subscribe placeholder: returns the current status snapshot. Streaming
    /// presentation events are telemetry-only and can never clear the fence
    /// (design §12), so the placeholder stays truthful until a consumer needs
    /// more than the snapshot.
    pub fn subscribe(&self) -> Value {
        self.status()
    }
}

fn workspace_status_value(repo_id: &str, worktree_id: &str, entry: &SessionEntry) -> Value {
    json!({
        "schemaVersion": DIAGNOSTICS_SERVICE_SCHEMA_VERSION,
        "repoId": repo_id,
        "worktreeId": worktree_id,
        "open": true,
        "openMutation": entry.open_mutation,
        "latestSealedEpoch": entry.session.latest_sealed().map(|epoch| epoch.epoch),
        "manifestDigest": entry
            .session
            .latest_sealed()
            .map(|epoch| epoch.source_manifest_digest.clone()),
        "fenceCleared": entry.session.cleared_decision().is_some(),
        "clearedOutcome": entry
            .session
            .cleared_decision()
            .map(|decision| gate_outcome_label(decision.outcome)),
        "clearedDecisionRef": entry
            .session
            .cleared_decision()
            .map(|decision| decision.snapshot_id.clone()),
    })
}

fn seeded_policies() -> HashMap<String, GatePolicyProfileV1> {
    let mut policies = HashMap::new();
    policies.insert(
        DEFAULT_POLICY_PROFILE_NAME.to_string(),
        GatePolicyProfileV1 {
            profile_name: DEFAULT_POLICY_PROFILE_NAME.to_string(),
            policy_version: "v1".to_string(),
            policy_digest: String::new(),
            blocking_codes: Vec::new(),
            required_capabilities: Vec::new(),
        },
    );
    policies
}

fn validate_epoch_identity(
    repo_id: &str,
    worktree_id: &str,
    epoch: &WorkspaceEpochV1,
) -> Result<(), LiveDiagnosticsServiceError> {
    if epoch.repo_id != repo_id || epoch.worktree_id != worktree_id {
        return Err(LiveDiagnosticsServiceError::MutationBoundary(format!(
            "epoch identifies {}/{}, request addresses {}/{}",
            epoch.repo_id, epoch.worktree_id, repo_id, worktree_id
        )));
    }
    Ok(())
}

fn classification_label(classification: ReconcileClassification) -> &'static str {
    match classification {
        ReconcileClassification::Cleared => "cleared",
        ReconcileClassification::Superseded => "superseded",
        ReconcileClassification::UnknownConflict => "unknown_conflict",
    }
}

fn gate_outcome_label(outcome: GateOutcome) -> String {
    serde_json::to_value(outcome)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn origin_label(epoch: &WorkspaceEpochV1) -> &'static str {
    match epoch.origin {
        WorkspaceEpochOrigin::Transactional => "transactional",
        WorkspaceEpochOrigin::ObservedHook => "observed_hook",
        WorkspaceEpochOrigin::Reconciliation => "reconciliation",
    }
}

fn capability_kind_label(kind: &CapabilityKind) -> &'static str {
    match kind {
        CapabilityKind::Parser => "parser",
        CapabilityKind::RepositoryFinding => "repository_finding",
        CapabilityKind::NativeLanguageService => "native_language_service",
        CapabilityKind::StaticAnalyzer => "static_analyzer",
        CapabilityKind::CompilerCheck => "compiler_check",
    }
}

fn side_effect_class_label(class: SideEffectClass) -> &'static str {
    match class {
        SideEffectClass::PureAnalysis => "pure_analysis",
        SideEffectClass::RepositoryPluginLoad => "repository_plugin_load",
        SideEffectClass::PackageManagerAccess => "package_manager_access",
        SideEffectClass::CompilerSpawn => "compiler_spawn",
        SideEffectClass::BuildScriptExecution => "build_script_execution",
        SideEffectClass::NetworkRequired => "network_required",
    }
}

fn convergence_class_label(class: ConvergenceClass) -> &'static str {
    match class {
        ConvergenceClass::PullExact => "pull_exact",
        ConvergenceClass::PushVersionedExact => "push_versioned_exact",
        ConvergenceClass::SnapshotCheckerExact => "snapshot_checker_exact",
        ConvergenceClass::PushUnversionedAdvisory => "push_unversioned_advisory",
        ConvergenceClass::Unsupported => "unsupported",
    }
}

fn cost_class_label(class: CostClass) -> &'static str {
    match class {
        CostClass::Instant => "instant",
        CostClass::Interactive => "interactive",
        CostClass::Verification => "verification",
        CostClass::Build => "build",
        CostClass::Test => "test",
    }
}

// ---------------------------------------------------------------------------
// Wire DTOs (camelCase, closed)
// ---------------------------------------------------------------------------

/// Identity of one addressed workspace session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceRequest {
    pub repo_id: String,
    pub worktree_id: String,
}

/// `mutation.seal` / `mutation.registerObserved` request carrying the full
/// resulting workspace epoch envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MutationEpochRequest {
    pub repo_id: String,
    pub worktree_id: String,
    pub epoch: WorkspaceEpochV1,
}

/// Reconciliation request: exact current manifest digest plus changed-file
/// hashes (design §4.1, §11).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReconcileRequest {
    pub repo_id: String,
    pub worktree_id: String,
    pub manifest_digest: String,
    #[serde(default)]
    pub hashes: Vec<ChangedFileHashV1>,
}

/// Snapshot acquisition request (design §12): planner profile name, required
/// capabilities supplied per request, cost ceiling, and absolute deadline.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SnapshotAwaitRequest {
    pub repo_id: String,
    pub worktree_id: String,
    pub policy_profile_name: String,
    #[serde(default)]
    pub required_capabilities: Vec<CapabilityVocabulary>,
    #[serde(default)]
    pub max_cost: Option<CostClass>,
    #[serde(default)]
    pub deadline_ms: Option<u64>,
}

/// Pure gate-evaluation request carrying caller-assembled evidence and
/// planner-owned policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FenceEvaluateRequest {
    pub snapshot: DiagnosticEvidenceSnapshotV1,
    pub expected_epoch: WorkspaceEpochV1,
    pub policy: GatePolicyProfileV1,
}

/// Named baseline capture/update request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BaselineRequest {
    pub repo_id: String,
    pub worktree_id: String,
    pub name: String,
}

/// Targeted provider restart request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderRestartRequest {
    pub key_digest: String,
}

/// Typed omission envelope returned by every failing diagnostics route.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OmissionEnvelope {
    error: OmissionErrorBody,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OmissionErrorBody {
    code: String,
    detail: String,
}

// ---------------------------------------------------------------------------
// HTTP surface
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct DiagnosticsRouteState {
    service: Arc<Mutex<DiagnosticsService>>,
    bearer: Option<Arc<str>>,
}

fn lock_service(service: &Mutex<DiagnosticsService>) -> MutexGuard<'_, DiagnosticsService> {
    service
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Build the diagnostics router around an existing service. When no bearer
/// expectation is configured the router performs no credential check; the
/// resident host always supplies the configured API token so production
/// enforces the same policy as every other non-public resident route.
pub fn diagnostics_router(service: Arc<Mutex<DiagnosticsService>>) -> Router {
    diagnostics_router_with_state(DiagnosticsRouteState {
        service,
        bearer: None,
    })
}

/// Production wiring used by the resident service: constructs the service on
/// the platform data root, attaches the bearer gate fed from the resident API
/// token, and returns the ready-to-merge router. Returns `None` only if the
/// service could not be constructed, which cannot happen under the default
/// configuration.
pub fn resident_diagnostics_routes(expected_bearer: Option<String>) -> Option<Router> {
    let service = DiagnosticsService::new().ok()?;
    Some(diagnostics_router_with_state(DiagnosticsRouteState {
        service: Arc::new(Mutex::new(service)),
        bearer: expected_bearer.map(Arc::<str>::from),
    }))
}

fn diagnostics_router_with_state(state: DiagnosticsRouteState) -> Router {
    Router::new()
        .route("/diagnostics/capabilities", get(get_capabilities))
        .route("/diagnostics/status", get(get_status))
        .route("/diagnostics/workspace/open", post(post_workspace_open))
        .route("/diagnostics/workspace/close", post(post_workspace_close))
        .route("/diagnostics/workspace/status", get(get_workspace_status))
        .route("/diagnostics/reconcile", post(post_reconcile))
        .route("/diagnostics/mutation/begin", post(post_mutation_begin))
        .route("/diagnostics/mutation/seal", post(post_mutation_seal))
        .route(
            "/diagnostics/mutation/registerObserved",
            post(post_mutation_register_observed),
        )
        .route("/diagnostics/snapshot/await", post(post_snapshot_await))
        .route("/diagnostics/fence/evaluate", post(post_fence_evaluate))
        .route("/diagnostics/baseline/capture", post(post_baseline_capture))
        .route("/diagnostics/baseline/update", post(post_baseline_update))
        .route("/diagnostics/provider/restart", post(post_provider_restart))
        .with_state(state.clone())
        .layer(DefaultBodyLimit::max(MAX_DIAGNOSTICS_BODY_BYTES))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            DIAGNOSTICS_REQUEST_TIMEOUT,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state,
            require_bearer,
        ))
}

/// Bearer gate mirroring the resident `dispatch` authorization so explicit
/// diagnostics routes never weaken the resident's credential policy.
async fn require_bearer(
    State(state): State<DiagnosticsRouteState>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Response {
    if let Some(expected) = state.bearer.as_deref() {
        let presented = headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "));
        let authorized = presented
            .map(|presented| constant_time_eq(presented.as_bytes(), expected.as_bytes()))
            .unwrap_or(false);
        if !authorized {
            return typed_omission_response(
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "valid bearer token required",
            );
        }
    }
    next.run(request).await
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}

fn json_ok(payload: Value) -> Response {
    let body =
        serde_json::to_string(&payload).unwrap_or_else(|_| fallback_envelope("serialization_failed"));
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        body,
    )
        .into_response()
}

fn typed_omission_response(status: StatusCode, code: &str, detail: &str) -> Response {
    let envelope = OmissionEnvelope {
        error: OmissionErrorBody {
            code: code.to_string(),
            detail: detail.to_string(),
        },
    };
    let body = serde_json::to_string(&envelope).unwrap_or_else(|_| fallback_envelope(code));
    (
        status,
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        body,
    )
        .into_response()
}

fn fallback_envelope(code: &str) -> String {
    format!("{{\"error\":{{\"code\":\"{code}\",\"detail\":\"response serialization failed\"}}}}")
}

fn error_http_status(code: &str) -> StatusCode {
    match code {
        "workspace_not_open" => StatusCode::NOT_FOUND,
        "policy_unknown" => StatusCode::BAD_REQUEST,
        "epoch_not_monotonic" | "mutation_boundary" | "no_sealed_epoch" | "fence_not_cleared" => {
            StatusCode::CONFLICT
        }
        _ => StatusCode::SERVICE_UNAVAILABLE,
    }
}

fn respond(result: Result<Value, LiveDiagnosticsServiceError>) -> Response {
    match result {
        Ok(payload) => json_ok(payload),
        Err(error) => typed_omission_response(
            error_http_status(error.code()),
            error.code(),
            &error.to_string(),
        ),
    }
}

fn parse_body<T: serde::de::DeserializeOwned>(body: &str) -> Result<T, Response> {
    serde_json::from_str(body).map_err(|error| {
        typed_omission_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            &format!("request body does not match the diagnostics contract: {error}"),
        )
    })
}

async fn get_capabilities(State(state): State<DiagnosticsRouteState>) -> Response {
    json_ok(lock_service(&state.service).capabilities())
}

async fn get_status(State(state): State<DiagnosticsRouteState>) -> Response {
    json_ok(lock_service(&state.service).status())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkspaceScopeQuery {
    repo_id: String,
    worktree_id: String,
}

async fn get_workspace_status(
    State(state): State<DiagnosticsRouteState>,
    Query(query): Query<WorkspaceScopeQuery>,
) -> Response {
    let service = lock_service(&state.service);
    respond(service.workspace_status(&query.repo_id, &query.worktree_id))
}

async fn post_workspace_open(
    State(state): State<DiagnosticsRouteState>,
    body: String,
) -> Response {
    let request: WorkspaceRequest = match parse_body(&body) {
        Ok(request) => request,
        Err(response) => return response,
    };
    let mut service = lock_service(&state.service);
    respond(service.workspace_open(&request.repo_id, &request.worktree_id))
}

async fn post_workspace_close(
    State(state): State<DiagnosticsRouteState>,
    body: String,
) -> Response {
    let request: WorkspaceRequest = match parse_body(&body) {
        Ok(request) => request,
        Err(response) => return response,
    };
    let mut service = lock_service(&state.service);
    respond(service.workspace_close(&request.repo_id, &request.worktree_id))
}

async fn post_mutation_begin(
    State(state): State<DiagnosticsRouteState>,
    body: String,
) -> Response {
    let request: WorkspaceRequest = match parse_body(&body) {
        Ok(request) => request,
        Err(response) => return response,
    };
    let mut service = lock_service(&state.service);
    respond(service.mutation_begin(&request.repo_id, &request.worktree_id))
}

async fn post_mutation_seal(
    State(state): State<DiagnosticsRouteState>,
    body: String,
) -> Response {
    let request: MutationEpochRequest = match parse_body(&body) {
        Ok(request) => request,
        Err(response) => return response,
    };
    let mut service = lock_service(&state.service);
    respond(service.mutation_seal(
        &request.repo_id,
        &request.worktree_id,
        request.epoch,
    ))
}

async fn post_mutation_register_observed(
    State(state): State<DiagnosticsRouteState>,
    body: String,
) -> Response {
    let request: MutationEpochRequest = match parse_body(&body) {
        Ok(request) => request,
        Err(response) => return response,
    };
    let mut service = lock_service(&state.service);
    respond(service.mutation_register_observed(
        &request.repo_id,
        &request.worktree_id,
        request.epoch,
    ))
}

async fn post_reconcile(State(state): State<DiagnosticsRouteState>, body: String) -> Response {
    let request: ReconcileRequest = match parse_body(&body) {
        Ok(request) => request,
        Err(response) => return response,
    };
    let mut service = lock_service(&state.service);
    match service.workspace_reconcile(
        &request.repo_id,
        &request.worktree_id,
        &request.manifest_digest,
        &request.hashes,
    ) {
        Ok(label) => json_ok(json!({
            "schemaVersion": DIAGNOSTICS_SERVICE_SCHEMA_VERSION,
            "repoId": request.repo_id,
            "worktreeId": request.worktree_id,
            "classification": label,
        })),
        Err(error) => respond(Err(error)),
    }
}

async fn post_snapshot_await(
    State(state): State<DiagnosticsRouteState>,
    body: String,
) -> Response {
    let request: SnapshotAwaitRequest = match parse_body(&body) {
        Ok(request) => request,
        Err(response) => return response,
    };
    let mut service = lock_service(&state.service);
    match service.snapshot_await(&request) {
        Ok(decision) => json_ok(json!({
            "fenceCleared": matches!(decision.outcome, GateOutcome::CleanExact),
            "decision": serde_json::to_value(&decision).unwrap_or(Value::Null),
        })),
        Err(error) => respond(Err(error)),
    }
}

async fn post_fence_evaluate(body: String) -> Response {
    let request: FenceEvaluateRequest = match parse_body(&body) {
        Ok(request) => request,
        Err(response) => return response,
    };
    let decision = DiagnosticsService::evaluate_fence(
        &request.snapshot,
        &request.expected_epoch,
        &request.policy,
    );
    json_ok(serde_json::to_value(&decision).unwrap_or(Value::Null))
}

async fn post_baseline_capture(
    State(state): State<DiagnosticsRouteState>,
    body: String,
) -> Response {
    let request: BaselineRequest = match parse_body(&body) {
        Ok(request) => request,
        Err(response) => return response,
    };
    let mut service = lock_service(&state.service);
    respond(service.baseline_capture(
        &request.repo_id,
        &request.worktree_id,
        &request.name,
    ))
}

async fn post_baseline_update(
    State(state): State<DiagnosticsRouteState>,
    body: String,
) -> Response {
    let request: BaselineRequest = match parse_body(&body) {
        Ok(request) => request,
        Err(response) => return response,
    };
    let mut service = lock_service(&state.service);
    respond(service.baseline_update(
        &request.repo_id,
        &request.worktree_id,
        &request.name,
    ))
}

async fn post_provider_restart(
    State(state): State<DiagnosticsRouteState>,
    body: String,
) -> Response {
    let request: ProviderRestartRequest = match parse_body(&body) {
        Ok(request) => request,
        Err(response) => return response,
    };
    let mut service = lock_service(&state.service);
    respond(service.provider_restart(&request.key_digest))
}

/// Static support description served to offline consumers such as the CLI
/// `membrane diagnostics capabilities` path. Contains no live state.
pub fn static_capabilities() -> Value {
    json!({
        "schemaVersion": DIAGNOSTICS_SERVICE_SCHEMA_VERSION,
        "surface": "membrane-live-diagnostics",
        "hostModes": ["transactional", "observed_hook", "reconciliation_only"],
        "policyProfiles": [DEFAULT_POLICY_PROFILE_NAME],
        "enforcementPath": "snapshot.await + fence.evaluate",
        "eventsClearFence": false,
        "audit": {
            "schemaVersion": DIAGNOSTICS_AUDIT_SCHEMA_VERSION,
            "relativePath": AUDIT_RELATIVE_PATH,
        },
        "endpoints": [
            "GET /diagnostics/capabilities",
            "GET /diagnostics/status",
            "POST /diagnostics/workspace/open",
            "POST /diagnostics/workspace/close",
            "GET /diagnostics/workspace/status",
            "POST /diagnostics/reconcile",
            "POST /diagnostics/mutation/begin",
            "POST /diagnostics/mutation/seal",
            "POST /diagnostics/mutation/registerObserved",
            "POST /diagnostics/snapshot/await",
            "POST /diagnostics/fence/evaluate",
            "POST /diagnostics/baseline/capture",
            "POST /diagnostics/baseline/update",
            "POST /diagnostics/provider/restart",
        ],
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use membrane_protocol::diagnostics::{
        DIAGNOSTIC_EVIDENCE_SNAPSHOT_SCHEMA_VERSION, WORKSPACE_EPOCH_SCHEMA_VERSION,
    };

    fn service_at(dir: &tempfile::TempDir) -> DiagnosticsService {
        DiagnosticsService::with_data_root(dir.path().to_path_buf()).unwrap()
    }

    fn audit_len(dir: &tempfile::TempDir) -> u64 {
        std::fs::metadata(dir.path().join(AUDIT_RELATIVE_PATH))
            .map(|metadata| metadata.len())
            .unwrap_or(0)
    }

    fn audit_kinds(dir: &tempfile::TempDir) -> Vec<String> {
        std::fs::read_to_string(dir.path().join(AUDIT_RELATIVE_PATH))
            .unwrap_or_default()
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                serde_json::from_str::<Value>(line)
                    .expect("audit sink writes one JSON object per line")["kind"]
                    .as_str()
                    .expect("audit kind is a string")
                    .to_string()
            })
            .collect()
    }

    fn test_epoch(n: u64) -> WorkspaceEpochV1 {
        let mut epoch = WorkspaceEpochV1::default();
        epoch.schema_version = WORKSPACE_EPOCH_SCHEMA_VERSION.to_string();
        epoch.repo_id = "repo-1".to_string();
        epoch.worktree_id = "wt-1".to_string();
        epoch.epoch = n;
        epoch.parent_epoch = if n > 1 { Some(n - 1) } else { None };
        epoch.source_manifest_digest = format!("manifest-{n}");
        epoch.changed_file_hashes = vec![ChangedFileHashV1 {
            path: "src/main.ts".to_string(),
            hash: format!("hash-{n}"),
        }];
        epoch.project_config_digest = "sha256:test-config".to_string();
        epoch.toolchain_digest = "sha256:test-toolchain".to_string();
        epoch.sandbox_policy_digest = "sha256:test-sandbox".to_string();
        epoch.origin = WorkspaceEpochOrigin::Transactional;
        epoch
    }

    fn snapshot_request(required: &[CapabilityVocabulary]) -> SnapshotAwaitRequest {
        SnapshotAwaitRequest {
            repo_id: "repo-1".to_string(),
            worktree_id: "wt-1".to_string(),
            policy_profile_name: DEFAULT_POLICY_PROFILE_NAME.to_string(),
            required_capabilities: required.to_vec(),
            max_cost: Some(CostClass::Instant),
            deadline_ms: Some(5_000),
        }
    }

    #[test]
    fn session_lifecycle_reconcile_and_fence_transitions() {
        let dir = tempfile::tempdir().unwrap();
        let mut service = service_at(&dir);

        // Unknown workspaces are typed errors before anything else.
        assert_eq!(
            service.workspace_status("repo-1", "wt-1").unwrap_err().code(),
            "workspace_not_open"
        );
        assert!(service
            .snapshot_await(&snapshot_request(&[CapabilityVocabulary::Syntax]))
            .is_err());

        service.workspace_open("repo-1", "wt-1").unwrap();

        // No sealed epoch yet: acquisition is a typed no_sealed_epoch.
        assert_eq!(
            service
                .snapshot_await(&snapshot_request(&[]))
                .unwrap_err()
                .code(),
            "no_sealed_epoch"
        );

        // Seal requires an open mutation batch first.
        assert_eq!(
            service
                .mutation_seal("repo-1", "wt-1", test_epoch(1))
                .unwrap_err()
                .code(),
            "mutation_boundary"
        );

        service.mutation_begin("repo-1", "wt-1").unwrap();

        // An epoch naming another workspace never seals into this session.
        let mut foreign = test_epoch(1);
        foreign.worktree_id = "wt-other".to_string();
        assert_eq!(
            service
                .mutation_seal("repo-1", "wt-1", foreign)
                .unwrap_err()
                .code(),
            "mutation_boundary"
        );

        service.mutation_seal("repo-1", "wt-1", test_epoch(1)).unwrap();
        assert_eq!(
            service.workspace_status("repo-1", "wt-1").unwrap()["latestSealedEpoch"],
            json!(1)
        );

        // With no providers registered, a required capability cannot be
        // covered exactly: unknown_incomplete, fence not cleared.
        let decision = service
            .snapshot_await(&snapshot_request(&[CapabilityVocabulary::Syntax]))
            .unwrap();
        assert_eq!(decision.outcome, GateOutcome::UnknownIncomplete);
        assert_eq!(
            service.workspace_status("repo-1", "wt-1").unwrap()["fenceCleared"],
            json!(false)
        );

        // Empty requirements with no blockers clear the fence exactly, and
        // reconciliation then confirms the same bytes are still current.
        service
            .snapshot_await(&snapshot_request(&[]))
            .unwrap();
        let cleared = service
            .workspace_reconcile(
                "repo-1",
                "wt-1",
                "manifest-1",
                &test_epoch(1).changed_file_hashes,
            )
            .unwrap();
        assert_eq!(cleared, "cleared");

        // External-write drift classifies unknown_conflict and invalidates
        // the clearance.
        let conflicted = service
            .workspace_reconcile("repo-1", "wt-1", "externally-modified", &[])
            .unwrap();
        assert_eq!(conflicted, "unknown_conflict");
        assert_eq!(
            service.workspace_status("repo-1", "wt-1").unwrap()["fenceCleared"],
            json!(false)
        );

        // A newer observed epoch supersedes older bytes; monotonicity and the
        // parent chain stay enforced.
        service
            .mutation_register_observed("repo-1", "wt-1", test_epoch(2))
            .unwrap();
        assert!(matches!(
            service.mutation_register_observed("repo-1", "wt-1", test_epoch(2)),
            Err(LiveDiagnosticsServiceError::Supervisor(
                LiveDiagnosticsError::EpochNotMonotonic(_)
            ))
        ));
        service
            .snapshot_await(&snapshot_request(&[]))
            .unwrap();
        assert_eq!(
            service
                .workspace_reconcile(
                    "repo-1",
                    "wt-1",
                    "manifest-2",
                    &test_epoch(2).changed_file_hashes,
                )
                .unwrap(),
            "cleared"
        );

        // Drift again invalidates clearance, so baselines refuse until the
        // fence re-clears.
        assert_eq!(
            service
                .workspace_reconcile("repo-1", "wt-1", "externally-modified", &[])
                .unwrap(),
            "unknown_conflict"
        );
        assert_eq!(
            service
                .baseline_capture("repo-1", "wt-1", "before-tests")
                .unwrap_err()
                .code(),
            "fence_not_cleared"
        );
        service
            .snapshot_await(&snapshot_request(&[]))
            .unwrap();
        let baseline = service
            .baseline_capture("repo-1", "wt-1", "before-tests")
            .unwrap();
        assert!(baseline.get("decisionRef").is_some());
        assert_eq!(
            baseline["manifestDigest"],
            json!("manifest-2")
        );
        let updated = service
            .baseline_update("repo-1", "wt-1", "before-tests")
            .unwrap();
        assert_eq!(updated["action"], json!("update"));

        // Closing removes the session; further queries fail closed.
        service.workspace_close("repo-1", "wt-1").unwrap();
        assert_eq!(
            service.workspace_status("repo-1", "wt-1").unwrap_err().code(),
            "workspace_not_open"
        );

        // The audit recorded every persisted class of state (design §14).
        let kinds = audit_kinds(&dir);
        for expected in ["epoch_sealed", "gate_decision", "reconcile", "baseline"] {
            assert!(
                kinds.iter().any(|kind| kind == expected),
                "missing audit kind {expected}: {kinds:?}"
            );
        }
    }

    #[test]
    fn policy_resolution_is_deterministic_and_capability_sensitive() {
        let dir = tempfile::tempdir().unwrap();
        let service = service_at(&dir);

        let with_syntax =
            service.resolve_policy(DEFAULT_POLICY_PROFILE_NAME, &[CapabilityVocabulary::Syntax]);
        let again =
            service.resolve_policy(DEFAULT_POLICY_PROFILE_NAME, &[CapabilityVocabulary::Syntax]);
        assert_eq!(with_syntax.unwrap().policy_digest, again.unwrap().policy_digest);

        let without_requirements =
            service.resolve_policy(DEFAULT_POLICY_PROFILE_NAME, &[]);
        assert_ne!(
            service
                .resolve_policy(DEFAULT_POLICY_PROFILE_NAME, &[CapabilityVocabulary::Syntax])
                .unwrap()
                .policy_digest,
            without_requirements.unwrap().policy_digest
        );

        assert_eq!(
            service
                .resolve_policy("no-such-profile", &[])
                .unwrap_err()
                .code(),
            "policy_unknown"
        );
    }

    #[test]
    fn provider_restart_unknown_digest_is_typed_provider_error() {
        let dir = tempfile::tempdir().unwrap();
        let mut service = service_at(&dir);
        let error = service.provider_restart("deadbeef").unwrap_err();
        assert_eq!(error.code(), "provider_error");
    }

    #[test]
    fn fence_evaluate_is_pure_and_leaves_service_state_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let mut service = service_at(&dir);
        service.workspace_open("repo-1", "wt-1").unwrap();
        service.mutation_begin("repo-1", "wt-1").unwrap();
        service.mutation_seal("repo-1", "wt-1", test_epoch(1)).unwrap();

        let epoch = test_epoch(1);
        let snapshot = DiagnosticEvidenceSnapshotV1 {
            schema_version: DIAGNOSTIC_EVIDENCE_SNAPSHOT_SCHEMA_VERSION.to_string(),
            snapshot_id: "snap-pure".to_string(),
            repo_id: epoch.repo_id.clone(),
            worktree_id: epoch.worktree_id.clone(),
            blueprint_freshness: membrane_protocol::diagnostics::BlueprintFreshness::Current,
            workspace_epoch: epoch.clone(),
            request_max_cost: CostClass::Instant,
            produced_at_ms: 1_000,
            ..Default::default()
        };
        let policy = GatePolicyProfileV1 {
            profile_name: DEFAULT_POLICY_PROFILE_NAME.to_string(),
            policy_version: "v1".to_string(),
            policy_digest: "sha256:fixed".to_string(),
            blocking_codes: Vec::new(),
            required_capabilities: vec![CapabilityVocabulary::Syntax],
        };

        let status_before = service.status().to_string();
        let audit_before = audit_len(&dir);

        let first = DiagnosticsService::evaluate_fence(&snapshot, &epoch, &policy);
        let second = DiagnosticsService::evaluate_fence(&snapshot, &epoch, &policy);
        assert_eq!(first, second);
        assert_eq!(first.outcome, GateOutcome::UnknownIncomplete);

        assert_eq!(status_before, service.status().to_string());
        assert_eq!(audit_before, audit_len(&dir));
    }
}
