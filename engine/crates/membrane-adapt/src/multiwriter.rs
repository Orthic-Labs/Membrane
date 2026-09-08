//! Multi-writer deterministic merge & convergence (canon §8).
//!
//! Multiple writers (hosts, agents, sessions) may produce records for the
//! same RuleKey. Merge is order-independent: applying the same set of
//! records in any order yields identical store state. Conflicts between
//! equal-precedence divergent payloads are PRESERVED as conflicts, never
//! silently resolved by arrival order.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

use crate::authority::{compare_precedence, PrecedenceTier};
use crate::record::RuleKey;

fn storage_key(rule_key: &RuleKey) -> String {
    // Length-prefix scope so `(a, b/c)` cannot collide with `(a/b, c)`.
    format!(
        "{}:{}{}",
        rule_key.scope.len(),
        rule_key.scope,
        rule_key.record_id
    )
}

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

/// A persisted writer request.  `WriterRecord` is intentionally kept as the
/// small merge primitive; this envelope adds the identities that must survive
/// the Cortex proposal boundary.  Adapt emits this value but never persists it
/// itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriterRequestV1 {
    pub repository_id: String,
    pub scope_id: String,
    pub record_id: String,
    /// Version is opaque to Adapt, but is part of request identity.  A writer
    /// may therefore retry an old version without being mistaken for a new
    /// write.
    pub version: String,
    pub writer_id: String,
    pub lamport: u64,
    pub payload_digest: String,
    pub precedence_tier: PrecedenceTier,
}

/// Descriptive alias used by runtime callers that need to make the durable
/// boundary explicit.
pub type DurableWriterRequestV1 = WriterRequestV1;

impl WriterRequestV1 {
    pub fn new(
        repository_id: impl Into<String>,
        scope_id: impl Into<String>,
        record_id: impl Into<String>,
        version: impl Into<String>,
        writer_id: impl Into<String>,
        lamport: u64,
        payload_digest: impl Into<String>,
        precedence_tier: PrecedenceTier,
    ) -> Self {
        Self {
            repository_id: repository_id.into(),
            scope_id: scope_id.into(),
            record_id: record_id.into(),
            version: version.into(),
            writer_id: writer_id.into(),
            lamport,
            payload_digest: payload_digest.into(),
            precedence_tier,
        }
    }

    pub fn from_writer_record(
        repository_id: impl Into<String>,
        version: impl Into<String>,
        record: &WriterRecord,
    ) -> Self {
        Self::new(
            repository_id,
            record.rule_key.scope.clone(),
            record.rule_key.record_id.clone(),
            version,
            record.writer_id.clone(),
            record.lamport,
            record.payload_digest.clone(),
            record.precedence_tier,
        )
    }

    pub fn writer_record(&self) -> WriterRecord {
        WriterRecord {
            writer_id: self.writer_id.clone(),
            lamport: self.lamport,
            rule_key: RuleKey::new(&self.scope_id, &self.record_id),
            payload_digest: self.payload_digest.clone(),
            precedence_tier: self.precedence_tier,
        }
    }

    fn validate_binding(&self, repository: &str, scope: &str) -> Result<(), String> {
        if self.repository_id != repository {
            return Err("multiwriter_repository_binding_denied".into());
        }
        if self.scope_id != scope {
            return Err("multiwriter_scope_binding_denied".into());
        }
        if self.record_id.trim().is_empty() {
            return Err("multiwriter_record_identity_required".into());
        }
        if self.version.trim().is_empty() {
            return Err("multiwriter_version_identity_required".into());
        }
        if self.writer_id.trim().is_empty() {
            return Err("multiwriter_writer_identity_required".into());
        }
        if self.payload_digest.trim().is_empty() {
            return Err("multiwriter_payload_identity_required".into());
        }
        Ok(())
    }

    fn as_value(&self) -> Value {
        serde_json::to_value(self).expect("WriterRequestV1 is serializable")
    }
}

pub const MULTIWRITER_SCHEMA_VERSION: u64 = 1;
pub const MULTIWRITER_KIND: &str = "adapt_multiwriter";
const MAX_WRITER_REQUESTS: usize = 128;

