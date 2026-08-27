//! Protocol shape for mechanically fitting a Membrane-authored packet.
//!
//! Push owns reduction mechanics, while Pull/Membrane owns which evidence is
//! admitted. A host may select only among these complete representations; it
//! must not construct a new one by dropping individual items.

use crate::host_observation::{
    ensure_same_estimator_basis, EstimatorBasisV1, HostObservationValidationError,
    ObservationCoverageV1, ObservationUnavailableReasonV1, RemainingContextCeilingV1,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Current [`PacketReductionPlanV1`] schema version.
pub const PACKET_REDUCTION_PLAN_SCHEMA_VERSION: u32 = 1;
pub const PACKET_REDUCTION_SELECTION_RECEIPT_SCHEMA_VERSION: u32 = 1;

/// One internally coherent representation published by Membrane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PacketReductionRepresentationV1 {
    /// Stable representation label such as `full`, `reduced_1`, or `floor`.
    pub id: String,
    /// Token count measured with plan's declared estimator basis.
    pub tokens: u64,
    /// Parent packet/document identity retained by this representation.
    pub parent_ref: String,
    /// Protected task-critical items retained verbatim by this representation.
    pub protected: Vec<String>,
    /// Evidence identities retained by this representation.
    pub evidence_refs: Vec<String>,
    /// Exact resolver paths for recovering source material.
    pub resolver_paths: Vec<String>,
    /// Minimum viable token size for this representation.
    pub minimum_viable_tokens: u64,
    /// Content-free note describing coverage supplied by this representation.
    pub coverage_note: String,
    /// Complete serialized content for this representation. The host emits
    /// this value exactly; it never edits packet membership itself.
    pub content: serde_json::Value,
}

/// A bounded ladder of complete representations for host-side capacity fit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PacketReductionPlanV1 {
    pub schema_version: u32,
    /// Basis used for every representation token count.
    pub estimator_basis: EstimatorBasisV1,
    pub representations: Vec<PacketReductionRepresentationV1>,
    /// The plan-wide protected set. Every representation must contain it.
    pub protected: Vec<String>,
    /// Smallest viable representation size for this plan.
    pub minimum_viable_tokens: u64,
}

/// Content-free receipt for one request-time host-capacity selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PacketReductionSelectionReceiptV1 {
    pub schema_version: u32,
    /// Stable packet identity shared by every representation in the plan.
    pub plan_ref: String,
    /// Host observation identity used for this selection.
    pub ceiling_id: String,
    pub session_id: String,
    pub selected_representation_id: String,
    pub selected_tokens: u64,
    pub remaining_tokens: u64,
    pub estimator_basis: EstimatorBasisV1,
    pub decision: String,
}

impl PacketReductionSelectionReceiptV1 {
    pub const SCHEMA_VERSION: u32 = PACKET_REDUCTION_SELECTION_RECEIPT_SCHEMA_VERSION;
}

