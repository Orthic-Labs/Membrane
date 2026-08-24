//! Deterministic recurrence: episode → issue formation (canon §6.3).
//!
//! Issues form ONLY from recurring episodes of the same family + signature.
//! Hybrid model-proposed merges are proposals until deterministically
//! verified; they NEVER auto-admit and never overwrite a deterministic
//! grouping. Issue state transitions are explicit and validated.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::model_boundary::ModelProposalError;
use super::{InsightIssueV1, IssueState, TranscriptEventV1};
use crate::insights::detectors::run_all_detectors;

/// Grouping key: family + signature. Deterministic and order-independent.
pub fn grouping_key(family: &str, signature: &str) -> String {
    format!("{family}\u{1f}{signature}")
}

/// Form issues from episodes. Recurring (>= `min_recurrence` episodes with
/// distinct evidence) groups become issues in `Observed`; singles stay
/// episodes and produce no issue.
pub fn form_issues(
    episodes: &[crate::insights::FailureEpisodeV1],
    min_recurrence: u32,
) -> Vec<InsightIssueV1> {
    let mut groups: BTreeMap<String, Vec<&crate::insights::FailureEpisodeV1>> = BTreeMap::new();
    for ep in episodes {
        groups
            .entry(grouping_key(&ep.family, &ep.signature))
            .or_default()
            .push(ep);
    }
    let mut out = Vec::new();
    for (_key, members) in groups {
        let count = members.len() as u32;
        if count < min_recurrence.max(2) {
            continue;
        }
        let mut ids: Vec<String> = members.iter().map(|m| m.episode_id.clone()).collect();
        ids.sort();
        ids.dedup();
        if ids.len() < 2 {
            continue;
        }
        let first = members[0];
        let sessions: Vec<String> = {
            let mut s: Vec<String> = members
                .iter()
                .flat_map(|m| m.sessions.iter().cloned())
                .collect();
            s.sort();
            s.dedup();
            s
        };
        let timestamps: Vec<String> = members.iter().filter_map(|m| m.timestamp.clone()).collect();
        out.push(InsightIssueV1 {
            schema_version: crate::insights::INSIGHT_ISSUE_SCHEMA.to_string(),
            issue_id: crate::canonical::derive_issue_id(&first.family, &first.signature),
            family: first.family.clone(),
            recurrence_signature: first.signature.clone(),
            canonical_description: first.observed_failure.clone(),
            applicability: applicability_of(&members),
            episode_ids: ids,
            recurrence_count: count,
            distinct_sessions: sessions.len() as u32,
            first_seen: timestamps.iter().min().cloned(),
            last_seen: timestamps.iter().max().cloned(),
            confidence: members.iter().map(|m| m.confidence).fold(0.0_f64, f64::min),
            state: IssueState::Observed,
            candidate_mechanisms: members
                .iter()
                .filter(|m| !m.likely_mechanism.is_empty())
                .map(|m| m.likely_mechanism.clone())
                .collect(),
            mitigation_links: vec![],
            recurrence_after_mitigation: 0,
            honesty_limit: crate::insights::HONESTY_LIMIT.to_string(),
        });
    }
    out
}

fn applicability_of(
    members: &[&crate::insights::FailureEpisodeV1],
) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    let hosts: Vec<String> = members
        .iter()
        .flat_map(|e| e.hosts.iter().cloned())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    if hosts.len() == 1 {
        m.insert("host".to_string(), hosts[0].clone());
    }
    m
}

/// Convenience pipeline over raw events.
pub fn mine_issues(events: &[TranscriptEventV1], min_recurrence: u32) -> Vec<InsightIssueV1> {
    form_issues(&run_all_detectors(events), min_recurrence)
}

/// A hybrid model-proposed merge of episodes into an existing issue (or a new
/// one). This is a PROPOSAL object only; nothing here writes durable truth.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HybridMergeProposal {
    pub proposer_id: String,
    pub target_issue_id: Option<String>,
    /// Episode IDs the model claims belong together.
    pub episode_ids: Vec<String>,
    pub rationale: String,
}

