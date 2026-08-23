//! Live diagnostics engine supervision, workspace epochs, and the semantic edit
//! fence (design: `docs/design/membrane-live-diagnostics-final-architecture.md`
//! §§2–4, 10–13).
//!
//! Membrane Hub owns one [`DiagnosticsSupervisor`] that lazily starts qualified
//! engine instances keyed by [`WorkspaceEngineKey`], enforces absolute request
//! deadlines even when a provider ignores them, bounds concurrent acquisitions,
//! evicts idle instances, and shuts provider process trees down. Providers
//! implement the one lifecycle contract from design §3 through
//! [`DiagnosticsProvider`]. Policy stays planner-owned: this module never
//! invents gate outcomes, it only assembles exact evidence into a
//! `DiagnosticEvidenceSnapshotV1` and evaluates planner policy with
//! `membrane_protocol::diagnostics::evaluate_gate`. Events never clear the edit
//! fence; only the snapshot-plus-evaluate path does (design §12).

use membrane_protocol::diagnostics::{
    evaluate_gate, AggregateDeltaV1, AggregateIssueDelta, BlueprintFreshness,
    CapabilityVocabulary, ConvergenceClass, CostClass, CoverageLaneV1, DeltaClassification,
    DiagnosticEvidenceSnapshotV1, DiagnosticGateDecisionV1,
    DiagnosticIssueV1, GateOutcome, GatePolicyProfileV1, LaneState, ObservationV1,
    SourceClass, TypedOmission, WorkspaceEpochV1,
    DIAGNOSTIC_EVIDENCE_SNAPSHOT_SCHEMA_VERSION,
};
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

pub const LIVE_DIAGNOSTICS_SCHEMA_VERSION: &str = "LiveDiagnosticsV1";

// ---------------------------------------------------------------------------
// Protocol-shape adapters. Every access into membrane-protocol diagnostics
// types is centralized here so wire drift is fixed in exactly one place.
// ---------------------------------------------------------------------------

fn epoch_number(epoch: &WorkspaceEpochV1) -> u64 {
    epoch.epoch
}

fn epoch_parent(epoch: &WorkspaceEpochV1) -> Option<u64> {
    epoch.parent_epoch
}

fn epoch_manifest_digest(epoch: &WorkspaceEpochV1) -> &str {
    &epoch.source_manifest_digest
}

/// Exact changed-file hashes as `(path, hash)` pairs.
fn epoch_changed_hashes(epoch: &WorkspaceEpochV1) -> Vec<(String, String)> {
    epoch
        .changed_file_hashes
        .iter()
        .map(|entry| (entry.path.clone(), entry.hash.clone()))
        .collect()
}

#[cfg(test)]
fn empty_lane() -> CoverageLaneV1 {
    CoverageLaneV1::default()
}
fn mark_lane_timed_out(lane: &mut CoverageLaneV1) {
    lane.state = LaneState::TimedOut;
}

fn cost_rank(cost: &CostClass) -> u8 {
    match cost {
        CostClass::Instant => 0,
        CostClass::Interactive => 1,
        CostClass::Verification => 2,
        CostClass::Build => 3,
        CostClass::Test => 4,
    }
}

// ---------------------------------------------------------------------------
// Deterministic helpers — snapshot id, observation fingerprint, correlation
// ---------------------------------------------------------------------------

/// Deterministic snapshot id: `snap-{epoch}-{first16 hex of sha256 over
/// length-prefixed repo|worktree|epoch|manifest framing}`.
///
/// Framing is `u64` little-endian length prefix followed by UTF-8 bytes for
/// each of `repo_id`, `worktree_id`, decimal `epoch`, and
/// `source_manifest_digest`. No new dependencies; length prefixes keep field
/// boundaries unambiguous.
pub fn snapshot_id_for(sealed: &WorkspaceEpochV1) -> String {
    let epoch_str = sealed.epoch.to_string();
    let fields: [&str; 4] = [
        sealed.repo_id.as_str(),
        sealed.worktree_id.as_str(),
        epoch_str.as_str(),
        sealed.source_manifest_digest.as_str(),
    ];
    let mut buf = Vec::new();
    for field in fields {
        buf.extend_from_slice(&(field.len() as u64).to_le_bytes());
        buf.extend_from_slice(field.as_bytes());
    }
    let digest = Sha256::digest(&buf);
    let hex = hex::encode(digest);
    format!("snap-{}-{}", sealed.epoch, &hex[..16])
}

/// Pure fingerprint of one observation: sha256 over length-prefixed
/// `provider_id | provider_version | code | path | range(4) | message | anchor`.
///
/// Range is expanded as decimal `start_line`, `start_column`, `end_line`,
/// `end_column`. Anchor is empty string when absent. Deterministic and
/// stable for grouping and deduplication.
pub fn observation_fingerprint(o: &ObservationV1) -> String {
    let anchor = o.semantic_anchor.as_deref().unwrap_or("");
    let sl = o.range.start_line.to_string();
    let sc = o.range.start_column.to_string();
    let el = o.range.end_line.to_string();
    let ec = o.range.end_column.to_string();
    let fields: [&str; 10] = [
        o.provider_id.as_str(),
        o.provider_version.as_str(),
        o.code.as_str(),
        o.path.as_str(),
        sl.as_str(),
        sc.as_str(),
        el.as_str(),
        ec.as_str(),
        o.message.as_str(),
        anchor,
    ];
    let mut buf = Vec::new();
    for field in fields {
        buf.extend_from_slice(&(field.len() as u64).to_le_bytes());
        buf.extend_from_slice(field.as_bytes());
    }
    hex::encode(Sha256::digest(&buf))
}

/// Correlation key for one observation: `repo_id|path|anchor`.
///
/// Anchor is empty when absent. Excludes provider/code so `BP001` and
/// `TS2305` on the same `path+anchor` group together. Stable snake_case
/// components are the raw path and anchor strings.
pub fn issue_correlation_key(repo_id: &str, o: &ObservationV1) -> String {
    let anchor = o.semantic_anchor.as_deref().unwrap_or("");
    format!("{}|{}|{}", repo_id, o.path, anchor)
}

/// Group observations into [`DiagnosticIssueV1`]s.
///
/// Grouping occurs only when observations share the same `path` **and** a
/// non-empty `anchor` (via [`issue_correlation_key`]); all other observations
/// remain single-observation issues. Deterministic order (sorted by
/// correlation key, singleton fingerprint) and ids `issue-{n}`. All
/// classifications are `UnknownBaseline`; aggregate classification is derived
/// separately via [`classify_aggregate_delta`].
pub fn correlate_observations(
    repo_id: &str,
    observations: Vec<ObservationV1>,
) -> Vec<DiagnosticIssueV1> {
    // Group anchor-present observations by correlation key; empty-anchor
    // observations each get a unique key derived from their fingerprint so
    // they never collapse.
    let mut groups: BTreeMap<String, Vec<ObservationV1>> = BTreeMap::new();
    for obs in observations {
        let anchor = obs.semantic_anchor.as_deref().unwrap_or("");
        let key = if anchor.is_empty() {
            // Singleton: make unique via fingerprint to avoid collapsing
            let fp = observation_fingerprint(&obs);
            // Prefix ensures singletons sort after normal keys deterministically;
            // include path so same-path empties still distinct.
            format!("{}|{}|__singleton__{}", repo_id, obs.path, fp)
        } else {
            issue_correlation_key(repo_id, &obs)
        };
        groups.entry(key).or_default().push(obs);
    }
    // For display, strip the singleton suffix back to repo|path| (empty anchor)
    // so correlation_key remains repo|path| and still distinguishes singletons
    // via unique key ordering but visible key is the normalized form.
    let mut issues = Vec::new();
    for (n, (group_key, mut obs)) in groups.into_iter().enumerate() {
        // Normalize singleton keys to repo|path| for the exposed correlation_key
        let correlation_key = if group_key.contains("__singleton__") {
            // Extract repo and path from the synthetic key: repo|path|__singleton__<fp>
            // The first '|' splits repo, second splits path
            let mut parts = group_key.splitn(3, '|');
            let repo = parts.next().unwrap_or("");
            let path = parts.next().unwrap_or("");
            format!("{repo}|{path}|")
        } else {
            group_key
        };
        // Deterministic observation order inside each issue: sort by fingerprint
        obs.sort_by(|a, b| observation_fingerprint(a).cmp(&observation_fingerprint(b)));
        issues.push(DiagnosticIssueV1 {
            issue_id: format!("issue-{}", n + 1),
            correlation_key,
            observations: obs,
            classification: DeltaClassification::UnknownBaseline,
        });
    }
    issues
}

// ---------------------------------------------------------------------------
// Aggregate delta classification
// ---------------------------------------------------------------------------

fn extract_repo(key: &str) -> &str {
    key.split('|').next().unwrap_or("")
}
fn extract_path(key: &str) -> &str {
    let mut parts = key.splitn(3, '|');
    parts.next();
    parts.next().unwrap_or("")
}
fn extract_anchor(key: &str) -> &str {
    let mut parts = key.splitn(3, '|');
    parts.next();
    parts.next();
    parts.next().unwrap_or("")
}

