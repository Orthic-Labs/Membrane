//! Multi-writer deterministic merge & convergence (canon §8).
//!
//! Multiple writers (hosts, agents, sessions) may produce records for the
//! same RuleKey. Merge is order-independent: applying the same set of
//! records in any order yields identical store state. Conflicts between
//! equal-precedence divergent payloads are PRESERVED as conflicts, never
//! silently resolved by arrival order.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::authority::{compare_precedence, PrecedenceTier};
use crate::record::RuleKey;

/// A writer-agnostic record envelope entering merge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriterRecord {
    pub writer_id: String,
    /// Logical clock for deterministic tie-breaks (writer sequence number).
    pub lamport: u64,
    pub rule_key: RuleKey,
    /// Digest of the sealed semantic payload.
    pub payload_digest: String,
    pub precedence_tier: PrecedenceTier,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeOutcome {
    /// New record won; previous stored digest recorded in `superseded`.
    Applied { superseded: Option<String> },
    /// Incoming record lost to an already-stored stronger/equal record.
    Ignored,
    /// Equal-precedence divergent payload: conflict preserved.
    ConflictPreserved { with_writer: String },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MergedStore {
    /// rule key -> (winning digest, winning writer)
    winners: BTreeMap<String, (String, String)>,
    /// rule key -> tier of the winning record
    tiers: BTreeMap<String, PrecedenceTier>,
    /// rule key -> specificity of the winning record (scope depth)
    specificity: BTreeMap<String, usize>,
    /// rule key -> preserved conflicting digests (sorted; order-independent)
    conflicts: BTreeMap<String, Vec<String>>,
}

impl MergedStore {
    pub fn apply(&mut self, rec: &WriterRecord) -> MergeOutcome {
        let key = rec.rule_key.formatted();
        match self.winners.get(&key) {
            None => {
                self.winners.insert(key.clone(), (rec.payload_digest.clone(), rec.writer_id.clone()));
                self.tiers.insert(key.clone(), rec.precedence_tier);
                self.specificity
                    .insert(key.clone(), rec.rule_key.scope.matches('/').count());
                MergeOutcome::Applied { superseded: None }
            }
            Some((digest, _)) if *digest == rec.payload_digest => MergeOutcome::Ignored,
            Some((digest, writer)) => {
                // Compare the incoming tier against the winner's tier. The
                // winner's tier is tracked alongside its digest; see
                // `apply_with_tier`. Plain `apply` assumes UserExplicit for
                // stored winners, so only strictly stronger incoming tiers
                // replace; anything else preserves a conflict.
                let stored_tier = self.tiers.get(&key).copied().unwrap_or(PrecedenceTier::CurrentExplicitUserInstruction);
                let _ = digest;
                let stored_specificity = self.specificity.get(&key).copied().unwrap_or(0);
                match compare_precedence(
                    rec.precedence_tier,
                    rec.rule_key.scope.matches('/').count(),
                    stored_tier,
                    stored_specificity,
                ) {
                    std::cmp::Ordering::Less => {
                        let old = self.winners.insert(
                            key.clone(),
                            (rec.payload_digest.clone(), rec.writer_id.clone()),
                        );
                        self.tiers.insert(key.clone(), rec.precedence_tier);
                        self.specificity
                            .insert(key.clone(), rec.rule_key.scope.matches('/').count());
                        MergeOutcome::Applied { superseded: old.map(|(d, _)| d) }
                    }
                    std::cmp::Ordering::Equal if rec.writer_id == *writer => MergeOutcome::Ignored,
                    _ => {
                        let slot = self.conflicts.entry(key).or_default();
                        if !slot.contains(&rec.payload_digest) {
                            slot.push(rec.payload_digest.clone());
                            slot.sort();
                        }
                        MergeOutcome::ConflictPreserved { with_writer: writer.clone() }
                    }
                }
            }
        }
    }

    pub fn winner(&self, rule_key: &RuleKey) -> Option<(&str, &str)> {
        self.winners
            .get(&rule_key.formatted())
            .map(|(d, w)| (d.as_str(), w.as_str()))
    }

    pub fn conflicts_for(&self, rule_key: &RuleKey) -> &[String] {
        self.conflicts
            .get(&rule_key.formatted())
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }
}

/// Order-independent convergence over a record set. Stronger tiers win in any
/// application order; equal-tier divergent payloads converge to one canonical
/// winner plus a sorted conflict set, so every permutation yields an
/// identical store.
pub fn converge(records: &[WriterRecord]) -> MergedStore {
    let mut ordered: Vec<&WriterRecord> = records.iter().collect();
    ordered.sort_by(|a, b| {
        a.rule_key
            .formatted()
            .cmp(&b.rule_key.formatted())
            .then_with(|| b.precedence_tier.cmp(&a.precedence_tier))
            .then_with(|| a.lamport.cmp(&b.lamport))
            .then_with(|| a.writer_id.cmp(&b.writer_id))
    });
    let mut store = MergedStore::default();
    for r in ordered {
        store.apply(r);
    }
    store
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rk(rule: &str) -> RuleKey {
        RuleKey::new("workflow", rule)
    }

    fn rec(writer: &str, lamport: u64, rule: &str, digest: &str, tier: PrecedenceTier) -> WriterRecord {
        WriterRecord {
            writer_id: writer.into(),
            lamport,
            rule_key: rk(rule),
            payload_digest: digest.into(),
            precedence_tier: tier,
        }
    }

    #[test]
    fn stronger_tier_wins_regardless_of_order() {
        let weak = rec("w1", 1, "always run tests", "aaa", PrecedenceTier::ProvisionalCandidate);
        let strong = rec("w2", 9, "always run tests", "bbb", PrecedenceTier::ExplicitGlobalUserPreference);
        let s1 = converge(&[weak.clone(), strong.clone()]);
        let s2 = converge(&[strong.clone(), weak.clone()]);
        assert_eq!(s1.winner(&rk("always run tests")).unwrap().0, "bbb");
        assert_eq!(s2.winner(&rk("always run tests")).unwrap().0, "bbb");
    }

    #[test]
    fn equal_tier_divergent_payload_preserves_conflict() {
        let a = rec("w1", 1, "prefer pnpm", "aaa", PrecedenceTier::ExplicitGlobalUserPreference);
        let b = rec("w2", 2, "prefer pnpm", "bbb", PrecedenceTier::ExplicitGlobalUserPreference);
        let mut s = MergedStore::default();
        s.apply(&a);
        s.apply(&b);
        assert!(!s.conflicts_for(&rk("prefer pnpm")).is_empty());
    }

    #[test]
    fn identical_payload_is_idempotent_across_writers() {
        let a = rec("w1", 1, "prefer pnpm", "same", PrecedenceTier::ExplicitGlobalUserPreference);
        let b = rec("w2", 5, "prefer pnpm", "same", PrecedenceTier::ExplicitGlobalUserPreference);
        let mut s = MergedStore::default();
        assert!(matches!(s.apply(&a), MergeOutcome::Applied { superseded: None }));
        assert!(matches!(s.apply(&b), MergeOutcome::Ignored));
    }

    #[test]
    fn convergence_is_permutation_invariant() {
        let set = vec![
            rec("w1", 1, "r1", "d1", PrecedenceTier::ExplicitGlobalUserPreference),
            rec("w2", 2, "r1", "d2", PrecedenceTier::ExplicitGlobalUserPreference),
            rec("w3", 3, "r1", "d3", PrecedenceTier::ProvisionalCandidate),
            rec("w4", 4, "r2", "d4", PrecedenceTier::CurrentExplicitUserInstruction),
            rec("w5", 5, "r2", "d5", PrecedenceTier::CurrentExplicitUserInstruction),
        ];
        let forward = converge(&set);
        let mut reversed = set.clone();
        reversed.reverse();
        let backward = converge(&reversed);
        // Same winners for every key.
        for key in ["r1", "r2"] {
            assert_eq!(
                forward.winner(&rk(key)).map(|(d, _)| d),
                backward.winner(&rk(key)).map(|(d, _)| d)
            );
            assert_eq!(
                forward.conflicts_for(&rk(key)),
                backward.conflicts_for(&rk(key))
            );
        }
    }
}
