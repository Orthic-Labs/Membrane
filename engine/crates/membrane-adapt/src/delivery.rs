//! Preference-specific delivery & effectiveness receipts.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::record::{InfluenceClass, LifecycleState, PreferenceRecordV1};
use crate::scope::ScopeDimensions;

pub const DELIVERY_RECEIPT_SCHEMA: &str = "adapt.preference-delivery.v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreferenceDeliveryReceiptV1 {
    pub schema_version: String,
    pub receipt_id: String,
    pub record_id: String,
    pub selected: bool,
    pub applicability_reason: String,
    pub model: Option<String>,
    pub client: Option<String>,
    pub rendered_sha256: Option<String>,
    pub timestamp: String,
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
    let mut ordered: Vec<&PreferenceRecordV1> = records.iter().collect();
    ordered.sort_by(|a, b| a.id.cmp(&b.id));
    let mut selected = Vec::new();
    let mut receipts = Vec::with_capacity(ordered.len());
    for record in ordered {
        let (eligible, reason) = if record.lifecycle_state != LifecycleState::Active {
            (false, "inactive_lifecycle")
        } else if record.influence_class != InfluenceClass::BehavioralDirective {
            (false, "non_directive_influence")
        } else if !record.scope_dimensions.matches(context) {
            (false, "scope_nonmatch")
        } else if selected.len() >= max_records {
            (false, "selection_budget")
        } else {
            (true, "applicable")
        };
        let rendered_sha256 = eligible.then(|| crate::canonical::sha256_hex(record.rule.as_bytes()));
        let identity = format!(
            "{}\0{}\0{}\0{}",
            record.id, eligible, reason, timestamp
        );
        receipts.push(PreferenceDeliveryReceiptV1 {
            schema_version: DELIVERY_RECEIPT_SCHEMA.into(),
            receipt_id: format!("pdr_{}", crate::canonical::sha256_hex(identity.as_bytes())),
            record_id: record.id.clone(),
            selected: eligible,
            applicability_reason: reason.into(),
            model: context.get("model").map(str::to_string),
            client: context.get("client").map(str::to_string),
            rendered_sha256,
            timestamp: timestamp.into(),
        });
        if eligible {
            selected.push(record.clone());
        }
    }
    DeliverySelectionV1 {
        records: selected,
        receipts,
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

    pub fn by_surface(&self, record_id: &str) -> BTreeMap<(String, String), EffectivenessMetricsV1> {
        let mut surfaces = BTreeMap::new();
        for event in self.events.iter().filter(|event| event.record_id == record_id) {
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
        for kind in [EffectivenessEventKind::Selected, EffectivenessEventKind::Adhered] {
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
        assert_eq!(ledger.metrics("r", Some("ox"), Some("pi")).adherence_rate(), Some(1.0));
        assert_eq!(ledger.metrics("r", Some("other"), None).selections, 0);
    }
}
