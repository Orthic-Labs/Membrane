//! Typed contract for tray-owned background review scheduling.
//!
//! This shape describes scheduling mechanics and content-free observations
//! only. Semantic learners may emit proposals through a separate governed
//! path; this contract never carries or admits durable writes.

use crate::{canonical_json_of, digest_str, HostObservationProvenanceV1};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Current background-review contract version.
pub const BACKGROUND_REVIEW_SCHEMA_VERSION: u32 = 1;

/// H5 uses the same versioned envelope as the scheduler contract.  The
/// separate names make the durable boundary explicit without creating a
/// second semantic-memory store.
pub const BACKGROUND_REVIEW_ACTIVITY_SCHEMA_VERSION: u32 = 1;
pub const BACKGROUND_REVIEW_EXECUTION_SCHEMA_VERSION: u32 = 1;
pub const BACKGROUND_REVIEW_RECEIPT_SCHEMA_VERSION: u32 = 1;
/// Version for the authenticated, host-neutral semantic provider seam.
pub const BACKGROUND_SEMANTIC_REVIEW_SCHEMA_VERSION: u32 = 1;
/// Hard bound for one semantic request's source events.
pub const BACKGROUND_SEMANTIC_REVIEW_MAX_EVENTS: usize = 256;
/// Hard bound for one semantic request/result frame.
pub const BACKGROUND_SEMANTIC_REVIEW_MAX_FRAME_BYTES: usize = 512 * 1024;

/// Closed set of jobs the tray-owned daemon may schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundReviewJobKindV1 {
    AdaptBehavioralReview,
    CortexSemanticDream,
    CortexMemoryCandidateExtraction,
}

impl BackgroundReviewJobKindV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AdaptBehavioralReview => "adapt_behavioral_review",
            Self::CortexSemanticDream => "cortex_semantic_dream",
            Self::CortexMemoryCandidateExtraction => "cortex_memory_candidate_extraction",
        }
    }
}

impl std::fmt::Display for BackgroundReviewJobKindV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Lifecycle state recorded for one scheduling decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundReviewStatusV1 {
    Started,
    Completed,
    Failed,
    Cancelled,
    Deferred,
}

/// Content-free reason for one background scheduling observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundReviewReasonV1 {
    Started,
    Completed,
    Failed,
    Cancelled,
    ConfigUnavailable,
    ConfigInvalid,
    Disabled,
    HubInactive,
    TimeGate,
    ActivityGate,
    AlreadyRunning,
    ForegroundPreempted,
    PerTurnBudgetExceeded,
    AggregateBudgetExceeded,
    RetryLimit,
    InvalidJob,
    NoEligibleWork,
    ModelInputUnavailable,
    SemanticProviderNotWired,
    ProposalSinkUnavailable,
    ForegroundMemoryEmissionSignalUnavailable,
    CursorInputUnavailable,
    InvalidProposal,
}

impl BackgroundReviewReasonV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::ConfigUnavailable => "config_unavailable",
            Self::ConfigInvalid => "config_invalid",
            Self::Disabled => "disabled",
            Self::HubInactive => "hub_inactive",
            Self::TimeGate => "time_gate",
            Self::ActivityGate => "activity_gate",
            Self::AlreadyRunning => "already_running",
            Self::ForegroundPreempted => "foreground_preempted",
            Self::PerTurnBudgetExceeded => "per_turn_budget_exceeded",
            Self::AggregateBudgetExceeded => "aggregate_budget_exceeded",
            Self::RetryLimit => "retry_limit",
            Self::InvalidJob => "invalid_job",
            Self::NoEligibleWork => "no_eligible_work",
            Self::ModelInputUnavailable => "model_input_unavailable",
            Self::SemanticProviderNotWired => "semantic_provider_not_wired",
            Self::ProposalSinkUnavailable => "proposal_sink_unavailable",
            Self::ForegroundMemoryEmissionSignalUnavailable => {
                "foreground_memory_emission_signal_unavailable"
            }
            Self::CursorInputUnavailable => "cursor_input_unavailable",
            Self::InvalidProposal => "invalid_proposal",
        }
    }
}

/// Host-owned foreground activity signal.  Activity units and token counts are
/// accepted only as supplied by the host; no scheduler path derives them from
/// content or from a guessed model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackgroundReviewActivitySignalV1 {
    pub schema_version: u32,
    pub session_id: String,
    pub turn_id: String,
    pub activity_units: u64,
    #[serde(default)]
    pub input_tokens: Option<u64>,
    pub foreground_active: bool,
    pub observed_at_unix_ms: u64,
}

impl BackgroundReviewActivitySignalV1 {
    pub const SCHEMA_VERSION: u32 = BACKGROUND_REVIEW_ACTIVITY_SCHEMA_VERSION;

