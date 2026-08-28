//! Additive contracts for the CodeRight/host seam (§12).
//!
//! These records are host observations, not Membrane durable truth.  They are
//! deliberately separate from the five public context shapes: a host can
//! report what happened without gaining authority to admit, classify, or
//! publish a record.  Every field whose value may be absent uses
//! [`ObservedFieldV1`], so absence remains a typed reason instead of a zero
//! or an empty value.

use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use thiserror::Error;

pub const HOST_OBSERVATION_SCHEMA_VERSION: u32 = 1;
pub const EXECUTION_OBSERVATION_SCHEMA_VERSION: u32 = HOST_OBSERVATION_SCHEMA_VERSION;
pub const EVALUATION_OUTCOME_SCHEMA_VERSION: u32 = HOST_OBSERVATION_SCHEMA_VERSION;
pub const REMAINING_CONTEXT_CEILING_SCHEMA_VERSION: u32 = HOST_OBSERVATION_SCHEMA_VERSION;
pub const LOADED_CONTEXT_IDENTITIES_SCHEMA_VERSION: u32 = HOST_OBSERVATION_SCHEMA_VERSION;
pub const PACKET_DELIVERY_ACKNOWLEDGEMENT_SCHEMA_VERSION: u32 = HOST_OBSERVATION_SCHEMA_VERSION;
pub const PACKET_DELIVERY_ACKNOWLEDGMENT_SCHEMA_VERSION: u32 =
    PACKET_DELIVERY_ACKNOWLEDGEMENT_SCHEMA_VERSION;
pub const HOST_OBSERVATION_PROVENANCE_SCHEMA_VERSION: u32 = HOST_OBSERVATION_SCHEMA_VERSION;

/// Closed reasons for a value that the producing host could not observe.
///
/// This enum intentionally does not contain a generic `unknown` or a numeric
/// fallback.  The reason is part of the evidence contract and survives all
/// the way to a caller that renders the observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationUnavailableReasonV1 {
    NotInstrumented,
    HubInactive,
    ProviderOmitted,
    HostUnsupported,
}

/// Coverage of one host-observed field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationCoverageV1 {
    Complete,
    Partial,
    Unavailable,
}

/// A value with an explicit coverage marker.  `Unavailable` can never carry a
/// value, which prevents missing numeric evidence from serializing as `0`.
/// `Partial` may carry a bounded value, but must still carry a typed reason.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ObservedFieldV1<T> {
    pub coverage: ObservationCoverageV1,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<ObservationUnavailableReasonV1>,
}

impl<T: Eq> Eq for ObservedFieldV1<T> {}

impl<T> ObservedFieldV1<T> {
    pub fn complete(value: T) -> Self {
        Self {
            coverage: ObservationCoverageV1::Complete,
            value: Some(value),
            unavailable_reason: None,
        }
    }

    pub fn partial(value: Option<T>, reason: ObservationUnavailableReasonV1) -> Self {
        Self {
            coverage: ObservationCoverageV1::Partial,
            value,
            unavailable_reason: Some(reason),
        }
    }

    pub fn unavailable(reason: ObservationUnavailableReasonV1) -> Self {
        Self {
            coverage: ObservationCoverageV1::Unavailable,
            value: None,
            unavailable_reason: Some(reason),
        }
    }

    pub fn validate(&self, field: &str) -> Result<(), HostObservationValidationError> {
        match self.coverage {
            ObservationCoverageV1::Complete => {
                if self.value.is_none() {
                    return Err(HostObservationValidationError::Coverage {
                        field: field.to_string(),
                        reason: "complete coverage requires a value".to_string(),
                    });
                }
                if self.unavailable_reason.is_some() {
                    return Err(HostObservationValidationError::Coverage {
                        field: field.to_string(),
                        reason: "complete coverage cannot carry an unavailable reason".to_string(),
                    });
                }
            }
            ObservationCoverageV1::Partial => {
                if self.unavailable_reason.is_none() {
                    return Err(HostObservationValidationError::Coverage {
                        field: field.to_string(),
                        reason: "partial coverage requires a typed reason".to_string(),
                    });
                }
            }
            ObservationCoverageV1::Unavailable => {
                if self.value.is_some() {
                    return Err(HostObservationValidationError::Coverage {
                        field: field.to_string(),
                        reason: "unavailable coverage cannot carry a value".to_string(),
                    });
                }
                if self.unavailable_reason.is_none() {
                    return Err(HostObservationValidationError::Coverage {
                        field: field.to_string(),
                        reason: "unavailable coverage requires a typed reason".to_string(),
                    });
                }
            }
        }
        Ok(())
    }
}

