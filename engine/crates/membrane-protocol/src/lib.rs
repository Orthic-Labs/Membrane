//! # membrane-protocol
//!
//! The **contract source of truth** for the Membrane protocol's five typed
//! shapes:
//!
//!   | Shape                  | Rust type                | Schema (v1)                        |
//!   |------------------------|--------------------------|------------------------------------|
//!   | ScopeGrant             | [`ScopeGrantV1`]         | `schemas/scope-grant.v1.schema.json` |
//!   | ContextCandidateSet    | [`ContextCandidateSetV1`]| `schemas/context-candidate-set.v1.schema.json` |
//!   | ContextPacket          | [`ContextPacketV1`]      | `schemas/context-packet.v1.schema.json` |
//!   | ContextReceipt         | [`ContextReceiptV1`]     | `schemas/context-receipt.v1.schema.json` |
//!   | KnowledgeEmission      | [`KnowledgeEmissionV1`]  | `schemas/knowledge-emission.v1.schema.json` |
//!
//! The Rust types in [`types`] are authoritative. The JSON Schemas under
//! `schemas/` are derived from them, and golden fixtures under `schemas/operations/`
//! are the shared canonical instances. The crate's tests prove — and the
//! TypeScript binding under `bindings/` independently re-proves — that each
//! golden fixture:
//!
//!   1. validates against its JSON Schema,
//!   2. deserializes + re-serializes to the **identical canonical bytes**, and
//!   3. yields the **same `sha256:` digest** in Rust and in TypeScript.
//!
//! See `docs/protocol/source-of-truth.md` for the full contract story.

pub mod background_review;
pub mod canonical;
pub mod compatibility_policy;
pub mod compression;
pub mod diagnostics;
pub mod federation;
pub mod fusion;
pub mod heartbeat;
pub mod host_observation;
pub mod hub;
pub mod installation;
pub mod lease;
pub mod membrane_status;
pub mod observable_event;
pub mod operations;
pub mod portable_pack;
pub mod provider_readiness;
pub mod push;
pub mod release_channel;
pub mod source_resolution;
pub mod status;
pub mod team_policy;
pub mod tray_daemon;
pub mod types;