    pub fn validate(&self) -> Result<(), BackgroundReviewValidationError> {
        if self.schema_version != Self::SCHEMA_VERSION {
            return Err(BackgroundReviewValidationError::SchemaVersion(
                self.schema_version,
            ));
        }
        if self.session_id.trim().is_empty() {
            return Err(BackgroundReviewValidationError::Empty("session_id"));
        }
        if self.turn_id.trim().is_empty() {
            return Err(BackgroundReviewValidationError::Empty("turn_id"));
        }
        Ok(())
    }
}

/// Fail-closed daemon configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackgroundReviewConfigV1 {
    pub schema_version: u32,
    pub enabled: bool,
    /// Minimum elapsed milliseconds between review starts.
    pub min_elapsed_ms: u64,
    /// Activity units required since prior review start.
    pub activity_threshold: u64,
    /// Maximum input tokens chargeable to one turn.
    pub per_turn_input_budget: u64,
    /// Maximum input tokens chargeable to this daemon lifetime.
    pub aggregate_input_budget: u64,
    /// Maximum allowed cancellation window in milliseconds.
    pub cancellation_timeout_ms: u64,
}

impl BackgroundReviewConfigV1 {
    pub const SCHEMA_VERSION: u32 = BACKGROUND_REVIEW_SCHEMA_VERSION;

    pub fn validate(&self) -> Result<(), BackgroundReviewValidationError> {
        if self.schema_version != Self::SCHEMA_VERSION {
            return Err(BackgroundReviewValidationError::SchemaVersion(
                self.schema_version,
            ));
        }
        if self.min_elapsed_ms == 0 {
            return Err(BackgroundReviewValidationError::Zero("min_elapsed_ms"));
        }
        if self.activity_threshold == 0 {
            return Err(BackgroundReviewValidationError::Zero("activity_threshold"));
        }
        if self.per_turn_input_budget == 0 {
            return Err(BackgroundReviewValidationError::Zero(
                "per_turn_input_budget",
            ));
        }
        if self.aggregate_input_budget == 0 {
            return Err(BackgroundReviewValidationError::Zero(
                "aggregate_input_budget",
            ));
        }
        if self.aggregate_input_budget < self.per_turn_input_budget {
            return Err(BackgroundReviewValidationError::AggregateBelowTurn);
        }
        if self.cancellation_timeout_ms == 0 {
            return Err(BackgroundReviewValidationError::Zero(
                "cancellation_timeout_ms",
            ));
        }
        Ok(())
    }

    pub fn from_json(input: &str) -> Result<Self, BackgroundReviewConfigError> {
        let config: Self = serde_json::from_str(input)
            .map_err(|error| BackgroundReviewConfigError::Json(error.to_string()))?;
        config
            .validate()
            .map_err(BackgroundReviewConfigError::Invalid)?;
        Ok(config)
    }
}

/// Job request received by scheduler from a daemon-owned producer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackgroundReviewJobV1 {
    pub schema_version: u32,
    pub job_id: String,
    pub kind: BackgroundReviewJobKindV1,
    pub turn_id: String,
    pub input_tokens: u64,
    pub requested_at_unix_ms: u64,
}

impl BackgroundReviewJobV1 {
    pub const SCHEMA_VERSION: u32 = BACKGROUND_REVIEW_SCHEMA_VERSION;

    pub fn validate(&self) -> Result<(), BackgroundReviewValidationError> {
        if self.schema_version != Self::SCHEMA_VERSION {
            return Err(BackgroundReviewValidationError::SchemaVersion(
                self.schema_version,
            ));
        }
        if self.job_id.trim().is_empty() {
            return Err(BackgroundReviewValidationError::Empty("job_id"));
        }
        if self.turn_id.trim().is_empty() {
            return Err(BackgroundReviewValidationError::Empty("turn_id"));
        }
        Ok(())
    }
}

/// Content-free scheduler observation. No semantic learner output is carried.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackgroundReviewObservationV1 {
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<BackgroundReviewJobKindV1>,
    pub status: BackgroundReviewStatusV1,
    pub reason: BackgroundReviewReasonV1,
    pub observed_at_unix_ms: u64,
    pub attempt: u8,
    pub input_tokens: u64,
    pub turn_input_tokens: u64,
    pub aggregate_input_tokens: u64,
    pub activity_units: u64,
    pub hub_active: bool,
    pub foreground_active: bool,
}

impl BackgroundReviewObservationV1 {
    pub const SCHEMA_VERSION: u32 = BACKGROUND_REVIEW_SCHEMA_VERSION;

