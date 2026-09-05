//! Exact-version Adapt procedural-effectiveness projection.
//!
//! The legacy H4/H6 ingress can identify an asset and its content digest, but
//! it does not carry the final H9/H10 loaded-representation acknowledgement.
//! This projection therefore separates every asset-content version, excludes
//! evaluator outcomes whose version attribution is ambiguous, and refuses to
//! label an intervention effective until exact loaded exposure is available.

use crate::host_observation_ingress::{
    join_host_observations, HostH4ObservationKindV1, HostObservationIngressV1,
    HostObservedFieldV1,
};
use membrane_adapt::lineage::LearningLineageV1;
use membrane_adapt::procedural_effectiveness::{
    project_host_effectiveness, Coverage, EffectivenessVerdict, HostEvaluationObservationV1,
    HostProceduralAssetObservationV1, Observed, ProceduralAssetEffectivenessV1,
    UnavailableReason,
};
use membrane_protocol::host_observation::{
    ObservationCoverageV1, ObservationUnavailableReasonV1,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const EXACT_PROCEDURAL_EFFECTIVENESS_SCHEMA: &str =
    "adapt.procedural-asset-effectiveness.exact-v1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExactProceduralAssetEffectivenessV1 {
    pub schema_version: String,
    pub asset_id: String,
    /// Exact H4 content digest. An unversioned row is retained as unavailable
    /// rather than merged into any versioned bucket.
    pub asset_digest: Observed<String>,
    /// Final H9/H10 representation loaded by the host. The legacy ingress
    /// cannot provide this; missing exposure keeps effectiveness unavailable.
    pub loaded_representation_digest: Observed<String>,
    pub h4_observation_ids: Vec<String>,
    pub h6_outcome_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excluded_ambiguous_h6_outcome_ids: Vec<String>,
    pub projection: ProceduralAssetEffectivenessV1,
    pub honesty_limit: String,
}

fn unavailable_reason(reason: ObservationUnavailableReasonV1) -> UnavailableReason {
    match reason {
        ObservationUnavailableReasonV1::NotInstrumented => UnavailableReason::NotInstrumented,
        ObservationUnavailableReasonV1::HubInactive => UnavailableReason::HubInactive,
        ObservationUnavailableReasonV1::ProviderOmitted => UnavailableReason::ProviderOmitted,
        ObservationUnavailableReasonV1::HostUnsupported => UnavailableReason::HostUnsupported,
    }
}

fn observed<T: Clone>(field: &HostObservedFieldV1<T>) -> Observed<T> {
    let reason = field
        .unavailable_reason
        .map(unavailable_reason)
        .unwrap_or(UnavailableReason::ProviderOmitted);
    match (field.coverage, field.value.clone()) {
        (ObservationCoverageV1::Complete, Some(value)) => Observed::complete(value),
        (ObservationCoverageV1::Partial, Some(value)) => Observed {
            coverage: Coverage::Partial,
            value: Some(value),
            unavailable_reason: Some(reason),
        },
        _ => Observed::unavailable(reason),
    }
}

fn exact_digest(field: &HostObservedFieldV1<String>) -> Option<&str> {
    (field.coverage == ObservationCoverageV1::Complete)
        .then_some(field.value.as_deref())
        .flatten()
        .filter(|value| !value.trim().is_empty())
}

fn h4_for_version<'a>(
    ingress: &'a HostObservationIngressV1,
    ids: &BTreeSet<String>,
    asset_id: &str,
    digest: &str,
) -> Vec<&'a crate::host_observation_ingress::HostH4ObservationV1> {
    ingress
        .h4
        .iter()
        .filter(|h4| {
            h4.kind == HostH4ObservationKindV1::ProceduralAsset
                && ids.contains(&h4.observation_id)
                && h4.asset_id.value.as_deref() == Some(asset_id)
                && exact_digest(&h4.digest) == Some(digest)
        })
        .collect()
}

fn host_observation(
    h4: &crate::host_observation_ingress::HostH4ObservationV1,
) -> HostProceduralAssetObservationV1 {
    HostProceduralAssetObservationV1 {
        observation_id: h4.observation_id.clone(),
        asset_id: observed(&h4.asset_id),
        assessed_at: Observed::unavailable(UnavailableReason::ProviderOmitted),
        exposures: observed(&h4.exposed_count),
        selections: observed(&h4.selected_count),
        applications: observed(&h4.applied_count),
        successes: observed(&h4.success_count),
        failures: observed(&h4.failure_count),
        corrections_after_use: observed(&h4.corrections_after_use),
        token_cost_per_turn: observed(&h4.token_estimate),
        model: observed(&h4.estimator_id),
        client: observed(&h4.estimator_version),
        evidence_refs: observed(&h4.refs),
    }
}