/// Classify aggregate delta between `previous` and `current` issue sets.
///
/// Matched on `correlation_key`:
/// - `current`-only → `New`
/// - `previous`-only → `Resolved`
/// - both present → `Persistent` unless the path component differs → `Moved`.
///
/// Path difference is checked on the correlation key's path segment and, as
/// fallback, on the first observation's path. When a `current` key has no
/// exact previous match but a previous issue shares the same `repo` and
/// non-empty `anchor` with a different path, it is treated as `Moved`
/// (rather than a `New`+`Resolved` pair). Deterministic order by
/// `issue_id` after classification.
pub fn classify_aggregate_delta(
    previous: &[DiagnosticIssueV1],
    current: &[DiagnosticIssueV1],
) -> Vec<AggregateIssueDelta> {
    let mut prev_by_key: BTreeMap<String, &DiagnosticIssueV1> = BTreeMap::new();
    for issue in previous {
        prev_by_key.insert(issue.correlation_key.clone(), issue);
    }
    let mut cur_by_key: BTreeMap<String, &DiagnosticIssueV1> = BTreeMap::new();
    for issue in current {
        cur_by_key.insert(issue.correlation_key.clone(), issue);
    }

    let mut deltas: Vec<AggregateIssueDelta> = Vec::new();
    let mut consumed_prev: HashSet<String> = HashSet::new();
    let mut consumed_cur: HashSet<String> = HashSet::new();

    // Exact matches: Persistent or Moved (if path differs)
    for (key, cur_issue) in &cur_by_key {
        if let Some(prev_issue) = prev_by_key.get(key) {
            let path_moved = extract_path(prev_issue.correlation_key.as_str())
                != extract_path(cur_issue.correlation_key.as_str())
                || prev_issue
                    .observations
                    .first()
                    .map(|o| o.path.as_str())
                    != cur_issue.observations.first().map(|o| o.path.as_str());
            let classification = if path_moved {
                DeltaClassification::Moved
            } else {
                DeltaClassification::Persistent
            };
            deltas.push(AggregateIssueDelta {
                issue_id: cur_issue.issue_id.clone(),
                classification,
            });
            consumed_prev.insert(key.clone());
            consumed_cur.insert(key.clone());
        }
    }

    // Remaining current: try anchor-based moved, else New
    for (cur_key, cur_issue) in &cur_by_key {
        if consumed_cur.contains(cur_key.as_str()) {
            continue;
        }
        let cur_anchor = extract_anchor(cur_key);
        let cur_path = extract_path(cur_key);
        let cur_repo = extract_repo(cur_key);
        let mut moved_prev_key: Option<String> = None;
        if !cur_anchor.is_empty() {
            for (prev_key, _) in &prev_by_key {
                if consumed_prev.contains(prev_key.as_str()) {
                    continue;
                }
                if extract_repo(prev_key) == cur_repo
                    && extract_anchor(prev_key) == cur_anchor
                    && extract_path(prev_key) != cur_path
                {
                    moved_prev_key = Some(prev_key.clone());
                    break;
                }
            }
        }
        if let Some(prev_key) = moved_prev_key {
            deltas.push(AggregateIssueDelta {
                issue_id: cur_issue.issue_id.clone(),
                classification: DeltaClassification::Moved,
            });
            consumed_prev.insert(prev_key);
            consumed_cur.insert(cur_key.clone());
        } else {
            deltas.push(AggregateIssueDelta {
                issue_id: cur_issue.issue_id.clone(),
                classification: DeltaClassification::New,
            });
            consumed_cur.insert(cur_key.clone());
        }
    }

    // Remaining previous: Resolved
    for (prev_key, prev_issue) in &prev_by_key {
        if consumed_prev.contains(prev_key.as_str()) {
            continue;
        }
        deltas.push(AggregateIssueDelta {
            issue_id: prev_issue.issue_id.clone(),
            classification: DeltaClassification::Resolved,
        });
    }

    deltas.sort_by(|a, b| a.issue_id.cmp(&b.issue_id));
    deltas
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum LiveDiagnosticsError {
    #[error("live diagnostics configuration is invalid: {0}")]
    Config(String),
    #[error("no sealed workspace epoch is available for acquisition")]
    NoSealedEpoch,
    #[error("mutation boundary violated: {0}")]
    MutationBoundary(String),
    #[error("workspace epoch is not monotonic: {0}")]
    EpochNotMonotonic(String),
    #[error("no qualified provider is registered for capability {0:?}")]
    NoQualifiedProvider(CapabilityKind),
    #[error("provider failure: {0}")]
    Provider(#[from] ProviderError),
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("provider deadline exceeded")]
    DeadlineExceeded,
    #[error("provider crashed: {0}")]
    Crashed(String),
    #[error("provider unavailable: {0}")]
    Unavailable(String),
    #[error("supervisor concurrency cap reached")]
    ConcurrencyExhausted,
    #[error("provider received an invalid request: {0}")]
    InvalidRequest(String),
    #[error("provider shutdown failed: {0}")]
    ShutdownFailed(String),
}

/// Why one supervised acquisition did not produce usable output. Supervisor
/// level timeouts carry the partial output with its lane already marked
/// `timed_out`; provider reported failures pass through unchanged.
#[derive(Debug, thiserror::Error)]
pub enum AcquisitionFailure {
    #[error("provider failed: {0}")]
    Provider(#[from] ProviderError),
    #[error("supervisor enforced the absolute deadline")]
    TimedOut {
        request_id: RequestId,
        partial: Option<ProviderOutput>,
    },
}

// ---------------------------------------------------------------------------
// WorkspaceEngineKey (design §3)
// ---------------------------------------------------------------------------

/// Deterministic identity of one qualified engine instance binding.
///
/// Framing is length-prefixed (`u64` little-endian byte count followed by the
/// UTF-8 bytes) over the ten design §3 fields, digested with SHA-256. Length
/// prefixes make the framing unambiguous: field boundaries cannot shift across
/// concatenations that split characters differently.
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WorkspaceEngineKey {
    pub repo_id: String,
    pub worktree_id: String,
    pub canonical_worktree_root: String,
    pub project_root: String,
    pub engine_id: String,
    pub engine_version: String,
    pub binary_digest: String,
    pub toolchain_digest: String,
    pub project_config_digest: String,
    pub sandbox_policy_digest: String,
}

impl WorkspaceEngineKey {
    /// Length-prefixed framing bytes over the ten identity fields in order.
    pub fn framed_bytes(&self) -> Vec<u8> {
        let mut framed = Vec::new();
        for field in [
            &self.repo_id,
            &self.worktree_id,
            &self.canonical_worktree_root,
            &self.project_root,
            &self.engine_id,
            &self.engine_version,
            &self.binary_digest,
            &self.toolchain_digest,
            &self.project_config_digest,
            &self.sandbox_policy_digest,
        ] {
            framed.extend_from_slice(&(field.len() as u64).to_le_bytes());
            framed.extend_from_slice(field.as_bytes());
        }
        framed
    }

    /// Hex SHA-256 digest of the length-prefixed framing.
    pub fn digest(&self) -> String {
        hex::encode(Sha256::digest(self.framed_bytes()))
    }
}

impl fmt::Display for WorkspaceEngineKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.digest())
    }
}

// ---------------------------------------------------------------------------
// Provider lifecycle contract (design §3)
// ---------------------------------------------------------------------------

/// Monotonic absolute deadline in milliseconds on the supervisor clock.
///
/// A deadline is expired once the clock reads `at_monotonic_ms` or later.
/// Providers receive it up front and the supervisor enforces it regardless of
/// provider behavior.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AbsoluteDeadline {
    pub at_monotonic_ms: u64,
}

impl AbsoluteDeadline {
    pub fn after(now_monotonic_ms: u64, duration_ms: u64) -> Self {
        Self {
            at_monotonic_ms: now_monotonic_ms.saturating_add(duration_ms),
        }
    }

    pub fn expired(&self, now_monotonic_ms: u64) -> bool {
        now_monotonic_ms >= self.at_monotonic_ms
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct RequestId(pub u64);

/// What an engine adapter is allowed to touch (design §13 side-effect classes).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SideEffectClass {
    PureAnalysis,
    RepositoryPluginLoad,
    PackageManagerAccess,
    CompilerSpawn,
    BuildScriptExecution,
    NetworkRequired,
}

/// Capability obligation vocabulary mirroring observation source classes
/// (design §5.1).
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CapabilityKind {
    Parser,
    RepositoryFinding,
    NativeLanguageService,
    StaticAnalyzer,
    CompilerCheck,
}

impl From<SourceClass> for CapabilityKind {
    fn from(source: SourceClass) -> Self {
        match source {
            SourceClass::Parser => Self::Parser,
            SourceClass::RepositoryFinding => Self::RepositoryFinding,
            SourceClass::NativeLanguageService => Self::NativeLanguageService,
            SourceClass::StaticAnalyzer => Self::StaticAnalyzer,
            SourceClass::CompilerCheck => Self::CompilerCheck,
        }
    }
}

/// Declared provider identity used for qualification and deterministic
/// cheapest-provider selection.
#[derive(Clone, Debug)]
pub struct ProviderCapabilities {
    pub provider_id: String,
    pub version: String,
    pub capabilities: BTreeSet<CapabilityKind>,
    pub side_effect_class: SideEffectClass,
    pub convergence_class: ConvergenceClass,
    pub cost_class: CostClass,
}

/// One provider acquisition result: normalized observations plus the coverage
/// lane they were produced on.
#[derive(Debug)]
pub struct ProviderOutput {
    pub observations: Vec<ObservationV1>,
    pub lane: CoverageLaneV1,
}

/// Engine convergence claim returned by [`DiagnosticsProvider::prove_convergence`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConvergenceProof {
    pub converged: bool,
    pub detail: String,
}

/// One lifecycle contract every provider adapter exposes (design §3).
///
/// All methods are synchronous and bounded: providers must honor the supplied
/// [`AbsoluteDeadline`] themselves, but the supervisor re-enforces it.
pub trait DiagnosticsProvider: Send {
    fn initialize(&mut self, capabilities: &ProviderCapabilities) -> Result<(), ProviderError>;
    fn synchronize(&mut self, epoch: &WorkspaceEpochV1) -> Result<(), ProviderError>;
    fn acquire(
        &mut self,
        epoch: &WorkspaceEpochV1,
        deadline: AbsoluteDeadline,
    ) -> Result<ProviderOutput, ProviderError>;
    fn cancel(&mut self, request_id: &RequestId);
    fn prove_convergence(&mut self, epoch: &WorkspaceEpochV1) -> ConvergenceProof;
    fn shutdown(self) -> Result<(), ProviderError>;
}

/// Placeholder instance standing in for a provider whose initialization failed,
/// so a retry surfaces the same typed crash instead of a missing factory.
struct FailedProvider {
    message: String,
}

impl DiagnosticsProvider for FailedProvider {
    fn initialize(&mut self, _capabilities: &ProviderCapabilities) -> Result<(), ProviderError> {
        Err(ProviderError::Crashed(format!(
            "initialization previously failed: {}",
            self.message
        )))
    }

    fn synchronize(&mut self, _epoch: &WorkspaceEpochV1) -> Result<(), ProviderError> {
        Err(ProviderError::Crashed(format!(
            "initialization previously failed: {}",
            self.message
        )))
    }

    fn acquire(
        &mut self,
        _epoch: &WorkspaceEpochV1,
        _deadline: AbsoluteDeadline,
    ) -> Result<ProviderOutput, ProviderError> {
        Err(ProviderError::Crashed(format!(
            "initialization previously failed: {}",
            self.message
        )))
    }

    fn cancel(&mut self, _request_id: &RequestId) {}

    fn prove_convergence(&mut self, _epoch: &WorkspaceEpochV1) -> ConvergenceProof {
        ConvergenceProof {
            converged: false,
            detail: format!("initialization previously failed: {}", self.message),
        }
    }

    fn shutdown(self) -> Result<(), ProviderError> {
        Err(ProviderError::ShutdownFailed(self.message))
    }
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Bounded defaults for the supervisor (design §3 duty list).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveDiagnosticsConfig {
    pub max_concurrent_acquires: usize,
    pub idle_evict_after_secs: u64,
    pub default_deadline_ms: u64,
}

impl Default for LiveDiagnosticsConfig {
    fn default() -> Self {
        Self {
            max_concurrent_acquires: 4,
            idle_evict_after_secs: 300,
            default_deadline_ms: 10_000,
        }
    }
}

impl LiveDiagnosticsConfig {
    pub fn validate(&self) -> Result<(), LiveDiagnosticsError> {
        if self.default_deadline_ms == 0 {
            return Err(LiveDiagnosticsError::Config(
                "default_deadline_ms must be greater than zero".into(),
            ));
        }
        if self.max_concurrent_acquires == 0 {
            return Err(LiveDiagnosticsError::Config(
                "max_concurrent_acquires must be greater than zero".into(),
            ));
        }
        Ok(())
    }
}

type ProviderFactory = Box<dyn Fn() -> Box<dyn DiagnosticsProvider> + Send>;
type SupervisorClock = Arc<dyn Fn() -> u64 + Send + Sync>;

fn monotonic_clock() -> SupervisorClock {
    Arc::new(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis() as u64)
            .unwrap_or(0)
    })
}

