//! ADP-020 proposal-only emergent failure patterns.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::insights::FailureEpisodeV1;

pub const CANDIDATE_PATTERN_SCHEMA_V1: &str = "adapt.candidate-pattern.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpisodeMembershipV1 {
    pub episode_id: String,
    pub episode_payload_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatternCaseRefV1 {
    pub case_id: String,
    pub case_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatternDatasetV1 {
    pub dataset_sha256: String,
    pub positive_cases: Vec<PatternCaseRefV1>,
    pub hard_negative_cases: Vec<PatternCaseRefV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QualificationPackageV1 {
    pub development: PatternDatasetV1,
    pub frozen_test: PatternDatasetV1,
    pub detector_version: String,
    pub evaluator_version: String,
    pub qualification_receipt_id: String,
    pub qualification_result_sha256: String,
    pub qualification_passed: bool,
    pub rollback_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundaryReviewReceiptV1 {
    pub method: String,
    pub reviewer_id: String,
    pub authority_receipt_id: String,
    pub authority_receipt_sha256: String,
    pub reviewed_pattern_sha256: String,
    pub verdict: BoundaryVerdict,
    pub receipt_sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryVerdict {
    Accepted,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternState {
    Proposed,
    BoundaryAccepted,
    Rejected,
    Qualified,
    Deactivated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatternStateReceiptV1 {
    pub from: PatternState,
    pub to: PatternState,
    pub actor: String,
    pub binding_sha256: String,
    pub receipt_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidatePatternPayloadV1 {
    pub proposer_id: String,
    pub source_episodes: Vec<EpisodeMembershipV1>,
    pub operational_family: String,
    pub operational_signature: String,
    pub operational_description: String,
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidatePatternV1 {
    pub schema_version: String,
    pub pattern_id: String,
    pub payload_sha256: String,
    pub payload: CandidatePatternPayloadV1,
    pub state: PatternState,
    pub boundary_review: Option<BoundaryReviewReceiptV1>,
    pub qualification: Option<QualificationPackageV1>,
    pub qualification_sha256: Option<String>,
    pub receipts: Vec<PatternStateReceiptV1>,
    pub activation_authorized: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CandidatePatternError {
    EmptyField(&'static str),
    InvalidDigest(&'static str),
    DuplicateEpisode(String),
    UnknownEpisode(String),
    EpisodeDigestChanged(String),
    EpisodeBoundaryMismatch(String),
    DuplicateCase(String),
    DatasetDigestMismatch(&'static str),
    DatasetLeakage,
    MissingHardNegatives(&'static str),
    MissingPositives(&'static str),
    MissingQualification,
    InvalidReview,
    ReviewRequired,
    IllegalTransition,
    PayloadDigestMismatch,
    ReceiptDigestMismatch,
    StateCoherence,
    QualificationConflict,
    ReviewConflict,
}

impl std::fmt::Display for CandidatePatternError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "candidate pattern validation failed: {self:?}")
    }
}

impl std::error::Error for CandidatePatternError {}

fn digest<T: Serialize>(value: &T) -> String {
    format!(
        "sha256:{}",
        crate::canonical::sha256_canonical(&serde_json::to_value(value).expect("serializable"))
    )
}

fn valid_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

fn hash_cases(dataset: &PatternDatasetV1) -> String {
    let mut cases = dataset
        .positive_cases
        .iter()
        .map(|case_ref| (case_ref.case_id.clone(), case_ref.case_sha256.clone(), "positive"))
        .chain(dataset.hard_negative_cases.iter().map(|case_ref| {
            (
                case_ref.case_id.clone(),
                case_ref.case_sha256.clone(),
                "hard_negative",
            )
        }))
        .collect::<Vec<_>>();
    cases.sort();
    digest(&cases)
}

fn case_set(dataset: &PatternDatasetV1) -> BTreeSet<(&str, &str)> {
    dataset
        .positive_cases
        .iter()
        .chain(&dataset.hard_negative_cases)
        .map(|case_ref| (case_ref.case_id.as_str(), case_ref.case_sha256.as_str()))
        .collect()
}

fn validate_dataset(
    dataset: &PatternDatasetV1,
    label: &'static str,
) -> Result<(), CandidatePatternError> {
    if !valid_digest(&dataset.dataset_sha256) {
        return Err(CandidatePatternError::InvalidDigest("dataset_sha256"));
    }
    if dataset.dataset_sha256 != hash_cases(dataset) {
        return Err(CandidatePatternError::DatasetDigestMismatch(label));
    }
    let mut ids = BTreeSet::new();
    let mut hashes = BTreeSet::new();
    for case_ref in dataset
        .positive_cases
        .iter()
        .chain(&dataset.hard_negative_cases)
    {
        if case_ref.case_id.trim().is_empty() || case_ref.case_id.len() > 256 {
            return Err(CandidatePatternError::EmptyField("case_id"));
        }
        if !valid_digest(&case_ref.case_sha256) {
            return Err(CandidatePatternError::InvalidDigest("case_sha256"));
        }
        if !ids.insert(case_ref.case_id.clone()) || !hashes.insert(case_ref.case_sha256.clone()) {
            return Err(CandidatePatternError::DuplicateCase(case_ref.case_id.clone()));
        }
    }
    if dataset.positive_cases.is_empty() {
        return Err(CandidatePatternError::MissingPositives(label));
    }
    if dataset.hard_negative_cases.is_empty() {
        return Err(CandidatePatternError::MissingHardNegatives(label));
    }
    Ok(())
}

fn validate_core(payload: &CandidatePatternPayloadV1) -> Result<(), CandidatePatternError> {
    for (name, value) in [
        ("proposer_id", &payload.proposer_id),
        ("operational_family", &payload.operational_family),
        ("operational_signature", &payload.operational_signature),
        ("operational_description", &payload.operational_description),
    ] {
        if value.trim().is_empty() {
            return Err(CandidatePatternError::EmptyField(name));
        }
    }
    if payload.source_episodes.len() < 2 {
        return Err(CandidatePatternError::EmptyField("source evidence"));
    }
    let mut ids = BTreeSet::new();
    let mut evidence_refs = BTreeSet::new();
    for member in &payload.source_episodes {
        if member.episode_id.trim().is_empty() || !valid_digest(&member.episode_payload_sha256) {
            return Err(CandidatePatternError::InvalidDigest("episode membership"));
        }
        if !ids.insert(member.episode_id.clone()) {
            return Err(CandidatePatternError::DuplicateEpisode(member.episode_id.clone()));
        }
        evidence_refs.insert(member.episode_id.clone());
    }
    let supplied_refs: BTreeSet<_> = payload
        .evidence_refs
        .iter()
        .map(|reference| reference.trim().to_owned())
        .collect();
    if supplied_refs.len() != payload.evidence_refs.len()
        || supplied_refs.iter().any(String::is_empty)
        || supplied_refs != evidence_refs
    {
        return Err(CandidatePatternError::InvalidReview);
    }
    Ok(())
}

fn validate_qualification(q: &QualificationPackageV1) -> Result<(), CandidatePatternError> {
    validate_dataset(&q.development, "development")?;
    validate_dataset(&q.frozen_test, "frozen_test")?;
    if q.development.dataset_sha256 == q.frozen_test.dataset_sha256
        || case_set(&q.development).iter().any(|(id, hash)| {
            case_set(&q.frozen_test)
                .iter()
                .any(|(other_id, other_hash)| id == other_id || hash == other_hash)
        })
    {
        return Err(CandidatePatternError::DatasetLeakage);
    }
    if !q.qualification_passed {
        return Err(CandidatePatternError::MissingQualification);
    }
    for value in [
        q.detector_version.as_str(),
        q.evaluator_version.as_str(),
        q.qualification_receipt_id.as_str(),
        q.rollback_ref.as_str(),
    ] {
        if value.trim().is_empty() || value.len() > 512 {
            return Err(CandidatePatternError::MissingQualification);
        }
    }
    if !valid_digest(&q.qualification_result_sha256) {
        return Err(CandidatePatternError::InvalidDigest("qualification_result_sha256"));
    }
    Ok(())
}

fn qualification_digest(q: &QualificationPackageV1) -> String {
    digest(&(
        q.development.dataset_sha256.clone(),
        q.frozen_test.dataset_sha256.clone(),
        q.detector_version.clone(),
        q.evaluator_version.clone(),
        q.qualification_receipt_id.clone(),
        q.qualification_result_sha256.clone(),
        q.qualification_passed,
        q.rollback_ref.clone(),
    ))
}

fn review_digest(receipt: &BoundaryReviewReceiptV1) -> String {
    digest(&(
        receipt.method.clone(),
        receipt.reviewer_id.clone(),
        receipt.authority_receipt_id.clone(),
        receipt.authority_receipt_sha256.clone(),
        receipt.reviewed_pattern_sha256.clone(),
        receipt.verdict,
    ))
}

fn state_digest(receipt: &PatternStateReceiptV1) -> String {
    digest(&(
        receipt.from,
        receipt.to,
        receipt.actor.clone(),
        receipt.binding_sha256.clone(),
    ))
}

impl CandidatePatternV1 {
    pub fn propose(payload: CandidatePatternPayloadV1) -> Result<Self, CandidatePatternError> {
        validate_core(&payload)?;
        let payload_sha256 = digest(&payload);
        Ok(Self {
            schema_version: CANDIDATE_PATTERN_SCHEMA_V1.into(),
            pattern_id: format!("cp_{}", payload_sha256.trim_start_matches("sha256:")),
            payload_sha256,
            payload,
            state: PatternState::Proposed,
            boundary_review: None,
            qualification: None,
            qualification_sha256: None,
            receipts: Vec::new(),
            activation_authorized: false,
        })
    }

    pub fn verify_against_episodes(
        &self,
        episodes: &[FailureEpisodeV1],
    ) -> Result<(), CandidatePatternError> {
        self.verify()?;
        let mut current = BTreeMap::new();
        for episode in episodes {
            if current
                .insert(episode.episode_id.as_str(), episode)
                .is_some()
            {
                return Err(CandidatePatternError::DuplicateEpisode(episode.episode_id.clone()));
            }
        }
        for member in &self.payload.source_episodes {
            let Some(episode) = current.get(member.episode_id.as_str()) else {
                return Err(CandidatePatternError::UnknownEpisode(member.episode_id.clone()));
            };
            if digest(*episode) != member.episode_payload_sha256 {
                return Err(CandidatePatternError::EpisodeDigestChanged(member.episode_id.clone()));
            }
            if episode.family != self.payload.operational_family
                || episode.signature != self.payload.operational_signature
            {
                return Err(CandidatePatternError::EpisodeBoundaryMismatch(
                    member.episode_id.clone(),
                ));
            }
        }
        Ok(())
    }

    pub fn boundary_review(
        &mut self,
        episodes: &[FailureEpisodeV1],
        receipt: BoundaryReviewReceiptV1,
    ) -> Result<(), CandidatePatternError> {
        if let Some(old) = &self.boundary_review {
            if old == &receipt {
                self.verify()?;
                return Ok(());
            }
            return Err(CandidatePatternError::ReviewConflict);
        }
        if self.state != PatternState::Proposed {
            return Err(CandidatePatternError::InvalidReview);
        }
        self.verify_against_episodes(episodes)?;
        let current = episodes
            .iter()
            .map(|episode| (episode.episode_id.as_str(), episode))
            .collect::<BTreeMap<_, _>>();
        let mut sessions = BTreeSet::new();
        for member in &self.payload.source_episodes {
            let episode = current
                .get(member.episode_id.as_str())
                .expect("exact membership was verified above");
            for session in &episode.sessions {
                let session = session.trim();
                if !session.is_empty() {
                    sessions.insert(session.to_owned());
                }
            }
        }
        if sessions.len() < 2 {
            return Err(CandidatePatternError::InvalidReview);
        }
        if (receipt.method != "human" && receipt.method != "deterministic")
            || receipt.reviewer_id.trim().is_empty()
            || receipt.authority_receipt_id.trim().is_empty()
            || !valid_digest(&receipt.authority_receipt_sha256)
            || receipt.reviewed_pattern_sha256 != self.payload_sha256
        {
            return Err(CandidatePatternError::InvalidReview);
        }
        let expected = review_digest(&receipt);
        if receipt.receipt_sha256 != expected {
            return Err(CandidatePatternError::ReceiptDigestMismatch);
        }
        let next = match receipt.verdict {
            BoundaryVerdict::Accepted => PatternState::BoundaryAccepted,
            BoundaryVerdict::Rejected => PatternState::Rejected,
        };
        self.transition(next, receipt.reviewer_id.clone(), receipt.receipt_sha256.clone())?;
        self.boundary_review = Some(receipt);
        Ok(())
    }

    pub fn qualify(
        &mut self,
        package: QualificationPackageV1,
        actor: &str,
    ) -> Result<(), CandidatePatternError> {
        self.verify()?;
        validate_qualification(&package)?;
        let qhash = qualification_digest(&package);
        if let Some(old) = &self.qualification {
            if old == &package && self.qualification_sha256.as_deref() == Some(qhash.as_str()) {
                self.verify()?;
                return Ok(());
            }
            return Err(CandidatePatternError::QualificationConflict);
        }
        if self.state != PatternState::BoundaryAccepted {
            return Err(CandidatePatternError::ReviewRequired);
        }
        self.transition(PatternState::Qualified, actor.into(), qhash.clone())?;
        self.qualification = Some(package);
        self.qualification_sha256 = Some(qhash);
        Ok(())
    }

    pub fn deactivate(
        &mut self,
        actor: &str,
        receipt_id: &str,
    ) -> Result<(), CandidatePatternError> {
        self.verify()?;
        let binding = digest(&(self.pattern_id.clone(), actor, receipt_id));
        if self.state == PatternState::Deactivated
            && self.receipts.last().is_some_and(|receipt| {
                receipt.to == PatternState::Deactivated && receipt.binding_sha256 == binding
            })
        {
            return Ok(());
        }
        self.transition(PatternState::Deactivated, actor.into(), binding)?;
        self.verify()
    }

    fn transition(
        &mut self,
        next: PatternState,
        actor: String,
        binding_sha256: String,
    ) -> Result<(), CandidatePatternError> {
        if actor.trim().is_empty() || !valid_digest(&binding_sha256) {
            return Err(CandidatePatternError::IllegalTransition);
        }
        let allowed = matches!(
            (self.state, next),
            (
                PatternState::Proposed,
                PatternState::BoundaryAccepted | PatternState::Rejected
            ) | (PatternState::BoundaryAccepted, PatternState::Qualified)
                | (PatternState::Qualified, PatternState::Deactivated)
        );
        if !allowed {
            return Err(CandidatePatternError::IllegalTransition);
        }
        let mut receipt = PatternStateReceiptV1 {
            from: self.state,
            to: next,
            actor,
            binding_sha256,
            receipt_sha256: String::new(),
        };
        receipt.receipt_sha256 = state_digest(&receipt);
        self.receipts.push(receipt);
        self.state = next;
        self.activation_authorized = false;
        Ok(())
    }

    pub fn verify(&self) -> Result<(), CandidatePatternError> {
        if self.schema_version != CANDIDATE_PATTERN_SCHEMA_V1
            || self.pattern_id
                != format!("cp_{}", self.payload_sha256.trim_start_matches("sha256:"))
            || !valid_digest(&self.payload_sha256)
        {
            return Err(CandidatePatternError::PayloadDigestMismatch);
        }
        validate_core(&self.payload)?;
        if digest(&self.payload) != self.payload_sha256 || self.activation_authorized {
            return Err(CandidatePatternError::StateCoherence);
        }
        if !matches!(
            (&self.qualification, &self.qualification_sha256),
            (None, None) | (Some(_), Some(_))
        ) {
            return Err(CandidatePatternError::StateCoherence);
        }
        match (&self.state, &self.boundary_review, &self.qualification, &self.receipts[..]) {
            (PatternState::Proposed, None, None, []) => {}
            (PatternState::BoundaryAccepted, Some(review), None, [transition])
                if review.verdict == BoundaryVerdict::Accepted
                    && transition.from == PatternState::Proposed
                    && transition.to == PatternState::BoundaryAccepted
                    && transition.binding_sha256 == review.receipt_sha256 => {}
            (PatternState::Rejected, Some(review), None, [transition])
                if review.verdict == BoundaryVerdict::Rejected
                    && transition.from == PatternState::Proposed
                    && transition.to == PatternState::Rejected
                    && transition.binding_sha256 == review.receipt_sha256 => {}
            (
                PatternState::Qualified,
                Some(review),
                Some(package),
                [review_transition, qualification_transition],
            ) if review.verdict == BoundaryVerdict::Accepted
                && review_transition.from == PatternState::Proposed
                && review_transition.to == PatternState::BoundaryAccepted
                && review_transition.binding_sha256 == review.receipt_sha256
                && qualification_transition.from == PatternState::BoundaryAccepted
                && qualification_transition.to == PatternState::Qualified
                && qualification_transition.binding_sha256 == qualification_digest(package)
                && self.qualification_sha256.as_deref() == Some(qualification_digest(package).as_str()) => {}
            (
                PatternState::Deactivated,
                Some(review),
                Some(package),
                [review_transition, qualification_transition, deactivation_transition],
            ) if review.verdict == BoundaryVerdict::Accepted
                && review_transition.from == PatternState::Proposed
                && review_transition.to == PatternState::BoundaryAccepted
                && review_transition.binding_sha256 == review.receipt_sha256
                && qualification_transition.from == PatternState::BoundaryAccepted
                && qualification_transition.to == PatternState::Qualified
                && qualification_transition.binding_sha256 == qualification_digest(package)
                && deactivation_transition.from == PatternState::Qualified
                && deactivation_transition.to == PatternState::Deactivated
                && self.qualification_sha256.as_deref() == Some(qualification_digest(package).as_str()) => {}
            _ => return Err(CandidatePatternError::StateCoherence),
        }
        if let Some(review) = &self.boundary_review {
            if !matches!(review.method.as_str(), "human" | "deterministic")
                || review.reviewer_id.trim().is_empty()
                || review.authority_receipt_id.trim().is_empty()
                || !valid_digest(&review.authority_receipt_sha256)
                || review.reviewed_pattern_sha256 != self.payload_sha256
                || review.receipt_sha256 != review_digest(review)
            {
                return Err(CandidatePatternError::ReceiptDigestMismatch);
            }
        }
        if let Some(package) = &self.qualification {
            validate_qualification(package)?;
        }
        let mut state = PatternState::Proposed;
        for receipt in &self.receipts {
            if receipt.from != state
                || !valid_digest(&receipt.binding_sha256)
                || receipt.receipt_sha256 != state_digest(receipt)
            {
                return Err(CandidatePatternError::ReceiptDigestMismatch);
            }
            let legal = matches!(
                (receipt.from, receipt.to),
                (
                    PatternState::Proposed,
                    PatternState::BoundaryAccepted | PatternState::Rejected
                ) | (PatternState::BoundaryAccepted, PatternState::Qualified)
                    | (PatternState::Qualified, PatternState::Deactivated)
            );
            if !legal {
                return Err(CandidatePatternError::IllegalTransition);
            }
            state = receipt.to;
        }
        if state != self.state {
            return Err(CandidatePatternError::StateCoherence);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(ch: char) -> String {
        format!("sha256:{}", ch.to_string().repeat(64))
    }

    fn episode(id: &str, session: &str) -> FailureEpisodeV1 {
        FailureEpisodeV1 {
            schema_version: "adapt.failure-episode.v1".into(),
            episode_id: id.into(),
            family: "family".into(),
            severity: crate::insights::Severity::Medium,
            confidence: 0.8,
            timestamp: None,
            sessions: vec![session.into()],
            hosts: vec![],
            agents: vec![],
            applicability: BTreeMap::new(),
            signature: "signature".into(),
            user_expectation: "expectation".into(),
            observed_failure: "failure".into(),
            evidence: vec![],
            likely_mechanism: String::new(),
            suggested_remediations: vec![],
            user_disposition: crate::insights::UserDisposition::Logged,
            honesty_limit: "observable only".into(),
        }
    }

    fn payload_for(episodes: &[FailureEpisodeV1]) -> CandidatePatternPayloadV1 {
        CandidatePatternPayloadV1 {
            source_episodes: episodes
                .iter()
                .map(|episode| EpisodeMembershipV1 {
                    episode_id: episode.episode_id.clone(),
                    episode_payload_sha256: digest(episode),
                })
                .collect(),
            proposer_id: "model".into(),
            operational_family: "family".into(),
            operational_signature: "signature".into(),
            operational_description: "observable pattern".into(),
            evidence_refs: vec!["ep-1".into(), "ep-2".into()],
        }
    }

    fn core() -> (CandidatePatternPayloadV1, Vec<FailureEpisodeV1>) {
        let episodes = vec![episode("ep-1", "session-1"), episode("ep-2", "session-2")];
        (payload_for(&episodes), episodes)
    }

    fn review(pattern: &CandidatePatternV1, verdict: BoundaryVerdict) -> BoundaryReviewReceiptV1 {
        let authority_receipt_id = "authority-receipt".to_string();
        let mut receipt = BoundaryReviewReceiptV1 {
            method: "human".into(),
            reviewer_id: "reviewer".into(),
            // Domain projection only: host resolves this external receipt.
            authority_receipt_sha256: h('7'),
            authority_receipt_id,
            reviewed_pattern_sha256: pattern.payload_sha256.clone(),
            verdict,
            receipt_sha256: String::new(),
        };
        receipt.receipt_sha256 = review_digest(&receipt);
        receipt
    }

    fn dataset(seed: char, pos: &str, neg: &str) -> PatternDatasetV1 {
        let mut dataset = PatternDatasetV1 {
            dataset_sha256: String::new(),
            positive_cases: vec![PatternCaseRefV1 {
                case_id: pos.into(),
                case_sha256: h(seed),
            }],
            hard_negative_cases: vec![PatternCaseRefV1 {
                case_id: neg.into(),
                case_sha256: h((seed as u8 + 1) as char),
            }],
        };
        dataset.dataset_sha256 = hash_cases(&dataset);
        dataset
    }

    fn qualification() -> QualificationPackageV1 {
        QualificationPackageV1 {
            development: dataset('b', "dev-pos", "dev-neg"),
            frozen_test: dataset('e', "test-pos", "test-neg"),
            detector_version: "det-v1".into(),
            evaluator_version: "eval-v1".into(),
            qualification_receipt_id: "q-receipt".into(),
            qualification_result_sha256: h('9'),
            qualification_passed: true,
            rollback_ref: "rollback://pattern".into(),
        }
    }

    fn accepted_pattern() -> CandidatePatternV1 {
        let (payload, episodes) = core();
        let mut pattern = CandidatePatternV1::propose(payload).unwrap();
        pattern
            .boundary_review(&episodes, review(&pattern, BoundaryVerdict::Accepted))
            .unwrap();
        pattern
    }

    #[test]
    fn proposed_object_is_unqualified_and_inactive() {
        let (payload, _) = core();
        let pattern = CandidatePatternV1::propose(payload).unwrap();
        assert_eq!(pattern.state, PatternState::Proposed);
        assert!(!pattern.activation_authorized);
        assert!(pattern.qualification.is_none());
        assert!(pattern.verify().is_ok());
    }

    #[test]
    fn exact_review_replay_is_idempotent() {
        let (payload, episodes) = core();
        let mut pattern = CandidatePatternV1::propose(payload).unwrap();
        let receipt = review(&pattern, BoundaryVerdict::Accepted);
        pattern.boundary_review(&episodes, receipt.clone()).unwrap();
        pattern.boundary_review(&episodes, receipt).unwrap();
        assert_eq!(pattern.receipts.len(), 1);
    }

    #[test]
    fn changed_review_conflicts() {
        let mut pattern = accepted_pattern();
        let (_, episodes) = core();
        let mut changed = review(&pattern, BoundaryVerdict::Accepted);
        changed.reviewer_id = "other".into();
        assert_eq!(
            pattern.boundary_review(&episodes, changed),
            Err(CandidatePatternError::ReviewConflict)
        );
    }

    #[test]
    fn membership_and_session_boundaries_block_review() {
        let (payload, episodes) = core();
        let mut pattern = CandidatePatternV1::propose(payload).unwrap();
        let mut unknown = episodes.clone();
        unknown[0].episode_id = "unknown".into();
        assert!(pattern
            .boundary_review(&unknown, review(&pattern, BoundaryVerdict::Accepted))
            .is_err());

        let mut changed = episodes.clone();
        changed[0].observed_failure = "changed".into();
        assert!(pattern
            .boundary_review(&changed, review(&pattern, BoundaryVerdict::Accepted))
            .is_err());

        let mut same_session = episodes.clone();
        same_session[1].sessions = vec!["session-1".into()];
        pattern = CandidatePatternV1::propose(payload_for(&same_session)).unwrap();
        assert!(pattern
            .boundary_review(&same_session, review(&pattern, BoundaryVerdict::Accepted))
            .is_err());

        let mut blank_session = episodes;
        blank_session[0].sessions = vec!["  ".into()];
        blank_session[1].sessions = vec!["".into()];
        pattern = CandidatePatternV1::propose(payload_for(&blank_session)).unwrap();
        assert!(pattern
            .boundary_review(&blank_session, review(&pattern, BoundaryVerdict::Accepted))
            .is_err());
    }

    #[test]
    fn unrelated_episode_family_or_signature_cannot_support_boundary() {
        let (_, mut episodes) = core();
        episodes[1].family = "unrelated-family".into();
        let mut pattern = CandidatePatternV1::propose(payload_for(&episodes)).unwrap();
        assert_eq!(
            pattern.boundary_review(
                &episodes,
                review(&pattern, BoundaryVerdict::Accepted),
            ),
            Err(CandidatePatternError::EpisodeBoundaryMismatch(
                "ep-2".into(),
            ))
        );

        let (_, mut episodes) = core();
        episodes[1].signature = "unrelated-signature".into();
        let mut pattern = CandidatePatternV1::propose(payload_for(&episodes)).unwrap();
        assert_eq!(
            pattern.boundary_review(
                &episodes,
                review(&pattern, BoundaryVerdict::Accepted),
            ),
            Err(CandidatePatternError::EpisodeBoundaryMismatch(
                "ep-2".into(),
            ))
        );
    }

    #[test]
    fn qualification_replay_and_changed_input_conflict() {
        let mut pattern = accepted_pattern();
        let package = qualification();
        pattern.qualify(package.clone(), "operator").unwrap();
        pattern.qualify(package, "operator").unwrap();
        let mut changed = qualification();
        changed.detector_version = "det-v2".into();
        assert_eq!(
            pattern.qualify(changed, "operator"),
            Err(CandidatePatternError::QualificationConflict)
        );
    }

    #[test]
    fn qualification_rejects_failed_or_leaking_datasets() {
        let mut pattern = accepted_pattern();
        let mut failed = qualification();
        failed.qualification_passed = false;
        assert!(pattern.qualify(failed, "operator").is_err());

        let mut leaking = qualification();
        leaking.frozen_test.positive_cases[0].case_id = "dev-pos".into();
        leaking.frozen_test.dataset_sha256 = hash_cases(&leaking.frozen_test);
        assert_eq!(
            pattern.qualify(leaking, "operator"),
            Err(CandidatePatternError::DatasetLeakage)
        );

        let mut digest_mismatch = qualification();
        digest_mismatch.development.dataset_sha256 = h('8');
        assert_eq!(
            pattern.qualify(digest_mismatch, "operator"),
            Err(CandidatePatternError::DatasetDigestMismatch("development"))
        );

        let mut missing_control = qualification();
        missing_control.frozen_test.hard_negative_cases.clear();
        missing_control.frozen_test.dataset_sha256 = hash_cases(&missing_control.frozen_test);
        assert_eq!(
            pattern.qualify(missing_control, "operator"),
            Err(CandidatePatternError::MissingHardNegatives("frozen_test"))
        );
    }

    #[test]
    fn dataset_hash_is_order_independent_and_rejects_duplicates() {
        let mut ordered = dataset('b', "dev-pos", "dev-neg");
        ordered.positive_cases.push(PatternCaseRefV1 {
            case_id: "dev-pos-2".into(),
            case_sha256: h('d'),
        });
        ordered.hard_negative_cases.push(PatternCaseRefV1 {
            case_id: "dev-neg-2".into(),
            case_sha256: h('e'),
        });
        ordered.dataset_sha256 = hash_cases(&ordered);
        let mut reversed = ordered.clone();
        reversed.positive_cases.reverse();
        reversed.hard_negative_cases.reverse();
        assert_eq!(hash_cases(&ordered), hash_cases(&reversed));
        ordered.positive_cases.push(ordered.positive_cases[0].clone());
        ordered.dataset_sha256 = hash_cases(&ordered);
        assert_eq!(
            validate_dataset(&ordered, "development"),
            Err(CandidatePatternError::DuplicateCase("dev-pos".into()))
        );

        let mut contradictory = dataset('b', "dev-pos", "dev-neg");
        contradictory.hard_negative_cases[0].case_sha256 =
            contradictory.positive_cases[0].case_sha256.clone();
        contradictory.dataset_sha256 = hash_cases(&contradictory);
        assert_eq!(
            validate_dataset(&contradictory, "development"),
            Err(CandidatePatternError::DuplicateCase("dev-neg".into()))
        );
    }

    #[test]
    fn envelope_and_transition_tampering_is_rejected() {
        let (payload, _) = core();
        let mut proposed = CandidatePatternV1::propose(payload).unwrap();
        proposed.qualification_sha256 = Some(h('8'));
        assert_eq!(
            proposed.verify(),
            Err(CandidatePatternError::StateCoherence)
        );

        let mut accepted = accepted_pattern();
        accepted.schema_version = "adapt.candidate-pattern.v2".into();
        assert!(accepted.verify().is_err());

        let mut accepted = accepted_pattern();
        accepted.pattern_id = "cp_forged".into();
        assert!(accepted.verify().is_err());

        let mut accepted = accepted_pattern();
        accepted.payload.operational_description = "changed".into();
        assert!(accepted.verify().is_err());

        let mut accepted = accepted_pattern();
        accepted.receipts[0].to = PatternState::Deactivated;
        accepted.receipts[0].receipt_sha256 = state_digest(&accepted.receipts[0]);
        assert_eq!(
            accepted.verify(),
            Err(CandidatePatternError::StateCoherence)
        );

        let mut accepted = accepted_pattern();
        accepted.receipts[0].actor = "tampered".into();
        assert_eq!(
            accepted.verify(),
            Err(CandidatePatternError::ReceiptDigestMismatch)
        );
    }

    #[test]
    fn rejected_pattern_cannot_qualify() {
        let (payload, episodes) = core();
        let mut pattern = CandidatePatternV1::propose(payload).unwrap();
        pattern
            .boundary_review(&episodes, review(&pattern, BoundaryVerdict::Rejected))
            .unwrap();
        assert_eq!(
            pattern.qualify(qualification(), "operator"),
            Err(CandidatePatternError::ReviewRequired)
        );
        assert!(!pattern.activation_authorized);
    }

    #[test]
    fn qualified_pattern_still_cannot_authorize_activation() {
        let mut pattern = accepted_pattern();
        pattern.qualify(qualification(), "operator").unwrap();
        assert_eq!(pattern.state, PatternState::Qualified);
        assert!(!pattern.activation_authorized);
        assert!(pattern.verify().is_ok());
    }

    #[test]
    fn deactivation_replay_is_exact_and_tamper_is_refused() {
        let mut pattern = accepted_pattern();
        pattern.qualify(qualification(), "operator").unwrap();
        pattern.deactivate("operator", "deactivate-receipt").unwrap();
        pattern.deactivate("operator", "deactivate-receipt").unwrap();
        assert_eq!(pattern.receipts.len(), 3);
        pattern.receipts[2].actor = "tampered".into();
        assert!(pattern.deactivate("operator", "deactivate-receipt").is_err());
    }
}
