//! P2 measured adaptive behavior. All outputs are measurements or proposals;
//! none mutate preference/issue authority automatically.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TasteExperimentVariant {
    BaselineNoAdapt,
    TasteSelection,
    StaticInjection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TasteTrialV1 {
    pub task_digest: String,
    pub variant: TasteExperimentVariant,
    pub model: String,
    pub client: String,
    pub applicable_preferences: u32,
    pub repeated_correction: bool,
    pub task_correct: bool,
    pub policy_compliant: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct VariantMetricsV1 {
    pub trials: u32,
    pub repeated_corrections: u32,
    pub correctness_failures: u32,
    pub policy_failures: u32,
}

impl VariantMetricsV1 {
    pub fn correction_rate(&self) -> Option<f64> {
        (self.trials > 0).then(|| self.repeated_corrections as f64 / self.trials as f64)
    }

    pub fn correctness_rate(&self) -> Option<f64> {
        (self.trials > 0).then(|| 1.0 - self.correctness_failures as f64 / self.trials as f64)
    }
}

pub fn evaluate_taste_trials(
    trials: &[TasteTrialV1],
) -> BTreeMap<(TasteExperimentVariant, String, String), VariantMetricsV1> {
    let mut report = BTreeMap::new();
    for trial in trials {
        let metrics = report
            .entry((trial.variant, trial.model.clone(), trial.client.clone()))
            .or_insert_with(VariantMetricsV1::default);
        metrics.trials += 1;
        metrics.repeated_corrections += u32::from(trial.repeated_correction);
        metrics.correctness_failures += u32::from(!trial.task_correct);
        metrics.policy_failures += u32::from(!trial.policy_compliant);
    }
    report
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsightSurfaceObservationV1 {
    pub family: String,
    pub model: String,
    pub client: String,
    pub opportunities: u32,
    pub recurrences: u32,
}

pub fn insight_recurrence_by_surface(
    observations: &[InsightSurfaceObservationV1],
) -> BTreeMap<(String, String, String), (u32, u32, Option<f64>)> {
    let mut totals: BTreeMap<(String, String, String), (u32, u32)> = BTreeMap::new();
    for observation in observations {
        let entry = totals
            .entry((
                observation.family.clone(),
                observation.model.clone(),
                observation.client.clone(),
            ))
            .or_default();
        entry.0 = entry.0.saturating_add(observation.opportunities);
        entry.1 = entry.1.saturating_add(observation.recurrences);
    }
    totals
        .into_iter()
        .map(|(key, (opportunities, recurrences))| {
            let rate = (opportunities > 0).then(|| recurrences as f64 / opportunities as f64);
            (key, (opportunities, recurrences, rate))
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetirementSuggestionV1 {
    pub suggestion_id: String,
    pub record_id: String,
    pub reason: String,
    pub evidence_window: u32,
    pub previous_surface_version: String,
    pub current_surface_version: String,
    pub requires_review: bool,
}

/// Suggest retirement after meaningful surface drift plus measured poor
/// outcomes. This never changes lifecycle state.
pub fn suggest_retirement(
    record_id: &str,
    previous_surface_version: &str,
    current_surface_version: &str,
    metrics: &crate::delivery::EffectivenessMetricsV1,
) -> Option<RetirementSuggestionV1> {
    if previous_surface_version == current_surface_version || metrics.selections < 10 {
        return None;
    }
    let adherence = metrics.adherence_rate().unwrap_or(0.0);
    let correction = metrics.correction_rate().unwrap_or(0.0);
    if adherence >= 0.8 && correction <= 0.1 && metrics.correctness_regressions == 0 {
        return None;
    }
    let source = format!(
        "{record_id}\0{previous_surface_version}\0{current_surface_version}\0{}\0{}",
        metrics.selections, metrics.corrections
    );
    Some(RetirementSuggestionV1 {
        suggestion_id: format!("rts_{}", crate::canonical::sha256_hex(source.as_bytes())),
        record_id: record_id.into(),
        reason: "surface_version_changed_and_effectiveness_degraded".into(),
        evidence_window: metrics.selections,
        previous_surface_version: previous_surface_version.into(),
        current_surface_version: current_surface_version.into(),
        requires_review: true,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AggregateError {
    CohortTooSmall { observed: u32, minimum: u32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrganizationInsightAggregateV1 {
    pub org_id_hash: String,
    pub cohort_size: u32,
    pub minimum_cohort_size: u32,
    pub family_counts: BTreeMap<String, u32>,
    pub total_opportunities: u32,
    pub total_recurrences: u32,
}

/// Privacy-preserving aggregate: accepts counts only, rejects small cohorts,
/// & has no transcript text, session IDs, event IDs, or personal record IDs.
pub fn organization_insight_aggregate(
    org_id_hash: &str,
    cohort_size: u32,
    minimum_cohort_size: u32,
    family_counts: BTreeMap<String, u32>,
    total_opportunities: u32,
) -> Result<OrganizationInsightAggregateV1, AggregateError> {
    if cohort_size < minimum_cohort_size {
        return Err(AggregateError::CohortTooSmall {
            observed: cohort_size,
            minimum: minimum_cohort_size,
        });
    }
    let total_recurrences = family_counts.values().copied().sum();
    Ok(OrganizationInsightAggregateV1 {
        org_id_hash: org_id_hash.into(),
        cohort_size,
        minimum_cohort_size,
        family_counts,
        total_opportunities,
        total_recurrences,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ab_metrics_keep_variants_separate() {
        let trials = vec![
            TasteTrialV1 {
                task_digest: "a".into(),
                variant: TasteExperimentVariant::BaselineNoAdapt,
                model: "m".into(),
                client: "c".into(),
                applicable_preferences: 0,
                repeated_correction: true,
                task_correct: true,
                policy_compliant: true,
            },
            TasteTrialV1 {
                task_digest: "b".into(),
                variant: TasteExperimentVariant::TasteSelection,
                model: "m".into(),
                client: "c".into(),
                applicable_preferences: 1,
                repeated_correction: false,
                task_correct: true,
                policy_compliant: true,
            },
        ];
        let report = evaluate_taste_trials(&trials);
        assert_eq!(report.len(), 2);
    }

    #[test]
    fn retirement_is_proposal_only_after_evidence() {
        let metrics = crate::delivery::EffectivenessMetricsV1 {
            selections: 12,
            adherences: 3,
            corrections: 4,
            ..Default::default()
        };
        assert!(suggest_retirement("r", "model-v1", "model-v2", &metrics).is_some());
        assert!(suggest_retirement("r", "model-v2", "model-v2", &metrics).is_none());
    }

    #[test]
    fn organization_aggregate_rejects_small_cohort() {
        assert!(organization_insight_aggregate("org", 2, 5, BTreeMap::new(), 10).is_err());
    }
}