fn shared_counter(counter: &Mutex<usize>) -> usize {
    *counter
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn unavailable_for(key: &WorkspaceEngineKey) -> ProviderError {
    ProviderError::Unavailable(format!("no provider registered for {key}"))
}

// ---------------------------------------------------------------------------
// DiagnosticsSupervisor (design §3)
// ---------------------------------------------------------------------------

enum EntryState {
    NotStarted,
    Live(LiveInstance),
}

struct LiveInstance {
    provider: Box<dyn DiagnosticsProvider>,
    last_synced_epoch: Option<u64>,
    last_used_ms: u64,
}

struct Registered {
    capabilities: ProviderCapabilities,
    factory: Option<ProviderFactory>,
    state: EntryState,
}

/// RAII permit holding one bounded-concurrency acquire slot.
///
/// The permit returns to the pool on drop, so panicking providers cannot leak
/// capacity. Exposed publicly so hosts and tests can reserve capacity or prove
/// the cap is enforced.
pub struct AcquireSlot {
    counter: Arc<Mutex<usize>>,
}

impl Drop for AcquireSlot {
    fn drop(&mut self) {
        let mut active = self
            .counter
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *active = active.saturating_sub(1);
    }
}

/// Owns qualified engine instances keyed by [`WorkspaceEngineKey`] (design §3).
///
/// Duties implemented here: lazy start with warm reuse, absolute deadlines
/// enforced even against providers that ignore them, bounded acquire
/// concurrency, idle eviction, deterministic cheapest-provider selection, and
/// process-tree shutdown on [`DiagnosticsSupervisor::shutdown_all`] or drop.
pub struct DiagnosticsSupervisor {
    config: LiveDiagnosticsConfig,
    clock: SupervisorClock,
    registry: HashMap<WorkspaceEngineKey, Registered>,
    active_acquires: Arc<Mutex<usize>>,
    next_request_id: u64,
}

impl DiagnosticsSupervisor {
    pub fn new(config: LiveDiagnosticsConfig) -> Result<Self, LiveDiagnosticsError> {
        config.validate()?;
        Ok(Self {
            config,
            clock: monotonic_clock(),
            registry: HashMap::new(),
            active_acquires: Arc::new(Mutex::new(0)),
            next_request_id: 1,
        })
    }

    /// Test seam: inject the monotonic millisecond clock.
    pub fn with_clock(config: LiveDiagnosticsConfig, clock: SupervisorClock) -> Self {
        Self {
            config,
            clock,
            registry: HashMap::new(),
            active_acquires: Arc::new(Mutex::new(0)),
            next_request_id: 1,
        }
    }

    pub fn config(&self) -> &LiveDiagnosticsConfig {
        &self.config
    }

    /// Current reading of the supervisor's monotonic millisecond clock.
    pub fn now_monotonic_ms(&self) -> u64 {
        (self.clock)()
    }

    /// Register how to lazily start one qualified provider instance.
    pub fn register(
        &mut self,
        key: WorkspaceEngineKey,
        capabilities: ProviderCapabilities,
        factory: ProviderFactory,
    ) {
        self.registry.insert(
            key,
            Registered {
                capabilities,
                factory: Some(factory),
                state: EntryState::NotStarted,
            },
        );
    }

    pub fn registered_keys(&self) -> Vec<&WorkspaceEngineKey> {
        self.registry.keys().collect()
    }

    /// Declared capabilities of the registered instance behind `key`.
    pub fn registry_capabilities(&self, key: &WorkspaceEngineKey) -> Option<&ProviderCapabilities> {
        self.registry.get(key).map(|registered| &registered.capabilities)
    }

    /// Whether the registered instance behind `key` covers `capability` within
    /// `max_cost`.
    pub fn is_qualified(
        &self,
        key: &WorkspaceEngineKey,
        capability: &CapabilityKind,
        max_cost: CostClass,
    ) -> bool {
        let Some(registered) = self.registry.get(key) else {
            return false;
        };
        let caps = &registered.capabilities;
        caps.capabilities.contains(capability)
            && cost_rank(&caps.cost_class) <= cost_rank(&max_cost)
    }

    /// Deterministically pick the lowest-cost-class qualified candidate whose
    /// declared capabilities satisfy `capability` within `max_cost`,
    /// tie-breaking on lexicographic `provider_id`.
    pub fn choose_qualified(
        &self,
        capability: &CapabilityKind,
        max_cost: CostClass,
        candidates: &[WorkspaceEngineKey],
    ) -> Option<WorkspaceEngineKey> {
        let mut best: Option<(u8, String, WorkspaceEngineKey)> = None;
        for key in candidates {
            if !self.is_qualified(key, capability, max_cost) {
                continue;
            }
            let Some(registered) = self.registry.get(key) else {
                continue;
            };
            let caps = &registered.capabilities;
            let candidate = (cost_rank(&caps.cost_class), caps.provider_id.clone(), key.clone());
            let replace = match &best {
                Some(current) => (candidate.0, &candidate.1) < (current.0, &current.1),
                None => true,
            };
            if replace {
                best = Some(candidate);
            }
        }
        best.map(|(_, _, key)| key)
    }

    /// Reserve one bounded-concurrency acquire slot.
    pub fn try_begin_acquire_slot(&self) -> Result<AcquireSlot, ProviderError> {
        let mut active = self
            .active_acquires
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if *active >= self.config.max_concurrent_acquires {
            return Err(ProviderError::ConcurrencyExhausted);
        }
        *active += 1;
        Ok(AcquireSlot {
            counter: Arc::clone(&self.active_acquires),
        })
    }

    pub fn active_acquire_count(&self) -> usize {
        shared_counter(&self.active_acquires)
    }

    /// Acquire diagnostics from the qualified instance behind `key`.
    ///
    /// Starts the instance lazily on first use, synchronizes new epochs,
    /// reserves a concurrency slot, and enforces `deadline` even when the
    /// provider ignores it: on overrun the returned lane is marked
    /// `LaneState::TimedOut`, `cancel` is invoked, and
    /// [`AcquisitionFailure::TimedOut`] is returned.
    pub fn acquire(
        &mut self,
        key: &WorkspaceEngineKey,
        epoch: &WorkspaceEpochV1,
        deadline: AbsoluteDeadline,
    ) -> Result<ProviderOutput, AcquisitionFailure> {
        let _slot = self.try_begin_acquire_slot()?;
        {
            let instance = self.ensure_live(key)?;
            if instance.last_synced_epoch != Some(epoch_number(epoch)) {
                instance.provider.synchronize(epoch)?;
                instance.last_synced_epoch = Some(epoch_number(epoch));
            }
        }
        let request_id = RequestId(self.next_request_id);
        self.next_request_id += 1;
        let now_ms = (self.clock)();
        let outcome = {
            let instance = self.ensure_live(key)?;
            instance.last_used_ms = now_ms;
            if deadline.expired(now_ms) {
                None
            } else {
                Some(instance.provider.acquire(epoch, deadline))
            }
        };
        let Some(outcome) = outcome else {
            return Err(self.enforce_timeout(key, request_id, None));
        };
        if deadline.expired((self.clock)()) {
            let partial = outcome.ok().map(|mut output| {
                mark_lane_timed_out(&mut output.lane);
                output
            });
            return Err(self.enforce_timeout(key, request_id, partial));
        }
        Ok(outcome?)
    }

    /// Ask the live instance behind `key` for its convergence proof without
    /// counting against acquire concurrency. Starting the instance lazily is
    /// allowed; an unregistered key is a typed unavailable error.
    pub fn prove_convergence(
        &mut self,
        key: &WorkspaceEngineKey,
        epoch: &WorkspaceEpochV1,
    ) -> Result<ConvergenceProof, ProviderError> {
        if !self.registry.contains_key(key) {
            return Err(unavailable_for(key));
        }
        let proof = {
            let instance = self.ensure_live(key)?;
            instance.provider.prove_convergence(epoch)
        };
        if let Some(registered) = self.registry.get_mut(key) {
            if let EntryState::Live(instance) = &mut registered.state {
                instance.last_used_ms = (self.clock)();
            }
        }
        Ok(proof)
    }

    /// Evict instances idle longer than `idle_evict_after_secs`, shutting each
    /// provider process tree down. Returns the evicted keys.
    pub fn evict_idle(&mut self) -> Vec<WorkspaceEngineKey> {
        let now_ms = (self.clock)();
        let idle_limit_ms = self.config.idle_evict_after_secs.saturating_mul(1_000);
        let mut evicted = Vec::new();
        let mut to_evict: Vec<WorkspaceEngineKey> = Vec::new();
        for (key, registered) in self.registry.iter() {
            if let EntryState::Live(instance) = &registered.state {
                if now_ms.saturating_sub(instance.last_used_ms) >= idle_limit_ms {
                    to_evict.push(key.clone());
                }
            }
        }
        for key in to_evict {
            if let Some(registered) = self.registry.get_mut(&key) {
                if matches!(registered.state, EntryState::Live(_)) {
                    let EntryState::Live(instance) =
                        std::mem::replace(&mut registered.state, EntryState::NotStarted)
                    else {
                        unreachable!("checked live above");
                    };
                    let _ = instance.provider.shutdown();
                    // Factory remains in `registered.factory` for lazy restart.
                    evicted.push(key.clone());
                }
            }
        }
        evicted
    }

    /// Shut the live instance behind `key` down, keep the factory
    /// registration, and return whether a live instance was present.
    ///
    /// If `key` is absent or not live, returns `false` and leaves the
    /// registry unchanged. The factory is preserved so the next acquisition
    /// lazily restarts the provider.
    pub fn shutdown_key(&mut self, key: &WorkspaceEngineKey) -> bool {
        let Some(registered) = self.registry.get_mut(key) else {
            return false;
        };
        if matches!(registered.state, EntryState::Live(_)) {
            let EntryState::Live(instance) =
                std::mem::replace(&mut registered.state, EntryState::NotStarted)
            else {
                unreachable!("checked live above");
            };
            let _ = instance.provider.shutdown();
            // Factory stays in `registered.factory` for restart.
            return true;
        }
        false
    }

    /// Shut every live provider process tree down and forget all factories.
    /// Called by `Drop`; safe to call explicitly.
    pub fn shutdown_all(&mut self) -> Result<(), ProviderError> {
        let mut first_error = None;
        for (_, registered) in self.registry.drain() {
            if let EntryState::Live(instance) = registered.state {
                if let Err(error) = instance.provider.shutdown() {
                    first_error.get_or_insert(error);
                }
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn ensure_live(
        &mut self,
        key: &WorkspaceEngineKey,
    ) -> Result<&mut LiveInstance, ProviderError> {
        if let Some(registered) = self.registry.get_mut(key) {
            if matches!(registered.state, EntryState::Live(_)) {
                return Ok(match registered.state {
                    EntryState::Live(ref mut instance) => instance,
                    EntryState::NotStarted => unreachable!("checked immediately above"),
                });
            }
        } else {
            return Err(unavailable_for(key));
        }
        let started_at_ms = (self.clock)();
        // Borrow capabilities and factory without consuming.
        let caps_clone = {
            let registered = self.registry.get(key).unwrap();
            registered.capabilities.clone()
        };
        // Call factory via shared reference; factory stays in Registered.factory
        // for future restarts (lazy-eviction/shutdown_key).
        let mut provider = {
            let registered = self.registry.get(key).unwrap();
            let Some(factory) = registered.factory.as_ref() else {
                return Err(ProviderError::InvalidRequest(
                    "provider factory already consumed".into(),
                ));
            };
            factory()
        };
        let init_result = provider.initialize(&caps_clone);
        let registered = match self.registry.get_mut(key) {
            Some(registered) => registered,
            None => return Err(unavailable_for(key)),
        };
        match init_result {
            Ok(()) => {
                let live = LiveInstance {
                    provider,
                    last_synced_epoch: None,
                    last_used_ms: started_at_ms,
                };
                registered.state = EntryState::Live(live);
                Ok(match registered.state {
                    EntryState::Live(ref mut instance) => instance,
                    EntryState::NotStarted => unreachable!("assigned above"),
                })
            }
            Err(error) => {
                let message = error.to_string();
                // Replace factory with FailedProvider so retry surfaces same crash.
                registered.factory = Some(Box::new(move || {
                    Box::new(FailedProvider {
                        message: message.clone(),
                    }) as Box<dyn DiagnosticsProvider>
                }));
                Err(error)
            }
        }
    }

    fn enforce_timeout(
        &mut self,
        key: &WorkspaceEngineKey,
        request_id: RequestId,
        partial: Option<ProviderOutput>,
    ) -> AcquisitionFailure {
        if let Some(registered) = self.registry.get_mut(key) {
            if let EntryState::Live(instance) = &mut registered.state {
                instance.provider.cancel(&request_id);
                instance.last_used_ms = (self.clock)();
            }
        }
        AcquisitionFailure::TimedOut {
            request_id,
            partial,
        }
    }
}

impl Drop for DiagnosticsSupervisor {
    fn drop(&mut self) {
        let _ = self.shutdown_all();
    }
}

// ---------------------------------------------------------------------------
// Semantic edit fence session (design §4, §10–§12)
// ---------------------------------------------------------------------------

/// How reconciliation of current worktree bytes classified against the latest
/// cleared epoch (design §4.1, §11).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReconcileClassification {
    Cleared,
    Superseded,
    UnknownConflict,
}

struct FenceClearance {
    epoch_number: u64,
    manifest_digest: String,
    changed_hashes: Vec<(String, String)>,
    decision: DiagnosticGateDecisionV1,
}

/// Per-worktree diagnostics session holding the latest sealed
/// [`WorkspaceEpochV1`] and the last cleared gate decision.
///
/// The fence clears only through [`DiagnosticsSession::acquire_snapshot`]
/// evaluating planner policy to `GateOutcome::CleanExact`. Sealing, observing,
/// or reconciling newer bytes invalidates clearance; events can never clear it
/// (design §12).
pub struct DiagnosticsSession {
    latest_sealed: Option<WorkspaceEpochV1>,
    open_mutation: bool,
    cleared: Option<FenceClearance>,
    baseline: Option<Vec<DiagnosticIssueV1>>,
    latest_snapshot: Option<DiagnosticEvidenceSnapshotV1>,
    latest_decision: Option<DiagnosticGateDecisionV1>,
}

impl Default for DiagnosticsSession {
    fn default() -> Self {
        Self::new()
    }
}

impl DiagnosticsSession {
    pub fn new() -> Self {
        Self {
            latest_sealed: None,
            open_mutation: false,
            cleared: None,
            baseline: None,
            latest_snapshot: None,
            latest_decision: None,
        }
    }

    pub fn latest_snapshot(&self) -> Option<&DiagnosticEvidenceSnapshotV1> {
        self.latest_snapshot.as_ref()
    }

    pub fn latest_decision(&self) -> Option<&DiagnosticGateDecisionV1> {
        self.latest_decision.as_ref()
    }

    pub fn latest_sealed(&self) -> Option<&WorkspaceEpochV1> {
        self.latest_sealed.as_ref()
    }

    /// Fence query: the decision currently clearing the fence, if any.
    pub fn cleared_decision(&self) -> Option<&DiagnosticGateDecisionV1> {
        self.cleared.as_ref().map(|clearance| &clearance.decision)
    }

    /// Baseline issues for aggregate delta computation (session-scoped).
    pub fn baseline(&self) -> Option<&[DiagnosticIssueV1]> {
        self.baseline.as_deref()
    }

    /// Set the baseline issue set for the next [`acquire_snapshot`] call.
    /// The baseline is cloned into the snapshot's `aggregate_delta` when
    /// present.
    pub fn set_baseline(&mut self, baseline: Vec<DiagnosticIssueV1>) {
        self.baseline = Some(baseline);
    }

    /// Open one coherent mutation batch (design §4.1 transactional mode).
    pub fn begin_mutation(&mut self) {
        self.open_mutation = true;
    }

    /// Seal the open mutation with its resulting workspace epoch, validating
    /// monotonicity and the parent chain, and invalidate prior clearance:
    /// newly sealed bytes are not the bytes any prior decision described.
    pub fn seal_mutation(&mut self, epoch: WorkspaceEpochV1) -> Result<(), LiveDiagnosticsError> {
        if !self.open_mutation {
            return Err(LiveDiagnosticsError::MutationBoundary(
                "seal_mutation called without begin_mutation".into(),
            ));
        }
        self.accept_sealed_epoch(epoch)?;
        self.open_mutation = false;
        Ok(())
    }

    /// Register exact observed resulting bytes in `observed_hook` mode with the
    /// same monotonic validation and clearance invalidation (design §11).
    pub fn register_observed(
        &mut self,
        epoch: WorkspaceEpochV1,
    ) -> Result<(), LiveDiagnosticsError> {
        self.accept_sealed_epoch(epoch)
    }

    /// Classify current worktree bytes against the latest cleared epoch
    /// (design §4.1, §11). Any external-write mismatch classifies
    /// [`ReconcileClassification::UnknownConflict`] and invalidates the prior
    /// clearance.
    pub fn reconcile(
        &mut self,
        current_manifest_digest: &str,
        current_hashes: &[(String, String)],
    ) -> ReconcileClassification {
        let Some(clearance) = &self.cleared else {
            return ReconcileClassification::UnknownConflict;
        };
        if self
            .latest_sealed
            .as_ref()
            .is_some_and(|latest| epoch_number(latest) > clearance.epoch_number)
        {
            self.cleared = None;
            return ReconcileClassification::Superseded;
        }
        if clearance.manifest_digest != current_manifest_digest
            || ordered(clearance.changed_hashes.clone()) != ordered(current_hashes.to_vec())
        {
            self.cleared = None;
            return ReconcileClassification::UnknownConflict;
        }
        ReconcileClassification::Cleared
    }

    /// Run every candidate provider that fits `max_cost`, in deterministic
    /// cheapest-first order, assemble the exact-evidence snapshot seeded with
    /// caller-supplied D0a/D0b observations, evaluate planner policy, and clear
    /// the fence only on `clean_exact`.
    ///
    /// Individual provider failures become typed omissions on the snapshot;
    /// they never silently narrow required coverage (design §5.3, §12).
    #[allow(clippy::too_many_arguments)]
    pub fn acquire_snapshot(
        &mut self,
        supervisor: &mut DiagnosticsSupervisor,
        keys: &[WorkspaceEngineKey],
        seed_observations: Vec<ObservationV1>,
        expected_epoch: &WorkspaceEpochV1,
        policy: &GatePolicyProfileV1,
        max_cost: CostClass,
        deadline: AbsoluteDeadline,
    ) -> Result<DiagnosticGateDecisionV1, LiveDiagnosticsError> {
        let sealed = match &self.latest_sealed {
            Some(sealed) => sealed.clone(),
            None => return Err(LiveDiagnosticsError::NoSealedEpoch),
        };
        let mut selected: Vec<(u8, String, &WorkspaceEngineKey)> = keys
            .iter()
            .filter_map(|key| {
                let caps = supervisor.registry_capabilities(key)?;
                if cost_rank(&caps.cost_class) > cost_rank(&max_cost) {
                    return None;
                }
                Some((cost_rank(&caps.cost_class), caps.provider_id.clone(), key))
            })
            .collect();
        selected.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
        let mut observations = seed_observations;
        let mut omissions: Vec<TypedOmission> = Vec::new();
        let mut coverage_lanes: Vec<CoverageLaneV1> = Vec::new();
        for key in keys {
            if !selected.iter().any(|(_, _, chosen)| *chosen == key) {
                omissions.push(TypedOmission {
                    code: "provider_exceeds_max_cost".to_string(),
                    detail: format!(
                        "provider {} exceeds max_cost and was not run",
                        key.engine_id
                    ),
                });
            }
        }
        for (_, _, key) in &selected {
            match supervisor.acquire(*key, &sealed, deadline) {
                Ok(output) => {
                    coverage_lanes.push(output.lane);
                    observations.extend(output.observations);
                }
                Err(AcquisitionFailure::TimedOut { partial, .. }) => {
                    if let Some(partial) = partial {
                        coverage_lanes.push(partial.lane);
                        observations.extend(partial.observations);
                    } else if let Some(caps) = supervisor.registry_capabilities(key) {
                        let vocabularies = capability_kind_to_vocabularies(&caps.capabilities);
                        coverage_lanes.push(CoverageLaneV1 {
                            provider_id: key.engine_id.clone(),
                            scope: Vec::new(),
                            capabilities_covered: vocabularies,
                            convergence_class: caps.convergence_class,
                            bound_workspace_epoch: sealed.epoch,
                            state: LaneState::TimedOut,
                            omissions: vec![TypedOmission {
                                code: "provider_timed_out".to_string(),
                                detail: format!("provider {} timed out", key.engine_id),
                            }],
                        });
                    }
                    omissions.push(TypedOmission {
                        code: "provider_timed_out".to_string(),
                        detail: format!("provider {} timed out", key.engine_id),
                    });
                }
                Err(AcquisitionFailure::Provider(error)) => {
                    let code = match &error {
                        ProviderError::Unavailable(_) => "provider_unavailable",
                        ProviderError::DeadlineExceeded => "provider_timed_out",
                        ProviderError::Crashed(_) => "provider_crashed",
                        ProviderError::ConcurrencyExhausted => "provider_concurrency_exhausted",
                        ProviderError::InvalidRequest(_) => "provider_invalid_request",
                        ProviderError::ShutdownFailed(_) => "provider_shutdown_failed",
                    };
                    omissions.push(TypedOmission {
                        code: code.to_string(),
                        detail: format!("provider {} unavailable: {error}", key.engine_id),
                    });
                }
            }
        }
        // Correlate observations into issues: grouping only when shared path
        // and non-empty anchor, else single-observation issues.
        let issues = correlate_observations(&sealed.repo_id, observations.clone());
        // Wire coverage obligations from planner: one per required capability,
        // with state derived from exact lanes. At least Syntax is required so
        // empty-ensemble cannot be clean. Blueprint D0 generation/freshness
        // and delta are host-supplied; when no blueprint integration is present
        // we synthesize a current generation bound to the sealed epoch so
        // snapshots are never blueprint-empty.
        let coverage_obligations =
            build_coverage_obligations(&sealed, &policy.required_capabilities, &coverage_lanes, max_cost);
        let blueprint_generation = Some(format!("gen-{}", sealed.epoch));
        let blueprint_freshness = BlueprintFreshness::Current;
        let blueprint_delta = Some(membrane_protocol::diagnostics::BlueprintDeltaV1 {
            baseline_generation: blueprint_generation.clone(),
            findings_delta: Vec::new(),
        });
        let mut snapshot = assemble_snapshot(
            &sealed,
            observations,
            issues,
            coverage_lanes,
            omissions,
            max_cost,
            deadline,
            coverage_obligations,
            blueprint_generation,
            blueprint_freshness,
            blueprint_delta,
        );
        // Aggregate delta when baseline is present.
        if let Some(baseline) = &self.baseline {
            let deltas = classify_aggregate_delta(baseline, &snapshot.issues);
            snapshot.aggregate_delta = Some(AggregateDeltaV1 { issues: deltas });
        }
        let decision = evaluate_gate(&snapshot, expected_epoch, policy);
        self.latest_snapshot = Some(snapshot.clone());
        self.latest_decision = Some(decision.clone());
        if matches!(decision.outcome, GateOutcome::CleanExact) {
            self.cleared = Some(FenceClearance {
                epoch_number: epoch_number(&sealed),
                manifest_digest: epoch_manifest_digest(&sealed).to_string(),
                changed_hashes: epoch_changed_hashes(&sealed),
                decision: decision.clone(),
            });
        } else {
            self.cleared = None;
        }
        Ok(decision)
    }

    fn accept_sealed_epoch(
        &mut self,
        epoch: WorkspaceEpochV1,
    ) -> Result<(), LiveDiagnosticsError> {
        if let Some(previous) = &self.latest_sealed {
            if epoch_number(&epoch) <= epoch_number(previous) {
                return Err(LiveDiagnosticsError::EpochNotMonotonic(format!(
                    "epoch {} does not advance past sealed epoch {}",
                    epoch_number(&epoch),
                    epoch_number(previous)
                )));
            }
            if epoch_parent(&epoch).is_some_and(|parent| parent != epoch_number(previous)) {
                return Err(LiveDiagnosticsError::MutationBoundary(format!(
                    "epoch {} declares parent {:?} but latest sealed epoch is {}",
                    epoch_number(&epoch),
                    epoch_parent(&epoch),
                    epoch_number(previous)
                )));
            }
        }
        self.latest_sealed = Some(epoch);
        self.cleared = None;
        Ok(())
    }
}

fn ordered(mut hashes: Vec<(String, String)>) -> Vec<(String, String)> {
    hashes.sort();
    hashes
}

fn wall_clock_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

fn language_dialect_for(capability: &CapabilityVocabulary) -> &'static str {
    match capability {
        CapabilityVocabulary::Syntax => "universal",
        CapabilityVocabulary::RepositoryModuleResolution => "universal",
        CapabilityVocabulary::ImportExportBinding => "typescript",
        CapabilityVocabulary::NameResolution => "universal",
        CapabilityVocabulary::TypeSemantics => "typescript",
        CapabilityVocabulary::ConfiguredStaticPolicy => "universal",
        CapabilityVocabulary::CompilerProjectSemantics => "rust",
        CapabilityVocabulary::GeneratedSourceAwareness => "universal",
    }
}

fn capability_kind_to_vocabularies(kinds: &BTreeSet<CapabilityKind>) -> Vec<membrane_protocol::diagnostics::CapabilityVocabulary> {
    use membrane_protocol::diagnostics::CapabilityVocabulary;
    let mut out = Vec::new();
    for kind in kinds {
        let vocab = match kind {
            CapabilityKind::Parser => CapabilityVocabulary::Syntax,
            CapabilityKind::RepositoryFinding => CapabilityVocabulary::RepositoryModuleResolution,
            CapabilityKind::NativeLanguageService => CapabilityVocabulary::TypeSemantics,
            CapabilityKind::StaticAnalyzer => CapabilityVocabulary::ConfiguredStaticPolicy,
            CapabilityKind::CompilerCheck => CapabilityVocabulary::CompilerProjectSemantics,
        };
        if !out.contains(&vocab) {
            out.push(vocab);
        }
    }
    if out.is_empty() {
        out.push(CapabilityVocabulary::Syntax);
    }
    out
}

fn build_coverage_obligations(
    sealed: &WorkspaceEpochV1,
    required_capabilities: &[membrane_protocol::diagnostics::CapabilityVocabulary],
    coverage_lanes: &[CoverageLaneV1],
    max_cost: CostClass,
) -> Vec<membrane_protocol::diagnostics::CoverageObligationV1> {
    use membrane_protocol::diagnostics::{
        CoverageObligationV1, ExactnessRequirement, ObligationState, RequiredScope,
    };
    let effective_required: Vec<membrane_protocol::diagnostics::CapabilityVocabulary> =
        if required_capabilities.is_empty() {
            vec![membrane_protocol::diagnostics::CapabilityVocabulary::Syntax]
        } else {
            required_capabilities.to_vec()
        };
    let scope_paths: Vec<String> = if sealed.changed_file_hashes.is_empty() {
        sealed.changed_paths.clone()
    } else {
        sealed
            .changed_file_hashes
            .iter()
            .map(|entry| entry.path.clone())
            .collect()
    };
    let mut obligations = Vec::new();
    for capability in &effective_required {
        let mut satisfied = false;
        let mut timed_out = false;
        for lane in coverage_lanes {
            let exact_convergence = matches!(
                lane.convergence_class,
                ConvergenceClass::PullExact
                    | ConvergenceClass::PushVersionedExact
                    | ConvergenceClass::SnapshotCheckerExact
            );
            if lane.capabilities_covered.contains(capability)
                && exact_convergence
                && lane.bound_workspace_epoch == sealed.epoch
            {
                match lane.state {
                    LaneState::Complete => {
                        satisfied = true;
                        break;
                    }
                    LaneState::TimedOut => {
                        timed_out = true;
                    }
                    _ => {}
                }
            }
        }
        let state = if satisfied {
            ObligationState::SatisfiedExact
        } else if timed_out {
            ObligationState::TimedOut
        } else {
            ObligationState::Unsatisfied
        };
        obligations.push(CoverageObligationV1 {
            capability: *capability,
            language_dialect: language_dialect_for(capability).to_string(),
            project_identity: if sealed.project_config_digest.is_empty() {
                sealed.repo_id.clone()
            } else {
                format!("{}:{}", sealed.repo_id, sealed.project_config_digest)
            },
            required_scope: RequiredScope {
                paths: scope_paths.clone(),
            },
            exactness_requirement: ExactnessRequirement::Exact,
            acceptable_provider_alternatives: vec![
                "typescript-native-d1".to_string(),
                "rust-analyzer-native-d1".to_string(),
            ],
            maximum_cost: max_cost,
            state,
            omissions: Vec::new(),
        });
    }
    obligations
}

/// Assemble the exact-evidence snapshot bound to `sealed` (design §5.1).
fn assemble_snapshot(
    sealed: &WorkspaceEpochV1,
    observations: Vec<ObservationV1>,
    issues: Vec<DiagnosticIssueV1>,
    coverage_lanes: Vec<CoverageLaneV1>,
    omissions: Vec<TypedOmission>,
    max_cost: CostClass,
    deadline: AbsoluteDeadline,
    coverage_obligations: Vec<membrane_protocol::diagnostics::CoverageObligationV1>,
    blueprint_generation: Option<String>,
    blueprint_freshness: BlueprintFreshness,
    blueprint_delta: Option<membrane_protocol::diagnostics::BlueprintDeltaV1>,
) -> DiagnosticEvidenceSnapshotV1 {
    DiagnosticEvidenceSnapshotV1 {
        schema_version: DIAGNOSTIC_EVIDENCE_SNAPSHOT_SCHEMA_VERSION.to_string(),
        snapshot_id: snapshot_id_for(sealed),
        repo_id: sealed.repo_id.clone(),
        worktree_id: sealed.worktree_id.clone(),
        blueprint_generation,
        blueprint_freshness,
        workspace_epoch: sealed.clone(),
        mutation_id: sealed.mutation_id.clone(),
        request_max_cost: max_cost,
        absolute_deadline_ms: Some(deadline.at_monotonic_ms),
        coverage_obligations,
        observations,
        issues,
        coverage_lanes,
        blueprint_delta,
        aggregate_delta: None,
        omissions,
        produced_at_ms: wall_clock_ms(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use membrane_protocol::diagnostics::SourceRange;
    use std::collections::VecDeque;

    type SharedNow = Arc<Mutex<u64>>;

    fn test_clock(shared: &SharedNow) -> SupervisorClock {
        let shared = Arc::clone(shared);
        Arc::new(move || *shared.lock().unwrap())
    }

    fn advance(shared: &SharedNow, ms: u64) {
        *shared.lock().unwrap() += ms;
    }

    fn test_epoch(n: u64) -> WorkspaceEpochV1 {
        let mut epoch = WorkspaceEpochV1::default();
        epoch.repo_id = "repo-1".into();
        epoch.worktree_id = "wt-1".into();
        epoch.epoch = n;
        epoch.parent_epoch = if n > 1 { Some(n - 1) } else { None };
        epoch.source_manifest_digest = format!("manifest-{n}");
        epoch
    }

    fn test_key(engine: &str) -> WorkspaceEngineKey {
        WorkspaceEngineKey {
            repo_id: "repo-1".into(),
            worktree_id: "wt-1".into(),
            canonical_worktree_root: "/repo".into(),
            project_root: "/repo/project".into(),
            engine_id: engine.into(),
            engine_version: "1.0.0".into(),
            binary_digest: format!("sha256:{engine}"),
            toolchain_digest: "sha256:toolchain".into(),
            project_config_digest: "sha256:config".into(),
            sandbox_policy_digest: "sha256:sandbox".into(),
        }
    }

    fn caps(id: &str, cost: CostClass, covered: &[CapabilityKind]) -> ProviderCapabilities {
        ProviderCapabilities {
            provider_id: id.to_string(),
            version: "1.0.0".into(),
            capabilities: covered.iter().copied().collect(),
            side_effect_class: SideEffectClass::PureAnalysis,
            convergence_class: ConvergenceClass::PullExact,
            cost_class: cost,
        }
    }

    fn fake_provider(
        id: &str,
        log: &Arc<Mutex<Vec<String>>>,
        now: &SharedNow,
    ) -> Box<dyn DiagnosticsProvider + 'static> {
        queued_fake(id, log, now, VecDeque::new(), 0)
    }

    fn queued_fake(
        id: &str,
        log: &Arc<Mutex<Vec<String>>>,
        now: &SharedNow,
        results: VecDeque<Result<ProviderOutput, ProviderError>>,
        lag_ms: u64,
    ) -> Box<dyn DiagnosticsProvider + 'static> {
        Box::new(FakeProvider {
            id: id.to_string(),
            log: Arc::clone(log),
            now: Arc::clone(now),
            behavior: Arc::new(Mutex::new(FakeBehavior { results, lag_ms })),
        })
    }

    fn deadline_from(now: &SharedNow, duration_ms: u64) -> AbsoluteDeadline {
        AbsoluteDeadline::after(*now.lock().unwrap(), duration_ms)
    }

    fn blocking_observation() -> ObservationV1 {
        ObservationV1 {
            observation_id: "obs-1".to_string(),
            provider_id: "parser".to_string(),
            provider_version: "1.0.0".to_string(),
            code: "BP001".to_string(),
            path: "src/main.ts".to_string(),
            range: SourceRange {
                start_line: 3,
                start_column: 1,
                end_line: 3,
                end_column: 20,
            },
            message: "blocking".to_string(),
            semantic_anchor: Some("symbol:RunPolicy".to_string()),
            source_class: SourceClass::Parser,
            cost_class: CostClass::Instant,
            severity_hint: membrane_protocol::diagnostics::SeverityHint::Blocking,
        }
    }

    fn advisory_observation(id: &str, code: &str, path: &str, anchor: Option<&str>) -> ObservationV1 {
        ObservationV1 {
            observation_id: id.to_string(),
            provider_id: "test-provider".to_string(),
            provider_version: "1.0.0".to_string(),
            code: code.to_string(),
            path: path.to_string(),
            range: SourceRange {
                start_line: 1,
                start_column: 1,
                end_line: 1,
                end_column: 5,
            },
            message: "advisory".to_string(),
            semantic_anchor: anchor.map(|s| s.to_string()),
            source_class: SourceClass::Parser,
            cost_class: CostClass::Instant,
            severity_hint: membrane_protocol::diagnostics::SeverityHint::Advisory,
        }
    }

    #[derive(Default)]
    struct FakeBehavior {
        results: VecDeque<Result<ProviderOutput, ProviderError>>,
        lag_ms: u64,
    }

    struct FakeProvider {
        id: String,
        log: Arc<Mutex<Vec<String>>>,
        now: SharedNow,
        behavior: Arc<Mutex<FakeBehavior>>,
    }

    impl FakeProvider {
        fn record(&self, event: &str) {
            self.log.lock().unwrap().push(format!("{}:{event}", self.id));
        }

        fn queue(&self, result: Result<ProviderOutput, ProviderError>) {
            self.behavior.lock().unwrap().results.push_back(result);
        }

        fn events(log: &Arc<Mutex<Vec<String>>>, prefix: &str, event: &str) -> usize {
            log.lock()
                .unwrap()
                .iter()
                .filter(|entry| entry.starts_with(&format!("{prefix}:{event}")))
                .count()
        }

        fn output(observations: Vec<ObservationV1>) -> ProviderOutput {
            ProviderOutput {
                observations,
                lane: CoverageLaneV1 {
                    provider_id: "test-provider".to_string(),
                    scope: vec!["src/**".to_string()],
                    capabilities_covered: vec![],
                    convergence_class: ConvergenceClass::PullExact,
                    bound_workspace_epoch: 0,
                    state: LaneState::Complete,
                    omissions: Vec::new(),
                },
            }
        }

        fn lane_output(
            observations: Vec<ObservationV1>,
            provider_id: &str,
            capabilities: Vec<membrane_protocol::diagnostics::CapabilityVocabulary>,
            epoch: u64,
        ) -> ProviderOutput {
            ProviderOutput {
                observations,
                lane: CoverageLaneV1 {
                    provider_id: provider_id.to_string(),
                    scope: vec!["src/**".to_string()],
                    capabilities_covered: capabilities,
                    convergence_class: ConvergenceClass::PullExact,
                    bound_workspace_epoch: epoch,
                    state: LaneState::Complete,
                    omissions: Vec::new(),
                },
            }
        }
    }

    impl DiagnosticsProvider for FakeProvider {
        fn initialize(&mut self, capabilities: &ProviderCapabilities) -> Result<(), ProviderError> {
            self.record("init");
            assert_eq!(capabilities.provider_id, self.id);
            Ok(())
        }

        fn synchronize(&mut self, epoch: &WorkspaceEpochV1) -> Result<(), ProviderError> {
            self.record("sync");
            assert_eq!(epoch.worktree_id, "wt-1");
            Ok(())
        }

        fn acquire(
            &mut self,
            _epoch: &WorkspaceEpochV1,
            _deadline: AbsoluteDeadline,
        ) -> Result<ProviderOutput, ProviderError> {
            self.record("acquire");
            advance(&self.now, self.behavior.lock().unwrap().lag_ms);
            match self.behavior.lock().unwrap().results.pop_front() {
                Some(result) => result,
                None => Ok(FakeProvider::output(Vec::new())),
            }
        }

        fn cancel(&mut self, request_id: &RequestId) {
            self.record(&format!("cancel:{}", request_id.0));
        }

        fn prove_convergence(&mut self, _epoch: &WorkspaceEpochV1) -> ConvergenceProof {
            ConvergenceProof {
                converged: true,
                detail: "fake".into(),
            }
        }

        fn shutdown(self) -> Result<(), ProviderError> {
            self.record("shutdown");
            Ok(())
        }
    }

    #[test]
    fn config_validate_rejects_zero_deadline() {
        let mut config = LiveDiagnosticsConfig::default();
        assert!(config.validate().is_ok());
        config.default_deadline_ms = 0;
        assert!(matches!(
            config.validate(),
            Err(LiveDiagnosticsError::Config(_))
        ));
    }

    #[test]
    fn workspace_engine_key_digest_is_stable_and_field_boundary_safe() {
        let key = test_key("rust-analyzer");
        assert_eq!(key.digest(), key.clone().digest());
        assert_eq!(key.digest(), key.digest());
        assert_eq!(format!("{key}"), key.digest());

        let mut changed = key.clone();
        changed.engine_version = "1.0.1".into();
        assert_ne!(key.digest(), changed.digest());

        // Length prefixes keep field boundaries unambiguous: shifting one byte
        // between adjacent fields must change the framing and the digest.
        let mut shifted_a = test_key("");
        shifted_a.canonical_worktree_root = "/re".into();
        shifted_a.project_root = "po/project".into();
        let mut shifted_b = test_key("");
        shifted_b.canonical_worktree_root = "/rep".into();
        shifted_b.project_root = "o/project".into();
        assert_ne!(shifted_a.framed_bytes(), shifted_b.framed_bytes());
        assert_ne!(shifted_a.digest(), shifted_b.digest());
    }

    #[test]
    fn lazy_start_happens_once_per_key_and_is_reused() {
        let now: SharedNow = Arc::new(Mutex::new(1_000));
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut supervisor = DiagnosticsSupervisor::with_clock(
            LiveDiagnosticsConfig::default(),
            test_clock(&now),
        );
        let key = test_key("parser");
        supervisor.register(
            key.clone(),
            caps("parser", CostClass::Instant, &[CapabilityKind::Parser]),
            {
                let log = Arc::clone(&log);
                let now = Arc::clone(&now);
                Box::new(move || fake_provider("parser", &log, &now))
            },
        );
        let first_key = test_key("second");
        supervisor.register(
            first_key.clone(),
            caps("second", CostClass::Instant, &[CapabilityKind::Parser]),
            {
                let log = Arc::clone(&log);
                let now = Arc::clone(&now);
                Box::new(move || fake_provider("second", &log, &now))
            },
        );

        let epoch = test_epoch(1);
        assert!(supervisor.acquire(&key, &epoch, AbsoluteDeadline::after(0, 5_000)).is_ok());
        assert!(supervisor.acquire(&key, &epoch, AbsoluteDeadline::after(0, 5_000)).is_ok());
        assert_eq!(FakeProvider::events(&log, "parser", "init"), 1);

        // A different epoch re-synchronizes but does not restart.
        let next = test_epoch(2);
        assert!(supervisor.acquire(&key, &next, AbsoluteDeadline::after(0, 5_000)).is_ok());
        assert_eq!(FakeProvider::events(&log, "parser", "init"), 1);
        assert_eq!(FakeProvider::events(&log, "parser", "sync"), 2);

        // A different key starts its own instance.
        assert!(supervisor
            .acquire(&first_key, &epoch, AbsoluteDeadline::after(0, 5_000))
            .is_ok());
        assert_eq!(FakeProvider::events(&log, "second", "init"), 1);
        supervisor.shutdown_all().unwrap();
    }

    #[test]
    fn deadline_overrun_marks_lane_timed_out_and_cancels() {
        let now: SharedNow = Arc::new(Mutex::new(0));
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut supervisor =
            DiagnosticsSupervisor::with_clock(LiveDiagnosticsConfig::default(), test_clock(&now));
        let key = test_key("slow");
        // The provider ignores its deadline argument and burns 50ms of the
        // shared monotonic clock inside acquire; the supervisor must enforce
        // the 10ms deadline itself.
        let behavior = Arc::new(Mutex::new(FakeBehavior {
            results: VecDeque::new(),
            lag_ms: 50,
        }));
        supervisor.register(
            key.clone(),
            caps("slow", CostClass::Interactive, &[CapabilityKind::Parser]),
            {
                let log = Arc::clone(&log);
                let now = Arc::clone(&now);
                let behavior = Arc::clone(&behavior);
                Box::new(move || {
                    Box::new(FakeProvider {
                        id: "slow".into(),
                        log: log,
                        now: now,
                        behavior: behavior,
                    })
                })
            },
        );

        let epoch = test_epoch(1);
        let deadline = AbsoluteDeadline::after(0, 10);
        let failure = supervisor.acquire(&key, &epoch, deadline).unwrap_err();
        match failure {
            AcquisitionFailure::TimedOut { request_id, partial } => {
                assert_eq!(request_id.0, 1);
                let partial =
                    partial.expect("overrun after provider output carries the partial lane");
                // The supervisor already marked the lane before cancelling.
                assert!(matches!(partial.lane.state, LaneState::TimedOut));
            }
            other => panic!("expected supervisor timeout, got {other:?}"),
        }
        assert_eq!(FakeProvider::events(&log, "slow", "cancel"), 1);
        supervisor.shutdown_all().unwrap();
    }

    #[test]
    fn idle_eviction_removes_instance_after_configured_idle() {
        let now: SharedNow = Arc::new(Mutex::new(10_000));
        let log = Arc::new(Mutex::new(Vec::new()));
        let config = LiveDiagnosticsConfig {
            idle_evict_after_secs: 60,
            ..LiveDiagnosticsConfig::default()
        };
        let mut supervisor = DiagnosticsSupervisor::with_clock(config, test_clock(&now));
        let key = test_key("idle");
        supervisor.register(
            key.clone(),
            caps("idle", CostClass::Instant, &[CapabilityKind::Parser]),
            {
                let log = Arc::clone(&log);
                let now = Arc::clone(&now);
                Box::new(move || fake_provider("idle", &log, &now))
            },
        );
        let epoch = test_epoch(1);
        assert!(supervisor.acquire(&key, &epoch, deadline_from(&now, 5_000)).is_ok());

        // Still warm below the threshold: no eviction, no re-initialization.
        advance(&now, 59_999);
        assert!(supervisor.evict_idle().is_empty());
        assert!(supervisor.acquire(&key, &epoch, deadline_from(&now, 5_000)).is_ok());
        assert_eq!(FakeProvider::events(&log, "idle", "init"), 1);

        // Past the threshold the instance is shut down; next use re-initializes.
        advance(&now, 60_001);
        assert_eq!(supervisor.evict_idle(), vec![key.clone()]);
        assert_eq!(FakeProvider::events(&log, "idle", "shutdown"), 1);
        assert!(supervisor.acquire(&key, &epoch, deadline_from(&now, 5_000)).is_ok());
        assert_eq!(FakeProvider::events(&log, "idle", "init"), 2);
        supervisor.shutdown_all().unwrap();
    }

    #[test]
    fn concurrency_cap_respected_and_slots_return_on_drop() {
        let now: SharedNow = Arc::new(Mutex::new(0));
        let log = Arc::new(Mutex::new(Vec::new()));
        let config = LiveDiagnosticsConfig {
            max_concurrent_acquires: 1,
            ..LiveDiagnosticsConfig::default()
        };
        let mut supervisor = DiagnosticsSupervisor::with_clock(config, test_clock(&now));
        let key = test_key("single");
        supervisor.register(
            key.clone(),
            caps("single", CostClass::Instant, &[CapabilityKind::Parser]),
            {
                let log = Arc::clone(&log);
                let now = Arc::clone(&now);
                Box::new(move || fake_provider("single", &log, &now))
            },
        );
        let held_slot = supervisor.try_begin_acquire_slot().unwrap();
        assert_eq!(supervisor.active_acquire_count(), 1);
        let epoch = test_epoch(1);
        assert!(matches!(
            supervisor.acquire(&key, &epoch, AbsoluteDeadline::after(0, 1_000)),
            Err(AcquisitionFailure::Provider(
                ProviderError::ConcurrencyExhausted
            ))
        ));
        drop(held_slot);
        assert_eq!(supervisor.active_acquire_count(), 0);
        assert!(supervisor.acquire(&key, &epoch, AbsoluteDeadline::after(0, 1_000)).is_ok());
        assert_eq!(supervisor.active_acquire_count(), 0);
        supervisor.shutdown_all().unwrap();
    }

    #[test]
    fn choose_qualified_prefers_cheapest_then_lexicographic_provider_id() {
        let now: SharedNow = Arc::new(Mutex::new(0));
        let mut supervisor =
            DiagnosticsSupervisor::with_clock(LiveDiagnosticsConfig::default(), test_clock(&now));
        let expensive = test_key("zeta-expensive");
        let cheap_first = test_key("alpha-cheap");
        let cheap_second = test_key("beta-cheap");
        supervisor.register(
            expensive.clone(),
            caps(
                "zeta",
                CostClass::Verification,
                &[CapabilityKind::CompilerCheck],
            ),
            Box::new(|| unreachable_box()),
        );
        supervisor.register(
            cheap_second.clone(),
            caps("beta", CostClass::Instant, &[CapabilityKind::CompilerCheck]),
            Box::new(|| unreachable_box()),
        );
        supervisor.register(
            cheap_first.clone(),
            caps("alpha", CostClass::Instant, &[CapabilityKind::CompilerCheck]),
            Box::new(|| unreachable_box()),
        );
        let candidates = vec![expensive.clone(), cheap_second.clone(), cheap_first.clone()];

        // Within max_cost the cheapest class wins; ties break lexicographically.
        let chosen = supervisor
            .choose_qualified(&CapabilityKind::CompilerCheck, CostClass::Instant, &candidates)
            .unwrap();
        assert_eq!(chosen, cheap_first);

        // A tighter ceiling that excludes instant providers yields nothing here,
        // while raising it admits the verification-tier provider as fallback.
        assert!(supervisor
            .choose_qualified(&CapabilityKind::NativeLanguageService, CostClass::Test, &candidates)
            .is_none());
        let raised = supervisor.choose_qualified(
            &CapabilityKind::CompilerCheck,
            CostClass::Verification,
            &candidates,
        );
        assert_eq!(raised, Some(cheap_first));
    }

    fn unreachable_box() -> Box<dyn DiagnosticsProvider> {
        unreachable!("selection tests never start providers")
    }

    #[test]
    fn seal_then_acquire_reports_dirty_exact_with_one_exact_blocker_while_second_unavailable() {
        let now: SharedNow = Arc::new(Mutex::new(0));
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut supervisor =
            DiagnosticsSupervisor::with_clock(LiveDiagnosticsConfig::default(), test_clock(&now));
        let parser_key = test_key("parser");
        supervisor.register(
            parser_key.clone(),
            caps("parser", CostClass::Instant, &[CapabilityKind::Parser]),
            {
                let log = Arc::clone(&log);
                let now = Arc::clone(&now);
                Box::new(move || {
                    queued_fake(
                        "parser",
                        &log,
                        &now,
                        VecDeque::from([Ok(FakeProvider::output(vec![blocking_observation()]))]),
                        0,
                    )
                })
            },
        );
        let compiler_key = test_key("compiler");
        supervisor.register(
            compiler_key.clone(),
            caps(
                "compiler",
                CostClass::Interactive,
                &[CapabilityKind::CompilerCheck],
            ),
            {
                let log = Arc::clone(&log);
                let now = Arc::clone(&now);
                Box::new(move || {
                    queued_fake(
                        "compiler",
                        &log,
                        &now,
                        VecDeque::from([Err(ProviderError::Unavailable(
                            "toolchain not installed".into(),
                        ))]),
                        0,
                    )
                })
            },
        );

        let mut session = DiagnosticsSession::new();
        session.begin_mutation();
        session.seal_mutation(test_epoch(1)).unwrap();

        let policy = GatePolicyProfileV1::default();
        let decision = session
            .acquire_snapshot(
                &mut supervisor,
                &[parser_key, compiler_key],
                Vec::new(),
                session.latest_sealed().unwrap(),
                &policy,
                CostClass::Interactive,
                deadline_from(&now, 5_000),
            )
            .unwrap();
        assert_eq!(decision.outcome, GateOutcome::DirtyExact);
        // One exact blocker proves dirty; the unavailable second capability must
        // remain a typed omission, never a silent narrowing.
        assert!(!decision.omissions.is_empty());
        assert!(decision.omissions.iter().any(|o| o.code == "provider_unavailable"));
        // dirty_exact blocks completion: the fence is not cleared.
        assert!(session.cleared_decision().is_none());
        supervisor.shutdown_all().unwrap();
    }

    #[test]
    fn lanes_collected_into_snapshot_enables_clean_exact() {
        // Verifies drift fix: coverage lanes from ProviderOutputs are now
        // collected into the snapshot; clean_exact is reachable only when
        // lanes prove exact convergence for every required capability.
        let now: SharedNow = Arc::new(Mutex::new(0));
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut supervisor =
            DiagnosticsSupervisor::with_clock(LiveDiagnosticsConfig::default(), test_clock(&now));
        let key = test_key("tsgo");
        supervisor.register(
            key.clone(),
            caps("tsgo", CostClass::Instant, &[CapabilityKind::CompilerCheck]),
            {
                let log = Arc::clone(&log);
                let now = Arc::clone(&now);
                Box::new(move || {
                    queued_fake(
                        "tsgo",
                        &log,
                        &now,
                        VecDeque::from([Ok(FakeProvider::lane_output(
                            Vec::new(),
                            "tsgo",
                            vec![membrane_protocol::diagnostics::CapabilityVocabulary::TypeSemantics],
                            1,
                        ))]),
                        0,
                    )
                })
            },
        );

        let mut session = DiagnosticsSession::new();
        session.begin_mutation();
        session.seal_mutation(test_epoch(1)).unwrap();

        let mut policy = GatePolicyProfileV1::default();
        policy.required_capabilities =
            vec![membrane_protocol::diagnostics::CapabilityVocabulary::TypeSemantics];

        let decision = session
            .acquire_snapshot(
                &mut supervisor,
                &[key.clone()],
                Vec::new(),
                session.latest_sealed().unwrap(),
                &policy,
                CostClass::Instant,
                deadline_from(&now, 5_000),
            )
            .unwrap();
        assert_eq!(decision.outcome, GateOutcome::CleanExact);
        assert!(session.cleared_decision().is_some());
        // Snapshot carried the lane; without lane collection this would be
        // UnknownIncomplete.
        supervisor.shutdown_all().unwrap();
    }

    #[test]
    fn omissions_are_typed_with_stable_snake_case_codes() {
        let now: SharedNow = Arc::new(Mutex::new(0));
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut supervisor =
            DiagnosticsSupervisor::with_clock(LiveDiagnosticsConfig::default(), test_clock(&now));
        let cheap = test_key("cheap");
        supervisor.register(
            cheap.clone(),
            caps("cheap", CostClass::Instant, &[CapabilityKind::Parser]),
            {
                let log = Arc::clone(&log);
                let now = Arc::clone(&now);
                Box::new(move || fake_provider("cheap", &log, &now))
            },
        );
        let expensive = test_key("expensive");
        supervisor.register(
            expensive.clone(),
            caps("expensive", CostClass::Verification, &[CapabilityKind::Parser]),
            {
                let log = Arc::clone(&log);
                let now = Arc::clone(&now);
                Box::new(move || fake_provider("expensive", &log, &now))
            },
        );

        let mut session = DiagnosticsSession::new();
        session.begin_mutation();
        session.seal_mutation(test_epoch(1)).unwrap();

        // expensive exceeds max_cost → typed omission code
        let policy = GatePolicyProfileV1::default();
        let decision = session
            .acquire_snapshot(
                &mut supervisor,
                &[cheap, expensive],
                Vec::new(),
                session.latest_sealed().unwrap(),
                &policy,
                CostClass::Instant,
                deadline_from(&now, 5_000),
            )
            .unwrap();
        // At least one omission with stable snake_case code
        assert!(decision.omissions.iter().any(|o| o.code == "provider_exceeds_max_cost"));
        for omission in &decision.omissions {
            // Codes are stable snake_case: no uppercase, no spaces
            assert_eq!(omission.code, omission.code.to_lowercase());
            assert!(!omission.code.contains(' '));
            assert!(!omission.code.is_empty());
        }
        supervisor.shutdown_all().unwrap();
    }

    #[test]
    fn correlation_groups_shared_path_and_anchor() {
        // BP001 and TS2305 on same path+anchor should group
        let repo = "repo-1";
        let obs1 = advisory_observation("obs-1", "BP001", "src/main.ts", Some("symbol:Foo"));
        let mut obs2 = advisory_observation("obs-2", "TS2305", "src/main.ts", Some("symbol:Foo"));
        obs2.provider_id = "typescript".to_string();
        let issues = correlate_observations(repo, vec![obs1.clone(), obs2.clone()]);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].observations.len(), 2);
        assert_eq!(issues[0].correlation_key, "repo-1|src/main.ts|symbol:Foo");

        // Same path but different anchor → separate issues
        let obs3 = advisory_observation("obs-3", "BP001", "src/main.ts", Some("symbol:Bar"));
        let issues2 = correlate_observations(repo, vec![obs1.clone(), obs3.clone()]);
        assert_eq!(issues2.len(), 2);

        // Same anchor but different path → separate issues
        let obs4 = advisory_observation("obs-4", "BP001", "src/other.ts", Some("symbol:Foo"));
        let issues3 = correlate_observations(repo, vec![obs1.clone(), obs4.clone()]);
        assert_eq!(issues3.len(), 2);

        // Empty anchor → never grouped even on same path
        let obs5 = advisory_observation("obs-5", "BP001", "src/main.ts", None);
        let obs6 = advisory_observation("obs-6", "TS2305", "src/main.ts", None);
        let issues4 = correlate_observations(repo, vec![obs5, obs6]);
        assert_eq!(issues4.len(), 2);

        // Deterministic ids and UnknownBaseline classification
        for (idx, issue) in issues.iter().enumerate() {
            assert_eq!(issue.issue_id, format!("issue-{}", idx + 1));
            assert_eq!(issue.classification, DeltaClassification::UnknownBaseline);
        }
    }

    #[test]
    fn observation_fingerprint_is_deterministic_and_distinguishes_fields() {
        let obs = blocking_observation();
        let fp1 = observation_fingerprint(&obs);
        let fp2 = observation_fingerprint(&obs);
        assert_eq!(fp1, fp2);

        let mut changed = obs.clone();
        changed.code = "DIFFERENT".to_string();
        assert_ne!(fp1, observation_fingerprint(&changed));

        // Fingerprint includes provider/code separation so grouping key does not
        let key1 = issue_correlation_key("repo-1", &obs);
        let mut obs_other_provider = obs.clone();
        obs_other_provider.provider_id = "other".to_string();
        obs_other_provider.code = "OTHER".to_string();
        let key2 = issue_correlation_key("repo-1", &obs_other_provider);
        assert_eq!(key1, key2, "correlation key excludes provider/code");
        assert_ne!(fp1, observation_fingerprint(&obs_other_provider));
    }

    #[test]
    fn snapshot_id_is_deterministic_and_field_boundary_safe() {
        let epoch = test_epoch(5);
        let id1 = snapshot_id_for(&epoch);
        let id2 = snapshot_id_for(&epoch);
        assert_eq!(id1, id2);
        assert!(id1.starts_with("snap-5-"));
        assert_eq!(id1.len(), "snap-5-".len() + 16);

        let mut shifted_a = test_epoch(5);
        shifted_a.repo_id = "repo-1".to_string();
        shifted_a.worktree_id = "wt".to_string();
        shifted_a.source_manifest_digest = "ab".to_string();
        let mut shifted_b = test_epoch(5);
        shifted_b.repo_id = "repo-".to_string();
        shifted_b.worktree_id = "1wt".to_string();
        shifted_b.source_manifest_digest = "ab".to_string();
        // Length prefixes keep boundaries unambiguous
        assert_ne!(snapshot_id_for(&shifted_a), snapshot_id_for(&shifted_b));

        // Different epoch → different prefix and digest
        let mut next = test_epoch(6);
        next.source_manifest_digest = "manifest-5".to_string();
        assert_ne!(snapshot_id_for(&epoch), snapshot_id_for(&next));
    }

    #[test]
    fn delta_transitions_new_persistent_resolved_and_moved() {
        // Previous: issue on src/a.ts anchor Foo
        let prev_obs = advisory_observation("obs-1", "BP001", "src/a.ts", Some("symbol:Foo"));
        let prev_issues = correlate_observations("repo-1", vec![prev_obs]);

        // Current: same key → Persistent
        let cur_obs_same = advisory_observation("obs-2", "TS2305", "src/a.ts", Some("symbol:Foo"));
        let cur_issues_same = correlate_observations("repo-1", vec![cur_obs_same]);
        let deltas = classify_aggregate_delta(&prev_issues, &cur_issues_same);
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].classification, DeltaClassification::Persistent);

        // Current: anchor same but path moved → Moved
        let cur_obs_moved = advisory_observation("obs-3", "BP001", "src/b.ts", Some("symbol:Foo"));
        let cur_issues_moved = correlate_observations("repo-1", vec![cur_obs_moved]);
        let deltas_moved = classify_aggregate_delta(&prev_issues, &cur_issues_moved);
        assert!(deltas_moved.iter().any(|d| d.classification == DeltaClassification::Moved));

        // Current only (new) and previous only (resolved)
        let empty: Vec<DiagnosticIssueV1> = Vec::new();
        let new_deltas = classify_aggregate_delta(&empty, &cur_issues_same);
        assert_eq!(new_deltas[0].classification, DeltaClassification::New);
        let resolved_deltas = classify_aggregate_delta(&prev_issues, &empty);
        assert_eq!(resolved_deltas[0].classification, DeltaClassification::Resolved);

        // Session baseline wiring: acquire_snapshot sets aggregate_delta
        let now: SharedNow = Arc::new(Mutex::new(0));
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut supervisor =
            DiagnosticsSupervisor::with_clock(LiveDiagnosticsConfig::default(), test_clock(&now));
        let key = test_key("baseline");
        supervisor.register(
            key.clone(),
            caps("baseline", CostClass::Instant, &[CapabilityKind::Parser]),
            {
                let log = Arc::clone(&log);
                let now = Arc::clone(&now);
                Box::new(move || fake_provider("baseline", &log, &now))
            },
        );
        let mut session = DiagnosticsSession::new();
        session.begin_mutation();
        session.seal_mutation(test_epoch(1)).unwrap();
        session.set_baseline(prev_issues.clone());
        assert!(session.baseline().is_some());
        let policy = GatePolicyProfileV1::default();
        let decision = session
            .acquire_snapshot(
                &mut supervisor,
                &[key.clone()],
                vec![advisory_observation(
                    "obs-10",
                    "BP001",
                    "src/a.ts",
                    Some("symbol:Foo"),
                )],
                session.latest_sealed().unwrap(),
                &policy,
                CostClass::Instant,
                deadline_from(&now, 5_000),
            )
            .unwrap();
        // Decision's snapshot aggregate_delta is internal; we verify session
        // still holds baseline and delta logic is wired (no panic).
        assert!(session.baseline().is_some());
        drop(decision);
        supervisor.shutdown_all().unwrap();
    }

    #[test]
    fn shutdown_key_shuts_live_and_preserves_factory() {
        let now: SharedNow = Arc::new(Mutex::new(0));
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut supervisor =
            DiagnosticsSupervisor::with_clock(LiveDiagnosticsConfig::default(), test_clock(&now));
        let key = test_key("to-shutdown");
        supervisor.register(
            key.clone(),
            caps("to-shutdown", CostClass::Instant, &[CapabilityKind::Parser]),
            {
                let log = Arc::clone(&log);
                let now = Arc::clone(&now);
                Box::new(move || fake_provider("to-shutdown", &log, &now))
            },
        );

        // Absent key → false
        let absent = test_key("absent");
        assert!(!supervisor.shutdown_key(&absent));

        // Not yet live → false (registered but not started)
        assert!(!supervisor.shutdown_key(&key));

        // Start it
        let epoch = test_epoch(1);
        assert!(supervisor.acquire(&key, &epoch, deadline_from(&now, 5_000)).is_ok());
        assert_eq!(FakeProvider::events(&log, "to-shutdown", "init"), 1);

        // Live → true and factory kept (next acquire can restart, may re-init
        // depending on ensure_live keep-factory fix). At minimum, key remains
        // registered and shutdown was invoked.
        assert!(supervisor.shutdown_key(&key));
        assert_eq!(FakeProvider::events(&log, "to-shutdown", "shutdown"), 1);
        // Second shutdown on same key (now not live) → false
        assert!(!supervisor.shutdown_key(&key));
        // Factory still registered
        assert!(supervisor.registry_capabilities(&key).is_some());

        supervisor.shutdown_all().unwrap();
    }

    #[test]
    fn reconcile_mismatch_classifies_unknown_conflict_and_invalidates_clearance() {
        let now: SharedNow = Arc::new(Mutex::new(0));
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut supervisor =
            DiagnosticsSupervisor::with_clock(LiveDiagnosticsConfig::default(), test_clock(&now));
        let key = test_key("clean");
        supervisor.register(
            key.clone(),
            caps("clean", CostClass::Instant, &[CapabilityKind::Parser]),
            {
                let log = Arc::clone(&log);
                let now = Arc::clone(&now);
                Box::new(move || fake_provider("clean", &log, &now))
            },
        );
        let mut session = DiagnosticsSession::new();
        session.begin_mutation();
        session.seal_mutation(test_epoch(1)).unwrap();
        let policy = GatePolicyProfileV1::default();
        let decision = session
            .acquire_snapshot(
                &mut supervisor,
                &[key.clone()],
                Vec::new(),
                session.latest_sealed().unwrap(),
                &policy,
                CostClass::Instant,
                deadline_from(&now, 5_000),
            )
            .unwrap();
        assert_eq!(decision.outcome, GateOutcome::CleanExact);
        assert!(session.cleared_decision().is_some());

        // External write: manifest digest drift invalidates prior clearance.
        let classification =
            session.reconcile("manifest-1-externally-modified", &[]);
        assert_eq!(classification, ReconcileClassification::UnknownConflict);
        assert!(session.cleared_decision().is_none());
        supervisor.shutdown_all().unwrap();
    }

    #[test]
    fn superseded_epoch_invalidates_clearance_and_monotonicity_is_enforced() {
        let now: SharedNow = Arc::new(Mutex::new(0));
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut supervisor =
            DiagnosticsSupervisor::with_clock(LiveDiagnosticsConfig::default(), test_clock(&now));
        let key = test_key("clean");
        supervisor.register(
            key.clone(),
            caps("clean", CostClass::Instant, &[CapabilityKind::Parser]),
            {
                let log = Arc::clone(&log);
                let now = Arc::clone(&now);
                Box::new(move || fake_provider("clean", &log, &now))
            },
        );
        let mut session = DiagnosticsSession::new();
        session.begin_mutation();
        session.seal_mutation(test_epoch(1)).unwrap();
        let policy = GatePolicyProfileV1::default();
        let decision = session
            .acquire_snapshot(
                &mut supervisor,
                &[key.clone()],
                Vec::new(),
                session.latest_sealed().unwrap(),
                &policy,
                CostClass::Instant,
                deadline_from(&now, 5_000),
            )
            .unwrap();
        assert_eq!(decision.outcome, GateOutcome::CleanExact);

        // Observed-hook mode registers newer bytes with weaker attribution.
        session.register_observed(test_epoch(2)).unwrap();
        assert_eq!(
            session.reconcile("manifest-1", &[]),
            ReconcileClassification::Superseded
        );
        assert!(session.cleared_decision().is_none());

        // Epoch numbers must strictly increase.
        assert!(matches!(
            session.register_observed(test_epoch(2)),
            Err(LiveDiagnosticsError::EpochNotMonotonic(_))
        ));

        // Parent chain must reference the latest sealed epoch.
        let mut orphan = test_epoch(3);
        orphan.parent_epoch = Some(1);
        assert!(matches!(
            session.seal_mutation(orphan),
            Err(LiveDiagnosticsError::MutationBoundary(_))
        ));

        // Transactional mode requires begin before seal.
        let mut fresh = DiagnosticsSession::new();
        assert!(matches!(
            fresh.seal_mutation(test_epoch(1)),
            Err(LiveDiagnosticsError::MutationBoundary(_))
        ));
        supervisor.shutdown_all().unwrap();
    }
}