fn sorted_requests(
    repository: &str,
    scope: &str,
    requests: &[WriterRequestV1],
) -> Result<Vec<WriterRequestV1>, String> {
    if repository.trim().is_empty() {
        return Err("multiwriter_repository_identity_required".into());
    }
    if scope.trim().is_empty() {
        return Err("multiwriter_scope_identity_required".into());
    }
    if requests.is_empty() || requests.len() > MAX_WRITER_REQUESTS {
        return Err("multiwriter_request_count_out_of_bounds".into());
    }
    let mut sorted = requests.to_vec();
    for request in &sorted {
        request.validate_binding(repository, scope)?;
    }
    sorted.sort_by(|left, right| {
        left.record_id
            .cmp(&right.record_id)
            .then_with(|| left.version.cmp(&right.version))
            .then_with(|| left.writer_id.cmp(&right.writer_id))
            .then_with(|| left.lamport.cmp(&right.lamport))
            .then_with(|| left.precedence_tier.cmp(&right.precedence_tier))
            .then_with(|| left.payload_digest.cmp(&right.payload_digest))
    });
    Ok(sorted)
}

/// Build the canonical, proposal-only Cortex emission for competing requests.
/// Sorting here makes exact retries and every request permutation derive one
/// stable proposal identity.
pub fn proposal_emission(
    repository: &str,
    scope: &str,
    requests: &[WriterRequestV1],
) -> Result<Value, String> {
    let requests = sorted_requests(repository, scope, requests)?;
    let records = requests.iter().map(WriterRequestV1::as_value).collect::<Vec<_>>();
    let merge_records = requests.iter().map(WriterRequestV1::writer_record).collect::<Vec<_>>();
    let convergence = convergence_value(&merge_records, &requests);
    let body = json!({
        "schemaVersion": MULTIWRITER_SCHEMA_VERSION,
        "kind": MULTIWRITER_KIND,
        "repositoryId": repository,
        "scopeId": scope,
        "records": records,
        "convergence": convergence,
    });
    Ok(json!({
        "schemaVersion": MULTIWRITER_SCHEMA_VERSION,
        "kind": MULTIWRITER_KIND,
        "text": crate::canonical::to_canonical_json(&body),
        "producer": "adapt_native",
        "epistemicClass": "inferred",
        "repositoryId": repository,
        "scopeId": scope,
        "records": body["records"].clone(),
        "convergence": body["convergence"].clone(),
    }))
}

/// Re-check a stored emission before Cortex admission.  This closes the
/// restart/crash path: a proposal row whose JSON was altered cannot be
/// admitted merely because its stored digest was also altered.
pub fn validate_proposal_emission(
    repository: &str,
    scope: &str,
    emission: &Value,
) -> Result<Vec<WriterRequestV1>, String> {
    let expected = emission
        .get("text")
        .and_then(Value::as_str)
        .ok_or_else(|| "multiwriter_text_required".to_string())?;
    let records = emission
        .get("records")
        .and_then(Value::as_array)
        .ok_or_else(|| "multiwriter_records_required".to_string())?;
    let requests = records
        .iter()
        .cloned()
        .map(serde_json::from_value)
        .collect::<Result<Vec<WriterRequestV1>, _>>()
        .map_err(|_| "multiwriter_record_invalid".to_string())?;
    let canonical = proposal_emission(repository, scope, &requests)?;
    if emission != &canonical || canonical.get("text") != Some(&Value::String(expected.to_owned())) {
        return Err("multiwriter_emission_binding_denied".into());
    }
    Ok(requests)
}

fn convergence_value(records: &[WriterRecord], requests: &[WriterRequestV1]) -> Value {
    let merged = converge(records);
    let mut keys = records.iter().map(|r| r.rule_key.clone()).collect::<Vec<_>>();
    keys.sort();
    keys.dedup();
    let winners = keys
        .iter()
        .filter_map(|key| {
            let (digest, writer) = merged.winner(key)?;
            let request = requests.iter().find(|r| {
                r.record_id == key.record_id && r.scope_id == key.scope
                    && r.payload_digest == digest && r.writer_id == writer
            })?;
            Some(json!({
                "scopeId": key.scope.clone(),
                "recordId": key.record_id.clone(),
                "version": request.version.clone(),
                "writerId": writer,
                "lamport": request.lamport,
                "payloadDigest": digest,
                "precedenceTier": request.precedence_tier,
                "conflicts": merged.conflicts_for(key),
            }))
        })
        .collect::<Vec<_>>();
    json!({
        "schemaVersion": MULTIWRITER_SCHEMA_VERSION,
        "winnerCount": winners.len(),
        "winners": winners,
    })
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
    /// rule key -> logical clock of the winning record
    #[serde(default)]
    lamport: BTreeMap<String, u64>,
    /// rule key -> preserved conflicting digests (sorted; order-independent)
    conflicts: BTreeMap<String, Vec<String>>,
}

