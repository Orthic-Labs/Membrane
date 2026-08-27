//! Read-only projection of procedural-asset telemetry into effectiveness.
//! Hosts supply mechanical H4 observations and H6 evaluation outcomes; Adapt
//! joins them without owning assets or inventing absent measurements.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const PROCEDURAL_ASSET_EFFECTIVENESS_SCHEMA: &str = "adapt.procedural-asset-effectiveness.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnavailableReason {
    NotInstrumented,
    HubInactive,
    ProviderOmitted,
    HostUnsupported,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Observed<T> {
    pub coverage: Coverage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<UnavailableReason>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Coverage {
    Complete,
    Partial,
    Unavailable,
}

pub type ObservationCoverage = Coverage;
pub type ObservationUnavailableReason = UnavailableReason;

impl<T> Observed<T> {
    pub fn complete(value: T) -> Self {
        Self {
            coverage: Coverage::Complete,
            value: Some(value),
            unavailable_reason: None,
        }
    }
    pub fn unavailable(reason: UnavailableReason) -> Self {
        Self {
            coverage: Coverage::Unavailable,
            value: None,
            unavailable_reason: Some(reason),
        }
    }
    pub fn validate(&self, field: &str) -> Result<(), String> {
        match self.coverage {
            Coverage::Complete if self.value.is_none() || self.unavailable_reason.is_some() => {
                Err(format!("{field}: invalid complete coverage"))
            }
            Coverage::Partial if self.unavailable_reason.is_none() => {
                Err(format!("{field}: partial coverage requires reason"))
            }
            Coverage::Unavailable if self.value.is_some() || self.unavailable_reason.is_none() => {
                Err(format!("{field}: invalid unavailable coverage"))
            }
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Provenance {
    pub receipt_id: String,
    pub receipt_digest: String,
    pub source: String,
    pub observed_at: String,
}

impl Provenance {
    pub fn validate(&self) -> Result<(), String> {
        if self.receipt_id.is_empty() || self.source.is_empty() || self.observed_at.is_empty() {
            return Err("provenance fields are required".into());
        }
        if !self
            .receipt_digest
            .strip_prefix("sha256:")
            .map(|v| v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit()))
            .unwrap_or(false)
        {
            return Err("provenance receipt_digest must be sha256: plus 64 hex characters".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProceduralAssetObservationV1 {
    pub observation_id: String,
    pub asset_id: String,
    pub assessed_at: String,
    pub exposures: Observed<u64>,
    pub selections: Observed<u64>,
    pub applications: Observed<u64>,
    pub successes: Observed<u64>,
    pub failures: Observed<u64>,
    pub corrections_after_use: Observed<u64>,
    pub token_cost_per_turn: Observed<u64>,
    pub model: Observed<String>,
    pub client: Observed<String>,
    pub evidence_refs: Observed<Vec<String>>,
    pub provenance: Provenance,
}

impl ProceduralAssetObservationV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.observation_id.is_empty() || self.asset_id.is_empty() || self.assessed_at.is_empty()
        {
            return Err("observation identity is required".into());
        }
        for (name, field) in [
            ("exposures", &self.exposures),
            ("selections", &self.selections),
            ("applications", &self.applications),
            ("successes", &self.successes),
            ("failures", &self.failures),
            ("correctionsAfterUse", &self.corrections_after_use),
            ("tokenCostPerTurn", &self.token_cost_per_turn),
        ] {
            field.validate(name)?;
        }
        self.model.validate("model")?;
        self.client.validate("client")?;
        self.evidence_refs.validate("evidenceRefs")?;
        self.provenance.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvaluationObservationV1 {
    pub outcome_id: String,
    pub asset_id: Observed<String>,
    pub evaluator: Observed<String>,
    pub dataset: Observed<String>,
    pub experiment: Observed<String>,
    pub score: Observed<f64>,
    pub evidence_refs: Observed<Vec<String>>,
    pub provenance: Provenance,
}

impl EvaluationObservationV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.outcome_id.is_empty() {
            return Err("outcome_id is required".into());
        }
        self.asset_id.validate("assetId")?;
        self.evaluator.validate("evaluator")?;
        self.dataset.validate("dataset")?;
        self.experiment.validate("experiment")?;
        self.score.validate("score")?;
        self.evidence_refs.validate("evidenceRefs")?;
        self.provenance.validate()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectivenessVerdict {
    Effective,
    Ineffective,
    Indeterminate,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProceduralAssetEffectivenessV1 {
    pub schema_version: String,
    pub asset_id: String,
    pub assessed_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_issue_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_proposal_id: Option<String>,
    pub exposures: Observed<u64>,
    pub selections: Observed<u64>,
    pub applications: Observed<u64>,
    pub successes: Observed<u64>,
    pub failures: Observed<u64>,
    pub corrections_after_use: Observed<u64>,
    pub token_cost_per_turn: Observed<u64>,
    pub model: Observed<String>,
    pub client: Observed<String>,
    pub effectiveness_verdict: Observed<EffectivenessVerdict>,
    pub evidence_refs: Observed<Vec<String>>,
    pub honesty_limit: String,
}

/// Host-neutral mechanical inputs used by the runtime ingress adapter.
/// These intentionally omit provenance objects whose digest may be absent at
/// the host boundary; each measurement retains its typed coverage instead.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostProceduralAssetObservationV1 {
    pub observation_id: String,
    pub asset_id: Observed<String>,
    pub assessed_at: Observed<String>,
    pub exposures: Observed<u64>,
    pub selections: Observed<u64>,
    pub applications: Observed<u64>,
    pub successes: Observed<u64>,
    pub failures: Observed<u64>,
    pub corrections_after_use: Observed<u64>,
    pub token_cost_per_turn: Observed<u64>,
    pub model: Observed<String>,
    pub client: Observed<String>,
    pub evidence_refs: Observed<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostEvaluationObservationV1 {
    pub outcome_id: String,
    pub asset_id: Observed<String>,
    pub evaluator: Observed<String>,
    pub dataset: Observed<String>,
    pub experiment: Observed<String>,
    pub score: Observed<f64>,
    pub evidence_refs: Observed<Vec<String>>,
}

/// Project host measurements without granting lifecycle or authority. Inputs
/// are already restricted to exact identity joins by the runtime adapter.
pub fn project_host_effectiveness(
    asset_id: &str,
    observations: &[HostProceduralAssetObservationV1],
    evaluations: &[HostEvaluationObservationV1],
) -> ProceduralAssetEffectivenessV1 {
    let rows: Vec<_> = observations
        .iter()
        .filter(|r| r.asset_id.value.as_deref() == Some(asset_id))
        .collect();
    let evals: Vec<_> = evaluations
        .iter()
        .filter(|r| r.asset_id.value.as_deref() == Some(asset_id))
        .collect();
    let refs = rows
        .iter()
        .flat_map(|r| r.evidence_refs.value.clone().unwrap_or_default())
        .chain(
            evals
                .iter()
                .flat_map(|r| r.evidence_refs.value.clone().unwrap_or_default()),
        )
        .collect::<BTreeSet<_>>();
    let verdict = if rows.is_empty()
        || evals.is_empty()
        || refs.is_empty()
        || evals.iter().any(|e| {
            [
                e.evaluator.value.is_none(),
                e.dataset.value.is_none(),
                e.experiment.value.is_none(),
                e.score.value.is_none(),
            ]
            .into_iter()
            .any(|missing| missing)
        }) {
        Observed::unavailable(if rows.is_empty() || evals.is_empty() {
            UnavailableReason::NotInstrumented
        } else {
            UnavailableReason::ProviderOmitted
        })
    } else if evals.iter().all(|e| e.score.value.unwrap_or(0.0) >= 0.8) {
        Observed::complete(EffectivenessVerdict::Effective)
    } else if evals.iter().any(|e| e.score.value.unwrap_or(0.0) < 0.5) {
        Observed::complete(EffectivenessVerdict::Ineffective)
    } else {
        Observed::complete(EffectivenessVerdict::Indeterminate)
    };
    let sum_host = |f: fn(&HostProceduralAssetObservationV1) -> &Observed<u64>| {
        if rows.is_empty() {
            return Observed::unavailable(UnavailableReason::NotInstrumented);
        }
        if rows.iter().all(|r| f(r).coverage == Coverage::Complete) {
            return Observed::complete(rows.iter().map(|r| f(r).value.unwrap_or(0)).sum());
        }
        Observed::unavailable(
            rows.iter()
                .find_map(|r| f(r).unavailable_reason)
                .unwrap_or(UnavailableReason::ProviderOmitted),
        )
    };
    let text = |f: fn(&HostProceduralAssetObservationV1) -> &Observed<String>| {
        rows.first()
            .and_then(|r| f(r).value.clone())
            .map(Observed::complete)
            .unwrap_or_else(|| Observed::unavailable(UnavailableReason::ProviderOmitted))
    };
    ProceduralAssetEffectivenessV1 {
        schema_version: PROCEDURAL_ASSET_EFFECTIVENESS_SCHEMA.into(),
        asset_id: asset_id.into(),
        assessed_at: rows
            .first()
            .and_then(|r| r.assessed_at.value.clone())
            .unwrap_or_default(),
        source_issue_id: None,
        source_proposal_id: None,
        exposures: sum_host(|r| &r.exposures),
        selections: sum_host(|r| &r.selections),
        applications: sum_host(|r| &r.applications),
        successes: sum_host(|r| &r.successes),
        failures: sum_host(|r| &r.failures),
        corrections_after_use: sum_host(|r| &r.corrections_after_use),
        token_cost_per_turn: sum_host(|r| &r.token_cost_per_turn),
        model: text(|r| &r.model),
        client: text(|r| &r.client),
        effectiveness_verdict: verdict,
        evidence_refs: if refs.is_empty() {
            Observed::unavailable(UnavailableReason::ProviderOmitted)
        } else {
            Observed::complete(refs.into_iter().collect())
        },
        honesty_limit:
            "Host observations and evaluations only; asset lifecycle remains owner-controlled."
                .into(),
    }
}

fn sum(values: &[Observed<u64>]) -> Observed<u64> {
    if values.is_empty() {
        return Observed::unavailable(UnavailableReason::NotInstrumented);
    }
    if values.iter().all(|v| v.coverage == Coverage::Complete) {
        return Observed::complete(values.iter().map(|v| v.value.unwrap_or(0)).sum());
    }
    let reason = values
        .iter()
        .find_map(|v| v.unavailable_reason)
        .unwrap_or(UnavailableReason::ProviderOmitted);
    Observed::unavailable(reason)
}

/// Join H4 observations and H6 outcomes by asset id. A verdict is emitted only
/// when all required evidence is joinable and complete.
pub fn project_effectiveness(
    asset_id: &str,
    observations: &[ProceduralAssetObservationV1],
    evaluations: &[EvaluationObservationV1],
) -> ProceduralAssetEffectivenessV1 {
    let rows: Vec<_> = observations
        .iter()
        .filter(|o| o.asset_id == asset_id)
        .collect();
    let first = rows.first();
    let unavailable = UnavailableReason::NotInstrumented;
    let text = |f: fn(&ProceduralAssetObservationV1) -> &Observed<String>| {
        first
            .and_then(|o| f(o).value.clone())
            .map(Observed::complete)
            .unwrap_or_else(|| Observed::unavailable(unavailable))
    };
    let refs: Vec<String> = rows
        .iter()
        .flat_map(|o| o.evidence_refs.value.clone().unwrap_or_default())
        .chain(
            evaluations
                .iter()
                .flat_map(|e| e.evidence_refs.value.clone().unwrap_or_default()),
        )
        .collect();
    let mut evidence = refs;
    evidence.sort();
    evidence.dedup();
    let evals: Vec<_> = evaluations
        .iter()
        .filter(|e| e.asset_id.value.as_deref() == Some(asset_id))
        .collect();
    let verdict = if rows.is_empty()
        || evals.is_empty()
        || evidence.is_empty()
        || evals.iter().any(|e| {
            e.dataset.value.is_none()
                || e.experiment.value.is_none()
                || e.evaluator.value.is_none()
                || e.score.value.is_none()
        }) {
        Observed::unavailable(unavailable)
    } else if evals.iter().all(|e| e.score.value.unwrap_or(0.0) >= 0.8) {
        Observed::complete(EffectivenessVerdict::Effective)
    } else if evals.iter().any(|e| e.score.value.unwrap_or(0.0) < 0.5) {
        Observed::complete(EffectivenessVerdict::Ineffective)
    } else {
        Observed::complete(EffectivenessVerdict::Indeterminate)
    };
    ProceduralAssetEffectivenessV1 {
        schema_version: PROCEDURAL_ASSET_EFFECTIVENESS_SCHEMA.into(),
        asset_id: asset_id.into(),
        assessed_at: first.map(|o| o.assessed_at.clone()).unwrap_or_default(),
        source_issue_id: None,
        source_proposal_id: None,
        exposures: sum(&rows.iter().map(|o| o.exposures.clone()).collect::<Vec<_>>()),
        selections: sum(&rows
            .iter()
            .map(|o| o.selections.clone())
            .collect::<Vec<_>>()),
        applications: sum(&rows
            .iter()
            .map(|o| o.applications.clone())
            .collect::<Vec<_>>()),
        successes: sum(&rows.iter().map(|o| o.successes.clone()).collect::<Vec<_>>()),
        failures: sum(&rows.iter().map(|o| o.failures.clone()).collect::<Vec<_>>()),
        corrections_after_use: sum(&rows
            .iter()
            .map(|o| o.corrections_after_use.clone())
            .collect::<Vec<_>>()),
        token_cost_per_turn: sum(&rows
            .iter()
            .map(|o| o.token_cost_per_turn.clone())
            .collect::<Vec<_>>()),
        model: text(|o| &o.model),
        client: text(|o| &o.client),
        effectiveness_verdict: verdict,
        evidence_refs: if evidence.is_empty() {
            Observed::unavailable(unavailable)
        } else {
            Observed::complete(evidence)
        },
        honesty_limit:
            "Host observations and evaluations only; asset lifecycle remains owner-controlled."
                .into(),
    }
}

pub fn project_closed_loop(
    asset_id: &str,
    observations: &[ProceduralAssetObservationV1],
    evaluations: &[EvaluationObservationV1],
) -> ProceduralAssetEffectivenessV1 {
    project_effectiveness(asset_id, observations, evaluations)
}

pub fn build_effectiveness_projection(
    asset_id: &str,
    observations: &[ProceduralAssetObservationV1],
    evaluations: &[EvaluationObservationV1],
) -> ProceduralAssetEffectivenessV1 {
    project_effectiveness(asset_id, observations, evaluations)
}

pub fn required_evidence_refs(record: &ProceduralAssetEffectivenessV1) -> BTreeSet<String> {
    record
        .evidence_refs
        .value
        .clone()
        .unwrap_or_default()
        .into_iter()
        .collect()
}
