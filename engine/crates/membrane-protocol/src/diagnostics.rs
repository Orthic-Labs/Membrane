//! Live Diagnostics operational contracts — mutation-bound diagnostic evidence
//! and Semantic Edit Fence gate decisions.
//!
//! Implements the wire shapes of
//! `docs/architecture/live-diagnostics.md` (§4 identity
//! model, §5 contracts, §8 coverage/convergence, §9 correlation). These are
//! *operational* schemas: they never mutate Membrane's frozen public V1 context
//! shapes, the Membrane planner remains the sole gate-policy owner, and
//! [`evaluate_gate`] is the pure deterministic evaluator over planner-owned
//! policy — it invents nothing and performs no I/O.
//!
//! Serde conventions follow the rest of the crate: `camelCase` struct fields,
//! `snake_case` closed enum vocabularies, `#[serde(default)]` on optional and
//! repeated fields.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Schema version stamped on every [`WorkspaceEpochV1`].
pub const WORKSPACE_EPOCH_SCHEMA_VERSION: &str = "workspace-epoch.v1";
/// Schema version stamped on every [`DiagnosticEvidenceSnapshotV1`].
pub const DIAGNOSTIC_EVIDENCE_SNAPSHOT_SCHEMA_VERSION: &str = "diagnostics-evidence-snapshot.v1";
/// Schema version stamped on every [`DiagnosticGateDecisionV1`].
pub const DIAGNOSTIC_GATE_DECISION_SCHEMA_VERSION: &str = "diagnostics-gate-decision.v1";

// ============================================================================
// Shared value types
// ============================================================================

/// Exact content hash of one changed file within a workspace epoch (§4).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangedFileHashV1 {
    /// Repo-relative path of the changed file.
    pub path: String,
    /// Content digest of the exact resulting bytes.
    pub hash: String,
}

/// One typed, machine-readable degradation recorded instead of silent data loss.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypedOmission {
    /// Stable omission code (e.g. `provider_unavailable`).
    pub code: String,
    /// Human-readable detail; never load-bearing for decisions.
    pub detail: String,
}

/// Half-open normalized source range; lines and columns are 1-based.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRange {
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

// ============================================================================
// Closed vocabularies
// ============================================================================

/// Economic class of one diagnostic producer or acquisition request (§6).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostClass {
    #[default]
    Instant,
    Interactive,
    Verification,
    Build,
    Test,
}

/// How a workspace epoch came to exist (§4.1 host modes).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceEpochOrigin {
    #[default]
    Transactional,
    ObservedHook,
    Reconciliation,
}

/// Semantic capability a coverage obligation may demand (§8).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityVocabulary {
    #[default]
    Syntax,
    RepositoryModuleResolution,
    ImportExportBinding,
    NameResolution,
    TypeSemantics,
    ConfiguredStaticPolicy,
    CompilerProjectSemantics,
    GeneratedSourceAwareness,
}

impl CapabilityVocabulary {
    /// Stable lowercase-snake name used in gate reason codes.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Syntax => "syntax",
            Self::RepositoryModuleResolution => "repository_module_resolution",
            Self::ImportExportBinding => "import_export_binding",
            Self::NameResolution => "name_resolution",
            Self::TypeSemantics => "type_semantics",
            Self::ConfiguredStaticPolicy => "configured_static_policy",
            Self::CompilerProjectSemantics => "compiler_project_semantics",
            Self::GeneratedSourceAwareness => "generated_source_awareness",
        }
    }
}

/// How exactly a lane's convergence was proven (§8).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExactnessRequirement {
    Exact,
    #[default]
    Advisory,
}

/// Satisfaction state of one coverage obligation.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObligationState {
    #[default]
    Unsatisfied,
    SatisfiedExact,
    SatisfiedAdvisory,
    Unavailable,
    TimedOut,
    Unsupported,
}

/// Authority class of the producer that emitted an observation (§5.1).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceClass {
    #[default]
    Parser,
    RepositoryFinding,
    NativeLanguageService,
    StaticAnalyzer,
    CompilerCheck,
}

/// Policy-facing hint for how an observation should gate (never enforcement).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeverityHint {
    Blocking,
    #[default]
    Advisory,
}

/// Aggregate delta classification across exact workspace epochs (§8.1).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeltaClassification {
    New,
    Persistent,
    Resolved,
    Moved,
    Changed,
    #[default]
    UnknownBaseline,
}

/// Qualified convergence classes; only the `*_exact` trio can clear the fence.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConvergenceClass {
    PullExact,
    PushVersionedExact,
    SnapshotCheckerExact,
    PushUnversionedAdvisory,
    #[default]
    Unsupported,
}

/// Completion state of one coverage lane (§8).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaneState {
    Complete,
    Partial,
    Unavailable,
    TimedOut,
    #[default]
    Unsupported,
}

/// Freshness of the Blueprint generation bound to a snapshot (§4).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlueprintFreshness {
    Current,
    Stale,
    #[default]
    Unknown,
}

/// Closed gate outcome vocabulary (§5.2). There is deliberately no
/// `clean_partial`, `clean_stale`, or `probably_clean`.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateOutcome {
    CleanExact,
    DirtyExact,
    #[default]
    UnknownIncomplete,
    UnknownUnavailable,
    UnknownTimedOut,
    UnknownConflict,
    Superseded,
}

// ============================================================================
// Envelopes
// ============================================================================

/// Monotonic operational identity for exact current worktree bytes in one
/// diagnostics session (§4). Derived from exact host bytes; never competes with
/// Blueprint source identity.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceEpochV1 {
    pub schema_version: String,
    pub repo_id: String,
    pub worktree_id: String,
    pub epoch: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_epoch: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mutation_id: Option<String>,
    pub source_manifest_digest: String,
    #[serde(default)]
    pub changed_paths: Vec<String>,
    #[serde(default)]
    pub changed_file_hashes: Vec<ChangedFileHashV1>,
    pub project_config_digest: String,
    pub toolchain_digest: String,
    pub sandbox_policy_digest: String,
    pub origin: WorkspaceEpochOrigin,
}

