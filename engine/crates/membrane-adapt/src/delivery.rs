//! Preference-specific delivery & effectiveness receipts.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::authority::{detect_rule_contradictions, PrecedenceTier, StoredRule};
use crate::record::{InfluenceClass, LifecycleState, PreferenceRecordV1, RecordClass};
use crate::scope::ScopeDimensions;

pub const DELIVERY_RECEIPT_SCHEMA: &str = "adapt.preference-delivery.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreferenceDeliveryReceiptV1 {
    pub schema_version: String,
    pub receipt_id: String,
    pub record_id: String,
    pub selected: bool,
    pub applicability_reason: String,
    pub model: Option<String>,
    pub machine: Option<String>,
    pub client: Option<String>,
    pub session_id: String,
    pub trace_id: String,
    #[serde(default)]
    pub request_id: String,
    pub rendered_sha256: Option<String>,
    pub rendered_chars: Option<usize>,
    pub timestamp: String,
}

#[derive(Debug, Clone)]
pub struct PreferenceDeliveryCandidateV1 {
    pub record_id: String,
    pub rule: String,
    pub class: RecordClass,
    pub scope: String,
    pub scope_dimensions: ScopeDimensions,
    pub machine_binding: Option<String>,
    pub authority_tier: PrecedenceTier,
    pub lifecycle_state: LifecycleState,
    pub lifecycle_eligible: bool,
    pub influence_class: InfluenceClass,
    pub semantic_verified: bool,
}

#[derive(Debug, Clone)]
pub struct PreferenceDeliveryContextV1 {
    pub allowed_scopes: Vec<String>,
    pub dimensions: ScopeDimensions,
    pub machine: Option<String>,
    pub max_core_records: usize,
    pub max_scoped_records: usize,
    pub max_total_records: usize,
    pub max_rendered_chars: usize,
    pub timestamp: String,
    pub session_id: String,
    pub trace_id: String,
    pub request_id: String,
    pub client: String,
    pub model: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DeliveredPreferenceV1 {
    pub record_id: String,
    pub rule: String,
    pub receipt: PreferenceDeliveryReceiptV1,
}

#[derive(Debug, Clone)]
pub struct PreferenceDeliveryPlanV1 {
    pub delivered: Vec<DeliveredPreferenceV1>,
    pub receipts: Vec<PreferenceDeliveryReceiptV1>,
}

fn base_omission_reason(
    candidate: &PreferenceDeliveryCandidateV1,
    context: &PreferenceDeliveryContextV1,
) -> Option<&'static str> {
    if candidate.lifecycle_state != LifecycleState::Active || !candidate.lifecycle_eligible {
        Some("inactive_lifecycle")
    } else if candidate.influence_class != InfluenceClass::BehavioralDirective {
        Some("non_directive_influence")
    } else if !candidate.semantic_verified {
        Some("invalid_semantic_seal")
    } else if candidate
        .machine_binding
        .as_ref()
        .is_some_and(|binding| context.machine.as_ref() != Some(binding))
    {
        Some("machine_nonmatch")
    } else if !delivery_scope_matches(candidate, context) {
        Some("scope_nonmatch")
    } else {
        None
    }
}

/// Signed host-act scopes are content-addressed from their narrowing
/// dimensions. Callers cannot know that opaque id in advance, so delivery is
/// governed by the sealed dimensions themselves. Recomputing the id here
/// prevents a hand-authored `dimensions:*` scope from bypassing ordinary
/// exact-scope admission or widening into another dimension set.
fn delivery_scope_matches(
    candidate: &PreferenceDeliveryCandidateV1,
    context: &PreferenceDeliveryContextV1,
) -> bool {
    if let Some(digest) = candidate.scope.strip_prefix("dimensions:") {
        if candidate.scope_dimensions.is_empty() {
            return false;
        }
        let dimensions = candidate
            .scope_dimensions
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<BTreeMap<_, _>>();
        let canonical_digest = crate::canonical::sha256_canonical(
            &serde_json::to_value(dimensions).expect("scope dimensions serialize"),
        );
        digest == &canonical_digest[..24] && candidate.scope_dimensions.matches(&context.dimensions)
    } else {
        context
            .allowed_scopes
            .iter()
            .any(|scope| scope == &candidate.scope)
            && candidate.scope_dimensions.matches(&context.dimensions)
    }
}

