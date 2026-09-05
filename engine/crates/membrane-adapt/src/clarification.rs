//! Evidence-bound clarification state for Adapt proposal formation.
//!
//! This module is deliberately authority-poor. It preserves one bounded
//! clarification question, one human-answer binding, and a same-lineage resume
//! decision. It never writes Cortex truth, admits a preference, or accepts an
//! agent-authored answer as human merely because a caller labels it that way.

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::canonical::sha256_canonical;

pub const CLARIFICATION_SCHEMA_VERSION: u32 = 1;
pub const CLARIFICATION_CONTRACT: &str = "adapt.clarification.v1";
pub const MAX_CLARIFICATION_TTL_MS: u64 = 24 * 60 * 60 * 1000;
pub const MAX_QUESTION_CHARS: usize = 2048;
pub const MAX_ANSWER_CHARS: usize = 8192;
pub const MAX_MISSING_EVIDENCE_ITEMS: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClarificationStateV1 {
    PendingHumanAnswer,
    Answered,
    Resumed,
    Cancelled,
    Stale,
    Expired,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HumanAnswerSourceV1 {
    /// Local operator UI/CLI that separately authenticates the human actor.
    LocalOperator,
    /// Signed adjudication whose signer is verified outside this pure module.
    VerifiedAdjudicator,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClarificationNeedV1 {
    pub schema_version: u32,
    pub clarification_id: String,
    pub lineage_id: String,
    pub scope: String,
    pub semantic_target: String,
    pub target_version: u64,
    pub evidence_sha256: String,
    pub question: String,
    pub missing_evidence: Vec<String>,
    pub opened_at_ms: u64,
    pub expires_at_ms: u64,
}

impl ClarificationNeedV1 {
    pub fn expected_id(&self) -> String {
        let digest = sha256_canonical(&json!([
            CLARIFICATION_CONTRACT,
            self.lineage_id,
            self.scope,
            self.semantic_target,
            self.target_version,
            self.evidence_sha256
        ]));
        format!("adapt-clarify-{digest}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClarificationAnswerV1 {
    pub schema_version: u32,
    pub clarification_id: String,
    pub human_actor_id: String,
    pub source: HumanAnswerSourceV1,
    /// Receipt proving that the transport authenticated a human or verified
    /// adjudicator. This module binds the receipt; the transport verifies it.
    pub human_receipt_id: String,
    pub human_receipt_sha256: String,
    pub answer: String,
    pub answered_at_ms: u64,
    pub observed_target_version: u64,
    pub observed_evidence_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClarificationResumeRequestV1 {
    pub schema_version: u32,
    pub clarification_id: String,
    pub resumed_at_ms: u64,
    pub observed_target_version: u64,
    pub observed_evidence_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClarificationResumeBindingV1 {
    pub clarification_id: String,
    pub lineage_id: String,
    pub semantic_target: String,
    pub target_version: u64,
    pub evidence_sha256: String,
    pub answer_sha256: String,
    pub human_receipt_id: String,
    pub human_receipt_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClarificationSnapshotV1 {
    pub contract: String,
    pub need: ClarificationNeedV1,
    pub state: ClarificationStateV1,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer: Option<ClarificationAnswerV1>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClarificationDecisionV1 {
    pub accepted: bool,
    pub snapshot: ClarificationSnapshotV1,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resume_binding: Option<ClarificationResumeBindingV1>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ClarificationFileV1 {
    schema_version: u32,
    revision: u64,
    records: BTreeMap<String, ClarificationSnapshotV1>,
}

impl Default for ClarificationFileV1 {
    fn default() -> Self {
        Self {
            schema_version: CLARIFICATION_SCHEMA_VERSION,
            revision: 0,
            records: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ClarificationError {
    #[error("invalid clarification contract: {0}")]
    Invalid(String),
    #[error("clarification state I/O failed: {0}")]
    Io(String),
    #[error("clarification state is malformed: {0}")]
    Corrupt(String),
    #[error("clarification not found: {0}")]
    NotFound(String),
    #[error("clarification identity already exists with different question/evidence state: {0}")]
    IdentityConflict(String),
    #[error("clarification store revision changed: expected {expected}, observed {observed}")]
    ConcurrentModification { expected: u64, observed: u64 },
}

/// File-backed non-truth state for a clarification interaction. The store uses
/// optimistic revision checks so restart is safe and stale writers fail closed.
pub struct ClarificationStore {
    path: PathBuf,
    state: ClarificationFileV1,
}

impl ClarificationStore {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, ClarificationError> {
        let path = path.into();
        let state = if path.exists() {
            read_file(&path)?
        } else {
            ClarificationFileV1::default()
        };
        validate_file(&state)?;
        Ok(Self { path, state })
    }

    pub fn revision(&self) -> u64 {
        self.state.revision
    }

    pub fn get(&self, clarification_id: &str) -> Option<&ClarificationSnapshotV1> {
        self.state.records.get(clarification_id)
    }

    /// Create exactly one clarification for its deterministic lineage/target/
    /// evidence identity. Exact retries are idempotent; changed question or
    /// missing-evidence material under the same identity is a typed conflict.
    pub fn create(
        &mut self,
        need: ClarificationNeedV1,
    ) -> Result<&ClarificationSnapshotV1, ClarificationError> {
        let snapshot = open(need)?;
        let id = snapshot.need.clarification_id.clone();
        match self
            .state
            .records
            .get(&id)
            .map(|existing| existing.need == snapshot.need)
        {
            Some(true) => {
                return Ok(self.state.records.get(&id).expect("existing clarification"));
            }
            Some(false) => return Err(ClarificationError::IdentityConflict(id)),
            None => {}
        }
        let previous = self.state.clone();
        self.state.records.insert(id.clone(), snapshot);
        self.persist_or_rollback(previous)?;
        Ok(self.state.records.get(&id).expect("inserted clarification"))
    }

    pub fn answer(
        &mut self,
        clarification_id: &str,
        answer: ClarificationAnswerV1,
    ) -> Result<ClarificationDecisionV1, ClarificationError> {
        let current = self
            .state
            .records
            .get(clarification_id)
            .cloned()
            .ok_or_else(|| ClarificationError::NotFound(clarification_id.into()))?;
        let decision = submit_answer(&current, answer)?;
        self.persist_decision_if_changed(current, &decision.snapshot)?;
        Ok(decision)
    }

    pub fn resume(
        &mut self,
        clarification_id: &str,
        request: ClarificationResumeRequestV1,
    ) -> Result<ClarificationDecisionV1, ClarificationError> {
        let current = self
            .state
            .records
            .get(clarification_id)
            .cloned()
            .ok_or_else(|| ClarificationError::NotFound(clarification_id.into()))?;
        let decision = resume(&current, request)?;
        self.persist_decision_if_changed(current, &decision.snapshot)?;
        Ok(decision)
    }

    pub fn cancel(
        &mut self,
        clarification_id: &str,
        now_ms: u64,
        reason: &str,
    ) -> Result<&ClarificationSnapshotV1, ClarificationError> {
        let current = self
            .state
            .records
            .get(clarification_id)
            .cloned()
            .ok_or_else(|| ClarificationError::NotFound(clarification_id.into()))?;
        let next = cancel(&current, now_ms, reason)?;
        self.persist_snapshot_if_changed(current, next, clarification_id)?;
        Ok(self
            .state
            .records
            .get(clarification_id)
            .expect("known clarification"))
    }

    pub fn mark_unsupported(
        &mut self,
        clarification_id: &str,
        now_ms: u64,
        reason: &str,
    ) -> Result<&ClarificationSnapshotV1, ClarificationError> {
        let current = self
            .state
            .records
            .get(clarification_id)
            .cloned()
            .ok_or_else(|| ClarificationError::NotFound(clarification_id.into()))?;
        let next = unsupported(&current, now_ms, reason)?;
        self.persist_snapshot_if_changed(current, next, clarification_id)?;
        Ok(self
            .state
            .records
            .get(clarification_id)
            .expect("known clarification"))
    }

    fn persist_decision_if_changed(
        &mut self,
        current: ClarificationSnapshotV1,
        next: &ClarificationSnapshotV1,
    ) -> Result<(), ClarificationError> {
        let id = current.need.clarification_id.clone();
        self.persist_snapshot_if_changed(current, next.clone(), &id)
    }

    fn persist_snapshot_if_changed(
        &mut self,
        current: ClarificationSnapshotV1,
        next: ClarificationSnapshotV1,
        clarification_id: &str,
    ) -> Result<(), ClarificationError> {
        if current == next {
            return Ok(());
        }
        let previous = self.state.clone();
        self.state.records.insert(clarification_id.into(), next);
        self.persist_or_rollback(previous)
    }

    fn persist_or_rollback(
        &mut self,
        previous: ClarificationFileV1,
    ) -> Result<(), ClarificationError> {
        if let Err(error) = self.persist() {
            self.state = previous;
            return Err(error);
        }
        Ok(())
    }

    fn persist(&mut self) -> Result<(), ClarificationError> {
        let observed_revision = if self.path.exists() {
            read_file(&self.path)?.revision
        } else {
            0
        };
        if observed_revision != self.state.revision {
            return Err(ClarificationError::ConcurrentModification {
                expected: self.state.revision,
                observed: observed_revision,
            });
        }
        if let Some(parent) = self
            .path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)
                .map_err(|error| ClarificationError::Io(error.to_string()))?;
        }
        let mut next = self.state.clone();
        next.revision = next.revision.saturating_add(1);
        let bytes = serde_json::to_vec_pretty(&next)
            .map_err(|error| ClarificationError::Corrupt(error.to_string()))?;
        std::fs::write(&self.path, bytes)
            .map_err(|error| ClarificationError::Io(error.to_string()))?;
        self.state = next;
        Ok(())
    }
}

pub fn open(mut need: ClarificationNeedV1) -> Result<ClarificationSnapshotV1, ClarificationError> {
    validate_need(&need)?;
    need.missing_evidence.sort();
    need.missing_evidence.dedup();
    if need.clarification_id != need.expected_id() {
        return Err(ClarificationError::Invalid(
            "clarification id is not the deterministic lineage/target/evidence id".into(),
        ));
    }
    Ok(ClarificationSnapshotV1 {
        contract: CLARIFICATION_CONTRACT.into(),
        need,
        state: ClarificationStateV1::PendingHumanAnswer,
        answer: None,
        terminal_reason: None,
    })
}

pub fn submit_answer(
    snapshot: &ClarificationSnapshotV1,
    answer: ClarificationAnswerV1,
) -> Result<ClarificationDecisionV1, ClarificationError> {
    validate_snapshot(snapshot)?;
    validate_answer(&answer)?;
    if answer.clarification_id != snapshot.need.clarification_id {
        return Err(ClarificationError::Invalid(
            "answer clarification identity mismatch".into(),
        ));
    }
    let current = reconcile_current(
        snapshot,
        answer.answered_at_ms,
        answer.observed_target_version,
        &answer.observed_evidence_sha256,
    );
    if current.state != ClarificationStateV1::PendingHumanAnswer {
        return Ok(ClarificationDecisionV1 {
            accepted: false,
            reason: terminal_reason(&current),
            snapshot: current,
            resume_binding: None,
        });
    }
    let mut next = current;
    next.state = ClarificationStateV1::Answered;
    next.answer = Some(answer);
    Ok(ClarificationDecisionV1 {
        accepted: true,
        snapshot: next,
        resume_binding: None,
        reason: "human_answer_bound".into(),
    })
}

pub fn resume(
    snapshot: &ClarificationSnapshotV1,
    request: ClarificationResumeRequestV1,
) -> Result<ClarificationDecisionV1, ClarificationError> {
    validate_snapshot(snapshot)?;
    validate_resume(&request)?;
    if request.clarification_id != snapshot.need.clarification_id {
        return Err(ClarificationError::Invalid(
            "resume clarification identity mismatch".into(),
        ));
    }
    let current = reconcile_current(
        snapshot,
        request.resumed_at_ms,
        request.observed_target_version,
        &request.observed_evidence_sha256,
    );
    if current.state != ClarificationStateV1::Answered {
        return Ok(ClarificationDecisionV1 {
            accepted: false,
            reason: if current.state == ClarificationStateV1::PendingHumanAnswer {
                "human_answer_required".into()
            } else {
                terminal_reason(&current)
            },
            snapshot: current,
            resume_binding: None,
        });
    }
    let answer = current.answer.as_ref().expect("answered state has answer");
    let answer_sha256 = sha256_canonical(&json!({
        "answer": answer.answer,
        "human_actor_id": answer.human_actor_id,
        "source": answer.source,
        "human_receipt_id": answer.human_receipt_id,
        "human_receipt_sha256": answer.human_receipt_sha256,
    }));
    let binding = ClarificationResumeBindingV1 {
        clarification_id: current.need.clarification_id.clone(),
        lineage_id: current.need.lineage_id.clone(),
        semantic_target: current.need.semantic_target.clone(),
        target_version: current.need.target_version,
        evidence_sha256: current.need.evidence_sha256.clone(),
        answer_sha256,
        human_receipt_id: answer.human_receipt_id.clone(),
        human_receipt_sha256: answer.human_receipt_sha256.clone(),
    };
    let mut next = current;
    next.state = ClarificationStateV1::Resumed;
    Ok(ClarificationDecisionV1 {
        accepted: true,
        snapshot: next,
        resume_binding: Some(binding),
        reason: "same_lineage_resume_bound".into(),
    })
}

pub fn cancel(
    snapshot: &ClarificationSnapshotV1,
    now_ms: u64,
    reason: &str,
) -> Result<ClarificationSnapshotV1, ClarificationError> {
    validate_snapshot(snapshot)?;
    validate_text("cancel reason", reason, 1, 512)?;
    let mut current = reconcile_current(
        snapshot,
        now_ms,
        snapshot.need.target_version,
        &snapshot.need.evidence_sha256,
    );
    if matches!(
        current.state,
        ClarificationStateV1::PendingHumanAnswer | ClarificationStateV1::Answered
    ) {
        current.state = ClarificationStateV1::Cancelled;
        current.terminal_reason = Some(reason.trim().into());
    }
    Ok(current)
}

pub fn unsupported(
    snapshot: &ClarificationSnapshotV1,
    now_ms: u64,
    reason: &str,
) -> Result<ClarificationSnapshotV1, ClarificationError> {
    validate_snapshot(snapshot)?;
    validate_text("unsupported reason", reason, 1, 512)?;
    let mut current = reconcile_current(
        snapshot,
        now_ms,
        snapshot.need.target_version,
        &snapshot.need.evidence_sha256,
    );
    if matches!(
        current.state,
        ClarificationStateV1::PendingHumanAnswer | ClarificationStateV1::Answered
    ) {
        current.state = ClarificationStateV1::Unsupported;
        current.terminal_reason = Some(reason.trim().into());
    }
    Ok(current)
}

/// Reconcile mutable clarification state with the exact target/evidence the
/// caller currently observes. A stale or expired clarification cannot resume.
pub fn reconcile_current(
    snapshot: &ClarificationSnapshotV1,
    now_ms: u64,
    observed_target_version: u64,
    observed_evidence_sha256: &str,
) -> ClarificationSnapshotV1 {
    if matches!(
        snapshot.state,
        ClarificationStateV1::Resumed
            | ClarificationStateV1::Cancelled
            | ClarificationStateV1::Stale
            | ClarificationStateV1::Expired
            | ClarificationStateV1::Unsupported
    ) {
        return snapshot.clone();
    }
    let mut next = snapshot.clone();
    if now_ms >= snapshot.need.expires_at_ms {
        next.state = ClarificationStateV1::Expired;
        next.terminal_reason = Some("clarification_expired".into());
    } else if observed_target_version != snapshot.need.target_version
        || observed_evidence_sha256 != snapshot.need.evidence_sha256
    {
        next.state = ClarificationStateV1::Stale;
        next.terminal_reason = Some("target_or_evidence_changed".into());
    }
    next
}

fn validate_need(need: &ClarificationNeedV1) -> Result<(), ClarificationError> {
    if need.schema_version != CLARIFICATION_SCHEMA_VERSION {
        return Err(ClarificationError::Invalid(
            "unsupported schema version".into(),
        ));
    }
    validate_text("clarification id", &need.clarification_id, 1, 128)?;
    validate_text("lineage id", &need.lineage_id, 1, 256)?;
    validate_text("scope", &need.scope, 1, 512)?;
    validate_text("semantic target", &need.semantic_target, 1, 512)?;
    validate_hash("evidence sha256", &need.evidence_sha256)?;
    validate_text("question", &need.question, 1, MAX_QUESTION_CHARS)?;
    if need.missing_evidence.is_empty() || need.missing_evidence.len() > MAX_MISSING_EVIDENCE_ITEMS
    {
        return Err(ClarificationError::Invalid(
            "clarification requires bounded missing-evidence reasons".into(),
        ));
    }
    for item in &need.missing_evidence {
        validate_text("missing evidence", item, 1, 256)?;
    }
    if need.opened_at_ms == 0
        || need.expires_at_ms <= need.opened_at_ms
        || need.expires_at_ms - need.opened_at_ms > MAX_CLARIFICATION_TTL_MS
    {
        return Err(ClarificationError::Invalid(
            "invalid clarification lifetime".into(),
        ));
    }
    Ok(())
}

fn validate_answer(answer: &ClarificationAnswerV1) -> Result<(), ClarificationError> {
    if answer.schema_version != CLARIFICATION_SCHEMA_VERSION {
        return Err(ClarificationError::Invalid(
            "unsupported answer schema version".into(),
        ));
    }
    validate_text("clarification id", &answer.clarification_id, 1, 128)?;
    validate_text("human actor id", &answer.human_actor_id, 1, 256)?;
    validate_text("human receipt id", &answer.human_receipt_id, 1, 512)?;
    validate_hash("human receipt sha256", &answer.human_receipt_sha256)?;
    validate_text("answer", &answer.answer, 1, MAX_ANSWER_CHARS)?;
    validate_hash("observed evidence sha256", &answer.observed_evidence_sha256)?;
    if answer.answered_at_ms == 0 {
        return Err(ClarificationError::Invalid(
            "answer timestamp required".into(),
        ));
    }
    Ok(())
}

fn validate_resume(request: &ClarificationResumeRequestV1) -> Result<(), ClarificationError> {
    if request.schema_version != CLARIFICATION_SCHEMA_VERSION {
        return Err(ClarificationError::Invalid(
            "unsupported resume schema version".into(),
        ));
    }
    validate_text("clarification id", &request.clarification_id, 1, 128)?;
    validate_hash(
        "observed evidence sha256",
        &request.observed_evidence_sha256,
    )?;
    if request.resumed_at_ms == 0 {
        return Err(ClarificationError::Invalid(
            "resume timestamp required".into(),
        ));
    }
    Ok(())
}

fn validate_snapshot(snapshot: &ClarificationSnapshotV1) -> Result<(), ClarificationError> {
    if snapshot.contract != CLARIFICATION_CONTRACT {
        return Err(ClarificationError::Invalid(
            "wrong clarification contract".into(),
        ));
    }
    validate_need(&snapshot.need)?;
    if snapshot.need.clarification_id != snapshot.need.expected_id() {
        return Err(ClarificationError::Invalid(
            "clarification identity drift".into(),
        ));
    }
    match snapshot.state {
        ClarificationStateV1::PendingHumanAnswer => {
            if snapshot.answer.is_some() || snapshot.terminal_reason.is_some() {
                return Err(ClarificationError::Invalid(
                    "pending state carries later fields".into(),
                ));
            }
        }
        ClarificationStateV1::Answered | ClarificationStateV1::Resumed => {
            let answer = snapshot
                .answer
                .as_ref()
                .ok_or_else(|| ClarificationError::Invalid("answered state lacks answer".into()))?;
            validate_answer(answer)?;
            if answer.clarification_id != snapshot.need.clarification_id {
                return Err(ClarificationError::Invalid("answer identity drift".into()));
            }
            if snapshot.terminal_reason.is_some() {
                return Err(ClarificationError::Invalid(
                    "nonterminal state has terminal reason".into(),
                ));
            }
        }
        ClarificationStateV1::Cancelled
        | ClarificationStateV1::Stale
        | ClarificationStateV1::Expired
        | ClarificationStateV1::Unsupported => {
            if snapshot
                .terminal_reason
                .as_deref()
                .unwrap_or("")
                .trim()
                .is_empty()
            {
                return Err(ClarificationError::Invalid(
                    "terminal state lacks reason".into(),
                ));
            }
        }
    }
    Ok(())
}

fn validate_file(file: &ClarificationFileV1) -> Result<(), ClarificationError> {
    if file.schema_version != CLARIFICATION_SCHEMA_VERSION {
        return Err(ClarificationError::Corrupt(
            "unsupported schema version".into(),
        ));
    }
    for (id, snapshot) in &file.records {
        validate_snapshot(snapshot)
            .map_err(|error| ClarificationError::Corrupt(error.to_string()))?;
        if id != &snapshot.need.clarification_id {
            return Err(ClarificationError::Corrupt(format!(
                "clarification map key does not match payload: {id}"
            )));
        }
    }
    Ok(())
}

fn read_file(path: &Path) -> Result<ClarificationFileV1, ClarificationError> {
    let bytes = std::fs::read(path).map_err(|error| ClarificationError::Io(error.to_string()))?;
    serde_json::from_slice(&bytes).map_err(|error| ClarificationError::Corrupt(error.to_string()))
}

fn validate_hash(field: &str, value: &str) -> Result<(), ClarificationError> {
    let value = value.strip_prefix("sha256:").unwrap_or(value);
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(ClarificationError::Invalid(format!("invalid {field}")));
    }
    Ok(())
}

fn validate_text(
    field: &str,
    value: &str,
    minimum: usize,
    maximum: usize,
) -> Result<(), ClarificationError> {
    let value = value.trim();
    let len = value.chars().count();
    if len < minimum || len > maximum || value.chars().any(char::is_control) {
        return Err(ClarificationError::Invalid(format!("invalid {field}")));
    }
    Ok(())
}

fn terminal_reason(snapshot: &ClarificationSnapshotV1) -> String {
    snapshot
        .terminal_reason
        .clone()
        .unwrap_or_else(|| match snapshot.state {
            ClarificationStateV1::Answered => "answer_already_bound".into(),
            ClarificationStateV1::PendingHumanAnswer => "human_answer_required".into(),
            ClarificationStateV1::Resumed => "clarification_already_resumed".into(),
            ClarificationStateV1::Cancelled => "clarification_cancelled".into(),
            ClarificationStateV1::Stale => "target_or_evidence_changed".into(),
            ClarificationStateV1::Expired => "clarification_expired".into(),
            ClarificationStateV1::Unsupported => "clarification_host_unsupported".into(),
        })
}