/// Paths a coverage obligation must be satisfied over (§8).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequiredScope {
    #[serde(default)]
    pub paths: Vec<String>,
}

/// One planner-derived capability requirement supplied with a snapshot (§8).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverageObligationV1 {
    pub capability: CapabilityVocabulary,
    pub language_dialect: String,
    pub project_identity: String,
    pub required_scope: RequiredScope,
    pub exactness_requirement: ExactnessRequirement,
    #[serde(default)]
    pub acceptable_provider_alternatives: Vec<String>,
    pub maximum_cost: CostClass,
    pub state: ObligationState,
    #[serde(default)]
    pub omissions: Vec<TypedOmission>,
}

/// One normalized diagnostic observation from one producer (§5.1, §9).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservationV1 {
    pub observation_id: String,
    pub provider_id: String,
    pub provider_version: String,
    pub code: String,
    pub path: String,
    pub range: SourceRange,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_anchor: Option<String>,
    pub source_class: SourceClass,
    pub cost_class: CostClass,
    pub severity_hint: SeverityHint,
}

/// Confidently equivalent observations grouped under one repair-packet issue
/// (§9). Uncertain matches remain separate issues.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticIssueV1 {
    pub issue_id: String,
    pub correlation_key: String,
    #[serde(default)]
    pub observations: Vec<ObservationV1>,
    pub classification: DeltaClassification,
}

/// One producer's reported coverage over one invocation (§8).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverageLaneV1 {
    pub provider_id: String,
    #[serde(default)]
    pub scope: Vec<String>,
    #[serde(default)]
    pub capabilities_covered: Vec<CapabilityVocabulary>,
    pub convergence_class: ConvergenceClass,
    pub bound_workspace_epoch: u64,
    pub state: LaneState,
    #[serde(default)]
    pub omissions: Vec<TypedOmission>,
}

/// Blueprint's Tier-0 finding delta carried without recomputation (§8.1).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlueprintDeltaV1 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_generation: Option<String>,
    #[serde(default)]
    pub findings_delta: Vec<DiagnosticIssueV1>,
}

/// Aggregate observation/issue delta across exact workspace epochs (§8.1).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregateDeltaV1 {
    #[serde(default)]
    pub issues: Vec<AggregateIssueDelta>,
}

/// Per-issue aggregate delta entry.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregateIssueDelta {
    pub issue_id: String,
    pub classification: DeltaClassification,
}

/// Raw mutation-bound diagnostic evidence (§5.1). Producers fill this; the gate
/// evaluator consumes it without mutating it.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticEvidenceSnapshotV1 {
    pub schema_version: String,
    pub snapshot_id: String,
    pub repo_id: String,
    pub worktree_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blueprint_generation: Option<String>,
    pub blueprint_freshness: BlueprintFreshness,
    /// Full workspace epoch envelope, embedded whole (§4).
    pub workspace_epoch: WorkspaceEpochV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mutation_id: Option<String>,
    pub request_max_cost: CostClass,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub absolute_deadline_ms: Option<u64>,
    #[serde(default)]
    pub coverage_obligations: Vec<CoverageObligationV1>,
    #[serde(default)]
    pub observations: Vec<ObservationV1>,
    #[serde(default)]
    pub issues: Vec<DiagnosticIssueV1>,
    #[serde(default)]
    pub coverage_lanes: Vec<CoverageLaneV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blueprint_delta: Option<BlueprintDeltaV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aggregate_delta: Option<AggregateDeltaV1>,
    #[serde(default)]
    pub omissions: Vec<TypedOmission>,
    pub produced_at_ms: u64,
}

/// Versioned planner-owned gate policy profile (§5.2). The evaluator never
/// invents policy; it only applies this profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatePolicyProfileV1 {
    pub profile_name: String,
    pub policy_version: String,
    pub policy_digest: String,
    #[serde(default)]
    pub blocking_codes: Vec<String>,
    #[serde(default)]
    pub required_capabilities: Vec<CapabilityVocabulary>,
}

impl Default for GatePolicyProfileV1 {
    fn default() -> Self {
        Self {
            profile_name: "default".to_string(),
            policy_version: String::new(),
            policy_digest: String::new(),
            blocking_codes: Vec::new(),
            required_capabilities: Vec::new(),
        }
    }
}

/// Deterministic result of evaluating planner policy against exact evidence
/// (§5.2). Hosts enforce this decision; presentation cannot alter it.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticGateDecisionV1 {
    pub schema_version: String,
    pub snapshot_id: String,
    pub policy_profile: String,
    pub policy_version: String,
    pub policy_digest: String,
    pub outcome: GateOutcome,
    #[serde(default)]
    pub blocking_issue_ids: Vec<String>,
    #[serde(default)]
    pub required_obligations: Vec<CoverageObligationV1>,
    #[serde(default)]
    pub reason_codes: Vec<String>,
    #[serde(default)]
    pub omissions: Vec<TypedOmission>,
}

// ============================================================================
// Gate evaluation
// ============================================================================