fn delivery_receipt(
    candidate: &PreferenceDeliveryCandidateV1,
    selected: bool,
    reason: &str,
    context: &PreferenceDeliveryContextV1,
) -> PreferenceDeliveryReceiptV1 {
    let rendered_sha256 = selected.then(|| crate::canonical::sha256_hex(candidate.rule.as_bytes()));
    let identity = format!(
        "{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}",
        candidate.record_id,
        selected,
        reason,
        context.timestamp,
        context.session_id,
        context.trace_id,
        context.request_id,
        context.client,
        context.model.as_deref().unwrap_or(""),
        context.machine.as_deref().unwrap_or(""),
        rendered_sha256.as_deref().unwrap_or("")
    );
    PreferenceDeliveryReceiptV1 {
        schema_version: DELIVERY_RECEIPT_SCHEMA.into(),
        receipt_id: format!("pdr.{}", crate::canonical::sha256_hex(identity.as_bytes())),
        record_id: candidate.record_id.clone(),
        selected,
        applicability_reason: reason.into(),
        model: context.model.clone(),
        machine: context.machine.clone(),
        client: Some(context.client.clone()),
        session_id: context.session_id.clone(),
        trace_id: context.trace_id.clone(),
        request_id: context.request_id.clone(),
        rendered_sha256,
        rendered_chars: selected.then(|| candidate.rule.chars().count()),
        timestamp: context.timestamp.clone(),
    }
}