    pub fn validate(&self) -> Result<(), BackgroundReviewValidationError> {
        if self.schema_version != Self::SCHEMA_VERSION {
            return Err(BackgroundReviewValidationError::SchemaVersion(
                self.schema_version,
            ));
        }
        if self
            .job_id
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(BackgroundReviewValidationError::Empty("job_id"));
        }
        if self.job_id.is_some() != self.kind.is_some() {
            return Err(BackgroundReviewValidationError::JobIdentityPair);
        }
        if matches!(
            self.status,
            BackgroundReviewStatusV1::Started
                | BackgroundReviewStatusV1::Completed
                | BackgroundReviewStatusV1::Failed
                | BackgroundReviewStatusV1::Cancelled
        ) && self.job_id.is_none()
        {
            return Err(BackgroundReviewValidationError::MissingJobIdentity);
        }
        if self.attempt > 2 {
            return Err(BackgroundReviewValidationError::AttemptLimit);
        }
        if matches!(
            self.status,
            BackgroundReviewStatusV1::Started
                | BackgroundReviewStatusV1::Completed
                | BackgroundReviewStatusV1::Failed
                | BackgroundReviewStatusV1::Cancelled
        ) && self.attempt == 0
        {
            return Err(BackgroundReviewValidationError::MissingAttempt);
        }
        let reason_matches_status = match self.status {
            BackgroundReviewStatusV1::Started => self.reason == BackgroundReviewReasonV1::Started,
            BackgroundReviewStatusV1::Completed => {
                self.reason == BackgroundReviewReasonV1::Completed
            }
            BackgroundReviewStatusV1::Failed => matches!(
                self.reason,
                BackgroundReviewReasonV1::Failed
                    | BackgroundReviewReasonV1::ModelInputUnavailable
                    | BackgroundReviewReasonV1::SemanticProviderNotWired
                    | BackgroundReviewReasonV1::ProposalSinkUnavailable
                    | BackgroundReviewReasonV1::ForegroundMemoryEmissionSignalUnavailable
                    | BackgroundReviewReasonV1::CursorInputUnavailable
                    | BackgroundReviewReasonV1::InvalidProposal
            ),
            BackgroundReviewStatusV1::Cancelled => matches!(
                self.reason,
                BackgroundReviewReasonV1::Cancelled
                    | BackgroundReviewReasonV1::ForegroundPreempted
                    | BackgroundReviewReasonV1::HubInactive
            ),
            BackgroundReviewStatusV1::Deferred => matches!(
                self.reason,
                BackgroundReviewReasonV1::ConfigUnavailable
                    | BackgroundReviewReasonV1::ConfigInvalid
                    | BackgroundReviewReasonV1::Disabled
                    | BackgroundReviewReasonV1::HubInactive
                    | BackgroundReviewReasonV1::TimeGate
                    | BackgroundReviewReasonV1::ActivityGate
                    | BackgroundReviewReasonV1::AlreadyRunning
                    | BackgroundReviewReasonV1::ForegroundPreempted
                    | BackgroundReviewReasonV1::PerTurnBudgetExceeded
                    | BackgroundReviewReasonV1::AggregateBudgetExceeded
                    | BackgroundReviewReasonV1::RetryLimit
                    | BackgroundReviewReasonV1::InvalidJob
                    | BackgroundReviewReasonV1::NoEligibleWork
                    | BackgroundReviewReasonV1::ModelInputUnavailable
                    | BackgroundReviewReasonV1::SemanticProviderNotWired
                    | BackgroundReviewReasonV1::ProposalSinkUnavailable
                    | BackgroundReviewReasonV1::ForegroundMemoryEmissionSignalUnavailable
                    | BackgroundReviewReasonV1::CursorInputUnavailable
                    | BackgroundReviewReasonV1::InvalidProposal
            ),
        };
        if !reason_matches_status {
            return Err(BackgroundReviewValidationError::StatusReasonMismatch);
        }
        Ok(())
    }
}

/// Opaque reference to a proposal emitted by a semantic learner.  The learner
/// may return these references, but this shape carries no model text and never
/// authorizes a Cortex mutation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackgroundReviewProposalRefV1 {
    pub proposal_id: String,
    pub proposal_digest: String,
}

impl BackgroundReviewProposalRefV1 {
    pub fn validate(&self) -> Result<(), BackgroundReviewValidationError> {
        if self.proposal_id.trim().is_empty() {
            return Err(BackgroundReviewValidationError::Empty("proposal_id"));
        }
        let Some(hex) = self.proposal_digest.strip_prefix("sha256:") else {
            return Err(BackgroundReviewValidationError::InvalidDigest(
                "proposal_digest",
            ));
        };
        if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(BackgroundReviewValidationError::InvalidDigest(
                "proposal_digest",
            ));
        }
        Ok(())
    }
}