pub use background_review::{
    BackgroundReviewActivitySignalV1, BackgroundReviewConfigError, BackgroundReviewConfigV1,
    BackgroundReviewExecutionStatusV1, BackgroundReviewExecutionV1, BackgroundReviewJobKindV1,
    BackgroundReviewJobV1, BackgroundReviewObservationReceiptV1, BackgroundReviewObservationV1,
    BackgroundReviewProposalRefV1, BackgroundReviewReasonV1, BackgroundReviewStatusV1,
    BackgroundReviewValidationError, BACKGROUND_REVIEW_ACTIVITY_SCHEMA_VERSION,
    BACKGROUND_REVIEW_EXECUTION_SCHEMA_VERSION, BACKGROUND_REVIEW_RECEIPT_SCHEMA_VERSION,
    BACKGROUND_REVIEW_SCHEMA_VERSION,
};
pub use canonical::{canonical_json_of, canonicalize, digest_str};
pub use compatibility_policy::{
    evaluate_compatibility, evaluate_release_channel, AdmittedRelease, CompatibilityViolation,
    RawReleaseDescriptor,
};
pub use compression::{
    CompressionReceiptV1, DroppedSpanV1, ImmutableSourceError, ImmutableSourceReason,
    NonEmptyString, SpanV1, COMPRESSION_RECEIPT_SCHEMA_VERSION,
};
pub use diagnostics::{
    evaluate_gate, AggregateDeltaV1, AggregateIssueDelta, BlueprintDeltaV1, BlueprintFreshness,
    CapabilityVocabulary, ChangedFileHashV1, ConvergenceClass, CostClass, CoverageLaneV1,
    CoverageObligationV1, DeltaClassification, DiagnosticEvidenceSnapshotV1,
    DiagnosticGateDecisionV1, DiagnosticIssueV1, ExactnessRequirement, GateOutcome,
    GatePolicyProfileV1, LaneState, ObligationState, ObservationV1, RequiredScope, SeverityHint,
    SourceClass, SourceRange, TypedOmission, WorkspaceEpochOrigin, WorkspaceEpochV1,
    DIAGNOSTIC_EVIDENCE_SNAPSHOT_SCHEMA_VERSION, DIAGNOSTIC_GATE_DECISION_SCHEMA_VERSION,
    WORKSPACE_EPOCH_SCHEMA_VERSION,
};
pub use federation::{
    sort_candidates, sort_omissions, sort_provider_ids, sort_provider_outputs, sort_warnings,
    Candidate, DeadlineBudget, FederationCandidate, FederationCandidateV1, FederationDiagnosticsV1,
    FederationError, FederationErrorV1, FederationOmission, FederationOmissionV1,
    FederationProviderStatus, FederationProviderStatusV1, FederationRequest, FederationRequestV1,
    FederationResponse, FederationResponseV1, FederationStatus, FederationValidationError,
    FreshnessSnapshotV1, InsufficientConfidenceLaneSearchV1, InsufficientConfidenceReasonV1,
    InsufficientConfidenceStatusV1, InsufficientConfidenceV1, ProviderContext, ProviderContextV1,
    ProviderDiagnosticsV1, ProviderId, ProviderIdV1, ProviderKind, ProviderKindV1,
    ProviderOmissionV1, ProviderOutput, ProviderOutputV1, ProviderStatusV1, ProviderWarningV1,
    PublicationFenceChangeV1, PublicationFenceStatusV1, PublicationFenceV1, ReasonCode,
    ReasonCodeV1, WarningSeverity, FEDERATION_REQUEST_SCHEMA_VERSION,
    FEDERATION_RESPONSE_SCHEMA_VERSION, INSUFFICIENT_CONFIDENCE_POLICY,
    INSUFFICIENT_CONFIDENCE_SCHEMA_VERSION, PROVIDER_ORDER, PROVIDER_OUTPUT_SCHEMA_VERSION,
    PUBLICATION_FENCE_POLICY, PUBLICATION_FENCE_SCHEMA_VERSION,
};
pub use fusion::{FusionDecisionV1, FusionReceiptV1};
pub use heartbeat::{
    AdapterHeartbeatV1, AdapterStatus, MechanismReceiptV1, ADAPTER_HEARTBEAT_SCHEMA_VERSION,
};
pub use host_observation::{
    compare_token_estimates, ensure_same_estimator_basis, sum_token_estimates,
    CodeRightEvaluationOutcomeV1, CodeRightExecutionObservationV1, CompletionEmissionStatusV1,
    CompletionEmissionV1, DeliveryAcknowledgementV1, EstimatorBasisV1, EvaluationOutcomeV1,
    ExecutionCostV1, ExecutionObservationKindV1, ExecutionObservationV1, ExecutionUsageV1,
    HostObservationCoverageV1, HostObservationProvenanceV1, HostObservationUnavailableReasonV1,
    HostObservationValidationError, HostProvenanceReceiptV1, LoadedContextIdentitiesV1,
    LoadedContextIdentityV1, ObservationCoverage, ObservationCoverageV1,
    ObservationUnavailableReason, ObservationUnavailableReasonV1, ObservedFieldV1,
    PacketDeliveryAcknowledgementStatusV1, PacketDeliveryAcknowledgementV1,
    PacketDeliveryAcknowledgmentV1, RemainingContextCeilingV1, TokenEstimateV1,
    EVALUATION_OUTCOME_SCHEMA_VERSION, EXECUTION_OBSERVATION_SCHEMA_VERSION,
    HOST_OBSERVATION_PROVENANCE_SCHEMA_VERSION, HOST_OBSERVATION_SCHEMA_VERSION,
    LOADED_CONTEXT_IDENTITIES_SCHEMA_VERSION, PACKET_DELIVERY_ACKNOWLEDGEMENT_SCHEMA_VERSION,
    PACKET_DELIVERY_ACKNOWLEDGMENT_SCHEMA_VERSION, REMAINING_CONTEXT_CEILING_SCHEMA_VERSION,
};
pub use hub::{
    AdmissionReasonCountV1, HubAdmissionV1, HubCapabilitiesV1, HubSectionV1, HubSnapshotV1,
    HubStateV1, HubStreamV1, HubSubsystemV1, HubSubsystemsV1, MembraneUnavailableReasonV1,
    MembraneUnavailableV1, SubsystemStateV1, HUB_ADMISSION_SCHEMA_VERSION, HUB_SCHEMA_VERSION,
};
pub use installation::{
    canonical_data_root, ApiSchemaV1, ComponentV1, HandshakeError, InstallationManifestV1,
    MANIFEST_SCHEMA_VERSION, PROTOCOL_SCHEMA_VERSION,
};
pub use lease::{
    ResidentEndpointV1, ResidentHelloV1, ResidentLeaseV1, ResidentLifecycleFrameV1,
    RESIDENT_LEASE_SCHEMA_VERSION,
};
pub use membrane_status::{
    membrane_parent_state, membrane_parent_state_from_supervisor_str, MembraneParentState,
    SUBSYSTEM_NAMES,
};
pub use observable_event::{
    ObservableEventKindV1, ObservableEventV1, OBSERVABLE_EVENT_SCHEMA_VERSION,
};
pub use operations::{
    operations, operations_slice, ErrorResult, OperationEnvelope, OperationIndexEntry,
    OperationParameter, OperationResult, OperationSpec, OperationsIndex, ResultKind, SuccessResult,
    OPERATIONS,
};
pub use portable_pack::{PortableContextPackV1, PortablePackError, PortablePackSignatureV1};
pub use provider_readiness::{
    ProviderIdentityV1, ProviderObservationV1, ProviderReadinessStateV1, ProviderReadinessV1,
    ProviderTestQueryV1, PROVIDER_READINESS_SCHEMA_VERSION,
};
pub use push::{
    PacketReductionPlanError, PacketReductionPlanSelectionError, PacketReductionPlanV1,
    PacketReductionRepresentationV1, PacketReductionSelectionError,
    PACKET_REDUCTION_PLAN_SCHEMA_VERSION,
};
pub use release_channel::{
    ReleaseChannel, ReleaseChannelV1, SchemaCompatibility, SupportState, SupportWindowV1,
    RELEASE_CHANNEL_SCHEMA_VERSION,
};
pub use source_resolution::{
    SourceResolutionReceiptV1, SourceResolutionStatusV1, SOURCE_RESOLUTION_RECEIPT_SCHEMA_VERSION,
};
pub use status::{
    AdapterLifecycleStatusV1, DeliveryReasonV1, DeliveryStatusV1, InstallationReasonV1,
    InstallationStatusV1, MultidimensionalStatusV1, MULTIDIMENSIONAL_STATUS_SCHEMA_VERSION,
};
pub use team_policy::{
    EncryptedReplicationEnvelopeV1, TeamPolicyReceiptV1, TeamPolicyScopeV1, TeamPolicySyncV1,
    TeamSyncOptInV1, TEAM_POLICY_SCHEMA_VERSION,
};
pub use tray_daemon::{
    decode_command_frame, decode_event_frame, decode_frame, decode_launch_frame, encode_frame,
    DaemonCommandKind, DaemonCommandV1, DaemonEventKind, DaemonEventV1, DaemonLaunchKind,
    DaemonLaunchV1, DaemonProtocolError, DAEMON_IPC_MAX_FRAME_BYTES, DAEMON_IPC_SCHEMA_VERSION,
};
pub use types::*;

