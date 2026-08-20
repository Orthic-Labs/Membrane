//! Task-effectiveness gating: gate memory injection on task effectiveness signals.
//!
//! Instead of relying solely on retrieval scores, this module tracks whether
//! injected memory entries actually contributed to task success.

use serde::{Deserialize, Serialize};

/// Per-candidate recall outcome. Sourced from an OBSERVED action (not a self-report):
/// `Used` = the agent fetched the full body after the preview; `Contradicted` = the agent
/// deleted / superseded / edited the recalled entry within the turn; `Ignored` = advisory only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Used,
    Ignored,
    Contradicted,
}

/// Record of a memory entry's effectiveness in a task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryUsageRecord {
    /// Unique identifier for the memory entry.
    pub entry_id: String,
    /// Whether this entry was injected into the task context.
    pub injected: bool,
    /// Whether the task ultimately succeeded (legacy task-level signal; used only when
    /// `outcome` is `None`, preserving the pre-feedback-rail `record_run` path).
    pub task_succeeded: bool,
    /// Per-candidate observed outcome. `None` = legacy task-level row.
    pub outcome: Option<Outcome>,
    /// Only verified rows (observed actions, or a verdict citing this id+sha) affect ranking.
    /// Unverified rows (agent self-report, uncited verdict) are advisory and skipped.
    pub verified: bool,
}

impl MemoryUsageRecord {
    /// Legacy task-level row: no per-candidate outcome, drives ranking via `task_succeeded`.
    pub fn legacy(entry_id: impl Into<String>, injected: bool, task_succeeded: bool) -> Self {
        Self {
            entry_id: entry_id.into(),
            injected,
            task_succeeded,
            outcome: None,
            verified: false,
        }
    }

    /// A verified per-candidate row from an observed action.
    pub fn verified_outcome(entry_id: impl Into<String>, outcome: Outcome) -> Self {
        Self {
            entry_id: entry_id.into(),
            injected: true,
            task_succeeded: matches!(outcome, Outcome::Used),
            outcome: Some(outcome),
            verified: true,
        }
    }

    /// Does this row count toward the effectiveness ratio? Legacy rows always count; a row with a
    /// per-candidate outcome counts only when verified and not `Ignored` (advisory).
    fn counts_for_ranking(&self) -> bool {
        match self.outcome {
            None => true,
            Some(Outcome::Ignored) => false,
            Some(_) => self.verified,
        }
    }

    /// Success for the ratio: legacy uses `task_succeeded`; a verified per-candidate row is a
    /// success iff `Used`.
    fn is_success(&self) -> bool {
        match self.outcome {
            None => self.task_succeeded,
            Some(Outcome::Used) => true,
            _ => false,
        }
    }

    /// A verified `Contradicted` row is a hard veto (until the entry is superseded — realized
    /// upstream because feedback is keyed by content sha, so a superseding memory has new bytes).
    fn is_veto(&self) -> bool {
        self.verified && self.injected && matches!(self.outcome, Some(Outcome::Contradicted))
    }
}

/// Gates memory injection based on historical effectiveness.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectivenessGate {
    /// Minimum effectiveness ratio (success rate) required to inject.
    /// In range [0.0, 1.0].
    pub min_effectiveness: f64,
}

impl EffectivenessGate {
    /// Create a new gate with the given minimum effectiveness threshold.
    pub fn new(min_effectiveness: f64) -> Self {
        Self {
            min_effectiveness: min_effectiveness.clamp(0.0, 1.0),
        }
    }

    /// Compute the historical effectiveness of an entry.
    ///
    /// Returns the ratio of successful tasks where this entry was injected
    /// over all tasks where it was injected. If the entry was never injected,
    /// returns 0.0.
    pub fn effectiveness(&self, history: &[MemoryUsageRecord], entry_id: &str) -> f64 {
        let usages: Vec<_> = history
            .iter()
            .filter(|r| r.entry_id == entry_id && r.injected && r.counts_for_ranking())
            .collect();

        if usages.is_empty() {
            return 0.0;
        }

        let successes = usages.iter().filter(|r| r.is_success()).count();
        successes as f64 / usages.len() as f64
    }

    /// Predicate: should this entry be injected?
    ///
    /// Returns false if any verified `Contradicted` veto exists for this entry. Otherwise returns
    /// true if:
    /// 1. No ranking-relevant history exists for this entry (default-allow), OR
    /// 2. The effectiveness >= min_effectiveness
    pub fn should_inject(&self, history: &[MemoryUsageRecord], entry_id: &str) -> bool {
        // A single verified Contradicted vetoes the entry (until superseded).
        if history
            .iter()
            .any(|r| r.entry_id == entry_id && r.is_veto())
        {
            return false;
        }

        let has_ranking_history = history
            .iter()
            .any(|r| r.entry_id == entry_id && r.injected && r.counts_for_ranking());
        if !has_ranking_history {
            return true; // Allow if no ranking-relevant history
        }

        self.effectiveness(history, entry_id) >= self.min_effectiveness
    }
}

impl Default for EffectivenessGate {
    fn default() -> Self {
        Self::new(0.5) // Default: require at least 50% success rate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn legacy(entry_id: &str, injected: bool, ok: bool) -> MemoryUsageRecord {
        MemoryUsageRecord::legacy(entry_id, injected, ok)
    }

    #[test]
    fn effectiveness_no_history_is_zero() {
        let gate = EffectivenessGate::new(0.5);
        let history = vec![];
        let eff = gate.effectiveness(&history, "entry-1");
        assert_eq!(eff, 0.0);
    }