/// Result of executing one admitted semantic job.  `proposals` contains only
/// opaque references; durable approval/admission remains outside this runner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundReviewExecutionStatusV1 {
    Proposals,
    Blocked,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackgroundReviewExecutionV1 {
    pub schema_version: u32,
    pub job_id: String,
    pub kind: BackgroundReviewJobKindV1,
    pub status: BackgroundReviewExecutionStatusV1,
    #[serde(default)]
    pub proposals: Vec<BackgroundReviewProposalRefV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<BackgroundReviewReasonV1>,
}

impl BackgroundReviewExecutionV1 {
    pub const SCHEMA_VERSION: u32 = BACKGROUND_REVIEW_EXECUTION_SCHEMA_VERSION;

    pub fn validate(&self) -> Result<(), BackgroundReviewValidationError> {
        if self.schema_version != Self::SCHEMA_VERSION {
            return Err(BackgroundReviewValidationError::SchemaVersion(
                self.schema_version,
            ));
        }
        if self.job_id.trim().is_empty() {
            return Err(BackgroundReviewValidationError::Empty("job_id"));
        }
        if self.proposals.len() > 64 {
            return Err(BackgroundReviewValidationError::ProposalLimit);
        }
        for proposal in &self.proposals {
            proposal.validate()?;
        }
        match self.status {
            BackgroundReviewExecutionStatusV1::Proposals => {
                if self.reason.is_some() {
                    return Err(BackgroundReviewValidationError::ExecutionReasonMismatch);
                }
            }
            BackgroundReviewExecutionStatusV1::Blocked
            | BackgroundReviewExecutionStatusV1::Failed => {
                let Some(reason) = self.reason else {
                    return Err(BackgroundReviewValidationError::ExecutionReasonRequired);
                };
                if !matches!(
                    reason,
                    BackgroundReviewReasonV1::ModelInputUnavailable
                        | BackgroundReviewReasonV1::SemanticProviderNotWired
                        | BackgroundReviewReasonV1::ProposalSinkUnavailable
                        | BackgroundReviewReasonV1::ForegroundMemoryEmissionSignalUnavailable
                        | BackgroundReviewReasonV1::CursorInputUnavailable
                        | BackgroundReviewReasonV1::InvalidProposal
                        | BackgroundReviewReasonV1::InvalidJob
                        | BackgroundReviewReasonV1::Failed
                ) {
                    return Err(BackgroundReviewValidationError::InvalidExecutionReason);
                }
                if !self.proposals.is_empty() {
                    return Err(BackgroundReviewValidationError::ExecutionReasonMismatch);
                }
            }
        }
        Ok(())
    }
}

/// Durable H5 receipt.  The receipt digest covers scheduler observation bytes,
/// while the provenance envelope binds that digest to this observation's
/// timestamp and producer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackgroundReviewObservationReceiptV1 {
    pub schema_version: u32,
    pub observation_id: String,
    pub observation: BackgroundReviewObservationV1,
    pub provenance_receipt: HostObservationProvenanceV1,
}

/// Stored event cursor supplied to a background semantic request.  The
/// daemon treats this as an opaque high-water mark and never rewinds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackgroundReviewCursorV1 {
    pub session_id: String,
    pub last_seq: u64,
}

impl BackgroundReviewCursorV1 {
    pub fn validate(&self) -> Result<(), BackgroundReviewValidationError> {
        if self.session_id.trim().is_empty() {
            return Err(BackgroundReviewValidationError::Empty("cursor.session_id"));
        }
        Ok(())
    }
}

/// Event provenance transported to one semantic provider.  Payload remains
/// untrusted provider input; governance and source identity stay explicit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackgroundReviewProvenanceRefV1 {
    pub source: String,
    #[serde(default)]
    pub source_event_ids: Vec<String>,
    #[serde(default)]
    pub producer: Option<String>,
}

/// Bounded session event included after stored cursor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackgroundReviewSessionEventV1 {
    pub schema_version: u32,
    pub session_id: String,
    pub seq: u64,
    pub event_id: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub payload: serde_json::Value,
    pub scope_id: String,
    pub authority: String,
    pub influence_class: String,
    pub lifecycle: String,
    pub retention: String,
    #[serde(default)]
    pub provenance: Vec<BackgroundReviewProvenanceRefV1>,
    pub occurred_at_ms: u64,
    pub recorded_at_ms: u64,
    pub content_hash: String,
}