use serde::Serialize;

/// The canonical byte contract shared with the TypeScript binding.
///
/// Implemented for every protocol type via its `Serialize` impl. The methods
/// return the canonical JSON (sorted keys, no whitespace) and its `sha256:`
/// digest — the exact bytes/digest the TS side must reproduce.
pub trait CanonicalSerialize: Serialize {
    /// Canonical JSON: object keys sorted, no insignificant whitespace.
    fn canonical_json(&self) -> String {
        canonical_json_of(self)
    }
    /// `sha256:<hex>` of [`Self::canonical_json`].
    fn canonical_digest(&self) -> String {
        crate::canonical::canonical_digest_of(self)
    }
}

// Blanket-impl the canonical contract for every serializable protocol type.
impl<T: Serialize> CanonicalSerialize for T {}

/// One typed contract shape: its name, its canonical JSON Schema, and its
/// golden fixture, all embedded so both the Rust tests and any downstream
/// consumer read exactly the same artifacts the TS binding reads from disk.
pub struct ContractShape {
    /// Stable shape name, e.g. `"ScopeGrantV1"`.
    pub name: &'static str,
    /// Repo-relative path of the JSON Schema (also embedded as `schema`).
    pub schema_path: &'static str,
    /// Repo-relative path of the golden fixture (also embedded as `fixture`).
    pub fixture_path: &'static str,
    /// The canonical JSON Schema text.
    pub schema: &'static str,
    /// The golden fixture JSON text.
    pub fixture: &'static str,
}