/// Provenance receipt attached to every host observation record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostObservationProvenanceV1 {
    pub schema_version: u32,
    /// Opaque host receipt or event id.  Membrane does not dereference it.
    pub receipt_id: String,
    /// Canonical producer label, e.g. `coderight` or `claude_code`.
    pub source: String,
    pub observed_at_unix_ms: u64,
    /// Digest of the host receipt/evidence that produced this record.
    pub receipt_digest: String,
}

impl HostObservationProvenanceV1 {
    pub fn new(
        receipt_id: impl Into<String>,
        source: impl Into<String>,
        observed_at_unix_ms: u64,
        receipt_digest: impl Into<String>,
    ) -> Self {
        Self {
            schema_version: HOST_OBSERVATION_PROVENANCE_SCHEMA_VERSION,
            receipt_id: receipt_id.into(),
            source: source.into(),
            observed_at_unix_ms,
            receipt_digest: receipt_digest.into(),
        }
    }

    pub fn validate(&self) -> Result<(), HostObservationValidationError> {
        require_nonempty("provenance.receipt_id", &self.receipt_id)?;
        require_nonempty("provenance.source", &self.source)?;
        validate_schema(
            "HostObservationProvenanceV1",
            self.schema_version,
            HOST_OBSERVATION_PROVENANCE_SCHEMA_VERSION,
        )?;
        validate_sha256("provenance.receipt_digest", &self.receipt_digest)
    }
}

/// Token estimator identity.  Values from different bases are not
/// interchangeable, even when both are expressed in tokens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EstimatorBasisV1 {
    pub id: String,
    pub version: String,
}

impl EstimatorBasisV1 {
    pub fn new(id: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            version: version.into(),
        }
    }

    pub fn validate(&self) -> Result<(), HostObservationValidationError> {
        require_nonempty("estimatorBasis.id", &self.id)?;
        require_nonempty("estimatorBasis.version", &self.version)
    }
}

/// A token value tied to an explicit estimator basis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TokenEstimateV1 {
    pub basis: EstimatorBasisV1,
    pub estimate: ObservedFieldV1<u64>,
}

impl TokenEstimateV1 {
    pub fn complete(basis: EstimatorBasisV1, value: u64) -> Self {
        Self {
            basis,
            estimate: ObservedFieldV1::complete(value),
        }
    }

    pub fn unavailable(basis: EstimatorBasisV1, reason: ObservationUnavailableReasonV1) -> Self {
        Self {
            basis,
            estimate: ObservedFieldV1::unavailable(reason),
        }
    }

    pub fn validate(&self, field: &str) -> Result<(), HostObservationValidationError> {
        self.basis.validate()?;
        self.estimate.validate(field)
    }
}

/// Compare estimates only when both carry the same basis and exact values.
pub fn compare_token_estimates(
    left: &TokenEstimateV1,
    right: &TokenEstimateV1,
) -> Result<Ordering, HostObservationValidationError> {
    left.validate("left.estimate")?;
    right.validate("right.estimate")?;
    ensure_same_estimator_basis(&left.basis, &right.basis)?;
    let left_value =
        left.estimate
            .value
            .ok_or_else(|| HostObservationValidationError::UnavailableValue {
                field: "left.estimate".to_string(),
            })?;
    let right_value =
        right
            .estimate
            .value
            .ok_or_else(|| HostObservationValidationError::UnavailableValue {
                field: "right.estimate".to_string(),
            })?;
    Ok(left_value.cmp(&right_value))
}

/// Add estimates only when both carry the same basis and exact values.
pub fn sum_token_estimates(
    left: &TokenEstimateV1,
    right: &TokenEstimateV1,
) -> Result<TokenEstimateV1, HostObservationValidationError> {
    left.validate("left.estimate")?;
    right.validate("right.estimate")?;
    ensure_same_estimator_basis(&left.basis, &right.basis)?;
    let left_value =
        left.estimate
            .value
            .ok_or_else(|| HostObservationValidationError::UnavailableValue {
                field: "left.estimate".to_string(),
            })?;
    let right_value =
        right
            .estimate
            .value
            .ok_or_else(|| HostObservationValidationError::UnavailableValue {
                field: "right.estimate".to_string(),
            })?;
    let value = left_value.checked_add(right_value).ok_or_else(|| {
        HostObservationValidationError::InvalidField {
            field: "estimate".to_string(),
            reason: "token estimate overflow".to_string(),
        }
    })?;
    Ok(TokenEstimateV1::complete(left.basis.clone(), value))
}