impl BackgroundReviewSessionEventV1 {
    pub fn validate(
        &self,
        session_id: &str,
        prior_seq: u64,
    ) -> Result<(), BackgroundReviewValidationError> {
        if self.schema_version != BACKGROUND_SEMANTIC_REVIEW_SCHEMA_VERSION {
            return Err(BackgroundReviewValidationError::SchemaVersion(
                self.schema_version,
            ));
        }
        if self.session_id != session_id {
            return Err(BackgroundReviewValidationError::CursorSessionMismatch);
        }
        if self.seq == 0 || self.seq <= prior_seq {
            return Err(BackgroundReviewValidationError::EventOrder);
        }
        if self.event_id.trim().is_empty() || self.event_type.trim().is_empty() {
            return Err(BackgroundReviewValidationError::Empty("event identity"));
        }
        if !self.payload.is_object() {
            return Err(BackgroundReviewValidationError::InvalidEventPayload);
        }
        if self.content_hash.trim().is_empty()
            || self.scope_id.trim().is_empty()
            || self.authority.trim().is_empty()
            || self.influence_class.trim().is_empty()
            || self.lifecycle.trim().is_empty()
            || self.retention.trim().is_empty()
        {
            return Err(BackgroundReviewValidationError::Empty("event governance"));
        }
        if self
            .provenance
            .iter()
            .any(|item| item.source.trim().is_empty())
        {
            return Err(BackgroundReviewValidationError::Empty("event provenance"));
        }
        Ok(())
    }
}

/// Half-open event range emitted by authoritative foreground memory writer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackgroundReviewEventRangeV1 {
    pub start_seq: u64,
    pub end_seq: u64,
}

impl BackgroundReviewEventRangeV1 {
    pub fn validate(&self) -> Result<(), BackgroundReviewValidationError> {
        if self.start_seq == 0 || self.start_seq >= self.end_seq {
            return Err(BackgroundReviewValidationError::InvalidForegroundRange);
        }
        Ok(())
    }

    pub fn overlaps(self, other: Self) -> bool {
        self.start_seq < other.end_seq && self.end_seq > other.start_seq
    }
}

/// Three-state foreground memory signal.  `AvailableNoEmission` explicitly
/// permits extraction while `Unavailable` fails closed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundReviewForegroundMemoryStateV1 {
    Unavailable,
    AvailableNoEmission,
    AvailableEmission { range: BackgroundReviewEventRangeV1 },
}

impl BackgroundReviewForegroundMemoryStateV1 {
    pub fn validate(&self) -> Result<(), BackgroundReviewValidationError> {
        if let Self::AvailableEmission { range } = self {
            range.validate()?;
        }
        Ok(())
    }
}

/// Authenticated host-neutral request sent by daemon to one loopback semantic
/// provider.  It carries no durable-memory authority.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackgroundSemanticReviewRequestV1 {
    pub schema_version: u32,
    pub job_id: String,
    pub job_kind: BackgroundReviewJobKindV1,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    pub turn_id: String,
    pub cursor: BackgroundReviewCursorV1,
    pub events: Vec<BackgroundReviewSessionEventV1>,
    pub foreground_memory_state: BackgroundReviewForegroundMemoryStateV1,
    pub per_turn_budget_remaining: u64,
    pub aggregate_budget_remaining: u64,
    pub deadline_unix_ms: u64,
    pub restricted_capabilities: Vec<String>,
}

impl BackgroundSemanticReviewRequestV1 {
    pub const SCHEMA_VERSION: u32 = BACKGROUND_SEMANTIC_REVIEW_SCHEMA_VERSION;

    pub fn validate(&self) -> Result<(), BackgroundReviewValidationError> {
        if self.schema_version != Self::SCHEMA_VERSION {
            return Err(BackgroundReviewValidationError::SchemaVersion(
                self.schema_version,
            ));
        }
        if self.job_id.trim().is_empty() {
            return Err(BackgroundReviewValidationError::Empty("job_id"));
        }
        if self.session_id.trim().is_empty() {
            return Err(BackgroundReviewValidationError::Empty("session_id"));
        }
        if self.turn_id.trim().is_empty() {
            return Err(BackgroundReviewValidationError::Empty("turn_id"));
        }
        if self
            .task_id
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(BackgroundReviewValidationError::Empty("task_id"));
        }
        self.cursor.validate()?;
        if self.cursor.session_id != self.session_id {
            return Err(BackgroundReviewValidationError::CursorSessionMismatch);
        }
        if self.events.len() > BACKGROUND_SEMANTIC_REVIEW_MAX_EVENTS {
            return Err(BackgroundReviewValidationError::EventLimit);
        }
        let mut prior_seq = self.cursor.last_seq;
        for event in &self.events {
            event.validate(&self.session_id, prior_seq)?;
            prior_seq = event.seq;
        }
        self.foreground_memory_state.validate()?;
        if self.restricted_capabilities.is_empty()
            || self
                .restricted_capabilities
                .iter()
                .any(|capability| capability.trim().is_empty())
        {
            return Err(BackgroundReviewValidationError::Empty(
                "restricted_capabilities",
            ));
        }
        Ok(())
    }
}

/// Optional measured provider usage attached to semantic result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackgroundSemanticReviewUsageV1 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
}