/// Publication failures for [`PacketReductionPlanV1`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PacketReductionPlanError {
    #[error("packet reduction plan schema version is unsupported: {0}")]
    SchemaVersion(u32),
    #[error("packet reduction plan has no representations")]
    NoRepresentations,
    #[error("packet reduction plan contains an empty {0}")]
    EmptyField(&'static str),
    #[error("packet reduction plan contains duplicate values in {0}")]
    DuplicateValue(&'static str),
    #[error("packet reduction plan contains duplicate representation id: {0}")]
    DuplicateRepresentation(String),
    #[error("packet reduction plan has inconsistent minimum viable tokens")]
    MinimumViableTokens,
    #[error("packet reduction plan estimator basis is invalid: {0}")]
    InvalidEstimatorBasis(#[source] HostObservationValidationError),
    #[error("representation {representation} is below minimum viable size")]
    RepresentationBelowMinimum { representation: String },
    #[error("representation {representation} omits protected item: {item}")]
    ProtectedItemMissing {
        representation: String,
        item: String,
    },
}

/// Refusal reasons for host-side selection from a published plan.
///
/// Selection accepts only an exact, validated host ceiling.  In particular,
/// partial or unavailable observations never become an invented numeric
/// budget.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PacketReductionSelectionError {
    #[error("packet reduction plan is invalid: {0}")]
    InvalidPlan(#[source] PacketReductionPlanError),
    #[error("remaining context ceiling is invalid: {0}")]
    InvalidCeiling(#[source] HostObservationValidationError),
    #[error("packet and ceiling estimator bases differ: {0}")]
    EstimatorBasisMismatch(#[source] HostObservationValidationError),
    #[error("remaining context ceiling is not exact: coverage={coverage:?}, reason={reason:?}")]
    CapacityUnavailable {
        coverage: ObservationCoverageV1,
        reason: ObservationUnavailableReasonV1,
    },
    #[error(
        "no packet reduction representation fits remaining capacity: {remaining_tokens} tokens available; {minimum_viable_tokens} required"
    )]
    NoRepresentationFits {
        remaining_tokens: u64,
        minimum_viable_tokens: u64,
    },
}

/// Descriptive alias for callers that name selection after its plan contract.
pub type PacketReductionPlanSelectionError = PacketReductionSelectionError;

impl PacketReductionPlanV1 {
    pub const SCHEMA_VERSION: u32 = PACKET_REDUCTION_PLAN_SCHEMA_VERSION;

    /// Validate invariants required before publishing a plan to a host.
    pub fn validate(&self) -> Result<(), PacketReductionPlanError> {
        if self.schema_version != Self::SCHEMA_VERSION {
            return Err(PacketReductionPlanError::SchemaVersion(self.schema_version));
        }
        if self.representations.is_empty() {
            return Err(PacketReductionPlanError::NoRepresentations);
        }
        self.estimator_basis
            .validate()
            .map_err(PacketReductionPlanError::InvalidEstimatorBasis)?;
        validate_non_empty(&self.protected, "protected")?;
        ensure_unique(&self.protected, "protected")?;

        let mut ids = HashSet::with_capacity(self.representations.len());
        let mut viable = false;
        for representation in &self.representations {
            if representation.id.is_empty() {
                return Err(PacketReductionPlanError::EmptyField("representation id"));
            }
            if !ids.insert(&representation.id) {
                return Err(PacketReductionPlanError::DuplicateRepresentation(
                    representation.id.clone(),
                ));
            }
            if representation.parent_ref.is_empty() {
                return Err(PacketReductionPlanError::EmptyField("parent_ref"));
            }
            validate_non_empty(&representation.protected, "representation protected")?;
            ensure_unique(&representation.protected, "representation protected")?;
            validate_non_empty(&representation.evidence_refs, "evidence_refs")?;
            ensure_unique(&representation.evidence_refs, "evidence_refs")?;
            validate_non_empty(&representation.resolver_paths, "resolver_paths")?;
            ensure_unique(&representation.resolver_paths, "resolver_paths")?;
            if representation.coverage_note.is_empty() {
                return Err(PacketReductionPlanError::EmptyField("coverage_note"));
            }
            if representation.content.is_null() {
                return Err(PacketReductionPlanError::EmptyField("content"));
            }
            if representation.minimum_viable_tokens != self.minimum_viable_tokens {
                return Err(PacketReductionPlanError::MinimumViableTokens);
            }
            if representation.tokens < self.minimum_viable_tokens {
                return Err(PacketReductionPlanError::RepresentationBelowMinimum {
                    representation: representation.id.clone(),
                });
            }
            viable = true;
            for item in &self.protected {
                if !representation
                    .protected
                    .iter()
                    .any(|candidate| candidate == item)
                {
                    return Err(PacketReductionPlanError::ProtectedItemMissing {
                        representation: representation.id.clone(),
                        item: item.clone(),
                    });
                }
            }
        }
        if !viable {
            return Err(PacketReductionPlanError::MinimumViableTokens);
        }
        Ok(())
    }

    /// Whether plan satisfies publication invariants.
    pub fn is_publishable(&self) -> bool {
        self.validate().is_ok()
    }

    /// Select the largest complete representation that fits host capacity.
    ///
    /// The host must provide its validated H8 observation.  Membrane does not
    /// derive capacity from packet contents, character budgets, or defaults.
    /// Plan validation runs first, so every returned representation retains
    /// plan-wide protected items and complete resolver/evidence lineage.
    pub fn select_for_capacity(
        &self,
        ceiling: &RemainingContextCeilingV1,
    ) -> Result<&PacketReductionRepresentationV1, PacketReductionSelectionError> {
        self.validate()
            .map_err(PacketReductionSelectionError::InvalidPlan)?;
        ceiling
            .validate()
            .map_err(PacketReductionSelectionError::InvalidCeiling)?;
        ensure_same_estimator_basis(&self.estimator_basis, &ceiling.remaining_tokens.basis)
            .map_err(PacketReductionSelectionError::EstimatorBasisMismatch)?;

        let estimate = &ceiling.remaining_tokens.estimate;
        if estimate.coverage != ObservationCoverageV1::Complete {
            let Some(reason) = estimate.unavailable_reason else {
                return Err(PacketReductionSelectionError::InvalidCeiling(
                    HostObservationValidationError::Coverage {
                        field: "remainingTokens.estimate".into(),
                        reason: "non-exact coverage requires a typed reason".into(),
                    },
                ));
            };
            return Err(PacketReductionSelectionError::CapacityUnavailable {
                coverage: estimate.coverage,
                reason,
            });
        }

        let remaining_tokens = estimate.value.ok_or_else(|| {
            PacketReductionSelectionError::InvalidCeiling(
                HostObservationValidationError::UnavailableValue {
                    field: "remainingTokens.estimate".into(),
                },
            )
        })?;

        let mut selected: Option<&PacketReductionRepresentationV1> = None;
        for representation in &self.representations {
            if representation.tokens <= remaining_tokens
                && selected
                    .as_ref()
                    .map_or(true, |current| representation.tokens > current.tokens)
            {
                selected = Some(representation);
            }
        }

        selected.ok_or(PacketReductionSelectionError::NoRepresentationFits {
            remaining_tokens,
            minimum_viable_tokens: self.minimum_viable_tokens,
        })
    }

    /// Compatibility spelling for callers that use the host-capacity term.
    pub fn select_for_host_capacity(
        &self,
        ceiling: &RemainingContextCeilingV1,
    ) -> Result<&PacketReductionRepresentationV1, PacketReductionSelectionError> {
        self.select_for_capacity(ceiling)
    }
}

fn validate_non_empty(
    values: &[String],
    field: &'static str,
) -> Result<(), PacketReductionPlanError> {
    if values.iter().any(String::is_empty) {
        Err(PacketReductionPlanError::EmptyField(field))
    } else {
        Ok(())
    }
}

fn ensure_unique(values: &[String], field: &'static str) -> Result<(), PacketReductionPlanError> {
    let mut seen = HashSet::with_capacity(values.len());
    if values.iter().all(|value| seen.insert(value)) {
        Ok(())
    } else {
        Err(PacketReductionPlanError::DuplicateValue(field))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_observation::{
        EstimatorBasisV1, HostObservationProvenanceV1, ObservationUnavailableReasonV1,
        ObservedFieldV1, TokenEstimateV1, REMAINING_CONTEXT_CEILING_SCHEMA_VERSION,
    };

    fn representation_with_tokens(
        id: &str,
        tokens: u64,
        protected: &[&str],
    ) -> PacketReductionRepresentationV1 {
        PacketReductionRepresentationV1 {
            id: id.into(),
            tokens,
            parent_ref: "packet://task-1".into(),
            protected: protected.iter().map(|value| (*value).into()).collect(),
            evidence_refs: vec!["evidence://result-1".into()],
            resolver_paths: vec!["resolver://result-1".into()],
            minimum_viable_tokens: 32,
            coverage_note: format!("{id} retains required coverage"),
            content: serde_json::json!({"representation": id}),
        }
    }

    fn representation(id: &str, protected: &[&str]) -> PacketReductionRepresentationV1 {
        representation_with_tokens(id, 128, protected)
    }

    fn plan() -> PacketReductionPlanV1 {
        PacketReductionPlanV1 {
            schema_version: PACKET_REDUCTION_PLAN_SCHEMA_VERSION,
            estimator_basis: EstimatorBasisV1::new("test-estimator", "v1"),
            representations: vec![
                representation("full", &["task-entity", "error-code"]),
                representation_with_tokens("floor", 32, &["task-entity", "error-code"]),
            ],
            protected: vec!["task-entity".into(), "error-code".into()],
            minimum_viable_tokens: 32,
        }
    }

    fn ceiling(remaining_tokens: u64) -> RemainingContextCeilingV1 {
        RemainingContextCeilingV1 {
            schema_version: REMAINING_CONTEXT_CEILING_SCHEMA_VERSION,
            ceiling_id: "ceiling-1".into(),
            session_id: "session-1".into(),
            task_id: ObservedFieldV1::complete("task-1".into()),
            requested_at_unix_ms: 1_700_000_000_000,
            remaining_tokens: TokenEstimateV1::complete(
                EstimatorBasisV1::new("test-estimator", "v1"),
                remaining_tokens,
            ),
            provenance_receipt: HostObservationProvenanceV1::new(
                "receipt-1",
                "test-host",
                1_700_000_000_000,
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ),
        }
    }

    #[test]
    fn valid_plan_keeps_every_protected_item_in_each_representation() {
        assert!(plan().validate().is_ok());
        let encoded = serde_json::to_string(&plan()).unwrap();
        let decoded: PacketReductionPlanV1 = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, plan());
    }

    #[test]
    fn publication_rejects_missing_protected_item() {
        let mut invalid = plan();
        invalid.representations[1].protected.pop();
        let error = invalid.validate().unwrap_err();
        assert!(matches!(
            error,
            PacketReductionPlanError::ProtectedItemMissing { .. }
        ));
    }

    #[test]
    fn publication_rejects_unachievable_minimum() {
        let mut invalid = plan();
        invalid.minimum_viable_tokens = 256;
        let error = invalid.validate().unwrap_err();
        assert!(matches!(
            error,
            PacketReductionPlanError::MinimumViableTokens
                | PacketReductionPlanError::RepresentationBelowMinimum { .. }
        ));
    }

    #[test]
    fn unknown_fields_are_rejected_by_wire_shape() {
        let mut value = serde_json::to_value(plan()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("hostSelectedEvidence".into(), serde_json::Value::Bool(true));
        assert!(serde_json::from_value::<PacketReductionPlanV1>(value).is_err());
    }

    #[test]
    fn selection_chooses_largest_fitting_representation_with_protected_items() {
        let plan = plan();
        let selected = plan.select_for_capacity(&ceiling(100)).unwrap();
        assert_eq!(selected.id, "floor");
        assert_eq!(selected.tokens, 32);
        assert_eq!(selected.protected.as_slice(), plan.protected.as_slice());

        let selected = plan.select_for_host_capacity(&ceiling(128)).unwrap();
        assert_eq!(selected.id, "full");
    }

    #[test]
    fn selection_refuses_unavailable_capacity_without_inventing_budget() {
        let mut unavailable = ceiling(128);
        unavailable.remaining_tokens = TokenEstimateV1::unavailable(
            EstimatorBasisV1::new("test-estimator", "v1"),
            ObservationUnavailableReasonV1::ProviderOmitted,
        );
        let error = plan().select_for_capacity(&unavailable).unwrap_err();
        assert!(matches!(
            error,
            PacketReductionSelectionError::CapacityUnavailable {
                coverage: ObservationCoverageV1::Unavailable,
                reason: ObservationUnavailableReasonV1::ProviderOmitted,
            }
        ));

        let mut partial = ceiling(128);
        partial.remaining_tokens.estimate =
            ObservedFieldV1::partial(Some(128), ObservationUnavailableReasonV1::NotInstrumented);
        let error = plan().select_for_capacity(&partial).unwrap_err();
        assert!(matches!(
            error,
            PacketReductionSelectionError::CapacityUnavailable {
                coverage: ObservationCoverageV1::Partial,
                reason: ObservationUnavailableReasonV1::NotInstrumented,
            }
        ));
    }

    #[test]
    fn selection_refuses_capacity_below_minimum_viable_size() {
        let error = plan().select_for_capacity(&ceiling(31)).unwrap_err();
        assert!(matches!(
            error,
            PacketReductionSelectionError::NoRepresentationFits {
                remaining_tokens: 31,
                minimum_viable_tokens: 32,
            }
        ));
    }

    #[test]
    fn selection_refuses_mismatched_estimator_basis() {
        let mut mismatched = ceiling(128);
        mismatched.remaining_tokens.basis = EstimatorBasisV1::new("other-estimator", "v2");
        let error = plan().select_for_capacity(&mismatched).unwrap_err();
        assert!(matches!(
            error,
            PacketReductionSelectionError::EstimatorBasisMismatch(_)
        ));
    }

    #[test]
    fn selection_validates_protected_items_before_host_fit() {
        let mut invalid = plan();
        invalid.representations[1].protected.pop();
        let error = invalid.select_for_capacity(&ceiling(128)).unwrap_err();
        assert!(matches!(
            error,
            PacketReductionSelectionError::InvalidPlan(
                PacketReductionPlanError::ProtectedItemMissing { .. }
            )
        ));
    }
}