/// Mechanical facts emitted by a host execution harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionObservationKindV1 {
    ModelSelected,
    RouteSelected,
    ModelCallStarted,
    ModelCallFinished,
    ModelCallFailed,
    ToolCall,
    ToolResult,
    ToolFailure,
    WriteEdit,
    VerificationStarted,
    VerificationResult,
    ApprovalRequested,
    ApprovalResult,
    Retry,
    Timeout,
    Cancellation,
    PlanCreated,
    PlanRevised,
    TaskScopeChanged,
    SubagentStarted,
    SubagentFinished,
    ArtifactProduced,
    CompletionClaimEmitted,
    CompletionAccepted,
    CompletionRejected,
    MembraneRetrieval,
    PushReduction,
    PushRestore,
    EvaluatorOutcome,
}

/// Host/provider usage, explicitly tied to estimator bases.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionUsageV1 {
    pub input_tokens: TokenEstimateV1,
    pub output_tokens: TokenEstimateV1,
    pub cache_read_input_tokens: ObservedFieldV1<u64>,
    pub cache_write_input_tokens: ObservedFieldV1<u64>,
}

impl ExecutionUsageV1 {
    pub fn validate(&self, field: &str) -> Result<(), HostObservationValidationError> {
        self.input_tokens
            .validate(&format!("{field}.inputTokens"))?;
        self.output_tokens
            .validate(&format!("{field}.outputTokens"))?;
        self.cache_read_input_tokens
            .validate(&format!("{field}.cacheReadInputTokens"))?;
        self.cache_write_input_tokens
            .validate(&format!("{field}.cacheWriteInputTokens"))
    }
}

/// A host cost measurement.  Unit and basis stay observable inputs; Membrane
/// does not infer currency or redistribute costs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionCostV1 {
    pub amount: ObservedFieldV1<u64>,
    pub unit: ObservedFieldV1<String>,
    pub basis: ObservedFieldV1<String>,
}

impl ExecutionCostV1 {
    pub fn validate(&self, field: &str) -> Result<(), HostObservationValidationError> {
        self.amount.validate(&format!("{field}.amount"))?;
        self.unit.validate(&format!("{field}.unit"))?;
        self.basis.validate(&format!("{field}.basis"))?;
        if let Some(unit) = &self.unit.value {
            require_nonempty(&format!("{field}.unit"), unit)?;
        }
        if let Some(basis) = &self.basis.value {
            require_nonempty(&format!("{field}.basis"), basis)?;
        }
        Ok(())
    }
}

/// A completion claim/result emitted by the host, without semantic labels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompletionEmissionV1 {
    pub emission_id: String,
    pub status: CompletionEmissionStatusV1,
    pub emitted_at_unix_ms: u64,
    pub artifact_refs: ObservedFieldV1<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionEmissionStatusV1 {
    Claimed,
    Accepted,
    Rejected,
}

impl CompletionEmissionV1 {
    pub fn validate(&self, field: &str) -> Result<(), HostObservationValidationError> {
        require_nonempty(&format!("{field}.emissionId"), &self.emission_id)?;
        self.artifact_refs
            .validate(&format!("{field}.artifactRefs"))?;
        if let Some(refs) = &self.artifact_refs.value {
            validate_refs(&format!("{field}.artifactRefs"), refs)?;
        }
        Ok(())
    }
}

