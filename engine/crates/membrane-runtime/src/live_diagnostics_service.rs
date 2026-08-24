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
    derive_required_capabilities, AbsoluteDeadline, BlueprintLaneInput, CapabilityKind,
    DiagnosticsProvider, DiagnosticsSession, DiagnosticsSupervisor, LiveDiagnosticsConfig,
    LiveDiagnosticsError, ProviderCapabilities, ReconcileClassification, SideEffectClass,
    WorkspaceEngineKey, LIVE_DIAGNOSTICS_SCHEMA_VERSION,
};
use crate::providers::blueprint_findings::{
    BlueprintFindingsClient, BlueprintFindingsError, DaemonFindingsClient,
};
use crate::providers::identity::{
    binary_digest, project_config_digest, sandbox_policy_digest, toolchain_digest,
};
use crate::providers::{rust_analyzer_provider, typescript_provider};
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
    #[error("workspace {repo_id}/{worktree_id} already bound to {existing} but requested {requested}")]
    WorkspaceProjectRootConflict {
        repo_id: String,
        worktree_id: String,
        existing: String,
        requested: String,
    },
    #[error("project root cannot be canonicalized: {0}")]
    ProjectRootInvalid(String),
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
            Self::WorkspaceProjectRootConflict { .. } => "workspace_project_root_conflict",
            Self::ProjectRootInvalid(_) => "project_root_invalid",
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

