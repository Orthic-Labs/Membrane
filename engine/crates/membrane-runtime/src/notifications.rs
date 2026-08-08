//! Deterministic, sparse provider notifications (MBR-711).
//!
//! Status is multidimensional evidence: an unavailable or missing dimension is
//! never interpreted as healthy. Delivery failures are coalesced by a stable
//! provider/dimension key and require a threshold inside a bounded window.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum EvidenceStatus {
    Healthy,
    Unavailable,
    Degraded,
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct StatusEvidence {
    pub provider: String,
    pub dimension: String,
    pub status: EvidenceStatus,
    pub observed_at: i64,
    pub source: String,
    pub evidence: String,
    pub resolver: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum NotificationKind {
    Alert,
    Resolution,
}

/// Content-free receipt. `dedupe_key` is stable across restarts.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NotificationReceipt {
    pub kind: NotificationKind,
    pub dedupe_key: String,
    pub observed_at: i64,
    pub source: String,
    pub evidence: String,
    pub resolver: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Entry {
    failures: Vec<i64>,
    alerted: bool,
    last_observed_at: i64,
    last_status: EvidenceStatus,
    source: String,
    evidence: String,
    resolver: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NotificationState {
    pub threshold: usize,
    pub window_seconds: i64,
    pub capacity: usize,
    entries: BTreeMap<String, Entry>,
}

impl NotificationState {
    pub fn new(threshold: usize, window_seconds: i64, capacity: usize) -> Self {
        Self {
            threshold: threshold.max(1),
            window_seconds: window_seconds.max(1),
            capacity: capacity.max(1),
            entries: BTreeMap::new(),
        }
    }

    pub fn observe(&mut self, evidence: StatusEvidence) -> Vec<NotificationReceipt> {
        if evidence.provider.trim().is_empty()
            || evidence.dimension.trim().is_empty()
            || evidence.source.trim().is_empty()
            || evidence.evidence.trim().is_empty()
            || evidence.resolver.trim().is_empty()
        {
            return Vec::new();
        }
        // Length-prefixing keeps pairs unambiguous when fields contain colons.
        let key = format!(
            "{}:{}:{}",
            evidence.provider.len(),
            evidence.provider,
            evidence.dimension
        );
        if !self.entries.contains_key(&key) {
            while self.entries.len() >= self.capacity {
                let victim = self
                    .entries
                    .iter()
                    .min_by_key(|(_, e)| e.last_observed_at)
                    .map(|(k, _)| k.clone());
                if let Some(k) = victim {
                    self.entries.remove(&k);
                } else {
                    break;
                }
            }
            self.entries.insert(
                key.clone(),
                Entry {
                    failures: Vec::new(),
                    alerted: false,
                    last_observed_at: evidence.observed_at,
                    last_status: evidence.status.clone(),
                    source: evidence.source.clone(),
                    evidence: evidence.evidence.clone(),
                    resolver: evidence.resolver.clone(),
                },
            );
        }
        let entry = self.entries.get_mut(&key).expect("inserted");
        // Do not let stale samples roll state back or resolve a newer failure.
        if evidence.observed_at < entry.last_observed_at {
            return Vec::new();
        }
        entry.last_observed_at = evidence.observed_at;
        entry.last_status = evidence.status.clone();
        entry.source = evidence.source.clone();
        entry.evidence = evidence.evidence.clone();
        entry.resolver = evidence.resolver.clone();
        let cutoff = evidence.observed_at.saturating_sub(self.window_seconds);
        entry.failures.retain(|t| *t >= cutoff);
        let mut out = Vec::new();
        match evidence.status {
            EvidenceStatus::Unavailable | EvidenceStatus::Degraded | EvidenceStatus::Unknown => {
                entry.failures.push(evidence.observed_at);
                if entry.failures.len() >= self.threshold && !entry.alerted {
                    entry.alerted = true;
                    out.push(NotificationReceipt {
                        kind: NotificationKind::Alert,
                        dedupe_key: key,
                        observed_at: evidence.observed_at,
                        source: evidence.source,
                        evidence: evidence.evidence,
                        resolver: evidence.resolver,
                    });
                }
            }
            EvidenceStatus::Healthy if entry.alerted => {
                // Healthy is explicit recovery only when source/evidence/resolver are present.
                if !evidence.source.is_empty()
                    && !evidence.evidence.is_empty()
                    && !evidence.resolver.is_empty()
                {
                    entry.alerted = false;
                    entry.failures.clear();
                    out.push(NotificationReceipt {
                        kind: NotificationKind::Resolution,
                        dedupe_key: key,
                        observed_at: evidence.observed_at,
                        source: evidence.source,
                        evidence: evidence.evidence,
                        resolver: evidence.resolver,
                    });
                }
            }
            EvidenceStatus::Healthy => entry.failures.clear(),
        }
        out
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ev(status: EvidenceStatus, at: i64) -> StatusEvidence {
        StatusEvidence {
            provider: "p".into(),
            dimension: "delivery".into(),
            status,
            observed_at: at,
            source: "probe".into(),
            evidence: "explicit".into(),
            resolver: "operator".into(),
        }
    }
    #[test]
    fn flaps_do_not_spam() {
        let mut s = NotificationState::new(2, 60, 8);
        assert!(s.observe(ev(EvidenceStatus::Unavailable, 1)).is_empty());
        assert!(matches!(
            s.observe(ev(EvidenceStatus::Unavailable, 2))[0].kind,
            NotificationKind::Alert
        ));
        assert!(s.observe(ev(EvidenceStatus::Healthy, 3)).len() == 1);
        assert!(s.observe(ev(EvidenceStatus::Unavailable, 4)).is_empty());
        assert!(s.observe(ev(EvidenceStatus::Unavailable, 5)).len() == 1);
    }
    #[test]
    fn unknown_never_healthy() {
        let mut s = NotificationState::new(2, 60, 8);
        assert!(s.observe(ev(EvidenceStatus::Unknown, 1)).is_empty());
        assert!(s.observe(ev(EvidenceStatus::Unknown, 2)).len() == 1);
    }
    #[test]
    fn serde_round_trip() {
        let mut s = NotificationState::new(2, 60, 8);
        s.observe(ev(EvidenceStatus::Unavailable, 1));
        let t: NotificationState =
            serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn malformed_and_stale_evidence_are_ignored() {
        let mut s = NotificationState::new(2, 60, 8);
        let mut bad = ev(EvidenceStatus::Unavailable, 1);
        bad.source.clear();
        assert!(s.observe(bad).is_empty());
        assert!(s.observe(ev(EvidenceStatus::Unavailable, 10)).is_empty());
        assert!(s.observe(ev(EvidenceStatus::Healthy, 9)).is_empty());
        assert!(s.observe(ev(EvidenceStatus::Unavailable, 11)).len() == 1);
    }

    #[test]
    fn dedupe_key_separates_colon_fields() {
        let mut s = NotificationState::new(1, 60, 8);
        let mut first = ev(EvidenceStatus::Unavailable, 1);
        first.provider = "a:b".into();
        first.dimension = "c".into();
        let mut second = ev(EvidenceStatus::Unavailable, 1);
        second.provider = "a".into();
        second.dimension = "b:c".into();
        let a = s.observe(first);
        let b = s.observe(second);
        assert_ne!(a[0].dedupe_key, b[0].dedupe_key);
        assert_eq!(s.len(), 2);
    }
}