/// H4: one structured execution observation from CodeRight or another host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionObservationV1 {
    pub schema_version: u32,
    pub observation_id: String,
    pub session_id: String,
    pub task_id: ObservedFieldV1<String>,
    pub parent_task_id: ObservedFieldV1<String>,
    pub agent_id: ObservedFieldV1<String>,
    pub agent_role: ObservedFieldV1<String>,
    pub observed_at_unix_ms: u64,
    pub model: String,
    pub provider: String,
    pub client: String,
    pub route_policy: ObservedFieldV1<String>,
    pub observation_kind: ExecutionObservationKindV1,
    pub subject_id: ObservedFieldV1<String>,
    pub tool: ObservedFieldV1<String>,
    pub call_id: ObservedFieldV1<String>,
    pub outcome: ObservedFieldV1<String>,
    pub exit_code: ObservedFieldV1<i32>,
    pub duration_ms: ObservedFieldV1<u64>,
    pub usage: ObservedFieldV1<ExecutionUsageV1>,
    pub tool_cost: ObservedFieldV1<ExecutionCostV1>,
    pub asset_cost: ObservedFieldV1<ExecutionCostV1>,
    pub repository: ObservedFieldV1<String>,
    pub scope: ObservedFieldV1<String>,
    pub artifact_refs: ObservedFieldV1<Vec<String>>,
    pub evidence_refs: ObservedFieldV1<Vec<String>>,
    pub completion: ObservedFieldV1<CompletionEmissionV1>,
    pub provenance_receipt: HostObservationProvenanceV1,
}

impl ExecutionObservationV1 {
    pub fn validate(&self) -> Result<(), HostObservationValidationError> {
        validate_schema(
            "ExecutionObservationV1",
            self.schema_version,
            EXECUTION_OBSERVATION_SCHEMA_VERSION,
        )?;
        require_nonempty("observationId", &self.observation_id)?;
        require_nonempty("sessionId", &self.session_id)?;
        require_nonempty("model", &self.model)?;
        require_nonempty("provider", &self.provider)?;
        require_nonempty("client", &self.client)?;
        self.provenance_receipt.validate()?;
        self.task_id.validate("taskId")?;
        self.parent_task_id.validate("parentTaskId")?;
        self.agent_id.validate("agentId")?;
        self.agent_role.validate("agentRole")?;
        self.route_policy.validate("routePolicy")?;
        self.subject_id.validate("subjectId")?;
        self.tool.validate("tool")?;
        self.call_id.validate("callId")?;
        self.outcome.validate("outcome")?;
        self.exit_code.validate("exitCode")?;
        self.duration_ms.validate("durationMs")?;
        self.usage.validate("usage")?;
        if let Some(usage) = &self.usage.value {
            usage.validate("usage")?;
        }
        self.tool_cost.validate("toolCost")?;
        if let Some(cost) = &self.tool_cost.value {
            cost.validate("toolCost")?;
        }
        self.asset_cost.validate("assetCost")?;
        if let Some(cost) = &self.asset_cost.value {
            cost.validate("assetCost")?;
        }
        self.repository.validate("repository")?;
        self.scope.validate("scope")?;
        self.artifact_refs.validate("artifactRefs")?;
        self.evidence_refs.validate("evidenceRefs")?;
        if let Some(refs) = &self.artifact_refs.value {
            validate_refs("artifactRefs", refs)?;
        }
        if let Some(refs) = &self.evidence_refs.value {
            validate_refs("evidenceRefs", refs)?;
        }
        self.completion.validate("completion")?;
        if let Some(completion) = &self.completion.value {
            completion.validate("completion")?;
        }
        Ok(())
    }
}

/// H6: an evaluation result supplied by the host's evaluator harness.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvaluationOutcomeV1 {
    pub schema_version: u32,
    pub outcome_id: String,
    pub evaluator_id: String,
    pub evaluator_version: String,
    pub dataset_id: ObservedFieldV1<String>,
    pub case_id: ObservedFieldV1<String>,
    pub experiment_id: ObservedFieldV1<String>,
    pub trace_id: ObservedFieldV1<String>,
    pub session_id: ObservedFieldV1<String>,
    pub task_id: ObservedFieldV1<String>,
    pub model: ObservedFieldV1<String>,
    pub client: ObservedFieldV1<String>,
    pub route_policy: ObservedFieldV1<String>,
    pub score_type: String,
    pub score: ObservedFieldV1<f64>,
    pub verdict: ObservedFieldV1<String>,
    pub expected: ObservedFieldV1<String>,
    pub execution_receipt: ObservedFieldV1<String>,
    pub baseline_ref: ObservedFieldV1<String>,
    pub observed_at_unix_ms: u64,
    pub provenance_receipt: HostObservationProvenanceV1,
}