/// Purely evaluate planner-owned `policy` against `snapshot` evidence bound to
/// `expected_epoch`.
///
/// Precedence follows design §5.3, whose exactness is deliberately **asymmetric**:
/// one current, exact blocking observation proves `dirty_exact` even when other
/// required capabilities degraded, while proving `clean_exact` requires complete
/// exact satisfaction of *every* required capability. Everything else is unknown;
/// there is deliberately no `clean_partial` / `clean_stale`.
///
/// 1. Identity, supersession, and manifest conflict: cross-repo/worktree
///    evidence is `unknown_conflict`; a snapshot whose epoch is not the expected
///    epoch (older bytes, or newer/diverged ancestry via the derivable
///    `parent_epoch` hop) is `superseded`; equal epochs with differing manifest
///    digests or file hashes are `unknown_conflict`.
/// 2. Any observation with `severity_hint == Blocking`, or whose code is listed
///    in `policy.blocking_codes`, proves `dirty_exact`.
/// 3. Otherwise `clean_exact` requires, for every required capability, some lane
///    with an exact convergence class (`pull_exact`, `push_versioned_exact`,
///    `snapshot_checker_exact`), `complete` state, and the snapshot's own bound
///    epoch — and no obligation left unsatisfied/unavailable/timed-out/
///    unsupported. Advisory convergence and quiet windows never clear the fence.
/// 4. Otherwise the applicable `unknown_*` outcome with stable snake_case
///    reason codes.
fn lane_scope_covers_required(lane_scope: &[String], required: &[String]) -> bool {
    if required.is_empty() {
        return true;
    }
    if lane_scope.is_empty() {
        return false;
    }
    let set: std::collections::HashSet<&str> = lane_scope.iter().map(|s| s.as_str()).collect();
    required.iter().all(|p| set.contains(p.as_str()))
}