impl MergedStore {
    pub fn apply(&mut self, rec: &WriterRecord) -> MergeOutcome {
        let key = storage_key(&rec.rule_key);
        let incoming_specificity = rec.rule_key.scope.matches('/').count();
        match self.winners.get(&key).cloned() {
            None => {
                self.winners.insert(
                    key.clone(),
                    (rec.payload_digest.clone(), rec.writer_id.clone()),
                );
                self.tiers.insert(key.clone(), rec.precedence_tier);
                self.specificity.insert(key.clone(), incoming_specificity);
                self.lamport.insert(key, rec.lamport);
                MergeOutcome::Applied { superseded: None }
            }
            Some((digest, writer)) => {
                let stored_tier = self
                    .tiers
                    .get(&key)
                    .copied()
                    .unwrap_or(PrecedenceTier::CurrentExplicitUserInstruction);
                let stored_specificity = self.specificity.get(&key).copied().unwrap_or(0);
                let stored_lamport = self.lamport.get(&key).copied().unwrap_or(0);
                let precedence = compare_precedence(
                    rec.precedence_tier,
                    incoming_specificity,
                    stored_tier,
                    stored_specificity,
                );
                let ordering = if precedence == std::cmp::Ordering::Equal {
                    rec.lamport
                        .cmp(&stored_lamport)
                        .then_with(|| rec.writer_id.cmp(&writer))
                        .then_with(|| rec.payload_digest.cmp(&digest))
                } else {
                    precedence
                };
                if digest == rec.payload_digest {
                    if ordering == std::cmp::Ordering::Less {
                        self.winners.insert(
                            key.clone(),
                            (rec.payload_digest.clone(), rec.writer_id.clone()),
                        );
                        self.tiers.insert(key.clone(), rec.precedence_tier);
                        self.specificity.insert(key.clone(), incoming_specificity);
                        self.lamport.insert(key.clone(), rec.lamport);
                    }
                    self.remove_conflict(&key, &rec.payload_digest);
                    return MergeOutcome::Ignored;
                }
                match ordering {
                    std::cmp::Ordering::Less => {
                        let old_digest = digest.clone();
                        let old = self.winners.insert(
                            key.clone(),
                            (rec.payload_digest.clone(), rec.writer_id.clone()),
                        );
                        self.tiers.insert(key.clone(), rec.precedence_tier);
                        self.specificity.insert(key.clone(), incoming_specificity);
                        self.lamport.insert(key.clone(), rec.lamport);
                        self.record_conflict(&key, &old_digest);
                        self.remove_conflict(&key, &rec.payload_digest);
                        MergeOutcome::Applied {
                            superseded: old.map(|(d, _)| d),
                        }
                    }
                    _ => {
                        self.record_conflict(&key, &rec.payload_digest);
                        MergeOutcome::ConflictPreserved {
                            with_writer: writer.clone(),
                        }
                    }
                }
            }
        }
    }

    fn record_conflict(&mut self, key: &str, digest: &str) {
        let slot = self.conflicts.entry(key.to_string()).or_default();
        if !slot.iter().any(|existing| existing == digest) {
            slot.push(digest.to_string());
            slot.sort();
        }
    }

    fn remove_conflict(&mut self, key: &str, digest: &str) {
        if let Some(slot) = self.conflicts.get_mut(key) {
            slot.retain(|existing| existing != digest);
            if slot.is_empty() {
                self.conflicts.remove(key);
            }
        }
    }

    pub fn winner(&self, rule_key: &RuleKey) -> Option<(&str, &str)> {
        self.winners
            .get(&storage_key(rule_key))
            .map(|(d, w)| (d.as_str(), w.as_str()))
    }