impl HybridMergeProposal {
    /// Deterministic verification: every referenced episode must exist and
    /// share one family+signature key. A verified proposal is still only a
    /// validated plan — Cortex admission stays a separate gate.
    pub fn verify(
        &self,
        episodes: &[crate::insights::FailureEpisodeV1],
        issues: &[InsightIssueV1],
    ) -> Result<Vec<String>, ModelProposalError> {
        let by_id: BTreeMap<&str, &crate::insights::FailureEpisodeV1> =
            episodes.iter().map(|e| (e.episode_id.as_str(), e)).collect();
        if self.episode_ids.is_empty() {
            return Err(ModelProposalError::UnboundEvidence);
        }
        let mut keys = std::collections::BTreeSet::new();
        for id in &self.episode_ids {
            let Some(ep) = by_id.get(id.as_str()) else {
                return Err(ModelProposalError::UnboundEvidence);
            };
            keys.insert(grouping_key(&ep.family, &ep.signature));
        }
        if keys.len() != 1 {
            return Err(ModelProposalError::ScopeBeyondEvidence);
        }
        if let Some(target) = &self.target_issue_id {
            if !issues.iter().any(|i| i.issue_id == *target) {
                return Err(ModelProposalError::UnboundEvidence);
            }
            if issues
                .iter()
                .any(|i| i.issue_id == *target && i.state == IssueState::Dismissed)
            {
                return Err(ModelProposalError::AuthorityEscalationAttempted);
            }
        }
        Ok(self.episode_ids.clone())
    }
}

/// Apply a validated issue-state transition. Returns the updated issue or an
/// error naming the illegal transition. Never bypasses `can_transition_to`.
pub fn transition_issue(issue: &InsightIssueV1, target: IssueState) -> Result<InsightIssueV1, String> {
    if !issue.state.can_transition_to(target) {
        return Err(format!("illegal issue transition {:?} -> {:?}", issue.state, target));
    }
    let mut next = issue.clone();
    next.state = target;
    Ok(next)
}

/// Record recurrence after mitigation; drives reopen logic in `outcomes`.
pub fn record_post_mitigation_recurrence(mut issue: InsightIssueV1) -> Result<InsightIssueV1, String> {
    if issue.state != IssueState::Mitigated {
        return Err("recurrence-after-mitigation requires Mitigated state".into());
    }
    issue.recurrence_after_mitigation += 1;
    transition_issue(&issue, IssueState::Reopened)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::insights::EventKind;

    fn ev(id: &str, session: &str, text: &str) -> TranscriptEventV1 {
        TranscriptEventV1 {
            event_id: id.into(),
            session_id: session.into(),
            host: "pi".into(),
            provenance: "external_user".into(),
            kind: EventKind::UserMessage,
            text: text.into(),
            timestamp: Some("2026-08-24T01:00:00Z".into()),
            byte_start: 0,
            byte_end: text.len() as i64,
            call_id: None,
            occurrence: 0,
            evidence_eligible: true,
        }
    }

    #[test]
    fn repeated_ask_forms_issue() {
        let events = vec![
            ev("a", "s1", "please run the full test suite before claiming done"),
            ev("b", "s2", "Please run the FULL test suite before claiming done."),
        ];
        let issues = mine_issues(&events, 2);
        assert!(issues.iter().any(|i| i.family == "repeated_ask"));
    }

    #[test]
    fn single_episode_does_not_form_issue() {
        let events = vec![ev("a", "s1", "please run the full test suite before claiming done")];
        assert!(mine_issues(&events, 2).is_empty());
    }

    #[test]
    fn hybrid_merge_verifies_but_never_admits() {
        let events = vec![
            ev("a", "s1", "please run the full test suite before claiming done"),
            ev("b", "s2", "Please run the FULL test suite before claiming done."),
        ];
        let eps = run_all_detectors(&events);
        assert!(!eps.is_empty());
        let good = HybridMergeProposal {
            proposer_id: "m".into(),
            target_issue_id: None,
            episode_ids: eps.iter().map(|e| e.episode_id.clone()).collect(),
            rationale: "same theme".into(),
        };
        // Even verified, this remains a plan — no admission occurred here.
        assert_eq!(good.verify(&eps, &[]).unwrap().len(), eps.len());
        let bad = HybridMergeProposal {
            proposer_id: "m".into(),
            target_issue_id: None,
            episode_ids: vec!["nonexistent".into()],
            rationale: "x".into(),
        };
        assert_eq!(
            bad.verify(&eps, &[]),
            Err(ModelProposalError::UnboundEvidence)
        );
    }

    #[test]
    fn illegal_issue_transition_is_refused() {
        let issue = InsightIssueV1 {
            schema_version: "adapt.insight-issue.v1".into(),
            issue_id: "i".into(),
            family: "f".into(),
            recurrence_signature: "sig".into(),
            canonical_description: "d".into(),
            applicability: Default::default(),
            episode_ids: vec!["a".into(), "b".into()],
            recurrence_count: 2,
            distinct_sessions: 2,
            first_seen: None,
            last_seen: None,
            confidence: 0.5,
            state: IssueState::Observed,
            candidate_mechanisms: vec![],
            mitigation_links: vec![],
            recurrence_after_mitigation: 0,
            honesty_limit: "honesty".into(),
        };
        assert!(transition_issue(&issue, IssueState::Mitigated).is_err());
        assert_eq!(
            transition_issue(&issue, IssueState::Recurring).unwrap().state,
            IssueState::Recurring
        );
    }
}