// Schemas & fixtures ship in this crate under `assets/`. The source repository
// retains matching canonical files at its root for its non-Rust bindings.
const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../..");

macro_rules! shape {
    ($name:literal, $schema:literal, $fixture:literal) => {
        ContractShape {
            name: $name,
            schema_path: concat!("schemas/", $schema),
            fixture_path: concat!("schemas/operations/", $fixture),
            schema: include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/assets/schemas/",
                $schema
            )),
            fixture: include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/assets/operations/",
                $fixture
            )),
        }
    };
}

/// Every typed contract shape, in canonical order. Used by the round-trip tests
/// and re-exported for downstream gates.
pub const SHAPES: &[ContractShape] = &[
    shape!(
        "ScopeGrantV1",
        "scope-grant.v1.schema.json",
        "scope-grant.v1.golden.json"
    ),
    shape!(
        "ContextCandidateSetV1",
        "context-candidate-set.v1.schema.json",
        "context-candidate-set.v1.golden.json"
    ),
    shape!(
        "ContextPacketV1",
        "context-packet.v1.schema.json",
        "context-packet.v1.golden.json"
    ),
    shape!(
        "ContextReceiptV1",
        "context-receipt.v1.schema.json",
        "context-receipt.v1.golden.json"
    ),
    shape!(
        "KnowledgeEmissionV1",
        "knowledge-emission.v1.schema.json",
        "knowledge-emission.v1.golden.json"
    ),
    shape!(
        "ObservableEventV1",
        "observable-event.v1.schema.json",
        "observable-event.v1.golden.json"
    ),
    shape!(
        "WorkspaceEpochV1",
        "workspace-epoch.v1.schema.json",
        "workspace-epoch.v1.golden.json"
    ),
    shape!(
        "DiagnosticEvidenceSnapshotV1",
        "diagnostic-evidence-snapshot.v1.schema.json",
        "diagnostic-evidence-snapshot.v1.golden.json"
    ),
    shape!(
        "DiagnosticGateDecisionV1",
        "diagnostic-gate-decision.v1.schema.json",
        "diagnostic-gate-decision.v1.golden.json"
    ),
    shape!(
        "CoverageObligationV1",
        "coverage-obligation.v1.schema.json",
        "coverage-obligation.v1.golden.json"
    ),
];

/// Absolute path to the repository root (for tests that read from disk).
pub fn repo_root() -> &'static str {
    ROOT
}