pub fn evaluate_gate(
    snapshot: &DiagnosticEvidenceSnapshotV1,
    expected_epoch: &WorkspaceEpochV1,
    policy: &GatePolicyProfileV1,
) -> DiagnosticGateDecisionV1 {
    let mut decision = DiagnosticGateDecisionV1 {
        schema_version: DIAGNOSTIC_GATE_DECISION_SCHEMA_VERSION.to_string(),
        snapshot_id: snapshot.snapshot_id.clone(),
        policy_profile: policy.profile_name.clone(),
        policy_version: policy.policy_version.clone(),
        policy_digest: policy.policy_digest.clone(),
        outcome: GateOutcome::UnknownIncomplete,
        blocking_issue_ids: Vec::new(),
        required_obligations: snapshot.coverage_obligations.clone(),
        reason_codes: Vec::new(),
        omissions: snapshot.omissions.clone(),
    };
    let finish = |mut decision: DiagnosticGateDecisionV1,
                  outcome,
                  reasons: Vec<String>|
     -> DiagnosticGateDecisionV1 {
        decision.outcome = outcome;
        decision.reason_codes = reasons;
        decision
    };

    // 1. Identity, supersession, and manifest conflict (§4, §5.3 step 1).
    let observed = &snapshot.workspace_epoch;
    if observed.repo_id != expected_epoch.repo_id
        || observed.worktree_id != expected_epoch.worktree_id
    {
        return finish(
            decision,
            GateOutcome::UnknownConflict,
            vec!["identity_mismatch".to_string()],
        );
    }
    if observed.epoch != expected_epoch.epoch {
        // Older evidence is stale. Newer evidence cannot clear an older
        // expectation either: the fence binds to exact bytes, so diverged or
        // advanced lineage (derivable through `parent_epoch`) still supersedes
        // the expectation and the host must await/re-evaluate the newest epoch.
        return finish(
            decision,
            GateOutcome::Superseded,
            vec!["superseded_epoch".to_string()],
        );
    }
    if observed.source_manifest_digest != expected_epoch.source_manifest_digest
        || observed.changed_file_hashes != expected_epoch.changed_file_hashes
    {
        return finish(
            decision,
            GateOutcome::UnknownConflict,
            vec!["manifest_conflict".to_string()],
        );
    }

    // 2. Exact blockers prove dirty regardless of any degraded lane
    //    (§5.3 step 2: dirty state proven for the bound bytes).
    let mut observed_blocking_ids = HashSet::new();
    let blocking_observation_ids: Vec<&str> = snapshot
        .observations
        .iter()
        .filter(|observation| {
            (observation.severity_hint == SeverityHint::Blocking
                || policy
                    .blocking_codes
                    .iter()
                    .any(|code| code == &observation.code))
                && observed_blocking_ids.insert(observation.observation_id.as_str())
        })
        .map(|observation| observation.observation_id.as_str())
        .collect();
    if !blocking_observation_ids.is_empty() {
        let mut emitted_issue_ids = HashSet::new();
        let mut represented_observation_ids = HashSet::new();
        let mut blocking_issue_ids = Vec::new();
        for issue in &snapshot.issues {
            let represented_for_issue: Vec<String> = issue
                .observations
                .iter()
                .filter(|observation| {
                    blocking_observation_ids.contains(&observation.observation_id.as_str())
                })
                .map(|observation| observation.observation_id.clone())
                .collect();
            if !represented_for_issue.is_empty() && emitted_issue_ids.insert(issue.issue_id.clone())
            {
                blocking_issue_ids.push(issue.issue_id.clone());
                represented_observation_ids.extend(represented_for_issue);
            }
        }
        // Observations not grouped into any issue still count; list them by id.
        for observation_id in blocking_observation_ids {
            if !represented_observation_ids.contains(observation_id) {
                blocking_issue_ids.push(observation_id.to_string());
            }
        }
        decision.blocking_issue_ids = blocking_issue_ids;
        return finish(
            decision,
            GateOutcome::DirtyExact,
            vec!["exact_blocker".to_string()],
        );
    }

    // Guard: empty policy + empty obligations + empty lanes must never be clean (rejected shape: empty arrays as clean).
    if policy.required_capabilities.is_empty() && snapshot.coverage_obligations.is_empty() {
        if snapshot.coverage_lanes.is_empty() {
            return finish(
                decision,
                GateOutcome::UnknownIncomplete,
                vec!["empty_coverage_no_evidence".to_string()],
            );
        }
        return finish(
            decision,
            GateOutcome::UnknownIncomplete,
            vec!["no_required_capabilities".to_string()],
        );
    }

    // 3./4. Exact coverage of every required capability decides clean vs unknown
    //       (§5.3 steps 3–4, §8).
    let bound_epoch = snapshot.workspace_epoch.epoch;
    let mut any_capability_timed_out = false;
    let mut any_capability_unavailable = false;
    let mut every_capability_covered_exactly = true;
    let mut uncovered_capabilities: Vec<&'static str> = Vec::new();
    let mut timed_out_providers: Vec<String> = Vec::new();
    let mut partial_providers: Vec<String> = Vec::new();

    for capability in &policy.required_capabilities {
        let mut complete = false;
        let mut timed_out = false;
        let mut unavailable = false;
        let mut candidate_count = 0usize;
        let required_scope: Vec<String> = snapshot
            .coverage_obligations
            .iter()
            .find(|o| &o.capability == capability)
            .map(|o| o.required_scope.paths.clone())
            .unwrap_or_else(|| {
                let mut set = std::collections::BTreeSet::new();
                for p in &snapshot.workspace_epoch.changed_paths {
                    set.insert(p.clone());
                }
                for h in &snapshot.workspace_epoch.changed_file_hashes {
                    set.insert(h.path.clone());
                }
                set.into_iter().collect()
            });
        for lane in &snapshot.coverage_lanes {
            let exact_convergence = matches!(
                lane.convergence_class,
                ConvergenceClass::PullExact
                    | ConvergenceClass::PushVersionedExact
                    | ConvergenceClass::SnapshotCheckerExact
            );
            if !lane.capabilities_covered.contains(capability)
                || !exact_convergence
                || lane.bound_workspace_epoch != bound_epoch
                || !lane_scope_covers_required(&lane.scope, &required_scope)
            {
                continue;
            }
            candidate_count += 1;
            match lane.state {
                LaneState::Complete => complete = true,
                LaneState::TimedOut => {
                    timed_out = true;
                    timed_out_providers.push(lane.provider_id.clone());
                }
                LaneState::Unavailable | LaneState::Unsupported => unavailable = true,
                LaneState::Partial => partial_providers.push(lane.provider_id.clone()),
            }
        }
        if complete {
            continue;
        }
        every_capability_covered_exactly = false;
        if timed_out {
            any_capability_timed_out = true;
        } else if unavailable {
            any_capability_unavailable = true;
            uncovered_capabilities.push(capability.as_str());
        } else if candidate_count == 0 {
            uncovered_capabilities.push(capability.as_str());
        }
    }

    let mut unsatisfied_obligation_capabilities: Vec<&'static str> = Vec::new();
    for obligation in &snapshot.coverage_obligations {
        if matches!(
            obligation.state,
            ObligationState::Unsatisfied
                | ObligationState::Unavailable
                | ObligationState::TimedOut
                | ObligationState::Unsupported
        ) {
            unsatisfied_obligation_capabilities.push(obligation.capability.as_str());
        }
    }

    if every_capability_covered_exactly && unsatisfied_obligation_capabilities.is_empty() {
        return finish(decision, GateOutcome::CleanExact, Vec::new());
    }

    let mut seen = HashSet::new();
    let mut reasons = Vec::new();
    let mut push_reason = |code: String, reasons: &mut Vec<String>| {
        if seen.insert(code.clone()) {
            reasons.push(code);
        }
    };
    for capability in &uncovered_capabilities {
        push_reason(format!("capability_uncovered:{capability}"), &mut reasons);
    }
    for provider in &timed_out_providers {
        push_reason(format!("lane_timed_out:{provider}"), &mut reasons);
    }
    for capability in &unsatisfied_obligation_capabilities {
        push_reason(format!("obligation_unsatisfied:{capability}"), &mut reasons);
    }
    for provider in &partial_providers {
        push_reason(format!("partial_lane:{provider}"), &mut reasons);
    }

    let outcome = if any_capability_timed_out {
        GateOutcome::UnknownTimedOut
    } else if any_capability_unavailable {
        GateOutcome::UnknownUnavailable
    } else {
        GateOutcome::UnknownIncomplete
    };
    finish(decision, outcome, reasons)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn epoch(at: u64) -> WorkspaceEpochV1 {
        WorkspaceEpochV1 {
            schema_version: WORKSPACE_EPOCH_SCHEMA_VERSION.to_string(),
            repo_id: "repo-1".to_string(),
            worktree_id: "wt-1".to_string(),
            epoch: at,
            parent_epoch: Some(at.wrapping_sub(1)),
            mutation_id: Some("mutation-1".to_string()),
            source_manifest_digest: "sha256:manifest".to_string(),
            changed_paths: vec!["src/main.ts".to_string()],
            changed_file_hashes: vec![ChangedFileHashV1 {
                path: "src/main.ts".to_string(),
                hash: "sha256:main".to_string(),
            }],
            project_config_digest: "sha256:config".to_string(),
            toolchain_digest: "sha256:toolchain".to_string(),
            sandbox_policy_digest: "sha256:sandbox".to_string(),
            origin: WorkspaceEpochOrigin::Transactional,
        }
    }

    fn snapshot_at(workspace_epoch: WorkspaceEpochV1) -> DiagnosticEvidenceSnapshotV1 {
        DiagnosticEvidenceSnapshotV1 {
            schema_version: DIAGNOSTIC_EVIDENCE_SNAPSHOT_SCHEMA_VERSION.to_string(),
            snapshot_id: "snap-1".to_string(),
            repo_id: workspace_epoch.repo_id.clone(),
            worktree_id: workspace_epoch.worktree_id.clone(),
            blueprint_freshness: BlueprintFreshness::Current,
            workspace_epoch,
            request_max_cost: CostClass::Interactive,
            produced_at_ms: 1_000,
            ..Default::default()
        }
    }

    fn lane(
        provider_id: &str,
        capabilities: &[CapabilityVocabulary],
        convergence_class: ConvergenceClass,
        state: LaneState,
        bound_workspace_epoch: u64,
    ) -> CoverageLaneV1 {
        CoverageLaneV1 {
            provider_id: provider_id.to_string(),
            scope: vec!["src/main.ts".to_string()],
            capabilities_covered: capabilities.to_vec(),
            convergence_class,
            bound_workspace_epoch,
            state,
            omissions: Vec::new(),
        }
    }

    fn policy(required: &[CapabilityVocabulary]) -> GatePolicyProfileV1 {
        GatePolicyProfileV1 {
            profile_name: "changed-files-zero".to_string(),
            policy_version: "1".to_string(),
            policy_digest: "sha256:policy".to_string(),
            blocking_codes: Vec::new(),
            required_capabilities: required.to_vec(),
        }
    }

    fn observation(observation_id: &str, severity_hint: SeverityHint, code: &str) -> ObservationV1 {
        ObservationV1 {
            observation_id: observation_id.to_string(),
            provider_id: "typescript".to_string(),
            provider_version: "5.6".to_string(),
            code: code.to_string(),
            path: "src/main.ts".to_string(),
            range: SourceRange {
                start_line: 3,
                start_column: 1,
                end_line: 3,
                end_column: 20,
            },
            message: "broken".to_string(),
            semantic_anchor: Some("symbol:RunPolicy".to_string()),
            source_class: SourceClass::NativeLanguageService,
            cost_class: CostClass::Interactive,
            severity_hint,
        }
    }

    #[test]
    fn workspace_epoch_round_trips_with_camel_case_keys() {
        let value = epoch(4);
        let encoded = serde_json::to_value(&value).unwrap();
        assert_eq!(encoded["schemaVersion"], json!("workspace-epoch.v1"));
        assert_eq!(encoded["sourceManifestDigest"], json!("sha256:manifest"));
        assert_eq!(
            encoded["changedFileHashes"][0]["path"],
            json!("src/main.ts")
        );
        assert_eq!(encoded["parentEpoch"], json!(3));
        assert_eq!(encoded["origin"], json!("transactional"));
        let decoded: WorkspaceEpochV1 = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn snapshot_round_trips_with_nested_camel_case_keys() {
        let mut value = snapshot_at(epoch(5));
        value.coverage_obligations.push(CoverageObligationV1 {
            capability: CapabilityVocabulary::TypeSemantics,
            language_dialect: "typescript".to_string(),
            project_identity: "tsconfig://engine".to_string(),
            required_scope: RequiredScope {
                paths: vec!["src/main.ts".to_string()],
            },
            exactness_requirement: ExactnessRequirement::Exact,
            acceptable_provider_alternatives: vec!["typescript".to_string()],
            maximum_cost: CostClass::Interactive,
            state: ObligationState::SatisfiedExact,
            omissions: Vec::new(),
        });
        value.blueprint_delta = Some(BlueprintDeltaV1::default());
        let encoded = serde_json::to_value(&value).unwrap();
        assert_eq!(encoded["workspaceEpoch"]["epoch"], json!(5));
        assert_eq!(
            encoded["workspaceEpoch"]["changedFileHashes"][0]["hash"],
            json!("sha256:main")
        );
        assert_eq!(encoded["requestMaxCost"], json!("interactive"));
        assert_eq!(
            encoded["coverageObligations"][0]["languageDialect"],
            json!("typescript")
        );
        assert_eq!(encoded["blueprintFreshness"], json!("current"));
        let decoded: DiagnosticEvidenceSnapshotV1 = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn gate_decision_round_trips_with_camel_case_keys() {
        let mut snapshot = snapshot_at(epoch(5));
        snapshot.issues.push(DiagnosticIssueV1 {
            issue_id: "issue-1".to_string(),
            correlation_key: "repo-1|src/main.ts|symbol:RunPolicy".to_string(),
            observations: vec![observation("obs-1", SeverityHint::Blocking, "TS2305")],
            classification: DeltaClassification::New,
        });
        snapshot
            .observations
            .push(observation("obs-1", SeverityHint::Blocking, "TS2305"));
        let value = evaluate_gate(&snapshot, &epoch(5), &policy(&[]));
        let encoded = serde_json::to_value(&value).unwrap();
        assert_eq!(
            encoded["schemaVersion"],
            json!("diagnostics-gate-decision.v1")
        );
        assert_eq!(encoded["blockingIssueIds"], json!(["issue-1"]));
        assert_eq!(encoded["reasonCodes"], json!(["exact_blocker"]));
        assert_eq!(encoded["policyDigest"], json!("sha256:policy"));
        assert_eq!(encoded["outcome"], json!("dirty_exact"));
        let decoded: DiagnosticGateDecisionV1 = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn gate_outcome_vocabulary_is_closed_snake_case() {
        let outcomes = [
            GateOutcome::CleanExact,
            GateOutcome::DirtyExact,
            GateOutcome::UnknownIncomplete,
            GateOutcome::UnknownUnavailable,
            GateOutcome::UnknownTimedOut,
            GateOutcome::UnknownConflict,
            GateOutcome::Superseded,
        ];
        let encoded = serde_json::to_value(outcomes).unwrap();
        assert_eq!(
            encoded,
            json!([
                "clean_exact",
                "dirty_exact",
                "unknown_incomplete",
                "unknown_unavailable",
                "unknown_timed_out",
                "unknown_conflict",
                "superseded"
            ])
        );
        let decoded: Vec<GateOutcome> = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded, outcomes);
    }

    #[test]
    fn supporting_vocabularies_serialize_snake_case() {
        assert_eq!(
            serde_json::to_value([
                CapabilityVocabulary::Syntax,
                CapabilityVocabulary::RepositoryModuleResolution,
                CapabilityVocabulary::ImportExportBinding,
                CapabilityVocabulary::NameResolution,
                CapabilityVocabulary::TypeSemantics,
                CapabilityVocabulary::ConfiguredStaticPolicy,
                CapabilityVocabulary::CompilerProjectSemantics,
                CapabilityVocabulary::GeneratedSourceAwareness,
            ])
            .unwrap(),
            json!([
                "syntax",
                "repository_module_resolution",
                "import_export_binding",
                "name_resolution",
                "type_semantics",
                "configured_static_policy",
                "compiler_project_semantics",
                "generated_source_awareness"
            ])
        );
        assert_eq!(
            serde_json::to_value([
                ConvergenceClass::PullExact,
                ConvergenceClass::PushVersionedExact,
                ConvergenceClass::SnapshotCheckerExact,
                ConvergenceClass::PushUnversionedAdvisory,
                ConvergenceClass::Unsupported,
            ])
            .unwrap(),
            json!([
                "pull_exact",
                "push_versioned_exact",
                "snapshot_checker_exact",
                "push_unversioned_advisory",
                "unsupported"
            ])
        );
        assert_eq!(
            serde_json::to_value([
                LaneState::Complete,
                LaneState::Partial,
                LaneState::Unavailable,
                LaneState::TimedOut,
                LaneState::Unsupported,
            ])
            .unwrap(),
            json!([
                "complete",
                "partial",
                "unavailable",
                "timed_out",
                "unsupported"
            ])
        );
        assert_eq!(
            serde_json::to_value(ObligationState::SatisfiedAdvisory).unwrap(),
            json!("satisfied_advisory")
        );
        assert_eq!(
            serde_json::to_value(DeltaClassification::UnknownBaseline).unwrap(),
            json!("unknown_baseline")
        );
        assert_eq!(
            serde_json::to_value(BlueprintFreshness::Current).unwrap(),
            json!("current")
        );
        assert_eq!(
            serde_json::to_value(WorkspaceEpochOrigin::ObservedHook).unwrap(),
            json!("observed_hook")
        );
    }

    #[test]
    fn defaults_are_sane_for_test_construction() {
        let lane_default = CoverageLaneV1::default();
        assert_eq!(lane_default.bound_workspace_epoch, 0);
        assert_eq!(lane_default.state, LaneState::Unsupported);
        let policy_default = GatePolicyProfileV1::default();
        assert_eq!(policy_default.profile_name, "default");
        assert!(policy_default.blocking_codes.is_empty());
        assert!(policy_default.required_capabilities.is_empty());
        assert_eq!(
            CoverageObligationV1::default().state,
            ObligationState::Unsatisfied
        );
        assert_eq!(WorkspaceEpochV1::default().epoch, 0);
        assert_eq!(
            DiagnosticEvidenceSnapshotV1::default().request_max_cost,
            CostClass::Instant
        );
    }

    #[test]
    fn stale_snapshot_is_superseded() {
        let decision = evaluate_gate(&snapshot_at(epoch(4)), &epoch(5), &policy(&[]));
        assert_eq!(decision.outcome, GateOutcome::Superseded);
        assert_eq!(decision.reason_codes, vec!["superseded_epoch"]);
    }

    #[test]
    fn newer_or_diverged_lineage_supersedes_expected_epoch() {
        let mut newer = snapshot_at(epoch(6));
        newer.workspace_epoch.parent_epoch = Some(5);
        assert_eq!(
            evaluate_gate(&newer, &epoch(5), &policy(&[])).outcome,
            GateOutcome::Superseded
        );
    }

    #[test]
    fn identity_mismatch_is_unknown_conflict() {
        let mut snapshot = snapshot_at(epoch(5));
        snapshot.workspace_epoch.worktree_id = "wt-other".to_string();
        let decision = evaluate_gate(&snapshot, &epoch(5), &policy(&[]));
        assert_eq!(decision.outcome, GateOutcome::UnknownConflict);
        assert_eq!(decision.reason_codes, vec!["identity_mismatch"]);
    }

    #[test]
    fn manifest_digest_mismatch_is_unknown_conflict() {
        let mut snapshot = snapshot_at(epoch(5));
        snapshot.workspace_epoch.source_manifest_digest = "sha256:other".to_string();
        let decision = evaluate_gate(&snapshot, &epoch(5), &policy(&[]));
        assert_eq!(decision.outcome, GateOutcome::UnknownConflict);
        assert_eq!(decision.reason_codes, vec!["manifest_conflict"]);
    }

    #[test]
    fn changed_file_hash_mismatch_conflicts_even_when_digest_matches() {
        let mut snapshot = snapshot_at(epoch(5));
        snapshot.workspace_epoch.changed_file_hashes[0].hash = "sha256:tampered".to_string();
        let decision = evaluate_gate(&snapshot, &epoch(5), &policy(&[]));
        assert_eq!(decision.outcome, GateOutcome::UnknownConflict);
        assert_eq!(decision.reason_codes, vec!["manifest_conflict"]);
    }

    #[test]
    fn one_blocking_observation_beats_a_timed_out_required_lane() {
        let mut snapshot = snapshot_at(epoch(5));
        snapshot.issues.push(DiagnosticIssueV1 {
            issue_id: "issue-1".to_string(),
            correlation_key: "repo-1|src/main.ts|symbol:RunPolicy".to_string(),
            observations: vec![observation("obs-1", SeverityHint::Blocking, "TS2305")],
            classification: DeltaClassification::New,
        });
        snapshot
            .observations
            .push(observation("obs-1", SeverityHint::Blocking, "TS2305"));
        snapshot.coverage_lanes.push(lane(
            "tsgo",
            &[CapabilityVocabulary::TypeSemantics],
            ConvergenceClass::PullExact,
            LaneState::TimedOut,
            5,
        ));
        let decision = evaluate_gate(
            &snapshot,
            &epoch(5),
            &policy(&[CapabilityVocabulary::TypeSemantics]),
        );
        assert_eq!(decision.outcome, GateOutcome::DirtyExact);
        assert_eq!(decision.blocking_issue_ids, vec!["issue-1"]);
        assert_eq!(decision.reason_codes, vec!["exact_blocker"]);
    }

    #[test]
    fn blocking_issue_ids_are_deduped_and_unissued_observations_listed_by_id() {
        let mut snapshot = snapshot_at(epoch(5));
        let shared_issue = DiagnosticIssueV1 {
            issue_id: "issue-1".to_string(),
            correlation_key: "repo-1|src/main.ts|symbol:RunPolicy".to_string(),
            observations: vec![
                observation("obs-1", SeverityHint::Blocking, "BP001"),
                observation("obs-2", SeverityHint::Blocking, "TS2305"),
            ],
            classification: DeltaClassification::New,
        };
        snapshot.issues.push(shared_issue);
        snapshot
            .observations
            .push(observation("obs-1", SeverityHint::Blocking, "BP001"));
        snapshot
            .observations
            .push(observation("obs-2", SeverityHint::Blocking, "TS2305"));
        snapshot
            .observations
            .push(observation("obs-3", SeverityHint::Blocking, "TS2322"));
        let decision = evaluate_gate(&snapshot, &epoch(5), &policy(&[]));
        assert_eq!(decision.outcome, GateOutcome::DirtyExact);
        assert_eq!(decision.blocking_issue_ids, vec!["issue-1", "obs-3"]);
    }

    #[test]
    fn blocking_code_matches_even_when_severity_hint_is_advisory() {
        let mut snapshot = snapshot_at(epoch(5));
        snapshot.observations.push(observation(
            "obs-1",
            SeverityHint::Advisory,
            "no-explicit-any",
        ));
        snapshot.issues.push(DiagnosticIssueV1 {
            issue_id: "issue-1".to_string(),
            correlation_key: "repo-1|src/main.ts|no-explicit-any".to_string(),
            observations: vec![observation(
                "obs-1",
                SeverityHint::Advisory,
                "no-explicit-any",
            )],
            classification: DeltaClassification::Persistent,
        });
        let mut gate_policy = policy(&[]);
        gate_policy.blocking_codes = vec!["no-explicit-any".to_string()];
        let decision = evaluate_gate(&snapshot, &epoch(5), &gate_policy);
        assert_eq!(decision.outcome, GateOutcome::DirtyExact);
        assert_eq!(decision.blocking_issue_ids, vec!["issue-1"]);
    }

    #[test]
    fn complete_exact_coverage_with_no_blockers_is_clean_exact() {
        let mut snapshot = snapshot_at(epoch(5));
        snapshot
            .observations
            .push(observation("obs-ok", SeverityHint::Advisory, "hint"));
        snapshot.coverage_lanes.push(lane(
            "typescript",
            &[CapabilityVocabulary::TypeSemantics],
            ConvergenceClass::PullExact,
            LaneState::Complete,
            5,
        ));
        snapshot.coverage_obligations.push(CoverageObligationV1 {
            capability: CapabilityVocabulary::TypeSemantics,
            state: ObligationState::SatisfiedExact,
            ..Default::default()
        });
        let decision = evaluate_gate(
            &snapshot,
            &epoch(5),
            &policy(&[CapabilityVocabulary::TypeSemantics]),
        );
        assert_eq!(decision.outcome, GateOutcome::CleanExact);
        assert!(decision.reason_codes.is_empty());
        assert!(decision.blocking_issue_ids.is_empty());
    }

    #[test]
    fn optional_duplicate_failure_does_not_invalidate_exact_coverage() {
        let mut snapshot = snapshot_at(epoch(5));
        snapshot.coverage_lanes.push(lane(
            "typescript",
            &[CapabilityVocabulary::TypeSemantics],
            ConvergenceClass::PullExact,
            LaneState::Complete,
            5,
        ));
        snapshot.coverage_lanes.push(lane(
            "duplicate-analyzer",
            &[CapabilityVocabulary::TypeSemantics],
            ConvergenceClass::PushUnversionedAdvisory,
            LaneState::Partial,
            5,
        ));
        let decision = evaluate_gate(
            &snapshot,
            &epoch(5),
            &policy(&[CapabilityVocabulary::TypeSemantics]),
        );
        assert_eq!(decision.outcome, GateOutcome::CleanExact);
    }

    #[test]
    fn missing_required_capability_is_unknown_incomplete() {
        let decision = evaluate_gate(
            &snapshot_at(epoch(5)),
            &epoch(5),
            &policy(&[CapabilityVocabulary::TypeSemantics]),
        );
        assert_eq!(decision.outcome, GateOutcome::UnknownIncomplete);
        assert_eq!(
            decision.reason_codes,
            vec!["capability_uncovered:type_semantics"]
        );
    }

    #[test]
    fn empty_evidence_with_requirements_is_never_clean() {
        let snapshot = snapshot_at(epoch(5));
        let decision = evaluate_gate(
            &snapshot,
            &epoch(5),
            &policy(&[CapabilityVocabulary::Syntax]),
        );
        assert_ne!(decision.outcome, GateOutcome::CleanExact);
        assert_eq!(decision.outcome, GateOutcome::UnknownIncomplete);
    }

    #[test]
    fn timed_out_required_lane_is_unknown_timed_out() {
        let mut snapshot = snapshot_at(epoch(5));
        snapshot.coverage_lanes.push(lane(
            "tsgo",
            &[CapabilityVocabulary::TypeSemantics],
            ConvergenceClass::PushVersionedExact,
            LaneState::TimedOut,
            5,
        ));
        let decision = evaluate_gate(
            &snapshot,
            &epoch(5),
            &policy(&[CapabilityVocabulary::TypeSemantics]),
        );
        assert_eq!(decision.outcome, GateOutcome::UnknownTimedOut);
        assert_eq!(decision.reason_codes, vec!["lane_timed_out:tsgo"]);
    }

    #[test]
    fn unsupported_required_lane_is_unknown_unavailable() {
        let mut snapshot = snapshot_at(epoch(5));
        snapshot.coverage_lanes.push(lane(
            "pyright",
            &[CapabilityVocabulary::NameResolution],
            ConvergenceClass::PullExact,
            LaneState::Unsupported,
            5,
        ));
        let decision = evaluate_gate(
            &snapshot,
            &epoch(5),
            &policy(&[CapabilityVocabulary::NameResolution]),
        );
        assert_eq!(decision.outcome, GateOutcome::UnknownUnavailable);
        assert_eq!(
            decision.reason_codes,
            vec!["capability_uncovered:name_resolution"]
        );
    }

    #[test]
    fn advisory_convergence_and_stale_bound_epochs_cannot_clear_the_fence() {
        let mut snapshot = snapshot_at(epoch(5));
        snapshot.coverage_lanes.push(lane(
            "watcher",
            &[CapabilityVocabulary::TypeSemantics],
            ConvergenceClass::PushUnversionedAdvisory,
            LaneState::Complete,
            5,
        ));
        snapshot.coverage_lanes.push(lane(
            "cold-service",
            &[CapabilityVocabulary::NameResolution],
            ConvergenceClass::PullExact,
            LaneState::Complete,
            4,
        ));
        let decision = evaluate_gate(
            &snapshot,
            &epoch(5),
            &policy(&[
                CapabilityVocabulary::TypeSemantics,
                CapabilityVocabulary::NameResolution,
            ]),
        );
        assert_eq!(decision.outcome, GateOutcome::UnknownIncomplete);
        assert_eq!(
            decision.reason_codes,
            vec![
                "capability_uncovered:type_semantics",
                "capability_uncovered:name_resolution"
            ]
        );
    }

    #[test]
    fn unsatisfied_obligation_blocks_clean_even_with_full_lane_coverage() {
        let mut snapshot = snapshot_at(epoch(5));
        snapshot.coverage_lanes.push(lane(
            "typescript",
            &[CapabilityVocabulary::TypeSemantics],
            ConvergenceClass::PullExact,
            LaneState::Complete,
            5,
        ));
        snapshot.coverage_obligations.push(CoverageObligationV1 {
            capability: CapabilityVocabulary::TypeSemantics,
            state: ObligationState::Unsatisfied,
            ..Default::default()
        });
        let decision = evaluate_gate(
            &snapshot,
            &epoch(5),
            &policy(&[CapabilityVocabulary::TypeSemantics]),
        );
        assert_eq!(decision.outcome, GateOutcome::UnknownIncomplete);
        assert_eq!(
            decision.reason_codes,
            vec!["obligation_unsatisfied:type_semantics"]
        );
    }

    #[test]
    fn exact_lane_scope_must_cover_required_scope() {
        let mut with_partial_scope = snapshot_at(epoch(5));
        with_partial_scope.coverage_obligations = vec![CoverageObligationV1 {
            capability: CapabilityVocabulary::TypeSemantics,
            state: ObligationState::SatisfiedExact,
            required_scope: RequiredScope {
                paths: vec!["a.rs".to_string(), "b.rs".to_string()],
            },
            ..Default::default()
        }];
        with_partial_scope.coverage_lanes = vec![CoverageLaneV1 {
            provider_id: "typescript".to_string(),
            scope: vec!["a.rs".to_string()],
            capabilities_covered: vec![CapabilityVocabulary::TypeSemantics],
            convergence_class: ConvergenceClass::PullExact,
            bound_workspace_epoch: 5,
            state: LaneState::Complete,
            omissions: Vec::new(),
        }];
        let decision_partial = evaluate_gate(
            &with_partial_scope,
            &epoch(5),
            &policy(&[CapabilityVocabulary::TypeSemantics]),
        );
        assert_ne!(
            decision_partial.outcome,
            GateOutcome::CleanExact,
            "lane scope [a.rs] must not satisfy required [a.rs,b.rs]"
        );
        assert!(decision_partial
            .reason_codes
            .iter()
            .any(|c| c.contains("type_semantics")));

        let mut with_full_scope = snapshot_at(epoch(5));
        with_full_scope.coverage_obligations = vec![CoverageObligationV1 {
            capability: CapabilityVocabulary::TypeSemantics,
            state: ObligationState::SatisfiedExact,
            required_scope: RequiredScope {
                paths: vec!["a.rs".to_string(), "b.rs".to_string()],
            },
            ..Default::default()
        }];
        with_full_scope.coverage_lanes = vec![CoverageLaneV1 {
            provider_id: "typescript".to_string(),
            scope: vec!["a.rs".to_string(), "b.rs".to_string()],
            capabilities_covered: vec![CapabilityVocabulary::TypeSemantics],
            convergence_class: ConvergenceClass::PullExact,
            bound_workspace_epoch: 5,
            state: LaneState::Complete,
            omissions: Vec::new(),
        }];
        let decision_full = evaluate_gate(
            &with_full_scope,
            &epoch(5),
            &policy(&[CapabilityVocabulary::TypeSemantics]),
        );
        assert_eq!(decision_full.outcome, GateOutcome::CleanExact);
    }
}
