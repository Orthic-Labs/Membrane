//! Immutable InsightIssue payload + receipted mutable lifecycle envelope.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::{FailureEpisodeV1, InsightIssueV1, IssueState};
use crate::gates::CortexAdmissionEnvelope;
use crate::record::InfluenceClass;

pub const SEALED_ISSUE_SCHEMA: &str = "1.0.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceQuality {
    Deterministic,
    HighPrecisionHeuristic,
    Heuristic,
    ModelAssisted,
}

impl EvidenceQuality {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Deterministic => "deterministic",
            Self::HighPrecisionHeuristic => "high_precision_heuristic",
            Self::Heuristic => "heuristic",
            Self::ModelAssisted => "model_assisted",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpisodeRefV1 {
    pub episode_id: String,
    pub episode_payload_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsightIssuePayloadV1 {
    pub record_kind: String,
    pub family: String,
    pub recurrence_signature: String,
    pub canonical_description: String,
    pub applicability: BTreeMap<String, Vec<String>>,
    pub authority_class: String,
    pub influence_class: String,
    pub episode_refs: Vec<EpisodeRefV1>,
    pub evidence_digests: Vec<String>,
    pub confidence: f64,
    pub evidence_quality: EvidenceQuality,
    pub candidate_mechanisms: Vec<String>,
    pub honesty_limit: String,
    pub admission_policy_version: String,
    pub redaction_contract_version: String,
    pub semantic_validator_receipt_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueStateReceiptV1 {
    pub transition: String,
    pub at: String,
    pub actor: String,
    pub prev_status: Option<String>,
    pub new_status: String,
    pub receipt_id: String,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueMitigationLinkV1 {
    pub proposal_id: String,
    pub linked_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsightIssueStateV1 {
    pub lifecycle: IssueState,
    pub recurrence_count: u32,
    pub first_seen: String,
    pub last_seen: String,
    pub updated_at: String,
    pub mitigation_links: Vec<IssueMitigationLinkV1>,
    pub recurrence_after_mitigation: Option<serde_json::Value>,
    pub receipts: Vec<IssueStateReceiptV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SealedInsightIssueV1 {
    pub schema_version: String,
    pub contract: String,
    pub issue_id: String,
    pub payload_sha256: String,
    pub payload: InsightIssuePayloadV1,
    pub state: InsightIssueStateV1,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SealedIssueError {
    MissingTimestamp,
    MissingEpisode(String),
    DuplicateEpisode(String),
    IdentityMismatch,
    PayloadDigestMismatch,
    IllegalTransition,
    MissingReceipt,
    EmptyEvidence,
    InvalidApplicability(String),
    InvalidValidatorReceipt,
    InvalidRecurrence,
}

fn state_name(state: IssueState) -> &'static str {
    match state {
        IssueState::Observed => "observed",
        IssueState::Recurring => "recurring",
        IssueState::Confirmed => "confirmed",
        IssueState::MitigationProposed => "mitigation_proposed",
        IssueState::Mitigated => "mitigated",
        IssueState::Reopened => "reopened",
        IssueState::Obsolete => "obsolete",
        IssueState::Dismissed => "dismissed",
    }
}

fn receipt_id(material: &str) -> String {
    format!("rcpt_{}", &crate::canonical::sha256_hex(material.as_bytes())[..32])
}

impl SealedInsightIssueV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn seal(
        issue: &InsightIssueV1,
        episodes: &[FailureEpisodeV1],
        evidence_quality: EvidenceQuality,
        admission_policy_version: &str,
        redaction_contract_version: &str,
        validator_receipt_id: Option<&str>,
        actor: &str,
        receipt_material: &str,
        updated_at: &str,
    ) -> Result<Self, SealedIssueError> {
        if issue.recurrence_count < 1 || issue.episode_ids.is_empty() {
            return Err(SealedIssueError::InvalidRecurrence);
        }
        if let Some(receipt) = validator_receipt_id {
            let suffix = receipt.strip_prefix("rcpt_").ok_or(SealedIssueError::InvalidValidatorReceipt)?;
            if suffix.len() != 32 || !suffix.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(SealedIssueError::InvalidValidatorReceipt);
            }
        }
        if issue.issue_id
            != crate::canonical::derive_issue_id(&issue.family, &issue.recurrence_signature)
        {
            return Err(SealedIssueError::IdentityMismatch);
        }
        let first_seen = issue.first_seen.clone().ok_or(SealedIssueError::MissingTimestamp)?;
        let last_seen = issue.last_seen.clone().ok_or(SealedIssueError::MissingTimestamp)?;
        let by_id: BTreeMap<&str, &FailureEpisodeV1> =
            episodes.iter().map(|episode| (episode.episode_id.as_str(), episode)).collect();
        let mut seen = BTreeSet::new();
        let mut episode_refs = Vec::new();
        let mut evidence_digests = BTreeSet::new();
        for id in &issue.episode_ids {
            if !seen.insert(id.clone()) {
                return Err(SealedIssueError::DuplicateEpisode(id.clone()));
            }
            let episode = by_id
                .get(id.as_str())
                .ok_or_else(|| SealedIssueError::MissingEpisode(id.clone()))?;
            episode_refs.push(EpisodeRefV1 {
                episode_id: id.clone(),
                episode_payload_sha256: crate::canonical::sha256_canonical(
                    &serde_json::to_value(episode).expect("episode serializes"),
                ),
            });
            for evidence in &episode.evidence {
                evidence_digests.insert(format!(
                    "sha256:{}",
                    crate::canonical::sha256_canonical(
                        &serde_json::to_value(evidence).expect("evidence serializes"),
                    )
                ));
            }
        }
        episode_refs.sort_by(|a, b| a.episode_id.cmp(&b.episode_id));
        let mut applicability: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (key, value) in &issue.applicability {
            let plural = match key.as_str() {
                "host" => "hosts",
                "agent" => "agents",
                "model" => "models",
                "client" => "clients",
                "tool" => "tools",
                "repo" => "repos",
                other => return Err(SealedIssueError::InvalidApplicability(other.into())),
            };
            applicability.entry(plural.into()).or_default().push(value.clone());
        }
        if episode_refs.is_empty() || evidence_digests.is_empty() {
            return Err(SealedIssueError::EmptyEvidence);
        }
        let payload = InsightIssuePayloadV1 {
            record_kind: "insight_issue".into(),
            family: issue.family.clone(),
            recurrence_signature: issue.recurrence_signature.clone(),
            canonical_description: issue.canonical_description.clone(),
            applicability,
            authority_class: "reference".into(),
            influence_class: "diagnostic_reference".into(),
            episode_refs,
            evidence_digests: evidence_digests.into_iter().collect(),
            confidence: issue.confidence,
            evidence_quality,
            candidate_mechanisms: issue.candidate_mechanisms.clone(),
            honesty_limit: issue.honesty_limit.clone(),
            admission_policy_version: admission_policy_version.into(),
            redaction_contract_version: redaction_contract_version.into(),
            semantic_validator_receipt_id: validator_receipt_id.map(str::to_string),
        };
        let payload_sha256 = crate::canonical::sha256_canonical(
            &serde_json::to_value(&payload).expect("issue payload serializes"),
        );
        let initial_receipt = IssueStateReceiptV1 {
            transition: "issue_sealed".into(),
            at: updated_at.into(),
            actor: actor.into(),
            prev_status: None,
            new_status: state_name(issue.state).into(),
            receipt_id: receipt_id(receipt_material),
            note: "immutable semantic payload sealed".into(),
        };
        Ok(Self {
            schema_version: SEALED_ISSUE_SCHEMA.into(),
            contract: "InsightIssueV1".into(),
            issue_id: issue.issue_id.clone(),
            payload_sha256,
            payload,
            state: InsightIssueStateV1 {
                lifecycle: issue.state,
                recurrence_count: issue.recurrence_count,
                first_seen,
                last_seen,
                updated_at: updated_at.into(),
                mitigation_links: issue
                    .mitigation_links
                    .iter()
                    .map(|proposal_id| IssueMitigationLinkV1 {
                        proposal_id: proposal_id.clone(),
                        linked_at: updated_at.into(),
                    })
                    .collect(),
                recurrence_after_mitigation: None,
                receipts: vec![initial_receipt],
            },
        })
    }

    pub fn verify(&self) -> Result<(), SealedIssueError> {
        if self.state.recurrence_count < 1
            || self.payload.episode_refs.is_empty()
            || self.payload.evidence_digests.is_empty()
        {
            return Err(SealedIssueError::InvalidRecurrence);
        }
        let digest = crate::canonical::sha256_canonical(
            &serde_json::to_value(&self.payload).expect("issue payload serializes"),
        );
        if digest != self.payload_sha256 {
            return Err(SealedIssueError::PayloadDigestMismatch);
        }
        if self.issue_id
            != crate::canonical::derive_issue_id(
                &self.payload.family,
                &self.payload.recurrence_signature,
            )
        {
            return Err(SealedIssueError::IdentityMismatch);
        }
        Ok(())
    }

    pub fn transition(
        &mut self,
        target: IssueState,
        actor: &str,
        receipt_material: &str,
        at: &str,
        note: &str,
    ) -> Result<(), SealedIssueError> {
        self.verify()?;
        if receipt_material.trim().is_empty() {
            return Err(SealedIssueError::MissingReceipt);
        }
        let previous = self.state.lifecycle;
        if !previous.can_transition_to(target) {
            return Err(SealedIssueError::IllegalTransition);
        }
        self.state.lifecycle = target;
        self.state.updated_at = at.into();
        self.state.receipts.push(IssueStateReceiptV1 {
            transition: "lifecycle_transition".into(),
            at: at.into(),
            actor: actor.into(),
            prev_status: Some(state_name(previous).into()),
            new_status: state_name(target).into(),
            receipt_id: receipt_id(receipt_material),
            note: note.into(),
        });
        Ok(())
    }

    pub fn cortex_admission_envelope(&self, installation_id: &str) -> CortexAdmissionEnvelope {
        let material = format!("{}\0{}", self.issue_id, self.payload_sha256);
        CortexAdmissionEnvelope {
            envelope_id: format!("cae_{}", crate::canonical::sha256_hex(material.as_bytes())),
            record_kind: "insight_issue".into(),
            seal_digest: self.payload_sha256.clone(),
            influence_class: InfluenceClass::ReferenceOnly,
            idempotency_key: self.issue_id.clone(),
            installation_id: installation_id.into(),
            cortex_verdict: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::insights::{EventKind, TranscriptEventV1};

    fn episode() -> FailureEpisodeV1 {
        let event = TranscriptEventV1 {
            event_id: "e".into(),
            session_id: "s".into(),
            host: "pi".into(),
            provenance: "external_user".into(),
            kind: EventKind::UserMessage,
            text: "wrong again".into(),
            timestamp: Some("2026-08-25T00:00:00Z".into()),
            byte_start: 0,
            byte_end: 11,
            call_id: None,
            occurrence: 0,
            evidence_eligible: true,
        };
        FailureEpisodeV1::new("f", super::super::Severity::High, 0.9, "sig", "failure", "", &[&event])
    }

    fn sealed() -> SealedInsightIssueV1 {
        let episode = episode();
        let issue = InsightIssueV1 {
            schema_version: super::super::INSIGHT_ISSUE_SCHEMA.into(),
            issue_id: crate::canonical::derive_issue_id("f", "sig"),
            family: "f".into(),
            recurrence_signature: "sig".into(),
            canonical_description: "failure".into(),
            applicability: BTreeMap::new(),
            episode_ids: vec![episode.episode_id.clone()],
            recurrence_count: 1,
            distinct_sessions: 1,
            first_seen: Some("2026-08-25T00:00:00Z".into()),
            last_seen: Some("2026-08-25T00:00:00Z".into()),
            confidence: 0.9,
            state: IssueState::Observed,
            candidate_mechanisms: vec![],
            mitigation_links: vec![],
            recurrence_after_mitigation: 0,
            honesty_limit: super::super::HONESTY_LIMIT.into(),
        };
        SealedInsightIssueV1::seal(
            &issue,
            &[episode],
            EvidenceQuality::Deterministic,
            "policy-v1",
            "redaction-v1",
            None,
            "adapt",
            "receipt",
            "2026-08-25T00:00:00Z",
        )
        .unwrap()
    }

    #[test]
    fn payload_mutation_is_detected() {
        let mut issue = sealed();
        assert!(issue.verify().is_ok());
        issue.payload.family = "tampered".into();
        assert_eq!(issue.verify(), Err(SealedIssueError::PayloadDigestMismatch));
    }

    #[test]
    fn lifecycle_transition_is_receipted_and_payload_stays_sealed() {
        let mut issue = sealed();
        let digest = issue.payload_sha256.clone();
        issue
            .transition(IssueState::Recurring, "reviewer", "receipt-2", "t2", "recurs")
            .unwrap();
        assert_eq!(issue.payload_sha256, digest);
        assert_eq!(issue.state.receipts.len(), 2);
    }

    #[test]
    fn cortex_request_is_reference_only() {
        let envelope = sealed().cortex_admission_envelope("inst");
        assert_eq!(envelope.influence_class, InfluenceClass::ReferenceOnly);
        assert!(!envelope.is_durable());
    }
}