    pub fn conflicts_for(&self, rule_key: &RuleKey) -> &[String] {
        self.conflicts
            .get(&storage_key(rule_key))
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
            .cmp(&b.rule_key)
            .then_with(|| b.precedence_tier.cmp(&a.precedence_tier))
            .then_with(|| a.lamport.cmp(&b.lamport))
            .then_with(|| a.writer_id.cmp(&b.writer_id))
            .then_with(|| a.payload_digest.cmp(&b.payload_digest))
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

    fn rec(
        writer: &str,
        lamport: u64,
        rule: &str,
        digest: &str,
        tier: PrecedenceTier,
    ) -> WriterRecord {
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
        let weak = rec(
            "w1",
            1,
            "always run tests",
            "aaa",
            PrecedenceTier::ProvisionalCandidate,
        );
        let strong = rec(
            "w2",
            9,
            "always run tests",
            "bbb",
            PrecedenceTier::ExplicitGlobalUserPreference,
        );
        let s1 = converge(&[weak.clone(), strong.clone()]);
        let s2 = converge(&[strong.clone(), weak.clone()]);
        assert_eq!(s1.winner(&rk("always run tests")).unwrap().0, "bbb");
        assert_eq!(s2.winner(&rk("always run tests")).unwrap().0, "bbb");
    }

    #[test]
    fn equal_tier_divergent_payload_preserves_conflict() {
        let a = rec(
            "w1",
            1,
            "prefer pnpm",
            "aaa",
            PrecedenceTier::ExplicitGlobalUserPreference,
        );
        let b = rec(
            "w2",
            2,
            "prefer pnpm",
            "bbb",
            PrecedenceTier::ExplicitGlobalUserPreference,
        );
        let mut s = MergedStore::default();
        s.apply(&a);
        s.apply(&b);
        assert!(!s.conflicts_for(&rk("prefer pnpm")).is_empty());
    }

    #[test]
    fn identical_payload_is_idempotent_across_writers() {
        let a = rec(
            "w1",
            1,
            "prefer pnpm",
            "same",
            PrecedenceTier::ExplicitGlobalUserPreference,
        );
        let b = rec(
            "w2",
            5,
            "prefer pnpm",
            "same",
            PrecedenceTier::ExplicitGlobalUserPreference,
        );
        let mut s = MergedStore::default();
        assert!(matches!(
            s.apply(&a),
            MergeOutcome::Applied { superseded: None }
        ));
        assert!(matches!(s.apply(&b), MergeOutcome::Ignored));
    }

    #[test]
    fn convergence_is_permutation_invariant() {
        let set = vec![
            rec(
                "w1",
                1,
                "r1",
                "d1",
                PrecedenceTier::ExplicitGlobalUserPreference,
            ),
            rec(
                "w2",
                2,
                "r1",
                "d2",
                PrecedenceTier::ExplicitGlobalUserPreference,
            ),
            rec("w3", 3, "r1", "d3", PrecedenceTier::ProvisionalCandidate),
            rec(
                "w4",
                4,
                "r2",
                "d4",
                PrecedenceTier::CurrentExplicitUserInstruction,
            ),
            rec(
                "w5",
                5,
                "r2",
                "d5",
                PrecedenceTier::CurrentExplicitUserInstruction,
            ),
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

    #[test]
    fn direct_application_is_permutation_invariant() {
        let weak = rec(
            "weak",
            9,
            "r",
            "weak-digest",
            PrecedenceTier::ProvisionalCandidate,
        );
        let strong = rec(
            "strong",
            1,
            "r",
            "strong-digest",
            PrecedenceTier::ExplicitGlobalUserPreference,
        );
        let mut forward = MergedStore::default();
        forward.apply(&weak);
        forward.apply(&strong);
        let mut reverse = MergedStore::default();
        reverse.apply(&strong);
        reverse.apply(&weak);
        assert_eq!(forward.winner(&rk("r")), reverse.winner(&rk("r")));
        assert_eq!(
            forward.conflicts_for(&rk("r")),
            reverse.conflicts_for(&rk("r"))
        );
        assert_eq!(
            forward.conflicts_for(&rk("r")),
            &["weak-digest".to_string()]
        );
    }

    #[test]
    fn equal_precedence_uses_stable_clock_and_writer_tie_break() {
        let early = rec(
            "writer-b",
            1,
            "r",
            "early",
            PrecedenceTier::ExplicitGlobalUserPreference,
        );
        let late = rec(
            "writer-a",
            2,
            "r",
            "late",
            PrecedenceTier::ExplicitGlobalUserPreference,
        );
        let mut forward = MergedStore::default();
        forward.apply(&early);
        forward.apply(&late);
        let mut reverse = MergedStore::default();
        reverse.apply(&late);
        reverse.apply(&early);
        assert_eq!(forward.winner(&rk("r")), reverse.winner(&rk("r")));
        assert_eq!(
            forward.conflicts_for(&rk("r")),
            reverse.conflicts_for(&rk("r"))
        );
        assert_eq!(forward.winner(&rk("r")).unwrap().0, "early");
        assert_eq!(forward.conflicts_for(&rk("r")), &["late".to_string()]);
    }

    #[test]
    fn same_payload_metadata_and_conflicts_are_permutation_invariant() {
        let weak = rec(
            "weak",
            9,
            "r",
            "shared",
            PrecedenceTier::ProvisionalCandidate,
        );
        let strong = rec(
            "strong",
            1,
            "r",
            "shared",
            PrecedenceTier::ExplicitGlobalUserPreference,
        );
        let competitor = rec(
            "competitor",
            2,
            "r",
            "competitor",
            PrecedenceTier::ExplicitGlobalUserPreference,
        );
        let records = [weak, strong, competitor];
        let orders = [
            [0usize, 1, 2],
            [0, 2, 1],
            [1, 0, 2],
            [1, 2, 0],
            [2, 0, 1],
            [2, 1, 0],
        ];
        let baseline = {
            let mut store = MergedStore::default();
            for index in orders[0] {
                store.apply(&records[index]);
            }
            store
        };
        for order in orders.iter().skip(1) {
            let mut store = MergedStore::default();
            for index in *order {
                store.apply(&records[index]);
            }
            assert_eq!(store.winner(&rk("r")), baseline.winner(&rk("r")));
            assert_eq!(
                store.conflicts_for(&rk("r")),
                baseline.conflicts_for(&rk("r"))
            );
        }
        assert_eq!(baseline.winner(&rk("r")).unwrap().0, "shared");
        assert_eq!(baseline.winner(&rk("r")).unwrap().1, "strong");
        assert_eq!(
            baseline.conflicts_for(&rk("r")),
            &["competitor".to_string()]
        );
    }

    #[test]
    fn equal_writer_clock_divergence_and_formatted_key_collisions_converge() {
        let lower_digest = rec(
            "writer",
            1,
            "r",
            "aaa",
            PrecedenceTier::ExplicitGlobalUserPreference,
        );
        let higher_digest = rec(
            "writer",
            1,
            "r",
            "bbb",
            PrecedenceTier::ExplicitGlobalUserPreference,
        );
        let mut forward = MergedStore::default();
        forward.apply(&higher_digest);
        forward.apply(&lower_digest);
        let mut reverse = MergedStore::default();
        reverse.apply(&lower_digest);
        reverse.apply(&higher_digest);
        assert_eq!(forward.winner(&rk("r")), reverse.winner(&rk("r")));
        assert_eq!(forward.winner(&rk("r")).unwrap().0, "aaa");
        assert_eq!(forward.conflicts_for(&rk("r")), &["bbb".to_string()]);
        assert_eq!(
            forward.conflicts_for(&rk("r")),
            reverse.conflicts_for(&rk("r"))
        );

        let first_key = RuleKey::new("a", "b/c");
        let second_key = RuleKey::new("a/b", "c");
        assert_eq!(first_key.formatted(), second_key.formatted());
        let mut first = lower_digest.clone();
        first.rule_key = first_key.clone();
        let mut second = higher_digest.clone();
        second.rule_key = second_key.clone();
        let mut store = MergedStore::default();
        store.apply(&first);
        store.apply(&second);
        assert_eq!(store.winner(&first_key).unwrap().0, "aaa");
        assert_eq!(store.winner(&second_key).unwrap().0, "bbb");
    }
}