/// Manifest digest of a workspace epoch, if one is present.
fn epoch_manifest_of(epoch: &Option<WorkspaceEpochV1>) -> Option<String> {
    epoch
        .as_ref()
        .map(|epoch| epoch.source_manifest_digest.clone())
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
/// transactional-mutation flag and the exact addressed project root. The
/// canonical root is bound at `workspace.open` (design §3
/// `WorkspaceEngineKey`) so two distinct worktrees can never share one engine
/// lane merely because they share a process current directory.
struct SessionEntry {
    session: DiagnosticsSession,
    open_mutation: bool,
    /// Canonical absolute root this session was opened against.
    project_root: PathBuf,
    /// Whether production providers have been registered for this root.
    providers_registered: bool,
}

impl SessionEntry {
    fn new(project_root: PathBuf) -> Self {
        Self {
            session: DiagnosticsSession::new(),
            open_mutation: false,
            project_root,
            providers_registered: false,
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
/// seeded planner policy map, named baselines, the Blueprint findings client,
/// and the audit sink.
pub struct DiagnosticsService {
    supervisor: DiagnosticsSupervisor,
    sessions: HashMap<(String, String), SessionEntry>,
    policies: HashMap<String, GatePolicyProfileV1>,
    baselines: HashMap<(String, String), HashMap<String, NamedBaseline>>,
    /// The one production seam to Blueprint's public resident findings
    /// service (design §7.1 item 6). `None` degrades every acquisition to a
    /// typed `blueprint_unavailable` — never fabricated evidence.
    blueprint_client: Option<Box<dyn BlueprintFindingsClient>>,
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
            blueprint_client: None,
            audit: AuditSink::under_data_root(&data_root),
        })
    }

    /// Install the production Blueprint findings client (daemon IPC). Called
    /// by `production_service`; tests inject fakes or leave `None`.
    pub fn with_blueprint_client(
        mut self,
        client: Box<dyn BlueprintFindingsClient>,
    ) -> Self {
        self.blueprint_client = Some(client);
        self
    }

    /// Registration seam for qualified engine adapters, delegating to the
    /// supervisor. Factories may be invoked lazily on every acquisition and
    /// restart, so they are `Fn` rather than `FnOnce`.
    pub fn register_provider(
        &mut self,
        key: WorkspaceEngineKey,
        capabilities: ProviderCapabilities,
        factory: Box<dyn Fn() -> Box<dyn DiagnosticsProvider> + Send>,
    ) {
        self.supervisor.register(key, capabilities, factory);
    }

    // -- workspace lifecycle ------------------------------------------------

    /// Open (or idempotently reopen) the diagnostics workspace session for
    /// one repo/worktree pair, binding the session to an exact canonical
    /// project root (design §3 `WorkspaceEngineKey`). An explicit
    /// `project_root` is canonicalized and stored; when absent the current
    /// directory is canonicalized and recorded as the bound root so identity
    /// is always explicit rather than ambient.
    pub fn workspace_open(
        &mut self,
        repo_id: &str,
        worktree_id: &str,
        project_root: Option<&str>,
    ) -> Result<Value, LiveDiagnosticsServiceError> {
        let key = session_key(repo_id, worktree_id);
        if let Some(entry) = self.sessions.get(&key) {
            // Already bound: enforce exact identity.
            if let Some(requested_str) = project_root {
                let requested = PathBuf::from(requested_str);
                let canonical = std::fs::canonicalize(&requested).map_err(|e| {
                    LiveDiagnosticsServiceError::ProjectRootInvalid(format!(
                        "cannot canonicalize projectRoot {}: {}",
                        requested_str, e
                    ))
                })?;
                if canonical != entry.project_root {
                    return Err(LiveDiagnosticsServiceError::WorkspaceProjectRootConflict {
                        repo_id: repo_id.to_string(),
                        worktree_id: worktree_id.to_string(),
                        existing: entry.project_root.to_string_lossy().into_owned(),
                        requested: canonical.to_string_lossy().into_owned(),
                    });
                }
            }
            let mut status = self.workspace_status(repo_id, worktree_id)?;
            if let Some(object) = status.as_object_mut() {
                object.insert("created".to_string(), Value::Bool(false));
                object.insert(
                    "projectRoot".to_string(),
                    Value::String(
                        self.sessions
                            .get(&session_key(repo_id, worktree_id))
                            .map(|entry| entry.project_root.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                    ),
                );
            }
            return Ok(status);
        }
        // First open: bind one canonical absolute projectRoot, fail closed if
        // it cannot be canonicalized. Do NOT fall back to an uncanonicalized path.
        let requested = project_root.map(PathBuf::from).unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        });
        let canonical = std::fs::canonicalize(&requested).map_err(|e| {
            LiveDiagnosticsServiceError::ProjectRootInvalid(format!(
                "cannot canonicalize projectRoot {}: {}",
                requested.display(),
                e
            ))
        })?;
        self.sessions.insert(key, SessionEntry::new(canonical));
        let mut status = self.workspace_status(repo_id, worktree_id)?;
        if let Some(object) = status.as_object_mut() {
            object.insert("created".to_string(), Value::Bool(true));
            object.insert(
                "projectRoot".to_string(),
                Value::String(
                    self.sessions
                        .get(&session_key(repo_id, worktree_id))
                        .map(|entry| entry.project_root.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                ),
            );
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
    /// epoch to the audit and invalidates prior fence clearance. Obvious
    /// path escape from the bound root is rejected fail-closed.
    pub fn mutation_seal(
        &mut self,
        repo_id: &str,
        worktree_id: &str,
        epoch: WorkspaceEpochV1,
    ) -> Result<Value, LiveDiagnosticsServiceError> {
        validate_epoch_identity(repo_id, worktree_id, &epoch)?;
        let entry = self
            .sessions
            .get(&session_key(repo_id, worktree_id))
            .ok_or_else(|| workspace_not_open(repo_id, worktree_id))?;
        validate_epoch_paths_within_root(&epoch, &entry.project_root)?;
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
            .get(&session_key(repo_id, worktree_id))
            .ok_or_else(|| workspace_not_open(repo_id, worktree_id))?;
        validate_epoch_paths_within_root(&epoch, &entry.project_root)?;
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
            .get(&session_key(repo_id, worktree_id))
            .ok_or_else(|| workspace_not_open(repo_id, worktree_id))?;
        for h in hashes {
            if is_obvious_path_escape(&h.path, &entry.project_root) {
                return Err(LiveDiagnosticsServiceError::MutationBoundary(format!(
                    "path escapes bound projectRoot {}: {}",
                    entry.project_root.display(),
                    h.path
                )));
            }
        }
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

    /// Resolve the effective planner policy for one acquisition. Precedence
    /// (design §8): request-supplied capabilities first, then the stored
    /// profile's requirements, then derivation from the touched scope —
    /// never a blanket default capability. A JS/TS mutation therefore
    /// demands repository module resolution, import/export binding, and type
    /// semantics on top of syntax; a Rust mutation demands name/type/compiler
    /// project semantics. The digest is `sha256:` over the sorted-key
    /// canonical JSON serialization of the effective profile with
    /// `policyDigest` left empty during hashing, so it is reproducible from
    /// the profile alone.
    fn resolve_policy(
        &self,
        profile_name: &str,
        required_capabilities: &[CapabilityVocabulary],
        touched_paths: &[String],
    ) -> Result<GatePolicyProfileV1, LiveDiagnosticsServiceError> {
        let stored = self
            .policies
            .get(profile_name)
            .ok_or_else(|| LiveDiagnosticsServiceError::PolicyUnknown {
                profile_name: profile_name.to_string(),
            })?;
        let mut effective = stored.clone();
        if !required_capabilities.is_empty() {
            effective.required_capabilities = required_capabilities.to_vec();
        } else if stored.required_capabilities.is_empty() {
            effective.required_capabilities = derive_required_capabilities(touched_paths);
        }
        effective.policy_digest = String::new();
        effective.policy_digest = effective.canonical_digest();
        Ok(effective)
    }

    fn ensure_workspace_providers(&mut self, repo_id: &str, worktree_id: &str) {
        let Some(entry) = self.sessions.get(&session_key(repo_id, worktree_id)) else {
            return;
        };
        if entry.providers_registered {
            return;
        }
        let root = entry.project_root.clone();
        self.register_production_providers_for(repo_id, worktree_id, &root);
        if let Some(entry) = self.sessions.get_mut(&session_key(repo_id, worktree_id)) {
            entry.providers_registered = true;
        }
    }

    /// Register D1 providers bound to the session's exact canonical root with
    /// real identity digests (design §3): binary/toolchain digests come from
    /// the resolved engine binary's bytes and install directory, the config
    /// digest from the project files the engine actually reads, and the
    /// sandbox digest from the effective containment policy inputs. A binary
    /// that cannot be resolved is simply not registered — acquisition then
    /// reports its capability as unsatisfied instead of inventing identity.
    fn register_production_providers_for(
        &mut self,
        repo_id: &str,
        worktree_id: &str,
        project_root: &Path,
    ) {
        let canonical_root = std::fs::canonicalize(project_root)
            .unwrap_or_else(|_| project_root.to_path_buf())
            .to_string_lossy()
            .into_owned();
        let search_path = crate::providers::default_search_path();
        let sandbox_digest = sandbox_policy_digest(&search_path);

        if let Some((_engine_name, ts_binary)) =
            typescript_provider::resolve_engine(&search_path)
        {
            let identity = typescript_provider::identity_inputs(project_root, &ts_binary);
            if let (
                Some(binary),
                Some(toolchain),
                Some(config),
            ) = (identity.binary, identity.toolchain, identity.config)
            {
                let key = WorkspaceEngineKey {
                    repo_id: repo_id.to_string(),
                    worktree_id: worktree_id.to_string(),
                    canonical_worktree_root: canonical_root.clone(),
                    project_root: project_root.to_string_lossy().to_string(),
                    engine_id: typescript_provider::PROVIDER_ID.to_string(),
                    engine_version: typescript_provider::ADAPTER_VERSION.to_string(),
                    binary_digest: binary,
                    toolchain_digest: toolchain,
                    project_config_digest: config,
                    sandbox_policy_digest: sandbox_digest.clone(),
                };
                let ts_root = project_root.to_path_buf();
                let ts_search = search_path.clone();
                self.register_provider(
                    key,
                    typescript_provider::qualified_capabilities(),
                    Box::new(move || {
                        Box::new(typescript_provider::TypeScriptProvider::with_search_path(
                            ts_root.clone(),
                            ts_search.clone(),
                        )) as Box<dyn DiagnosticsProvider>
                    }),
                );
            }
        }

        if let Some(ra_binary) = rust_analyzer_provider::resolve_engine(&search_path) {
            let config_files: Vec<&str> = vec!["Cargo.toml", "rust-toolchain.toml", "rust-toolchain"];
            let binary = binary_digest(&ra_binary);
            let toolchain = toolchain_digest(&ra_binary);
            let config = project_config_digest(project_root, &config_files);
            if let (Some(binary), Some(toolchain), Some(config)) = (binary, toolchain, config) {
                let key = WorkspaceEngineKey {
                    repo_id: repo_id.to_string(),
                    worktree_id: worktree_id.to_string(),
                    canonical_worktree_root: canonical_root,
                    project_root: project_root.to_string_lossy().to_string(),
                    engine_id: rust_analyzer_provider::PROVIDER_ID.to_string(),
                    engine_version: rust_analyzer_provider::ADAPTER_VERSION.to_string(),
                    binary_digest: binary,
                    toolchain_digest: toolchain,
                    project_config_digest: config,
                    sandbox_policy_digest: sandbox_digest,
                };
                let ra_root = project_root.to_path_buf();
                let ra_search = search_path.clone();
                self.register_provider(
                    key,
                    rust_analyzer_provider::qualified_capabilities(),
                    Box::new(move || {
                        Box::new(rust_analyzer_provider::RustAnalyzerProvider::with_search_path(
                            ra_root.clone(),
                            ra_search.clone(),
                        )) as Box<dyn DiagnosticsProvider>
                    }),
                );
            }
        }
    }

    /// Production service constructor: installs the real Blueprint findings
    /// client (daemon IPC) and registers providers lazily per opened
    /// workspace against each workspace's exact bound root.
    pub fn production_service() -> Result<Self, LiveDiagnosticsServiceError> {
        Ok(Self::new()?.with_blueprint_client(Box::new(
            DaemonFindingsClient::from_environment(),
        )))
    }

    /// Fetch Blueprint D0a/D0b evidence through the public resident findings
    /// service (design §7.1 item 6). Any transport/protocol failure becomes a
    /// typed unavailable lane input — this is the only Blueprint path, and it
    /// never fabricates generation or freshness evidence.
    ///
    /// Exact-byte binding (§7.1 item 5): the caller's sealed
    /// `WorkspaceEpochV1` is the authority for byte identity. Every
    /// `perFileContentHashes` entry retained in the bundle is compared against
    /// `sealed.changed_file_hashes`. Only when every relevant touched file has
    /// a present and matching hash AND the bundle carries no
    /// coverage-affecting omission may an exact D0 lane be produced. Any hash
    /// mismatch, missing required hash, staleness, or coverage-affecting
    /// omission yields no exact lane and typed omissions instead — the
    /// affected obligation remains unsatisfied and cannot become `clean_exact`.
    fn blueprint_lane_input(
        &mut self,
        project_root: &Path,
        timeout_ms: u64,
        sealed: &WorkspaceEpochV1,
    ) -> BlueprintLaneInput {
        let Some(client) = &mut self.blueprint_client else {
            return BlueprintLaneInput::unavailable(
                "no blueprint findings client is configured".to_string(),
            );
        };
        match client.fetch(project_root, timeout_ms) {
            Ok(result) => {
                if !result.freshness_is_current() {
                    // Stale evidence is never silently recomputed as current:
                    // typed omission, freshness Stale, no exact lane.
                    return BlueprintLaneInput {
                        generation: None,
                        freshness:
                            membrane_protocol::diagnostics::BlueprintFreshness::Stale,
                        observations: Vec::new(),
                        lane: None,
                        delta: None,
                        omissions: vec![membrane_protocol::diagnostics::TypedOmission {
                            code: "stale_generation".to_string(),
                            detail: format!(
                                "blueprint working tree moved past sealed generation {}",
                                result.generation_id
                            ),
                        }],
                    };
                }
                // Coverage-affecting omissions prevent Complete exact coverage
                // for the affected scope (design §7). Treat any such omission as
                // blocking exact D0 for the sealed scope.
                let has_coverage_block = result.has_coverage_affecting_omissions();
                // Exact byte identity: compare retained per-file hashes against
                // sealed epoch's changed_file_hashes for the touched scope.
                let hash_issues = result.verify_hashes_against_epoch(sealed, None);
                let has_hash_block = !hash_issues.is_empty();
                let generation = result.generation_id.clone();
                let observations = result
                    .findings
                    .iter()
                    .map(|finding| finding.to_observation(&generation))
                    .collect();
                if has_coverage_block || has_hash_block {
                    // Fail closed: carry observations but produce no exact lane.
                    // Omissions explain why exact coverage is not proven; the
                    // obligation remains unsatisfied unless another qualified
                    // exact provider satisfies it.
                    let mut omissions = result.omissions;
                    omissions.extend(hash_issues);
                    return BlueprintLaneInput {
                        generation: Some(generation),
                        freshness: membrane_protocol::diagnostics::BlueprintFreshness::Current,
                        observations,
                        lane: None,
                        delta: None,
                        omissions,
                    };
                }
                BlueprintLaneInput::current(generation, observations, 0, result.omissions)
            }
            Err(error @ BlueprintFindingsError::DeadlineExceeded) => {
                BlueprintLaneInput::unavailable(format!(
                    "blueprint findings service did not answer within {timeout_ms}ms: {error}"
                ))
            }
            Err(error) => BlueprintLaneInput::unavailable(format!(
                "blueprint findings service unavailable: {error}"
            )),
        }
    }

    /// Run every registered provider for the workspace within the request's
    /// cost ceiling and absolute deadline, assemble the exact-evidence
    /// snapshot seeded with real Blueprint D0 evidence, evaluate planner
    /// policy, and clear the fence only on `clean_exact`. Returns the gate
    /// decision.
    pub fn snapshot_await(
        &mut self,
        request: &SnapshotAwaitRequest,
    ) -> Result<DiagnosticGateDecisionV1, LiveDiagnosticsServiceError> {
        self.ensure_workspace_providers(&request.repo_id, &request.worktree_id);
        let deadline_ms = request
            .deadline_ms
            .unwrap_or(self.supervisor.config().default_deadline_ms);
        let deadline = AbsoluteDeadline::after(self.supervisor.now_monotonic_ms(), deadline_ms);
        // Short-lived borrow: pull the sealed epoch identity out of the
        // session before touching other service fields.
        let (sealed, project_root) = {
            let entry = self
                .sessions
                .get_mut(&session_key(&request.repo_id, &request.worktree_id))
                .ok_or_else(|| workspace_not_open(&request.repo_id, &request.worktree_id))?;
            let sealed = entry
                .session
                .latest_sealed()
                .cloned()
                .ok_or(LiveDiagnosticsServiceError::NoSealedEpoch)?;
            (sealed, entry.project_root.clone())
        };
        // Planner policy resolution derives obligations from the touched
        // scope when neither request nor profile supplies them (design §8).
        let touched_paths = sealed.changed_file_hashes.iter().map(|h| h.path.clone())
            .chain(sealed.changed_paths.iter().cloned())
            .collect::<Vec<_>>();
        let policy = self.resolve_policy(
            &request.policy_profile_name,
            &request.required_capabilities,
            &touched_paths,
        )?;
        let keys: Vec<WorkspaceEngineKey> = self
            .supervisor
            .registered_keys()
            .into_iter()
            .filter(|key| key.repo_id == request.repo_id && key.worktree_id == request.worktree_id)
            .cloned()
            .collect();
        let max_cost = request.max_cost.unwrap_or_default();
        // Real Blueprint D0a/D0b via the public resident findings service,
        // bound to the sealed epoch's exact bytes (fail-closed on hash mismatch
        // or coverage-affecting omissions).
        let mut blueprint =
            self.blueprint_lane_input(&project_root, deadline_ms.max(1_000), &sealed);
        if let BlueprintLaneInput {
            lane: Some(lane),
            ..
        } = &mut blueprint
        {
            lane.bound_workspace_epoch = sealed.epoch;
        }
        let entry = self
            .sessions
            .get_mut(&session_key(&request.repo_id, &request.worktree_id))
            .ok_or_else(|| workspace_not_open(&request.repo_id, &request.worktree_id))?;
        let decision = entry.session.acquire_snapshot(
            &mut self.supervisor,
            &keys,
            blueprint,
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

    /// Shared capture/update body: a named baseline records the currently
    /// cleared decision reference plus the manifest digest and changed-file
    /// hashes of the exact bytes that decision describes. An uncleared fence
    /// is a typed `fence_not_cleared` error (design §12).
    fn record_baseline(
        &mut self,
        repo_id: &str,
        worktree_id: &str,
        name: &str,
        action: &'static str,
    ) -> Result<Value, LiveDiagnosticsServiceError> {
        let entry = self
            .sessions
            .get_mut(&session_key(repo_id, worktree_id))
            .ok_or_else(|| workspace_not_open(repo_id, worktree_id))?;
        let Some(clearance) = entry.session.cleared_decision() else {
            return Err(LiveDiagnosticsServiceError::FenceNotCleared {
                repo_id: repo_id.to_string(),
                worktree_id: worktree_id.to_string(),
            });
        };
        let latest = entry.session.latest_sealed().cloned();
        let baseline = NamedBaseline {
            name: name.to_string(),
            decision_ref: clearance.snapshot_id.clone(),
            policy_digest: clearance.policy_digest.clone(),
            manifest_digest: epoch_manifest_of(&latest).unwrap_or_default(),
            epoch_number: latest.as_ref().map(|epoch| epoch.epoch).unwrap_or(0),
            captured_at_unix_ms: now_unix_ms(),
        };
        let payload = baseline.payload(repo_id, worktree_id, action);
        self.baselines
            .entry(session_key(repo_id, worktree_id))
            .or_default()
            .insert(name.to_string(), baseline);
        self.audit.record("baseline", payload);
        Ok(json!({
            "schemaVersion": DIAGNOSTICS_SERVICE_SCHEMA_VERSION,
            "repoId": repo_id,
            "worktreeId": worktree_id,
            "action": action,
            "name": name,
            "fenceCleared": true,
        }))
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
        let mut restarted = Vec::new();
        for key in &matched {
            if self.supervisor.shutdown_key(key) {
                restarted.push(key.clone());
            }
        }
        Ok(json!({
            "schemaVersion": DIAGNOSTICS_SERVICE_SCHEMA_VERSION,
            "keyDigest": key_digest,
            "matched": true,
            "semantics": "shutdown_key",
            "restarted": restarted
                .iter()
                .map(|key| json!({
                    "keyDigest": key.digest(),
                    "engineId": key.engine_id,
                }))
                .collect::<Vec<_>>(),
        }))
    }

    // -- snapshots ------------------------------------------------------------

    /// Return the latest evidence snapshot for the workspace if one exists.
    pub fn snapshot_get(
        &self,
        repo_id: &str,
        worktree_id: &str,
    ) -> Result<Value, LiveDiagnosticsServiceError> {
        let entry = self
            .sessions
            .get(&session_key(repo_id, worktree_id))
            .ok_or_else(|| workspace_not_open(repo_id, worktree_id))?;
        let snapshot = entry
            .session
            .latest_snapshot()
            .ok_or(LiveDiagnosticsServiceError::NoSealedEpoch)?;
        Ok(serde_json::to_value(snapshot).unwrap_or(Value::Null))
    }

    /// Explain the latest gate decision for the workspace: outcome, blocking
    /// issues, reason codes and the bound snapshot id.
    pub fn snapshot_explain(
        &self,
        repo_id: &str,
        worktree_id: &str,
    ) -> Result<Value, LiveDiagnosticsServiceError> {
        let entry = self
            .sessions
            .get(&session_key(repo_id, worktree_id))
            .ok_or_else(|| workspace_not_open(repo_id, worktree_id))?;
        let snapshot = entry
            .session
            .latest_snapshot()
            .ok_or(LiveDiagnosticsServiceError::NoSealedEpoch)?;
        let decision = entry
            .session
            .latest_decision()
            .ok_or(LiveDiagnosticsServiceError::NoSealedEpoch)?;
        Ok(json!({
            "schemaVersion": DIAGNOSTICS_SERVICE_SCHEMA_VERSION,
            "repoId": repo_id,
            "worktreeId": worktree_id,
            "snapshotId": snapshot.snapshot_id,
            "outcome": gate_outcome_label(decision.outcome),
            "blockingIssueIds": decision.blocking_issue_ids,
            "reasonCodes": decision.reason_codes,
            "omissions": decision.omissions,
            "coverageObligations": snapshot.coverage_obligations,
        }))
    }

    /// Return the aggregate delta between the latest snapshot issues and the
    /// stored baseline, if any.
    pub fn snapshot_delta(
        &self,
        repo_id: &str,
        worktree_id: &str,
    ) -> Result<Value, LiveDiagnosticsServiceError> {
        let entry = self
            .sessions
            .get(&session_key(repo_id, worktree_id))
            .ok_or_else(|| workspace_not_open(repo_id, worktree_id))?;
        let snapshot = entry
            .session
            .latest_snapshot()
            .ok_or(LiveDiagnosticsServiceError::NoSealedEpoch)?;
        Ok(json!({
            "schemaVersion": DIAGNOSTICS_SERVICE_SCHEMA_VERSION,
            "repoId": repo_id,
            "worktreeId": worktree_id,
            "snapshotId": snapshot.snapshot_id,
            "aggregateDelta": snapshot.aggregate_delta,
            "issues": snapshot.issues,
        }))
    }

    // -- provider list/status -------------------------------------------------

    /// List all registered providers with their digests and declared capabilities (§12 provider.list).
    pub fn provider_list(&self) -> Value {
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
                    "capabilities": caps.capabilities.iter().map(capability_kind_label).collect::<Vec<_>>(),
                    "sideEffectClass": side_effect_class_label(caps.side_effect_class),
                    "convergenceClass": convergence_class_label(caps.convergence_class),
                    "costClass": cost_class_label(caps.cost_class),
                }))
            })
            .collect::<Vec<_>>();
        json!({
            "schemaVersion": DIAGNOSTICS_SERVICE_SCHEMA_VERSION,
            "providers": providers,
        })
    }

    /// Status for a single provider by digest, or typed provider_error if unknown (§12 provider.status).
    pub fn provider_status(
        &self,
        key_digest: &str,
    ) -> Result<Value, LiveDiagnosticsServiceError> {
        let key = self
            .supervisor
            .registered_keys()
            .into_iter()
            .find(|key| key.digest() == key_digest)
            .cloned()
            .ok_or_else(|| LiveDiagnosticsServiceError::Provider(
                format!("no registered provider key matches digest {key_digest}")
            ))?;
        let caps = self.supervisor.registry_capabilities(&key)
            .ok_or_else(|| LiveDiagnosticsServiceError::Provider(
                format!("no capabilities for key digest {key_digest}")
            ))?;
        Ok(json!({
            "schemaVersion": DIAGNOSTICS_SERVICE_SCHEMA_VERSION,
            "keyDigest": key.digest(),
            "repoId": key.repo_id,
            "worktreeId": key.worktree_id,
            "engineId": key.engine_id,
            "engineVersion": key.engine_version,
            "providerId": caps.provider_id,
            "version": caps.version,
            "capabilities": caps.capabilities.iter().map(capability_kind_label).collect::<Vec<_>>(),
            "sideEffectClass": side_effect_class_label(caps.side_effect_class),
            "convergenceClass": convergence_class_label(caps.convergence_class),
            "costClass": cost_class_label(caps.cost_class),
        }))
    }

    /// Host fence query for enforcement: true if the session's fence is cleared (clean_exact).
    pub fn is_fence_cleared(&self, repo_id: &str, worktree_id: &str) -> bool {
        self.sessions
            .get(&session_key(repo_id, worktree_id))
            .map(|entry| entry.session.cleared_decision().is_some())
            .unwrap_or(false)
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

    /// Subscribe: returns a subscription id plus the current status snapshot.
    /// Streaming presentation events are telemetry-only and can never clear the
    /// fence (design §12). This long-poll placeholder stays truthful until a
    /// consumer needs real SSE; the id is derived from the current time so the
    /// caller can deduplicate. Hosts should treat the returned status as the
    /// immediate snapshot and not assume fence clearance from subscription alone.
    pub fn subscribe(&self) -> Value {
        let status = self.status();
        let subscription_id = format!("sub-{}", now_unix_ms());
        json!({
            "schemaVersion": DIAGNOSTICS_SERVICE_SCHEMA_VERSION,
            "subscriptionId": subscription_id,
            "status": status,
            "note": "presentation events are telemetry-only and never clear the fence; poll status or snapshot.await for enforcement",
        })
    }

    /// Host fence enforcement helper: returns whether the given workspace's
    /// fence is cleared. CodeRight, Claude Code and Codex hosts should call
    /// this before allowing tests/builds/releases when they have opted into
    /// fence enforcement. Returns false for unknown workspaces (fail-closed).
    pub fn fence_allows_build(&self, repo_id: &str, worktree_id: &str) -> bool {
        self.is_fence_cleared(repo_id, worktree_id)
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
            // Intentionally empty: requirements derive per request from the
            // touched scope (design §8). A blanket default capability would
            // under-check — a D1 provider could clear the fence without the
            // D0 repository obligations this system exists to enforce.
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

fn validate_epoch_paths_within_root(
    epoch: &WorkspaceEpochV1,
    project_root: &Path,
) -> Result<(), LiveDiagnosticsServiceError> {
    for path in epoch.changed_paths.iter().chain(
        epoch.changed_file_hashes.iter().map(|h| &h.path),
    ) {
        if is_obvious_path_escape(path, project_root) {
            return Err(LiveDiagnosticsServiceError::MutationBoundary(format!(
                "path escapes bound projectRoot {}: {}",
                project_root.display(),
                path
            )));
        }
    }
    Ok(())
}

fn is_obvious_path_escape(path: &str, _project_root: &Path) -> bool {
    // Reject absolute paths (they cannot be repo-relative) and any
    // segment containing `..` that would allow escaping the bound root.
    if path.starts_with('/') || path.starts_with('\\') {
        return true;
    }
    // Windows absolute like C:\ or C:/
    if path.len() >= 2 && path.chars().nth(1) == Some(':') {
        return true;
    }
    for segment in path.split(['/', '\\']) {
        if segment == ".." {
            return true;
        }
    }
    // Also reject paths that after join would still escape: this cheap
    // check covers the common cases without canonicalizing disk state.
    false
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

/// Identity of one addressed workspace session. `projectRoot` binds the
/// session to an exact canonical worktree/project root at open time (design
/// §3); when omitted the resident's canonicalized current directory is
/// recorded so identity is always explicit.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceRequest {
    pub repo_id: String,
    pub worktree_id: String,
    #[serde(default)]
    pub project_root: Option<String>,
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
    let service = DiagnosticsService::production_service().ok()?;
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
        .route("/diagnostics/snapshot/get", get(get_snapshot))
        .route("/diagnostics/snapshot/explain", get(get_snapshot_explain))
        .route("/diagnostics/snapshot/delta", get(get_snapshot_delta))
        .route("/diagnostics/fence/evaluate", post(post_fence_evaluate))
        .route("/diagnostics/baseline/capture", post(post_baseline_capture))
        .route("/diagnostics/baseline/update", post(post_baseline_update))
        .route("/diagnostics/provider/list", get(get_provider_list))
        .route("/diagnostics/provider/status", get(get_provider_status))
        .route("/diagnostics/provider/restart", post(post_provider_restart))
        .route("/diagnostics/subscribe", get(get_subscribe))
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
        "policy_unknown" | "project_root_invalid" => StatusCode::BAD_REQUEST,
        "workspace_project_root_conflict" => StatusCode::CONFLICT,
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
    respond(service.workspace_open(
        &request.repo_id,
        &request.worktree_id,
        request.project_root.as_deref(),
    ))
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

async fn get_snapshot(
    State(state): State<DiagnosticsRouteState>,
    Query(query): Query<WorkspaceScopeQuery>,
) -> Response {
    let service = lock_service(&state.service);
    respond(service.snapshot_get(&query.repo_id, &query.worktree_id))
}

async fn get_snapshot_explain(
    State(state): State<DiagnosticsRouteState>,
    Query(query): Query<WorkspaceScopeQuery>,
) -> Response {
    let service = lock_service(&state.service);
    respond(service.snapshot_explain(&query.repo_id, &query.worktree_id))
}

async fn get_snapshot_delta(
    State(state): State<DiagnosticsRouteState>,
    Query(query): Query<WorkspaceScopeQuery>,
) -> Response {
    let service = lock_service(&state.service);
    respond(service.snapshot_delta(&query.repo_id, &query.worktree_id))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProviderStatusQuery {
    key_digest: String,
}

async fn get_provider_list(State(state): State<DiagnosticsRouteState>) -> Response {
    json_ok(lock_service(&state.service).provider_list())
}

async fn get_provider_status(
    State(state): State<DiagnosticsRouteState>,
    Query(query): Query<ProviderStatusQuery>,
) -> Response {
    let service = lock_service(&state.service);
    respond(service.provider_status(&query.key_digest))
}

async fn get_subscribe(State(state): State<DiagnosticsRouteState>) -> Response {
    json_ok(lock_service(&state.service).subscribe())
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
            "GET /diagnostics/snapshot/get",
            "GET /diagnostics/snapshot/explain",
            "GET /diagnostics/snapshot/delta",
            "POST /diagnostics/fence/evaluate",
            "POST /diagnostics/baseline/capture",
            "POST /diagnostics/baseline/update",
            "GET /diagnostics/provider/list",
            "GET /diagnostics/provider/status",
            "POST /diagnostics/provider/restart",
            "GET /diagnostics/subscribe",
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
        GateOutcome, DIAGNOSTIC_EVIDENCE_SNAPSHOT_SCHEMA_VERSION, WORKSPACE_EPOCH_SCHEMA_VERSION,
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

        service.workspace_open("repo-1", "wt-1", None).unwrap();

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

        // Empty requirements now resolve to default Syntax (seeded policy) and
        // carry an Unsatisfied coverage obligation, so with no exact lane they
        // cannot clear the fence (empty-ensemble fix). Reconciliation reports
        // unknown_conflict when no cleared decision exists.
        let empty_decision = service
            .snapshot_await(&snapshot_request(&[]))
            .unwrap();
        assert_eq!(empty_decision.outcome, GateOutcome::UnknownIncomplete);
        assert_eq!(
            service.workspace_status("repo-1", "wt-1").unwrap()["fenceCleared"],
            json!(false)
        );
        let not_cleared = service
            .workspace_reconcile(
                "repo-1",
                "wt-1",
                "manifest-1",
                &test_epoch(1).changed_file_hashes,
            )
            .unwrap();
        assert_eq!(not_cleared, "unknown_conflict");
        assert_eq!(
            service.workspace_status("repo-1", "wt-1").unwrap()["fenceCleared"],
            json!(false)
        );

        // External-write drift still classifies unknown_conflict.
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
        let empty_decision2 = service
            .snapshot_await(&snapshot_request(&[]))
            .unwrap();
        assert_eq!(empty_decision2.outcome, GateOutcome::UnknownIncomplete);
        assert_eq!(
            service
                .workspace_reconcile(
                    "repo-1",
                    "wt-1",
                    "manifest-2",
                    &test_epoch(2).changed_file_hashes,
                )
                .unwrap(),
            "unknown_conflict"
        );

        // Drift again keeps fence uncleared, so baselines refuse.
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
        let empty_decision3 = service
            .snapshot_await(&snapshot_request(&[]))
            .unwrap();
        assert_eq!(empty_decision3.outcome, GateOutcome::UnknownIncomplete);
        assert_eq!(
            service
                .baseline_capture("repo-1", "wt-1", "before-tests")
                .unwrap_err()
                .code(),
            "fence_not_cleared"
        );

        // Closing removes the session; further queries fail closed.
        service.workspace_close("repo-1", "wt-1").unwrap();
        assert_eq!(
            service.workspace_status("repo-1", "wt-1").unwrap_err().code(),
            "workspace_not_open"
        );

        // The audit recorded persisted state (design §14). Baseline is not
        // expected when the fence never cleared (empty-ensemble fix).
        let kinds = audit_kinds(&dir);
        for expected in ["epoch_sealed", "gate_decision", "reconcile"] {
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
        let ts_scope = vec!["src/app.ts".to_string()];

        let with_syntax = service.resolve_policy(
            DEFAULT_POLICY_PROFILE_NAME,
            &[CapabilityVocabulary::Syntax],
            &[],
        );
        let again = service.resolve_policy(
            DEFAULT_POLICY_PROFILE_NAME,
            &[CapabilityVocabulary::Syntax],
            &[],
        );
        assert_eq!(with_syntax.unwrap().policy_digest, again.unwrap().policy_digest);

        // Empty request + empty stored profile derives from the touched scope
        // (design §8): a TS mutation requires the full D0+D1 obligation set,
        // never a blanket single default.
        let derived =
            service
                .resolve_policy(DEFAULT_POLICY_PROFILE_NAME, &[], &ts_scope)
                .unwrap();
        assert_eq!(
            derived.required_capabilities,
            vec![
                CapabilityVocabulary::Syntax,
                CapabilityVocabulary::RepositoryModuleResolution,
                CapabilityVocabulary::ImportExportBinding,
                CapabilityVocabulary::TypeSemantics,
            ]
        );
        let derived_again =
            service
                .resolve_policy(DEFAULT_POLICY_PROFILE_NAME, &[], &ts_scope)
                .unwrap();
        assert_eq!(derived.policy_digest, derived_again.policy_digest);

        let with_type = service
            .resolve_policy(
                DEFAULT_POLICY_PROFILE_NAME,
                &[CapabilityVocabulary::TypeSemantics],
                &[],
            )
            .unwrap();
        let with_syntax_again = service
            .resolve_policy(
                DEFAULT_POLICY_PROFILE_NAME,
                &[CapabilityVocabulary::Syntax],
                &[],
            )
            .unwrap();
        assert_ne!(with_type.policy_digest, with_syntax_again.policy_digest);

        assert_eq!(
            service
                .resolve_policy("no-such-profile", &[], &[])
                .unwrap_err()
                .code(),
            "policy_unknown"
        );
    }

    #[test]
    fn workspace_open_binds_exact_project_root_per_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let repo_dir = tempfile::tempdir().unwrap();
        let mut service = service_at(&dir);

        let status = service
            .workspace_open(
                "repo-1",
                "wt-a",
                Some(repo_dir.path().to_str().unwrap()),
            )
            .unwrap();
        let bound_a = status["projectRoot"].as_str().unwrap().to_string();
        assert!(!bound_a.is_empty());
        // Canonicalized: the same root through a different spelling binds to
        // the same canonical path.
        let canonical = std::fs::canonicalize(repo_dir.path()).unwrap();
        assert_eq!(
            bound_a,
            canonical.to_string_lossy(),
            "workspace identity must bind the exact canonical root"
        );

        // A second worktree of the same repo binds its own root.
        let other_dir = tempfile::tempdir().unwrap();
        let status_b = service
            .workspace_open(
                "repo-1",
                "wt-b",
                Some(other_dir.path().to_str().unwrap()),
            )
            .unwrap();
        let bound_b = status_b["projectRoot"].as_str().unwrap().to_string();
        assert_ne!(bound_a, bound_b, "distinct worktrees must bind distinct roots");
    }

    #[test]
    fn workspace_open_same_ids_same_root_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let repo_dir = tempfile::tempdir().unwrap();
        let mut service = service_at(&dir);
        let root_str = repo_dir.path().to_str().unwrap();
        let first = service.workspace_open("repo-1", "wt-1", Some(root_str)).unwrap();
        assert_eq!(first["created"], serde_json::json!(true));
        let second = service.workspace_open("repo-1", "wt-1", Some(root_str)).unwrap();
        assert_eq!(second["created"], serde_json::json!(false));
        assert_eq!(first["projectRoot"], second["projectRoot"]);
    }

    #[test]
    fn workspace_open_same_ids_different_root_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let repo_dir_a = tempfile::tempdir().unwrap();
        let repo_dir_b = tempfile::tempdir().unwrap();
        let mut service = service_at(&dir);
        service
            .workspace_open("repo-1", "wt-1", Some(repo_dir_a.path().to_str().unwrap()))
            .unwrap();
        let err = service
            .workspace_open("repo-1", "wt-1", Some(repo_dir_b.path().to_str().unwrap()))
            .unwrap_err();
        assert_eq!(err.code(), "workspace_project_root_conflict");
    }

    #[test]
    fn workspace_open_uncanonicalizable_root_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let mut service = service_at(&dir);
        let err = service
            .workspace_open("repo-1", "wt-1", Some("/nonexistent/path/that/does/not/exist/xyz123"))
            .unwrap_err();
        assert_eq!(err.code(), "project_root_invalid");
    }

    #[test]
    fn workspace_paths_escaping_bound_root_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let repo_dir = tempfile::tempdir().unwrap();
        let mut service = service_at(&dir);
        service
            .workspace_open("repo-1", "wt-1", Some(repo_dir.path().to_str().unwrap()))
            .unwrap();
        service.mutation_begin("repo-1", "wt-1").unwrap();
        let mut epoch = test_epoch(1);
        epoch.changed_paths = vec!["../escape.txt".to_string()];
        epoch.changed_file_hashes = vec![ChangedFileHashV1 {
            path: "../escape.txt".to_string(),
            hash: "sha256:abc".to_string(),
        }];
        let err = service.mutation_seal("repo-1", "wt-1", epoch).unwrap_err();
        assert_eq!(err.code(), "mutation_boundary");
    }

    #[test]
    fn workspace_open_exposes_project_root_through_status() {
        let dir = tempfile::tempdir().unwrap();
        let repo_dir = tempfile::tempdir().unwrap();
        let mut service = service_at(&dir);
        service
            .workspace_open("repo-1", "wt-1", Some(repo_dir.path().to_str().unwrap()))
            .unwrap();
        let status = service.workspace_status("repo-1", "wt-1").unwrap();
        assert!(status["projectRoot"].is_string());
        assert!(!status["projectRoot"].as_str().unwrap().is_empty());
    }

    #[test]
    fn blueprint_unavailable_degrades_typed_never_fabricates() {
        use crate::providers::blueprint_findings::{
            BlueprintFindingsError, BlueprintFindingsResult,
        };

        struct FailingClient;
        impl crate::providers::blueprint_findings::BlueprintFindingsClient for FailingClient {
            fn fetch(
                &mut self,
                _repo_root: &std::path::Path,
                _timeout_ms: u64,
            ) -> Result<BlueprintFindingsResult, BlueprintFindingsError> {
                Err(BlueprintFindingsError::Unavailable(
                    "daemon endpoint missing".into(),
                ))
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let mut service =
            service_at(&dir).with_blueprint_client(Box::new(FailingClient));
        service.workspace_open("repo-1", "wt-1", None).unwrap();
        service.mutation_begin("repo-1", "wt-1").unwrap();
        service.mutation_seal("repo-1", "wt-1", test_epoch(1)).unwrap();

        let decision = service.snapshot_await(&snapshot_request(&[])).unwrap();
        let snapshot = service.snapshot_get("repo-1", "wt-1").unwrap();
        assert_eq!(snapshot["blueprintGeneration"], Value::Null);
        assert_eq!(snapshot["blueprintFreshness"], json!("unknown"));
        assert_eq!(decision.outcome, GateOutcome::UnknownIncomplete);
        assert!(decision
            .omissions
            .iter()
            .any(|omission| omission.code == "blueprint_unavailable"));
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
        service.workspace_open("repo-1", "wt-1", None).unwrap();
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

    #[test]
    fn rust_d1_alone_leaves_compiler_project_semantics_unsatisfied() {
        // Touched scope with .rs file requires CompilerProjectSemantics;
        // without a V1 verification lane it must remain Unsatisfied.
        let dir = tempfile::tempdir().unwrap();
        let mut service = service_at(&dir);
        let repo_dir = tempfile::tempdir().unwrap();
        service
            .workspace_open("repo-1", "wt-1", Some(repo_dir.path().to_str().unwrap()))
            .unwrap();
        service.mutation_begin("repo-1", "wt-1").unwrap();
        let mut epoch = test_epoch(1);
        epoch.changed_paths = vec!["src/main.rs".to_string()];
        epoch.changed_file_hashes = vec![ChangedFileHashV1 {
            path: "src/main.rs".to_string(),
            hash: "sha256:abc".to_string(),
        }];
        // Need canonical file to pass path escape check + hash check for provider? Create file.
        std::fs::create_dir_all(repo_dir.path().join("src")).unwrap();
        std::fs::write(repo_dir.path().join("src/main.rs"), "fn main() {}").unwrap();
        // Update epoch hash to match actual file hash to avoid hash_mismatch branch,
        // but still no V1 provider is registered, so CompilerProjectSemantics stays unsatisfied.
        let bytes = std::fs::read(repo_dir.path().join("src/main.rs")).unwrap();
        let actual = format!("sha256:{}", hex::encode(sha2::Sha256::digest(&bytes)));
        epoch.changed_file_hashes[0].hash = actual.clone();
        // Compute manifest digest matching helper used elsewhere is not critical for gate.
        service.mutation_seal("repo-1", "wt-1", epoch).unwrap();
        let decision = service
            .snapshot_await(&SnapshotAwaitRequest {
                repo_id: "repo-1".to_string(),
                worktree_id: "wt-1".to_string(),
                policy_profile_name: DEFAULT_POLICY_PROFILE_NAME.to_string(),
                required_capabilities: vec![],
                max_cost: Some(CostClass::Interactive),
                deadline_ms: Some(5_000),
            })
            .unwrap();
        // Rust touched: derived required includes compiler_project_semantics, which has no exact lane.
        assert!(
            decision
                .reason_codes
                .iter()
                .any(|c| c.contains("compiler_project_semantics")),
            "expected compiler_project_semantics unsatisfied: {:?}",
            decision.reason_codes
        );
        assert_ne!(decision.outcome, GateOutcome::CleanExact);
    }
}