impl EvaluationOutcomeV1 {
    pub fn validate(&self) -> Result<(), HostObservationValidationError> {
        validate_schema(
            "EvaluationOutcomeV1",
            self.schema_version,
            EVALUATION_OUTCOME_SCHEMA_VERSION,
        )?;
        require_nonempty("outcomeId", &self.outcome_id)?;
        require_nonempty("evaluatorId", &self.evaluator_id)?;
        require_nonempty("evaluatorVersion", &self.evaluator_version)?;
        require_nonempty("scoreType", &self.score_type)?;
        self.provenance_receipt.validate()?;
        self.dataset_id.validate("datasetId")?;
        self.case_id.validate("caseId")?;
        self.experiment_id.validate("experimentId")?;
        self.trace_id.validate("traceId")?;
        self.session_id.validate("sessionId")?;
        self.task_id.validate("taskId")?;
        self.model.validate("model")?;
        self.client.validate("client")?;
        self.route_policy.validate("routePolicy")?;
        self.score.validate("score")?;
        if let Some(score) = self.score.value {
            if !score.is_finite() {
                return Err(HostObservationValidationError::InvalidField {
                    field: "score".to_string(),
                    reason: "score must be finite".to_string(),
                });
            }
        }
        self.verdict.validate("verdict")?;
        self.expected.validate("expected")?;
        self.execution_receipt.validate("executionReceipt")?;
        self.baseline_ref.validate("baselineRef")
    }
}

/// H8: the host's true remaining rendered-context ceiling at request time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemainingContextCeilingV1 {
    pub schema_version: u32,
    pub ceiling_id: String,
    pub session_id: String,
    pub task_id: ObservedFieldV1<String>,
    pub requested_at_unix_ms: u64,
    pub remaining_tokens: TokenEstimateV1,
    pub provenance_receipt: HostObservationProvenanceV1,
}

impl RemainingContextCeilingV1 {
    pub fn validate(&self) -> Result<(), HostObservationValidationError> {
        validate_schema(
            "RemainingContextCeilingV1",
            self.schema_version,
            REMAINING_CONTEXT_CEILING_SCHEMA_VERSION,
        )?;
        require_nonempty("ceilingId", &self.ceiling_id)?;
        require_nonempty("sessionId", &self.session_id)?;
        self.task_id.validate("taskId")?;
        self.remaining_tokens.validate("remainingTokens")?;
        self.provenance_receipt.validate()
    }
}

/// One identity in the host's loaded-context set.  The host reports exact
/// opaque refs/digests; it does not send rendered content through this seam.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LoadedContextIdentityV1 {
    pub identity: String,
    pub source_ref: String,
    pub source_digest: String,
}

impl LoadedContextIdentityV1 {
    pub fn validate(&self, field: &str) -> Result<(), HostObservationValidationError> {
        require_nonempty(&format!("{field}.identity"), &self.identity)?;
        require_nonempty(&format!("{field}.sourceRef"), &self.source_ref)?;
        validate_sha256(&format!("{field}.sourceDigest"), &self.source_digest)
    }
}

/// H9: currently loaded context identities, including a host compaction
/// generation so a post-compaction update can replace an older set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LoadedContextIdentitiesV1 {
    pub schema_version: u32,
    pub snapshot_id: String,
    pub session_id: String,
    pub compaction_generation: ObservedFieldV1<u64>,
    pub identities: ObservedFieldV1<Vec<LoadedContextIdentityV1>>,
    pub observed_at_unix_ms: u64,
    pub provenance_receipt: HostObservationProvenanceV1,
}

impl LoadedContextIdentitiesV1 {
    pub fn validate(&self) -> Result<(), HostObservationValidationError> {
        validate_schema(
            "LoadedContextIdentitiesV1",
            self.schema_version,
            LOADED_CONTEXT_IDENTITIES_SCHEMA_VERSION,
        )?;
        require_nonempty("snapshotId", &self.snapshot_id)?;
        require_nonempty("sessionId", &self.session_id)?;
        self.compaction_generation
            .validate("compactionGeneration")?;
        self.identities.validate("identities")?;
        if let Some(identities) = &self.identities.value {
            for (index, identity) in identities.iter().enumerate() {
                identity.validate(&format!("identities[{index}]"))?;
            }
        }
        self.provenance_receipt.validate()
    }
}

/// H10 status after the host serialized a packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PacketDeliveryAcknowledgementStatusV1 {
    Acknowledged,
    Rejected,
}