/// Result status returned from provider. Proposal material remains untrusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundSemanticReviewStatusV1 {
    Proposals,
    Blocked { reason: BackgroundReviewReasonV1 },
    Failed { reason: BackgroundReviewReasonV1 },
}

/// Authenticated semantic result bound to one request identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackgroundSemanticReviewResultV1 {
    pub schema_version: u32,
    pub job_id: String,
    pub job_kind: BackgroundReviewJobKindV1,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    pub turn_id: String,
    #[serde(default)]
    pub curation_proposals: Vec<serde_json::Value>,
    #[serde(default)]
    pub memory_candidates: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<BackgroundReviewCursorV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<BackgroundSemanticReviewUsageV1>,
    pub provenance_receipt: HostObservationProvenanceV1,
    pub status: BackgroundSemanticReviewStatusV1,
}

impl BackgroundSemanticReviewResultV1 {
    pub const SCHEMA_VERSION: u32 = BACKGROUND_SEMANTIC_REVIEW_SCHEMA_VERSION;

    pub fn validate_against(
        &self,
        request: &BackgroundSemanticReviewRequestV1,
    ) -> Result<(), BackgroundReviewValidationError> {
        if self.schema_version != Self::SCHEMA_VERSION {
            return Err(BackgroundReviewValidationError::SchemaVersion(
                self.schema_version,
            ));
        }
        if self.job_id != request.job_id
            || self.job_kind != request.job_kind
            || self.session_id != request.session_id
            || self.task_id != request.task_id
            || self.turn_id != request.turn_id
        {
            return Err(BackgroundReviewValidationError::ResultIdentityMismatch);
        }
        if self
            .curation_proposals
            .len()
            .saturating_add(self.memory_candidates.len())
            > BACKGROUND_SEMANTIC_REVIEW_MAX_EVENTS
        {
            return Err(BackgroundReviewValidationError::ProposalLimit);
        }
        if let Some(cursor) = &self.next_cursor {
            cursor.validate()?;
            let exceeds_window = request
                .events
                .last()
                .map(|event| cursor.last_seq > event.seq)
                .unwrap_or(cursor.last_seq > request.cursor.last_seq);
            if cursor.session_id != request.session_id
                || cursor.last_seq < request.cursor.last_seq
                || exceeds_window
            {
                return Err(BackgroundReviewValidationError::CursorAdvanceInvalid);
            }
        }
        self.provenance_receipt
            .validate()
            .map_err(|_| BackgroundReviewValidationError::InvalidProvenance)?;
        match self.status {
            BackgroundSemanticReviewStatusV1::Proposals => {
                if self.curation_proposals.is_empty() && self.memory_candidates.is_empty() {
                    if self.next_cursor.is_some() {
                        return Err(BackgroundReviewValidationError::EmptyResult);
                    }
                }
            }
            BackgroundSemanticReviewStatusV1::Blocked { .. }
            | BackgroundSemanticReviewStatusV1::Failed { .. } => {
                if !self.curation_proposals.is_empty() || !self.memory_candidates.is_empty() {
                    return Err(BackgroundReviewValidationError::ExecutionReasonMismatch);
                }
                if self.next_cursor.is_some() {
                    return Err(BackgroundReviewValidationError::CursorAdvanceInvalid);
                }
            }
        }
        Ok(())
    }
}

impl BackgroundReviewObservationReceiptV1 {
    pub const SCHEMA_VERSION: u32 = BACKGROUND_REVIEW_RECEIPT_SCHEMA_VERSION;

    pub fn from_observation(
        observation: BackgroundReviewObservationV1,
    ) -> Result<Self, BackgroundReviewValidationError> {
        observation.validate()?;
        let digest = digest_str(&canonical_json_of(&observation));
        let digest_hex = digest.strip_prefix("sha256:").unwrap_or(&digest);
        let observation_id = format!("background-review-{digest_hex}");
        let provenance_receipt = HostObservationProvenanceV1::new(
            observation_id.clone(),
            "membrane-runtime",
            observation.observed_at_unix_ms,
            digest.clone(),
        );
        let receipt = Self {
            schema_version: Self::SCHEMA_VERSION,
            observation_id,
            observation,
            provenance_receipt,
        };
        receipt.validate()?;
        Ok(receipt)
    }