    #[test]
    fn new_clamps_threshold_to_valid_range() {
        assert_eq!(EffectivenessGate::new(-1.0).min_effectiveness, 0.0);
        assert_eq!(EffectivenessGate::new(2.0).min_effectiveness, 1.0);
    }

    #[test]
    fn effectiveness_all_successes() {
        let gate = EffectivenessGate::new(0.5);
        let history = vec![legacy("entry-1", true, true), legacy("entry-1", true, true)];
        assert_eq!(gate.effectiveness(&history, "entry-1"), 1.0);
    }

    #[test]
    fn effectiveness_partial_success() {
        let gate = EffectivenessGate::new(0.5);
        let history = vec![
            legacy("entry-1", true, true),
            legacy("entry-1", true, false),
            legacy("entry-1", true, true),
        ];
        assert_eq!(gate.effectiveness(&history, "entry-1"), 2.0 / 3.0);
    }

    #[test]
    fn effectiveness_ignores_non_injected() {
        let gate = EffectivenessGate::new(0.5);
        let history = vec![
            legacy("entry-1", false, true),
            legacy("entry-1", true, true),
        ];
        assert_eq!(gate.effectiveness(&history, "entry-1"), 1.0); // Only the injected one counts
    }

    #[test]
    fn should_inject_default_allows_new_entry() {
        let gate = EffectivenessGate::new(0.5);
        assert!(gate.should_inject(&[], "new-entry"));
    }

    #[test]
    fn should_inject_allows_entry_with_only_non_injected_history() {
        let gate = EffectivenessGate::new(0.5);
        let history = vec![legacy("entry-1", false, false)];
        assert!(gate.should_inject(&history, "entry-1"));
    }

    #[test]
    fn should_inject_allows_effective_entry() {
        let gate = EffectivenessGate::new(0.5);
        let history = vec![legacy("entry-1", true, true), legacy("entry-1", true, true)];
        assert!(gate.should_inject(&history, "entry-1"));
    }

    #[test]
    fn should_inject_blocks_ineffective_entry() {
        let gate = EffectivenessGate::new(0.5);
        let history = vec![
            legacy("entry-1", true, false),
            legacy("entry-1", true, false),
        ];
        assert!(!gate.should_inject(&history, "entry-1"));
    }

    #[test]
    fn should_inject_respects_threshold() {
        let gate = EffectivenessGate::new(0.75); // High bar
        let history = vec![
            legacy("entry-1", true, true),
            legacy("entry-1", true, false),
            legacy("entry-1", true, false),
        ];
        // Effectiveness is 1/3, below 0.75 threshold.
        assert!(!gate.should_inject(&history, "entry-1"));
    }

    #[test]
    fn should_inject_allows_entry_at_exact_threshold() {
        let gate = EffectivenessGate::new(0.5);
        let history = vec![
            legacy("entry-1", true, true),
            legacy("entry-1", true, false),
        ];
        assert!(gate.should_inject(&history, "entry-1"));
    }

    #[test]
    fn should_inject_filters_by_entry_id() {
        let gate = EffectivenessGate::new(0.5);
        let history = vec![
            legacy("entry-1", true, false),
            legacy("entry-2", true, true),
        ];
        assert!(!gate.should_inject(&history, "entry-1"));
        assert!(gate.should_inject(&history, "entry-2"));
    }

    #[test]
    fn default_gate_threshold() {
        let gate = EffectivenessGate::default();
        assert_eq!(gate.min_effectiveness, 0.5);
    }

    // ---- Feedback-rail: per-candidate verified outcomes -------------------

    #[test]
    fn verified_contradicted_drives_below_threshold() {
        // The Council Loop 1 target: one verified Contradicted vetoes the entry even against
        // otherwise-positive history; an UNverified Contradicted does not.
        let gate = EffectivenessGate::new(0.5);
        let mut history = vec![legacy("entry-1", true, true), legacy("entry-1", true, true)];
        assert!(gate.should_inject(&history, "entry-1"));

        history.push(MemoryUsageRecord::verified_outcome(
            "entry-1",
            Outcome::Contradicted,
        ));
        assert!(
            !gate.should_inject(&history, "entry-1"),
            "a verified Contradicted must veto the entry"
        );
    }

    #[test]
    fn unverified_contradicted_does_not_veto() {
        let gate = EffectivenessGate::new(0.5);
        let history = vec![
            legacy("entry-1", true, true),
            MemoryUsageRecord {
                entry_id: "entry-1".to_string(),
                injected: true,
                task_succeeded: false,
                outcome: Some(Outcome::Contradicted),
                verified: false, // advisory only
            },
        ];
        assert!(
            gate.should_inject(&history, "entry-1"),
            "an unverified Contradicted is advisory and must not veto"
        );
    }

    #[test]
    fn verified_used_counts_as_success() {
        let gate = EffectivenessGate::new(0.75);
        let history = vec![
            MemoryUsageRecord::verified_outcome("entry-1", Outcome::Used),
            MemoryUsageRecord::verified_outcome("entry-1", Outcome::Used),
        ];
        assert_eq!(gate.effectiveness(&history, "entry-1"), 1.0);
        assert!(gate.should_inject(&history, "entry-1"));
    }

    #[test]
    fn ignored_is_advisory_and_skipped_from_ranking() {
        let gate = EffectivenessGate::new(0.5);
        // Only an Ignored row exists -> no ranking history -> default-allow, not a failure.
        let history = vec![MemoryUsageRecord {
            entry_id: "entry-1".to_string(),
            injected: true,
            task_succeeded: false,
            outcome: Some(Outcome::Ignored),
            verified: true,
        }];
        assert_eq!(gate.effectiveness(&history, "entry-1"), 0.0);
        assert!(gate.should_inject(&history, "entry-1"));
    }
}