/// H10: acknowledgement proving host-side packet serialization completed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PacketDeliveryAcknowledgementV1 {
    pub schema_version: u32,
    pub acknowledgement_id: String,
    pub packet_digest: String,
    pub host_serialized_digest: String,
    pub session_id: String,
    pub task_id: ObservedFieldV1<String>,
    pub status: PacketDeliveryAcknowledgementStatusV1,
    pub serialized_bytes: ObservedFieldV1<u64>,
    pub acknowledged_at_unix_ms: u64,
    pub provenance_receipt: HostObservationProvenanceV1,
}

impl PacketDeliveryAcknowledgementV1 {
    pub fn validate(&self) -> Result<(), HostObservationValidationError> {
        validate_schema(
            "PacketDeliveryAcknowledgementV1",
            self.schema_version,
            PACKET_DELIVERY_ACKNOWLEDGEMENT_SCHEMA_VERSION,
        )?;
        require_nonempty("acknowledgementId", &self.acknowledgement_id)?;
        require_nonempty("sessionId", &self.session_id)?;
        validate_sha256("packetDigest", &self.packet_digest)?;
        validate_sha256("hostSerializedDigest", &self.host_serialized_digest)?;
        self.task_id.validate("taskId")?;
        self.serialized_bytes.validate("serializedBytes")?;
        self.provenance_receipt.validate()
    }
}

/// Error returned before a host observation can cross the seam.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum HostObservationValidationError {
    #[error("{shape} has unsupported schema_version {found}; expected {expected}")]
    UnsupportedSchemaVersion {
        shape: &'static str,
        found: u32,
        expected: u32,
    },
    #[error("{field} must be non-empty")]
    RequiredField { field: String },
    #[error("invalid {field}: {reason}")]
    InvalidField { field: String, reason: String },
    #[error("invalid coverage for {field}: {reason}")]
    Coverage { field: String, reason: String },
    #[error("value for {field} is unavailable")]
    UnavailableValue { field: String },
    #[error("token estimator bases differ: {left} != {right}")]
    EstimatorBasisMismatch { left: String, right: String },
}

fn validate_schema(
    shape: &'static str,
    found: u32,
    expected: u32,
) -> Result<(), HostObservationValidationError> {
    if found != expected {
        return Err(HostObservationValidationError::UnsupportedSchemaVersion {
            shape,
            found,
            expected,
        });
    }
    Ok(())
}

fn require_nonempty(field: &str, value: &str) -> Result<(), HostObservationValidationError> {
    if value.trim().is_empty() {
        Err(HostObservationValidationError::RequiredField {
            field: field.to_string(),
        })
    } else {
        Ok(())
    }
}

fn validate_refs(field: &str, refs: &[String]) -> Result<(), HostObservationValidationError> {
    for (index, reference) in refs.iter().enumerate() {
        require_nonempty(&format!("{field}[{index}]"), reference)?;
    }
    Ok(())
}

fn validate_sha256(field: &str, digest: &str) -> Result<(), HostObservationValidationError> {
    let Some(hex) = digest.strip_prefix("sha256:") else {
        return Err(HostObservationValidationError::InvalidField {
            field: field.to_string(),
            reason: "must use sha256: prefix".to_string(),
        });
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(HostObservationValidationError::InvalidField {
            field: field.to_string(),
            reason: "must contain exactly 64 hexadecimal characters".to_string(),
        });
    }
    Ok(())
}

pub fn ensure_same_estimator_basis(
    left: &EstimatorBasisV1,
    right: &EstimatorBasisV1,
) -> Result<(), HostObservationValidationError> {
    if left != right {
        return Err(HostObservationValidationError::EstimatorBasisMismatch {
            left: format!("{}@{}", left.id, left.version),
            right: format!("{}@{}", right.id, right.version),
        });
    }
    Ok(())
}