fn host_evaluation(
    h6: &crate::host_observation_ingress::HostEvaluationOutcomeV1,
    asset_id: &str,
) -> HostEvaluationObservationV1 {
    HostEvaluationObservationV1 {
        outcome_id: h6.outcome_id.clone(),
        asset_id: Observed::complete(asset_id.to_string()),
        evaluator: observed(&h6.evaluator_id),
        dataset: observed(&h6.dataset_id),
        experiment: observed(&h6.experiment_id),
        score: observed(&h6.score),
        evidence_refs: observed(&h6.provenance.evidence_refs),
    }
}

/// Project only version-separable H4/H6 evidence.
///
/// H6 outcomes are admitted to a digest bucket only when every join path for
/// that `(asset_id, outcome_id)` resolves to exactly one complete H4 digest.
/// If one outcome can reach multiple versions, it is excluded from every
/// version instead of being duplicated or assigned by first-match order.
pub fn project_joined_effectiveness_exact(
    ingress: &HostObservationIngressV1,
    lineages: &[LearningLineageV1],
) -> Vec<ExactProceduralAssetEffectivenessV1> {
    let join = join_host_observations(ingress, lineages);

    let mut asset_versions: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut unversioned_assets = BTreeSet::new();
    for row in &join.rows {
        for h4_id in &row.h4_observation_ids {
            let Some(h4) = ingress.h4.iter().find(|candidate| {
                candidate.observation_id == *h4_id
                    && candidate.kind == HostH4ObservationKindV1::ProceduralAsset
            }) else {
                continue;
            };
            let Some(asset_id) = h4.asset_id.value.as_deref() else {
                continue;
            };
            if let Some(digest) = exact_digest(&h4.digest) {
                asset_versions
                    .entry(asset_id.to_string())
                    .or_default()
                    .insert(digest.to_string());
            } else {
                unversioned_assets.insert(asset_id.to_string());
            }
        }
    }

    // For each asset/outcome collect every complete digest reachable through
    // joined H4 rows. Exactly one is required for version attribution.
    let mut outcome_versions: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    let mut outcome_has_unversioned: BTreeSet<(String, String)> = BTreeSet::new();
    for row in &join.rows {
        let h4_rows: Vec<_> = row
            .h4_observation_ids
            .iter()
            .filter_map(|id| ingress.h4.iter().find(|h4| h4.observation_id == *id))
            .filter(|h4| h4.kind == HostH4ObservationKindV1::ProceduralAsset)
            .collect();
        for h6_id in &row.h6_outcome_ids {
            for h4 in &h4_rows {
                let Some(asset_id) = h4.asset_id.value.as_deref() else {
                    continue;
                };
                let key = (asset_id.to_string(), h6_id.clone());
                if let Some(digest) = exact_digest(&h4.digest) {
                    outcome_versions
                        .entry(key)
                        .or_default()
                        .insert(digest.to_string());
                } else {
                    outcome_has_unversioned.insert(key);
                }
            }
        }
    }

    let mut output = Vec::new();
    for (asset_id, versions) in asset_versions {
        for digest in versions {
            let relevant_rows: Vec<_> = join
                .rows
                .iter()
                .filter(|row| {
                    row.h4_observation_ids.iter().any(|id| {
                        ingress.h4.iter().any(|h4| {
                            h4.observation_id == *id
                                && h4.kind == HostH4ObservationKindV1::ProceduralAsset
                                && h4.asset_id.value.as_deref() == Some(asset_id.as_str())
                                && exact_digest(&h4.digest) == Some(digest.as_str())
                        })
                    })
                })
                .collect();
            let h4_ids: BTreeSet<String> = relevant_rows
                .iter()
                .flat_map(|row| row.h4_observation_ids.iter().cloned())
                .collect();
            let h4 = h4_for_version(ingress, &h4_ids, &asset_id, &digest);
            let observations = h4
                .iter()
                .map(|row| host_observation(row))
                .collect::<Vec<_>>();

            let candidate_h6_ids: BTreeSet<String> = relevant_rows
                .iter()
                .flat_map(|row| row.h6_outcome_ids.iter().cloned())
                .collect();
            let mut accepted_h6_ids = Vec::new();
            let mut excluded_h6_ids = Vec::new();
            for outcome_id in candidate_h6_ids {
                let key = (asset_id.clone(), outcome_id.clone());
                let exact = outcome_versions
                    .get(&key)
                    .is_some_and(|values| values.len() == 1 && values.contains(&digest));
                if exact && !outcome_has_unversioned.contains(&key) {
                    accepted_h6_ids.push(outcome_id);
                } else {
                    excluded_h6_ids.push(outcome_id);
                }
            }
            accepted_h6_ids.sort();
            excluded_h6_ids.sort();
            let evaluations = ingress
                .h6
                .iter()
                .filter(|h6| accepted_h6_ids.binary_search(&h6.outcome_id).is_ok())
                .map(|h6| host_evaluation(h6, &asset_id))
                .collect::<Vec<_>>();

            let mut projection =
                project_host_effectiveness(&asset_id, &observations, &evaluations);
            // H4/H6 version separation is necessary but not sufficient for an
            // intervention-effect claim. The final representation loaded by
            // the host is a separate H9/H10 fact and is absent from this
            // ingress, so the verdict must remain unknown.
            projection.effectiveness_verdict =
                Observed::unavailable(UnavailableReason::HostUnsupported);
            projection.honesty_limit = format!(
                "H4/H6 facts are separated by exact asset digest {digest}; ambiguous evaluator paths are excluded. Exact H9/H10 loaded-representation exposure is unavailable, so no intervention-effect verdict is emitted."
            );

            output.push(ExactProceduralAssetEffectivenessV1 {
                schema_version: EXACT_PROCEDURAL_EFFECTIVENESS_SCHEMA.into(),
                asset_id: asset_id.clone(),
                asset_digest: Observed::complete(digest),
                loaded_representation_digest: Observed::unavailable(
                    UnavailableReason::HostUnsupported,
                ),
                h4_observation_ids: observations
                    .iter()
                    .map(|row| row.observation_id.clone())
                    .collect(),
                h6_outcome_ids: accepted_h6_ids,
                excluded_ambiguous_h6_outcome_ids: excluded_h6_ids,
                projection,
                honesty_limit: "Version-separated mechanical evidence only; exact loaded exposure is required before effectiveness can be classified.".into(),
            });
        }
    }

    // Preserve the existence of legacy/unversioned evidence as an explicit
    // unavailable row. It must never be silently mixed into a version bucket.
    for asset_id in unversioned_assets {
        let mut projection = project_host_effectiveness(&asset_id, &[], &[]);
        projection.effectiveness_verdict =
            Observed::unavailable(UnavailableReason::ProviderOmitted);
        projection.honesty_limit =
            "Unversioned H4 evidence is retained but excluded from effectiveness aggregation."
                .into();
        output.push(ExactProceduralAssetEffectivenessV1 {
            schema_version: EXACT_PROCEDURAL_EFFECTIVENESS_SCHEMA.into(),
            asset_id,
            asset_digest: Observed::unavailable(UnavailableReason::ProviderOmitted),
            loaded_representation_digest: Observed::unavailable(
                UnavailableReason::HostUnsupported,
            ),
            h4_observation_ids: Vec::new(),
            h6_outcome_ids: Vec::new(),
            excluded_ambiguous_h6_outcome_ids: Vec::new(),
            projection,
            honesty_limit:
                "Asset digest is unavailable; this evidence is not co-aggregated with any versioned asset."
                    .into(),
        });
    }

    output.sort_by(|left, right| {
        left.asset_id.cmp(&right.asset_id).then_with(|| {
            left.asset_digest
                .value
                .as_deref()
                .unwrap_or("")
                .cmp(right.asset_digest.value.as_deref().unwrap_or(""))
        })
    });
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_projection_schema_declares_loaded_exposure_separately() {
        let rows = project_joined_effectiveness_exact(&HostObservationIngressV1::default(), &[]);
        assert!(rows.is_empty());
        assert_eq!(
            EXACT_PROCEDURAL_EFFECTIVENESS_SCHEMA,
            "adapt.procedural-asset-effectiveness.exact-v1"
        );
    }
}
