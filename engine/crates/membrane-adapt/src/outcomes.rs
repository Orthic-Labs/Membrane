//! Mitigation outcome ledger (canon §6.5).
//!
//! Tracks whether a mitigation actually reduced recurrence, with
//! exposure-adjusted outcomes and deterministic reopen logic. The ledger is
//! append-only; corrections are new entries that supersede, never delete.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Raw outcome of one observation window after mitigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RawOutcome {
    NoRecurrence,
    RecurredSameSignature,
    RecurredDifferentSignature,
    ObservationIncomplete,
}

/// Exposure adjustment: raw outcomes are normalized by how much the affected
/// surface was actually used in the window. Low exposure with no recurrence
/// is weaker evidence than high exposure with no recurrence.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Exposure {
    /// Number of opportunities for the failure mode to recur (sessions,
    /// invocations, or a domain-specific counter).
    pub opportunities: u32,
    /// Baseline expected opportunities for a comparable unmitigated window.
    pub baseline: u32,
}

impl Exposure {
    pub fn ratio(&self) -> f64 {
        if self.baseline == 0 {
            return 0.0;
        }
        (self.opportunities as f64 / self.baseline as f64).clamp(0.0, 1.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdjustedOutcome {
    /// Sufficient exposure, no recurrence: strong success signal.
    Effective,
    /// Some exposure, no recurrence: weak positive.
    ProbablyEffective,
    /// Recurrence at same signature: mitigation failed.
    Ineffective,
    /// Recurrence but different signature: partially effective.
    PartiallyEffective,
    Indeterminate,
}

/// One ledger entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutcomeEntryV1 {
    pub entry_id: String,
    pub issue_id: String,
    pub mitigation_proposal_id: String,
    pub raw: RawOutcome,
    pub exposure: Exposure,
    pub adjusted: AdjustedOutcome,
    #[serde(default)]
    pub note: String,
}

/// Append-only outcome ledger.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OutcomeLedger {
    entries: Vec<OutcomeEntryV1>,
    index: BTreeMap<String, Vec<usize>>, // issue_id -> entry indexes
}

fn adjust(raw: RawOutcome, exposure: &Exposure) -> AdjustedOutcome {
    let r = exposure.ratio();
    match raw {
        RawOutcome::NoRecurrence if r >= 0.8 => AdjustedOutcome::Effective,
        RawOutcome::NoRecurrence => AdjustedOutcome::ProbablyEffective,
        RawOutcome::RecurredSameSignature => AdjustedOutcome::Ineffective,
        RawOutcome::RecurredDifferentSignature => AdjustedOutcome::PartiallyEffective,
        RawOutcome::ObservationIncomplete => AdjustedOutcome::Indeterminate,
    }
}

impl OutcomeLedger {
    pub fn record(
        &mut self,
        issue_id: &str,
        mitigation_proposal_id: &str,
        raw: RawOutcome,
        exposure: Exposure,
        note: &str,
    ) -> &OutcomeEntryV1 {
        let id_src = format!(
            "{issue_id}\u{0}{mitigation_proposal_id}\u{0}{:?}\u{0}{}",
            raw, exposure.opportunities
        );
        let entry = OutcomeEntryV1 {
            entry_id: format!("out-{}", &crate::canonical::sha256_hex(id_src.as_bytes())[..12].to_string()),
            issue_id: issue_id.to_string(),
            mitigation_proposal_id: mitigation_proposal_id.to_string(),
            raw,
            exposure,
            adjusted: adjust(raw, &exposure),
            note: note.to_string(),
        };
        let idx = self.entries.len();
        self.index.entry(issue_id.to_string()).or_default().push(idx);
        self.entries.push(entry);
        &self.entries[idx]
    }

    pub fn entries_for_issue(&self, issue_id: &str) -> Vec<&OutcomeEntryV1> {
        self.index
            .get(issue_id)
            .map(|idxs| idxs.iter().map(|i| &self.entries[*i]).collect())
            .unwrap_or_default()
    }

    /// Deterministic reopen decision: an issue reopens iff its latest
    /// effective-relevant entry records same-signature recurrence. Different-
    /// signature recurrence does NOT reopen; it stays as partial evidence.
    pub fn should_reopen(&self, issue_id: &str) -> bool {
        self.entries_for_issue(issue_id)
            .last()
            .map(|e| e.raw == RawOutcome::RecurredSameSignature)
            .unwrap_or(false)
    }

    /// Aggregate effectiveness across all entries for an issue.
    pub fn aggregate_effectiveness(&self, issue_id: &str) -> f64 {
        let score = |a: &AdjustedOutcome| match a {
            AdjustedOutcome::Effective => 1.0,
            AdjustedOutcome::ProbablyEffective => 0.6,
            AdjustedOutcome::PartiallyEffective => 0.4,
            AdjustedOutcome::Indeterminate => 0.2,
            AdjustedOutcome::Ineffective => 0.0,
        };
        let es = self.entries_for_issue(issue_id);
        if es.is_empty() {
            return 0.0;
        }
        es.iter().map(|e| score(&e.adjusted)).sum::<f64>() / es.len() as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exp(o: u32) -> Exposure {
        Exposure { opportunities: o, baseline: 10 }
    }

    #[test]
    fn exposure_adjusts_outcome() {
        assert_eq!(adjust(RawOutcome::NoRecurrence, &exp(9)), AdjustedOutcome::Effective);
        assert_eq!(adjust(RawOutcome::NoRecurrence, &exp(3)), AdjustedOutcome::ProbablyEffective);
        assert_eq!(
            adjust(RawOutcome::RecurredSameSignature, &exp(9)),
            AdjustedOutcome::Ineffective
        );
    }

    #[test]
    fn reopen_only_on_same_signature_recurrence() {
        let mut l = OutcomeLedger::default();
        l.record("i", "m", RawOutcome::NoRecurrence, exp(9), "");
        assert!(!l.should_reopen("i"));
        l.record("i", "m", RawOutcome::RecurredDifferentSignature, exp(9), "");
        assert!(!l.should_reopen("i"));
        l.record("i", "m", RawOutcome::RecurredSameSignature, exp(9), "");
        assert!(l.should_reopen("i"));
    }

    #[test]
    fn aggregate_effectiveness_is_deterministic_mean() {
        let mut l = OutcomeLedger::default();
        l.record("i", "m", RawOutcome::NoRecurrence, exp(10), "");
        l.record("i", "m", RawOutcome::RecurredSameSignature, exp(10), "");
        assert!((l.aggregate_effectiveness("i") - 0.5).abs() < 1e-9);
        assert_eq!(l.aggregate_effectiveness("missing"), 0.0);
    }
}