/// Compatibility aliases make the CodeRight ownership explicit without
/// introducing a second, divergent schema.
pub type CodeRightExecutionObservationV1 = ExecutionObservationV1;
pub type CodeRightEvaluationOutcomeV1 = EvaluationOutcomeV1;
pub type DeliveryAcknowledgementV1 = PacketDeliveryAcknowledgementV1;
pub type PacketDeliveryAcknowledgmentV1 = PacketDeliveryAcknowledgementV1;
pub type HostProvenanceReceiptV1 = HostObservationProvenanceV1;
pub type HostObservationCoverageV1 = ObservationCoverageV1;
pub type HostObservationUnavailableReasonV1 = ObservationUnavailableReasonV1;
pub type ObservationCoverage = ObservationCoverageV1;
pub type ObservationUnavailableReason = ObservationUnavailableReasonV1;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn digest() -> String {
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into()
    }

    fn provenance() -> HostObservationProvenanceV1 {
        HostObservationProvenanceV1::new("receipt-1", "coderight", 1_700_000_000_000, digest())
    }

    fn required_execution() -> ExecutionObservationV1 {
        let basis = EstimatorBasisV1::new("coderight-tokenizer", "1");
        ExecutionObservationV1 {
            schema_version: EXECUTION_OBSERVATION_SCHEMA_VERSION,
            observation_id: "obs-1".into(),
            session_id: "session-1".into(),
            task_id: ObservedFieldV1::complete("task-1".into()),
            parent_task_id: ObservedFieldV1::unavailable(
                ObservationUnavailableReasonV1::ProviderOmitted,
            ),
            agent_id: ObservedFieldV1::unavailable(ObservationUnavailableReasonV1::HostUnsupported),
            agent_role: ObservedFieldV1::unavailable(
                ObservationUnavailableReasonV1::HostUnsupported,
            ),
            observed_at_unix_ms: 1_700_000_000_001,
            model: "model-1".into(),
            provider: "provider-1".into(),
            client: "coderight".into(),
            route_policy: ObservedFieldV1::complete("control".into()),
            observation_kind: ExecutionObservationKindV1::ModelCallFinished,
            subject_id: ObservedFieldV1::complete("call-1".into()),
            tool: ObservedFieldV1::unavailable(ObservationUnavailableReasonV1::ProviderOmitted),
            call_id: ObservedFieldV1::complete("call-1".into()),
            outcome: ObservedFieldV1::complete("success".into()),
            exit_code: ObservedFieldV1::unavailable(
                ObservationUnavailableReasonV1::HostUnsupported,
            ),
            duration_ms: ObservedFieldV1::complete(25),
            usage: ObservedFieldV1::complete(ExecutionUsageV1 {
                input_tokens: TokenEstimateV1::complete(basis.clone(), 20),
                output_tokens: TokenEstimateV1::complete(basis, 5),
                cache_read_input_tokens: ObservedFieldV1::complete(0),
                cache_write_input_tokens: ObservedFieldV1::complete(0),
            }),
            tool_cost: ObservedFieldV1::unavailable(
                ObservationUnavailableReasonV1::HostUnsupported,
            ),
            asset_cost: ObservedFieldV1::unavailable(
                ObservationUnavailableReasonV1::HostUnsupported,
            ),
            repository: ObservedFieldV1::unavailable(
                ObservationUnavailableReasonV1::HostUnsupported,
            ),
            scope: ObservedFieldV1::complete("workspace".into()),
            artifact_refs: ObservedFieldV1::complete(Vec::new()),
            evidence_refs: ObservedFieldV1::complete(Vec::new()),
            completion: ObservedFieldV1::unavailable(
                ObservationUnavailableReasonV1::ProviderOmitted,
            ),
            provenance_receipt: provenance(),
        }
    }

    #[test]
    fn unavailable_value_has_typed_reason_and_never_serializes_zero() {
        let field =
            ObservedFieldV1::<u64>::unavailable(ObservationUnavailableReasonV1::HostUnsupported);
        field.validate("tokens").unwrap();
        let value = serde_json::to_value(field).unwrap();
        assert_eq!(value["coverage"], "unavailable");
        assert_eq!(value["unavailableReason"], "host_unsupported");
        assert!(value.get("value").is_none());
        assert!(!value.to_string().contains(":0"));
    }

    #[test]
    fn incomplete_coverage_without_reason_is_rejected() {
        let field = ObservedFieldV1::<u64> {
            coverage: ObservationCoverageV1::Partial,
            value: None,
            unavailable_reason: None,
        };
        assert!(matches!(
            field.validate("tokens"),
            Err(HostObservationValidationError::Coverage { .. })
        ));
    }

    #[test]
    fn unavailable_value_cannot_smuggle_zero() {
        let field = ObservedFieldV1 {
            coverage: ObservationCoverageV1::Unavailable,
            value: Some(0_u64),
            unavailable_reason: Some(ObservationUnavailableReasonV1::ProviderOmitted),
        };
        assert!(field.validate("tokens").is_err());
    }

    #[test]
    fn estimator_bases_must_match_before_comparison_or_sum() {
        let left = TokenEstimateV1::complete(EstimatorBasisV1::new("bytes-ratio", "1"), 10);
        let right = TokenEstimateV1::complete(EstimatorBasisV1::new("provider", "1"), 10);
        assert!(matches!(
            compare_token_estimates(&left, &right),
            Err(HostObservationValidationError::EstimatorBasisMismatch { .. })
        ));
        assert!(matches!(
            sum_token_estimates(&left, &right),
            Err(HostObservationValidationError::EstimatorBasisMismatch { .. })
        ));
    }

    #[test]
    fn same_estimator_basis_can_be_compared_and_summed() {
        let basis = EstimatorBasisV1::new("provider", "1");
        let left = TokenEstimateV1::complete(basis.clone(), 10);
        let right = TokenEstimateV1::complete(basis, 20);
        assert_eq!(
            compare_token_estimates(&left, &right).unwrap(),
            Ordering::Less
        );
        assert_eq!(
            sum_token_estimates(&left, &right).unwrap().estimate.value,
            Some(30)
        );
    }

    #[test]
    fn host_record_is_closed_against_adapt_labels_and_admission_bypass() {
        let mut value = serde_json::to_value(required_execution()).unwrap();
        let object = value.as_object_mut().unwrap();
        object.insert("category".into(), json!("preference"));
        assert!(serde_json::from_value::<ExecutionObservationV1>(value).is_err());

        let mut value = serde_json::to_value(required_execution()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("admitted".into(), json!(true));
        assert!(serde_json::from_value::<ExecutionObservationV1>(value).is_err());
    }

    #[test]
    fn execution_observation_validates_without_semantic_prelabel() {
        let observation = required_execution();
        observation.validate().unwrap();
        let encoded = serde_json::to_value(observation).unwrap();
        assert_eq!(
            encoded["schemaVersion"],
            EXECUTION_OBSERVATION_SCHEMA_VERSION
        );
        assert!(encoded.get("category").is_none());
        assert!(encoded.get("admitted").is_none());
    }

    #[test]
    fn provenance_receipt_is_required_and_digest_bound() {
        let mut observation = required_execution();
        observation.provenance_receipt.receipt_digest = "not-a-digest".into();
        assert!(matches!(
            observation.validate(),
            Err(HostObservationValidationError::InvalidField { field, .. }) if field == "provenance.receipt_digest"
        ));
    }

    #[test]
    fn loaded_context_empty_set_is_exact_not_unavailable() {
        let identities = LoadedContextIdentitiesV1 {
            schema_version: LOADED_CONTEXT_IDENTITIES_SCHEMA_VERSION,
            snapshot_id: "snapshot-1".into(),
            session_id: "session-1".into(),
            compaction_generation: ObservedFieldV1::complete(0),
            identities: ObservedFieldV1::complete(Vec::new()),
            observed_at_unix_ms: 1_700_000_000_000,
            provenance_receipt: provenance(),
        };
        identities.validate().unwrap();
        assert_eq!(identities.identities.value, Some(Vec::new()));
    }

    #[test]
    fn coderight_h8_wire_shape_is_admitted() {
        let value = json!({
            "schemaVersion": REMAINING_CONTEXT_CEILING_SCHEMA_VERSION,
            "ceilingId": "coderight-h8:session-1:1700000000000",
            "sessionId": "session-1",
            "taskId": {
                "coverage": "unavailable",
                "unavailableReason": "not_instrumented"
            },
            "requestedAtUnixMs": 1_700_000_000_000_u64,
            "remainingTokens": {
                "basis": {
                    "id": "provider_reported_context_tokens",
                    "version": "1"
                },
                "estimate": {
                    "coverage": "complete",
                    "value": 60
                }
            },
            "provenanceReceipt": {
                "schemaVersion": HOST_OBSERVATION_PROVENANCE_SCHEMA_VERSION,
                "receiptId": "coderight-h8:session-1:1700000000000",
                "source": "coderight",
                "observedAtUnixMs": 1_700_000_000_000_u64,
                "receiptDigest": digest()
            }
        });
        let ceiling: RemainingContextCeilingV1 = serde_json::from_value(value).unwrap();
        ceiling.validate().unwrap();
        assert_eq!(ceiling.remaining_tokens.estimate.value, Some(60));
    }
}