/// Canonical two-layer Taste delivery. Broad root standing preferences are
/// considered before scoped preferences, but both share one record and exact
/// rendered-character budget. Every observed candidate receives a receipt;
/// only selected records carry rendered bytes.
pub fn select_delivery_candidates(
    candidates: &[PreferenceDeliveryCandidateV1],
    context: &PreferenceDeliveryContextV1,
) -> PreferenceDeliveryPlanV1 {
    let mut ordered: Vec<&PreferenceDeliveryCandidateV1> = candidates.iter().collect();
    ordered.sort_by_key(|candidate| {
        let core = candidate.class == RecordClass::StandingPreference
            && candidate.scope == "global"
            && candidate.scope_dimensions.is_empty();
        (!core, candidate.record_id.as_str())
    });
    let mut delivered = Vec::new();
    let mut receipts = Vec::with_capacity(ordered.len());
    let applicable = ordered
        .iter()
        .copied()
        .filter(|candidate| base_omission_reason(candidate, context).is_none())
        .collect::<Vec<_>>();
    let mut conflict_reasons = BTreeMap::<String, &'static str>::new();
    for (index, candidate) in applicable.iter().enumerate() {
        for other in applicable.iter().skip(index + 1) {
            let same_rule = crate::canonical::normalize_text(&candidate.rule)
                == crate::canonical::normalize_text(&other.rule);
            let stored = StoredRule {
                id: other.record_id.clone(),
                rule: other.rule.clone(),
                scope: other.scope.clone(),
                lifecycle_state: "active".into(),
            };
            let contradictory =
                !detect_rule_contradictions(&candidate.rule, &candidate.scope, [&stored])
                    .is_empty();
            if !same_rule && !contradictory {
                continue;
            }
            let ordering = candidate
                .authority_tier
                .cmp(&other.authority_tier)
                .then_with(|| {
                    other
                        .scope_dimensions
                        .len()
                        .cmp(&candidate.scope_dimensions.len())
                });
            match ordering {
                std::cmp::Ordering::Less => {
                    conflict_reasons.insert(
                        other.record_id.clone(),
                        if same_rule {
                            "duplicate_delivery"
                        } else {
                            "shadowed_by_precedence"
                        },
                    );
                }
                std::cmp::Ordering::Greater => {
                    conflict_reasons.insert(
                        candidate.record_id.clone(),
                        if same_rule {
                            "duplicate_delivery"
                        } else {
                            "shadowed_by_precedence"
                        },
                    );
                }
                std::cmp::Ordering::Equal if same_rule => {
                    let loser = if candidate.record_id < other.record_id {
                        other.record_id.clone()
                    } else {
                        candidate.record_id.clone()
                    };
                    conflict_reasons.insert(loser, "duplicate_delivery");
                }
                std::cmp::Ordering::Equal => {
                    conflict_reasons.insert(candidate.record_id.clone(), "unresolved_conflict");
                    conflict_reasons.insert(other.record_id.clone(), "unresolved_conflict");
                }
            }
        }
    }
    let mut core_count = 0usize;
    let mut scoped_count = 0usize;
    let mut rendered_chars = 0usize;
    for candidate in ordered {
        let core = candidate.class == RecordClass::StandingPreference
            && candidate.scope == "global"
            && candidate.scope_dimensions.is_empty();
        // Current Cortex lifecycle/influence truth is the most actionable
        // omission reason. A bad seal still fails closed whenever the envelope
        // would otherwise permit directive delivery.
        let base_reason = base_omission_reason(candidate, context)
            .or_else(|| conflict_reasons.get(&candidate.record_id).copied());
        let rule_chars = candidate.rule.chars().count();
        let layer_full = if core {
            core_count >= context.max_core_records
        } else {
            scoped_count >= context.max_scoped_records
        };
        let budget_full = delivered.len() >= context.max_total_records
            || rendered_chars.saturating_add(rule_chars) > context.max_rendered_chars;
        let reason = base_reason.unwrap_or(if layer_full || budget_full {
            "selection_budget"
        } else {
            "applicable"
        });
        let selected = reason == "applicable";
        let receipt = delivery_receipt(candidate, selected, reason, context);
        if selected {
            rendered_chars += rule_chars;
            if core {
                core_count += 1;
            } else {
                scoped_count += 1;
            }
            delivered.push(DeliveredPreferenceV1 {
                record_id: candidate.record_id.clone(),
                rule: candidate.rule.clone(),
                receipt: receipt.clone(),
            });
        }
        receipts.push(receipt);
    }
    PreferenceDeliveryPlanV1 {
        delivered,
        receipts,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliverySelectionV1 {
    pub records: Vec<PreferenceRecordV1>,
    pub receipts: Vec<PreferenceDeliveryReceiptV1>,
}

/// Cheap structured selection. No semantic search participates; inactive,
/// provisional, nonmatching, or excess records receive explicit omission
/// receipts.
pub fn select_preferences(
    records: &[PreferenceRecordV1],
    context: &ScopeDimensions,
    max_records: usize,
    timestamp: &str,
) -> DeliverySelectionV1 {
    let candidates = records
        .iter()
        .map(|record| PreferenceDeliveryCandidateV1 {
            record_id: record.id.clone(),
            rule: record.rule.clone(),
            class: record.class,
            scope: record.scope.clone(),
            scope_dimensions: record.scope_dimensions.clone(),
            machine_binding: None,
            authority_tier: if record.scope == "global" {
                PrecedenceTier::ExplicitGlobalUserPreference
            } else {
                PrecedenceTier::ExplicitScopedUserPreference
            },
            lifecycle_state: record.lifecycle_state,
            lifecycle_eligible: true,
            influence_class: record.influence_class,
            semantic_verified: true,
        })
        .collect::<Vec<_>>();
    let plan = select_delivery_candidates(
        &candidates,
        &PreferenceDeliveryContextV1 {
            allowed_scopes: records.iter().map(|record| record.scope.clone()).collect(),
            dimensions: context.clone(),
            machine: None,
            max_core_records: max_records,
            max_scoped_records: max_records,
            max_total_records: max_records,
            max_rendered_chars: usize::MAX,
            timestamp: timestamp.into(),
            session_id: String::new(),
            trace_id: String::new(),
            request_id: format!("legacy-{timestamp}"),
            client: context.get("client").unwrap_or("").into(),
            model: context.get("model").map(str::to_string),
        },
    );
    let selected_ids = plan
        .delivered
        .iter()
        .map(|delivery| delivery.record_id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let selected = records
        .iter()
        .filter(|record| selected_ids.contains(record.id.as_str()))
        .cloned()
        .collect();
    DeliverySelectionV1 {
        records: selected,
        receipts: plan.receipts,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectivenessEventKind {
    Selected,
    Adhered,
    Corrected,
    Overridden,
    Retired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectivenessEventV1 {
    pub record_id: String,
    pub delivery_receipt_id: String,
    pub kind: EffectivenessEventKind,
    pub model: Option<String>,
    pub client: Option<String>,
    pub correctness_preserved: Option<bool>,
    pub timestamp: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EffectivenessMetricsV1 {
    pub selections: u32,
    pub adherences: u32,
    pub corrections: u32,
    pub overrides: u32,
    pub retirements: u32,
    pub correctness_regressions: u32,
}

impl EffectivenessMetricsV1 {
    pub fn adherence_rate(&self) -> Option<f64> {
        (self.selections > 0).then(|| self.adherences as f64 / self.selections as f64)
    }

    pub fn correction_rate(&self) -> Option<f64> {
        (self.selections > 0).then(|| self.corrections as f64 / self.selections as f64)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EffectivenessLedgerV1 {
    pub events: Vec<EffectivenessEventV1>,
}

impl EffectivenessLedgerV1 {
    pub fn record(&mut self, event: EffectivenessEventV1) {
        self.events.push(event);
    }

    pub fn metrics(
        &self,
        record_id: &str,
        model: Option<&str>,
        client: Option<&str>,
    ) -> EffectivenessMetricsV1 {
        let mut metrics = EffectivenessMetricsV1::default();
        for event in self.events.iter().filter(|event| {
            event.record_id == record_id
                && model.is_none_or(|value| event.model.as_deref() == Some(value))
                && client.is_none_or(|value| event.client.as_deref() == Some(value))
        }) {
            match event.kind {
                EffectivenessEventKind::Selected => metrics.selections += 1,
                EffectivenessEventKind::Adhered => metrics.adherences += 1,
                EffectivenessEventKind::Corrected => metrics.corrections += 1,
                EffectivenessEventKind::Overridden => metrics.overrides += 1,
                EffectivenessEventKind::Retired => metrics.retirements += 1,
            }
            if event.correctness_preserved == Some(false) {
                metrics.correctness_regressions += 1;
            }
        }
        metrics
    }

    pub fn by_surface(
        &self,
        record_id: &str,
    ) -> BTreeMap<(String, String), EffectivenessMetricsV1> {
        let mut surfaces = BTreeMap::new();
        for event in self
            .events
            .iter()
            .filter(|event| event.record_id == record_id)
        {
            let key = (
                event.model.clone().unwrap_or_else(|| "unknown".into()),
                event.client.clone().unwrap_or_else(|| "unknown".into()),
            );
            surfaces.insert(
                key.clone(),
                self.metrics(record_id, Some(&key.0), Some(&key.1)),
            );
        }
        surfaces
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::RecordClass;

    fn opaque_dimension_scope(dimensions: &ScopeDimensions) -> String {
        let dimensions = dimensions
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<BTreeMap<_, _>>();
        let digest = crate::canonical::sha256_canonical(
            &serde_json::to_value(dimensions).expect("scope dimensions serialize"),
        );
        format!("dimensions:{}", &digest[..24])
    }

    fn active() -> PreferenceRecordV1 {
        let mut record = PreferenceRecordV1::new_candidate(
            "Always run focused tests",
            "verification",
            RecordClass::StandingPreference,
            "repo",
            ScopeDimensions::default(),
            1.0,
            vec!["ev".into()],
            "t",
        )
        .unwrap();
        record.lifecycle_state = LifecycleState::Active;
        record.influence_class = InfluenceClass::BehavioralDirective;
        record
    }

    #[test]
    fn delivery_filters_inactive_and_emits_receipts() {
        let active = active();
        let mut inactive = active.clone();
        inactive.id.push('x');
        inactive.lifecycle_state = LifecycleState::Retired;
        let result = select_preferences(&[active, inactive], &ScopeDimensions::default(), 8, "t");
        assert_eq!(result.records.len(), 1);
        assert_eq!(result.receipts.len(), 2);
        assert!(result.receipts.iter().any(|receipt| !receipt.selected));
    }

    #[test]
    fn effectiveness_is_surface_specific() {
        let mut ledger = EffectivenessLedgerV1::default();
        for kind in [
            EffectivenessEventKind::Selected,
            EffectivenessEventKind::Adhered,
        ] {
            ledger.record(EffectivenessEventV1 {
                record_id: "r".into(),
                delivery_receipt_id: "d".into(),
                kind,
                model: Some("ox".into()),
                client: Some("pi".into()),
                correctness_preserved: Some(true),
                timestamp: "t".into(),
            });
        }
        assert_eq!(
            ledger.metrics("r", Some("ox"), Some("pi")).adherence_rate(),
            Some(1.0)
        );
        assert_eq!(ledger.metrics("r", Some("other"), None).selections, 0);
    }

    #[test]
    fn production_selection_prioritizes_core_and_shares_exact_budgets() {
        let mut scoped = active();
        scoped.id = "a-scoped".into();
        scoped.class = RecordClass::ScopedPreference;
        scoped.scope = "repo".into();
        scoped.rule = "scoped rule".into();
        let mut core = active();
        core.id = "z-core".into();
        core.scope = "global".into();
        core.rule = "core rule".into();
        let candidates = [&scoped, &core]
            .into_iter()
            .map(|record| PreferenceDeliveryCandidateV1 {
                record_id: record.id.clone(),
                rule: record.rule.clone(),
                class: record.class,
                scope: record.scope.clone(),
                scope_dimensions: record.scope_dimensions.clone(),
                machine_binding: None,
                authority_tier: if record.scope == "global" {
                    PrecedenceTier::ExplicitGlobalUserPreference
                } else {
                    PrecedenceTier::ExplicitScopedUserPreference
                },
                lifecycle_state: record.lifecycle_state,
                lifecycle_eligible: true,
                influence_class: record.influence_class,
                semantic_verified: true,
            })
            .collect::<Vec<_>>();
        let context = PreferenceDeliveryContextV1 {
            allowed_scopes: vec!["global".into(), "repo".into()],
            dimensions: ScopeDimensions::default(),
            machine: None,
            max_core_records: 1,
            max_scoped_records: 1,
            max_total_records: 1,
            max_rendered_chars: 100,
            timestamp: "2026-08-26T00:00:00Z".into(),
            session_id: "session-1".into(),
            trace_id: "trace-1".into(),
            request_id: "request-1".into(),
            client: "codex".into(),
            model: Some("gpt".into()),
        };
        let plan = select_delivery_candidates(&candidates, &context);
        assert_eq!(plan.delivered.len(), 1);
        assert_eq!(plan.delivered[0].record_id, "z-core");
        assert_eq!(plan.receipts.len(), 2);
        assert!(plan.receipts.iter().any(|receipt| {
            receipt.record_id == "a-scoped"
                && !receipt.selected
                && receipt.applicability_reason == "selection_budget"
        }));
        assert_eq!(
            plan.delivered[0].receipt.rendered_sha256.as_deref(),
            Some(crate::canonical::sha256_hex(b"core rule").as_str())
        );

        let mut changed = context;
        changed.trace_id = "trace-2".into();
        let changed = select_delivery_candidates(&candidates, &changed);
        assert_ne!(
            plan.delivered[0].receipt.receipt_id,
            changed.delivered[0].receipt.receipt_id
        );
    }

    #[test]
    fn production_selection_fails_closed_for_untrusted_candidates() {
        let record = active();
        let base = PreferenceDeliveryCandidateV1 {
            record_id: record.id,
            rule: record.rule,
            class: record.class,
            scope: record.scope,
            scope_dimensions: record.scope_dimensions,
            machine_binding: None,
            authority_tier: PrecedenceTier::ExplicitScopedUserPreference,
            lifecycle_state: record.lifecycle_state,
            lifecycle_eligible: true,
            influence_class: record.influence_class,
            semantic_verified: false,
        };
        let context = PreferenceDeliveryContextV1 {
            allowed_scopes: vec!["repo".into()],
            dimensions: ScopeDimensions::default(),
            machine: None,
            max_core_records: 2,
            max_scoped_records: 2,
            max_total_records: 2,
            max_rendered_chars: 100,
            timestamp: "2026-08-26T00:00:00Z".into(),
            session_id: "session-1".into(),
            trace_id: "trace-1".into(),
            request_id: "request-1".into(),
            client: "codex".into(),
            model: None,
        };
        let plan = select_delivery_candidates(&[base], &context);
        assert!(plan.delivered.is_empty());
        assert_eq!(
            plan.receipts[0].applicability_reason,
            "invalid_semantic_seal"
        );
        assert!(plan.receipts[0].rendered_sha256.is_none());
    }

    #[test]
    fn signed_dimension_scope_delivers_without_opaque_caller_scope() {
        let dimensions = ScopeDimensions::normalize(&BTreeMap::from([
            ("language".into(), "rust".into()),
            ("path_prefix".into(), "engine/src".into()),
        ]))
        .unwrap();
        let candidate = PreferenceDeliveryCandidateV1 {
            record_id: "signed-scoped".into(),
            rule: "Prefer focused Rust verification.".into(),
            class: RecordClass::ScopedPreference,
            scope: opaque_dimension_scope(&dimensions),
            scope_dimensions: dimensions,
            machine_binding: None,
            authority_tier: PrecedenceTier::ExplicitScopedUserPreference,
            lifecycle_state: LifecycleState::Active,
            lifecycle_eligible: true,
            influence_class: InfluenceClass::BehavioralDirective,
            semantic_verified: true,
        };
        let context = PreferenceDeliveryContextV1 {
            allowed_scopes: vec!["global".into(), "D--Claude-repo".into()],
            dimensions: ScopeDimensions::normalize(&BTreeMap::from([
                ("language".into(), "Rust".into()),
                ("path_prefix".into(), "engine/src/adapt".into()),
            ]))
            .unwrap(),
            machine: None,
            max_core_records: 1,
            max_scoped_records: 1,
            max_total_records: 1,
            max_rendered_chars: 100,
            timestamp: "2026-08-26T00:00:00Z".into(),
            session_id: "session-1".into(),
            trace_id: "trace-1".into(),
            request_id: "request-1".into(),
            client: "codex".into(),
            model: None,
        };

        let plan = select_delivery_candidates(&[candidate], &context);
        assert_eq!(plan.delivered.len(), 1);
        assert_eq!(plan.receipts[0].applicability_reason, "applicable");
    }

    #[test]
    fn signed_dimension_scope_omits_nonmatch_and_rejects_digest_substitution() {
        let dimensions = ScopeDimensions::normalize(&BTreeMap::from([
            ("language".into(), "rust".into()),
            ("path_prefix".into(), "engine/src".into()),
        ]))
        .unwrap();
        let base = PreferenceDeliveryCandidateV1 {
            record_id: "signed-scoped".into(),
            rule: "Prefer focused Rust verification.".into(),
            class: RecordClass::ScopedPreference,
            scope: opaque_dimension_scope(&dimensions),
            scope_dimensions: dimensions,
            machine_binding: None,
            authority_tier: PrecedenceTier::ExplicitScopedUserPreference,
            lifecycle_state: LifecycleState::Active,
            lifecycle_eligible: true,
            influence_class: InfluenceClass::BehavioralDirective,
            semantic_verified: true,
        };
        let context = PreferenceDeliveryContextV1 {
            allowed_scopes: vec!["global".into(), "D--Claude-repo".into()],
            dimensions: ScopeDimensions::normalize(&BTreeMap::from([
                ("language".into(), "python".into()),
                ("path_prefix".into(), "engine/src/adapt".into()),
            ]))
            .unwrap(),
            machine: None,
            max_core_records: 2,
            max_scoped_records: 2,
            max_total_records: 2,
            max_rendered_chars: 200,
            timestamp: "2026-08-26T00:00:00Z".into(),
            session_id: "session-1".into(),
            trace_id: "trace-1".into(),
            request_id: "request-1".into(),
            client: "codex".into(),
            model: None,
        };

        let nonmatch = select_delivery_candidates(&[base.clone()], &context);
        assert!(nonmatch.delivered.is_empty());
        assert_eq!(nonmatch.receipts[0].applicability_reason, "scope_nonmatch");

        let mut substituted = base;
        substituted.record_id = "substituted-scope".into();
        substituted.scope = format!("dimensions:{}", "0".repeat(24));
        let matching_context = PreferenceDeliveryContextV1 {
            dimensions: substituted.scope_dimensions.clone(),
            ..context
        };
        let substituted = select_delivery_candidates(&[substituted], &matching_context);
        assert!(substituted.delivered.is_empty());
        assert_eq!(
            substituted.receipts[0].applicability_reason,
            "scope_nonmatch"
        );
    }

    #[test]
    fn production_selection_dedupes_and_resolves_cross_tier_conflicts() {
        let context = PreferenceDeliveryContextV1 {
            allowed_scopes: vec!["global".into(), "repo".into()],
            dimensions: ScopeDimensions::default(),
            machine: None,
            max_core_records: 4,
            max_scoped_records: 4,
            max_total_records: 4,
            max_rendered_chars: 400,
            timestamp: "2026-08-26T00:00:00Z".into(),
            session_id: "session-1".into(),
            trace_id: "trace-1".into(),
            request_id: "request-1".into(),
            client: "codex".into(),
            model: None,
        };
        let candidate = |id: &str, rule: &str, scope: &str, tier| PreferenceDeliveryCandidateV1 {
            record_id: id.into(),
            rule: rule.into(),
            class: if scope == "global" {
                RecordClass::StandingPreference
            } else {
                RecordClass::ScopedPreference
            },
            scope: scope.into(),
            scope_dimensions: ScopeDimensions::default(),
            machine_binding: None,
            authority_tier: tier,
            lifecycle_state: LifecycleState::Active,
            lifecycle_eligible: true,
            influence_class: InfluenceClass::BehavioralDirective,
            semantic_verified: true,
        };
        let duplicate = select_delivery_candidates(
            &[
                candidate(
                    "global-copy",
                    "Always squash commits",
                    "global",
                    PrecedenceTier::ExplicitGlobalUserPreference,
                ),
                candidate(
                    "scoped-copy",
                    "Always squash commits",
                    "repo",
                    PrecedenceTier::ExplicitScopedUserPreference,
                ),
            ],
            &context,
        );
        assert_eq!(duplicate.delivered.len(), 1);
        assert_eq!(duplicate.delivered[0].record_id, "scoped-copy");
        assert!(duplicate.receipts.iter().any(|receipt| {
            receipt.record_id == "global-copy"
                && receipt.applicability_reason == "duplicate_delivery"
        }));

        let conflict = select_delivery_candidates(
            &[
                candidate(
                    "global-allow",
                    "Always squash commits",
                    "global",
                    PrecedenceTier::ExplicitGlobalUserPreference,
                ),
                candidate(
                    "scoped-deny",
                    "Never squash commits",
                    "repo",
                    PrecedenceTier::ExplicitScopedUserPreference,
                ),
            ],
            &context,
        );
        assert_eq!(conflict.delivered.len(), 1);
        assert_eq!(conflict.delivered[0].record_id, "scoped-deny");
        assert!(conflict.receipts.iter().any(|receipt| {
            receipt.record_id == "global-allow"
                && receipt.applicability_reason == "shadowed_by_precedence"
        }));
    }
}