    pub fn validate(&self) -> Result<(), BackgroundReviewValidationError> {
        if self.schema_version != Self::SCHEMA_VERSION {
            return Err(BackgroundReviewValidationError::SchemaVersion(
                self.schema_version,
            ));
        }
        if self.observation_id.trim().is_empty() {
            return Err(BackgroundReviewValidationError::Empty("observation_id"));
        }
        self.observation.validate()?;
        self.provenance_receipt
            .validate()
            .map_err(|_| BackgroundReviewValidationError::InvalidProvenance)?;
        let expected_digest = digest_str(&canonical_json_of(&self.observation));
        if self.provenance_receipt.receipt_digest != expected_digest
            || self.provenance_receipt.receipt_id != self.observation_id
            || self.provenance_receipt.observed_at_unix_ms != self.observation.observed_at_unix_ms
        {
            return Err(BackgroundReviewValidationError::ReceiptMismatch);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BackgroundReviewValidationError {
    #[error("background review schema version is unsupported: {0}")]
    SchemaVersion(u32),
    #[error("background review field is empty: {0}")]
    Empty(&'static str),
    #[error("background review field must be non-zero: {0}")]
    Zero(&'static str),
    #[error("aggregate input budget is below per-turn input budget")]
    AggregateBelowTurn,
    #[error("background review attempt exceeds one retry")]
    AttemptLimit,
    #[error("background review terminal observation is missing job identity")]
    MissingJobIdentity,
    #[error("background review terminal observation is missing attempt")]
    MissingAttempt,
    #[error("background review observation has only one job identity field")]
    JobIdentityPair,
    #[error("background review observation status and reason do not match")]
    StatusReasonMismatch,
    #[error("background review digest is invalid: {0}")]
    InvalidDigest(&'static str),
    #[error("background review proposal count exceeds the bounded limit")]
    ProposalLimit,
    #[error("background review execution requires a typed reason")]
    ExecutionReasonRequired,
    #[error("background review execution reason is not an execution blocker")]
    InvalidExecutionReason,
    #[error("background review execution reason and proposals do not match")]
    ExecutionReasonMismatch,
    #[error("background review provenance receipt is invalid")]
    InvalidProvenance,
    #[error("background review provenance receipt does not match observation")]
    ReceiptMismatch,
    #[error("background review event sequence is invalid")]
    EventOrder,
    #[error("background review event session does not match cursor")]
    CursorSessionMismatch,
    #[error("background review foreground emission range is invalid")]
    InvalidForegroundRange,
    #[error("background review event payload must be an object")]
    InvalidEventPayload,
    #[error("background semantic review event count exceeds the bounded limit")]
    EventLimit,
    #[error("background semantic review result identity does not match request")]
    ResultIdentityMismatch,
    #[error("background semantic review cursor advance is invalid")]
    CursorAdvanceInvalid,
    #[error("background semantic review returned no proposals")]
    EmptyResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BackgroundReviewConfigError {
    #[error("background review configuration could not be read: {0}")]
    Io(String),
    #[error("background review configuration JSON is invalid: {0}")]
    Json(String),
    #[error("background review configuration is invalid: {0}")]
    Invalid(BackgroundReviewValidationError),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> BackgroundReviewConfigV1 {
        BackgroundReviewConfigV1 {
            schema_version: 1,
            enabled: true,
            min_elapsed_ms: 100,
            activity_threshold: 2,
            per_turn_input_budget: 50,
            aggregate_input_budget: 100,
            cancellation_timeout_ms: 25,
        }
    }

    fn job() -> BackgroundReviewJobV1 {
        BackgroundReviewJobV1 {
            schema_version: 1,
            job_id: "job-1".into(),
            kind: BackgroundReviewJobKindV1::AdaptBehavioralReview,
            turn_id: "turn-1".into(),
            input_tokens: 10,
            requested_at_unix_ms: 1,
        }
    }

    #[test]
    fn config_round_trips_and_rejects_unknown_fields() {
        let encoded = serde_json::to_string(&config()).unwrap();
        assert_eq!(
            BackgroundReviewConfigV1::from_json(&encoded).unwrap(),
            config()
        );
        let mut value: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("runLearner".into(), true.into());
        assert!(BackgroundReviewConfigV1::from_json(&value.to_string()).is_err());
    }

    #[test]
    fn invalid_config_fails_closed_at_contract_boundary() {
        let mut invalid = config();
        invalid.aggregate_input_budget = 1;
        assert!(matches!(
            invalid.validate(),
            Err(BackgroundReviewValidationError::AggregateBelowTurn)
        ));
        let mut wrong_schema = config();
        wrong_schema.schema_version = 2;
        assert!(matches!(
            wrong_schema.validate(),
            Err(BackgroundReviewValidationError::SchemaVersion(2))
        ));
    }

    #[test]
    fn typed_job_and_observation_round_trip_without_output_payload() {
        let encoded = serde_json::to_string(&job()).unwrap();
        let decoded: BackgroundReviewJobV1 = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, job());
        let observation = BackgroundReviewObservationV1 {
            schema_version: 1,
            job_id: Some("job-1".into()),
            kind: Some(BackgroundReviewJobKindV1::AdaptBehavioralReview),
            status: BackgroundReviewStatusV1::Deferred,
            reason: BackgroundReviewReasonV1::HubInactive,
            observed_at_unix_ms: 2,
            attempt: 0,
            input_tokens: 10,
            turn_input_tokens: 0,
            aggregate_input_tokens: 0,
            activity_units: 0,
            hub_active: false,
            foreground_active: false,
        };
        observation.validate().unwrap();
        let value = serde_json::to_value(observation).unwrap();
        assert!(value.get("output").is_none());
    }

    #[test]
    fn observation_validation_rejects_partial_identity_and_mismatched_status() {
        let mut partial = BackgroundReviewObservationV1 {
            schema_version: 1,
            job_id: Some("job-1".into()),
            kind: None,
            status: BackgroundReviewStatusV1::Deferred,
            reason: BackgroundReviewReasonV1::NoEligibleWork,
            observed_at_unix_ms: 2,
            attempt: 0,
            input_tokens: 0,
            turn_input_tokens: 0,
            aggregate_input_tokens: 0,
            activity_units: 0,
            hub_active: true,
            foreground_active: false,
        };
        assert!(matches!(
            partial.validate(),
            Err(BackgroundReviewValidationError::JobIdentityPair)
        ));

        partial.kind = Some(BackgroundReviewJobKindV1::AdaptBehavioralReview);
        partial.status = BackgroundReviewStatusV1::Started;
        partial.reason = BackgroundReviewReasonV1::HubInactive;
        partial.attempt = 1;
        assert!(matches!(
            partial.validate(),
            Err(BackgroundReviewValidationError::StatusReasonMismatch)
        ));
    }

    #[test]
    fn observation_validation_requires_attempt_for_terminal_job_states() {
        let observation = BackgroundReviewObservationV1 {
            schema_version: 1,
            job_id: Some("job-1".into()),
            kind: Some(BackgroundReviewJobKindV1::AdaptBehavioralReview),
            status: BackgroundReviewStatusV1::Completed,
            reason: BackgroundReviewReasonV1::Completed,
            observed_at_unix_ms: 2,
            attempt: 0,
            input_tokens: 0,
            turn_input_tokens: 0,
            aggregate_input_tokens: 0,
            activity_units: 0,
            hub_active: true,
            foreground_active: false,
        };
        assert!(matches!(
            observation.validate(),
            Err(BackgroundReviewValidationError::MissingAttempt)
        ));
    }

    #[test]
    fn activity_signal_keeps_host_activity_and_missing_tokens_explicit() {
        let signal = BackgroundReviewActivitySignalV1 {
            schema_version: BACKGROUND_REVIEW_ACTIVITY_SCHEMA_VERSION,
            session_id: "session-1".into(),
            turn_id: "turn-1".into(),
            activity_units: 3,
            input_tokens: None,
            foreground_active: false,
            observed_at_unix_ms: 3,
        };
        signal.validate().unwrap();
        let encoded = serde_json::to_value(signal).unwrap();
        assert!(encoded
            .get("inputTokens")
            .is_some_and(|value| value.is_null()));
    }

    #[test]
    fn execution_contract_rejects_output_with_blocked_reason() {
        let execution = BackgroundReviewExecutionV1 {
            schema_version: BACKGROUND_REVIEW_EXECUTION_SCHEMA_VERSION,
            job_id: "job-1".into(),
            kind: BackgroundReviewJobKindV1::CortexSemanticDream,
            status: BackgroundReviewExecutionStatusV1::Blocked,
            proposals: vec![BackgroundReviewProposalRefV1 {
                proposal_id: "proposal-1".into(),
                proposal_digest:
                    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            }],
            reason: Some(BackgroundReviewReasonV1::SemanticProviderNotWired),
        };
        assert!(matches!(
            execution.validate(),
            Err(BackgroundReviewValidationError::ExecutionReasonMismatch)
        ));
    }

    #[test]
    fn observation_receipt_is_digest_bound_and_content_free() {
        let observation = BackgroundReviewObservationV1 {
            schema_version: BACKGROUND_REVIEW_SCHEMA_VERSION,
            job_id: None,
            kind: None,
            status: BackgroundReviewStatusV1::Deferred,
            reason: BackgroundReviewReasonV1::ModelInputUnavailable,
            observed_at_unix_ms: 4,
            attempt: 0,
            input_tokens: 0,
            turn_input_tokens: 0,
            aggregate_input_tokens: 0,
            activity_units: 3,
            hub_active: true,
            foreground_active: false,
        };
        let receipt = BackgroundReviewObservationReceiptV1::from_observation(observation).unwrap();
        receipt.validate().unwrap();
        let value = serde_json::to_value(receipt).unwrap();
        assert!(value.get("prompt").is_none());
        assert!(value.get("output").is_none());
    }
}
