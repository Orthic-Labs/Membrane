//! Persisted sealed proposal lifecycle for model-assisted Adapt work.
//!
//! Model output may supply wording/grouping/remediation hints only. Callers
//! must first construct a deterministic [`SemanticPayloadV1`]; this store then
//! binds exact plan identity, payload seal, risk, expiry, target version,
//! approval, verification, & lifecycle. It performs no Cortex write and grants
//! no authority.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::seal::{verify_seal, SemanticPayloadV1};

pub const PROPOSAL_PLAN_SCHEMA_VERSION: u32 = 1;
pub const PROPOSAL_PLAN_CONTRACT: &str = "adapt.proposal-plan.v1";
pub const MAX_PROPOSAL_TTL_SECONDS: u64 = 24 * 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalRisk {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalPlanState {
    Proposed,
    Approved,
    Committed,
    Expired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalApprovalV1 {
    pub reviewer_id: String,
    pub max_risk: ProposalRisk,
    pub approved_at: u64,
}

/// Deterministic post-effect proof required before a plan may enter committed
/// state. Receipt content is caller-produced, but every binding is verified by
/// this store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalVerificationV1 {
    pub receipt_id: String,
    pub plan_id: String,
    pub proposal_seal_sha256: String,
    pub target_version: u64,
    pub passed: bool,
    pub checked_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalPlanV1 {
    pub contract: String,
    pub plan_id: String,
    pub state: ProposalPlanState,
    pub risk: ProposalRisk,
    pub created_at: u64,
    pub expires_at: u64,
    pub expected_target_version: u64,
    pub semantic_payload: SemanticPayloadV1,
    pub seal_digest: String,
    pub proposal_seal_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval: Option<ProposalApprovalV1>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verification: Option<ProposalVerificationV1>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub committed_at: Option<u64>,
}

/// Stable identity of the semantic surface being mutated. Candidate wording,
/// evidence receipts and authority effect deliberately do not participate: two
/// variants for the same surface/version must contend for one apply-eligible
/// slot rather than evade exclusion by changing their text.
pub fn semantic_target_sha256(payload: &SemanticPayloadV1) -> String {
    crate::canonical::sha256_canonical(&json!({
        "record_kind": &payload.record_kind,
        "category": &payload.category,
        "scope": &payload.scope,
        "scope_dimensions": &payload.scope_dimensions,
        "record_class": &payload.record_class,
        "machine_binding": &payload.machine_binding,
    }))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProposalPlanFileV1 {
    schema_version: u32,
    revision: u64,
    plans: BTreeMap<String, ProposalPlanV1>,
}

impl Default for ProposalPlanFileV1 {
    fn default() -> Self {
        Self {
            schema_version: PROPOSAL_PLAN_SCHEMA_VERSION,
            revision: 0,
            plans: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProposalStateError {
    #[error("proposal store I/O failed: {0}")]
    Io(String),
    #[error("proposal store is malformed: {0}")]
    Corrupt(String),
    #[error("invalid plan id")]
    InvalidPlanId,
    #[error("plan already exists: {0}")]
    PlanExists(String),
    #[error("plan not found: {0}")]
    PlanNotFound(String),
    #[error("proposal TTL must be between 1 and {MAX_PROPOSAL_TTL_SECONDS} seconds")]
    InvalidTtl,
    #[error("sealed payload does not match recorded digest")]
    SealMismatch,
    #[error("plan expired: {0}")]
    Expired(String),
    #[error("invalid lifecycle transition from {from:?} to {to:?}")]
    InvalidTransition {
        from: ProposalPlanState,
        to: ProposalPlanState,
    },
    #[error("approval risk ceiling is below plan risk")]
    RiskApprovalInsufficient,
    #[error("target version changed: expected {expected}, observed {observed}")]
    VersionConflict { expected: u64, observed: u64 },
    #[error("apply-eligible proposal already exists for semantic target {target_sha256} at version {target_version}: {existing_plan_id}")]
    TargetConflict {
        target_sha256: String,
        target_version: u64,
        existing_plan_id: String,
    },
    #[error("verification receipt does not bind plan, seal, version, time, or passing result")]
    VerificationRejected,
    #[error("proposal lifecycle timestamp precedes its required prior state")]
    InvalidTimestamp,
    #[error("proposal store revision changed: expected {expected}, observed {observed}")]
    ConcurrentModification { expected: u64, observed: u64 },
}

/// File-backed plan store. Each mutation checks current persisted revision so
/// two stale writers cannot silently overwrite each other.
pub struct ProposalPlanStore {
    path: PathBuf,
    state: ProposalPlanFileV1,
}

impl ProposalPlanStore {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, ProposalStateError> {
        let path = path.into();
        let state = if path.exists() {
            read_file(&path)?
        } else {
            ProposalPlanFileV1::default()
        };
        validate_file(&state)?;
        Ok(Self { path, state })
    }

    pub fn revision(&self) -> u64 {
        self.state.revision
    }

    pub fn get(&self, plan_id: &str) -> Option<&ProposalPlanV1> {
        self.state.plans.get(plan_id)
    }

    pub fn propose(
        &mut self,
        plan_id: &str,
        semantic_payload: SemanticPayloadV1,
        risk: ProposalRisk,
        now: u64,
        ttl_seconds: u64,
        expected_target_version: u64,
    ) -> Result<&ProposalPlanV1, ProposalStateError> {
        validate_plan_id(plan_id)?;
        if ttl_seconds == 0 || ttl_seconds > MAX_PROPOSAL_TTL_SECONDS {
            return Err(ProposalStateError::InvalidTtl);
        }
        if self.state.plans.contains_key(plan_id) {
            return Err(ProposalStateError::PlanExists(plan_id.into()));
        }

        let target_sha256 = semantic_target_sha256(&semantic_payload);
        let seal_digest = semantic_payload.seal_digest();
        let existing = self
            .state
            .plans
            .values()
            .find(|candidate| {
                matches!(
                    candidate.state,
                    ProposalPlanState::Proposed | ProposalPlanState::Approved
                ) && candidate.expires_at > now
                    && candidate.expected_target_version == expected_target_version
                    && semantic_target_sha256(&candidate.semantic_payload) == target_sha256
            })
            .map(|candidate| {
                (
                    candidate.plan_id.clone(),
                    candidate.seal_digest.clone(),
                    candidate.risk,
                )
            });
        if let Some((existing_plan_id, existing_seal, existing_risk)) = existing {
            if existing_seal == seal_digest && existing_risk == risk {
                // Exact semantic replay converges on the existing pending plan;
                // no second mutation or store revision is created.
                return Ok(self
                    .state
                    .plans
                    .get(&existing_plan_id)
                    .expect("existing plan was selected from this store"));
            }
            return Err(ProposalStateError::TargetConflict {
                target_sha256,
                target_version: expected_target_version,
                existing_plan_id,
            });
        }

        let expires_at = now
            .checked_add(ttl_seconds)
            .ok_or(ProposalStateError::InvalidTtl)?;
        let previous = self.state.clone();
        let mut plan = ProposalPlanV1 {
            contract: PROPOSAL_PLAN_CONTRACT.into(),
            plan_id: plan_id.into(),
            state: ProposalPlanState::Proposed,
            risk,
            created_at: now,
            expires_at,
            expected_target_version,
            semantic_payload,
            seal_digest,
            proposal_seal_sha256: String::new(),
            approval: None,
            verification: None,
            committed_at: None,
        };
        plan.proposal_seal_sha256 = proposal_seal(&plan);
        self.state.plans.insert(plan_id.into(), plan);
        self.persist_or_rollback(previous)?;
        Ok(self.state.plans.get(plan_id).expect("inserted plan"))
    }

    pub fn approve(
        &mut self,
        plan_id: &str,
        reviewer_id: &str,
        max_risk: ProposalRisk,
        now: u64,
        observed_target_version: u64,
    ) -> Result<&ProposalPlanV1, ProposalStateError> {
        if reviewer_id.trim().is_empty() {
            return Err(ProposalStateError::VerificationRejected);
        }
        self.guard_transition(
            plan_id,
            ProposalPlanState::Proposed,
            ProposalPlanState::Approved,
            now,
            observed_target_version,
        )?;
        let plan = self
            .state
            .plans
            .get(plan_id)
            .expect("guarded plan exists");
        if now < plan.created_at {
            return Err(ProposalStateError::InvalidTimestamp);
        }
        if max_risk < plan.risk {
            return Err(ProposalStateError::RiskApprovalInsufficient);
        }
        let previous = self.state.clone();
        let plan = self
            .state
            .plans
            .get_mut(plan_id)
            .expect("guarded plan exists");
        plan.state = ProposalPlanState::Approved;
        plan.approval = Some(ProposalApprovalV1 {
            reviewer_id: reviewer_id.trim().into(),
            max_risk,
            approved_at: now,
        });
        self.persist_or_rollback(previous)?;
        Ok(self.state.plans.get(plan_id).expect("approved plan"))
    }

    pub fn commit(
        &mut self,
        plan_id: &str,
        now: u64,
        observed_target_version: u64,
        verification: ProposalVerificationV1,
    ) -> Result<&ProposalPlanV1, ProposalStateError> {
        self.guard_transition(
            plan_id,
            ProposalPlanState::Approved,
            ProposalPlanState::Committed,
            now,
            observed_target_version,
        )?;
        let plan = self
            .state
            .plans
            .get(plan_id)
            .expect("guarded plan exists");
        let approved_at = plan
            .approval
            .as_ref()
            .map(|approval| approval.approved_at)
            .ok_or(ProposalStateError::VerificationRejected)?;
        if verification.receipt_id.trim().is_empty()
            || !verification.passed
            || verification.plan_id != plan.plan_id
            || verification.proposal_seal_sha256 != plan.proposal_seal_sha256
            || verification.target_version != plan.expected_target_version
            || verification.checked_at < approved_at
            || verification.checked_at > now
        {
            return Err(ProposalStateError::VerificationRejected);
        }
        let previous = self.state.clone();
        let plan = self
            .state
            .plans
            .get_mut(plan_id)
            .expect("guarded plan exists");
        plan.state = ProposalPlanState::Committed;
        plan.verification = Some(verification);
        plan.committed_at = Some(now);
        self.persist_or_rollback(previous)?;
        Ok(self.state.plans.get(plan_id).expect("committed plan"))
    }

    fn guard_transition(
        &mut self,
        plan_id: &str,
        expected_state: ProposalPlanState,
        next_state: ProposalPlanState,
        now: u64,
        observed_target_version: u64,
    ) -> Result<(), ProposalStateError> {
        validate_plan_id(plan_id)?;
        let plan = self
            .state
            .plans
            .get(plan_id)
            .ok_or_else(|| ProposalStateError::PlanNotFound(plan_id.into()))?;
        if !plan_seal_valid(plan) {
            return Err(ProposalStateError::SealMismatch);
        }
        if now >= plan.expires_at {
            if plan.state != ProposalPlanState::Committed
                && plan.state != ProposalPlanState::Expired
            {
                let previous = self.state.clone();
                self.state
                    .plans
                    .get_mut(plan_id)
                    .expect("known plan")
                    .state = ProposalPlanState::Expired;
                self.persist_or_rollback(previous)?;
            }
            return Err(ProposalStateError::Expired(plan_id.into()));
        }
        if plan.expected_target_version != observed_target_version {
            return Err(ProposalStateError::VersionConflict {
                expected: plan.expected_target_version,
                observed: observed_target_version,
            });
        }
        if plan.state != expected_state {
            return Err(ProposalStateError::InvalidTransition {
                from: plan.state,
                to: next_state,
            });
        }
        Ok(())
    }

    fn persist_or_rollback(
        &mut self,
        previous: ProposalPlanFileV1,
    ) -> Result<(), ProposalStateError> {
        if let Err(error) = self.persist() {
            self.state = previous;
            return Err(error);
        }
        Ok(())
    }

    fn persist(&mut self) -> Result<(), ProposalStateError> {
        let observed_revision = if self.path.exists() {
            read_file(&self.path)?.revision
        } else {
            0
        };
        if observed_revision != self.state.revision {
            return Err(ProposalStateError::ConcurrentModification {
                expected: self.state.revision,
                observed: observed_revision,
            });
        }
        if let Some(parent) = self.path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)
                .map_err(|error| ProposalStateError::Io(error.to_string()))?;
        }
        let mut next = self.state.clone();
        next.revision = next.revision.saturating_add(1);
        let bytes = serde_json::to_vec_pretty(&next)
            .map_err(|error| ProposalStateError::Corrupt(error.to_string()))?;
        std::fs::write(&self.path, bytes)
            .map_err(|error| ProposalStateError::Io(error.to_string()))?;
        self.state = next;
        Ok(())
    }
}

fn validate_plan_id(plan_id: &str) -> Result<(), ProposalStateError> {
    if plan_id.is_empty()
        || plan_id.len() > 128
        || !plan_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(ProposalStateError::InvalidPlanId);
    }
    Ok(())
}

fn read_file(path: &Path) -> Result<ProposalPlanFileV1, ProposalStateError> {
    let bytes = std::fs::read(path).map_err(|error| ProposalStateError::Io(error.to_string()))?;
    serde_json::from_slice(&bytes).map_err(|error| ProposalStateError::Corrupt(error.to_string()))
}

fn proposal_seal(plan: &ProposalPlanV1) -> String {
    fn field(hasher: &mut Sha256, bytes: &[u8]) {
        hasher.update((bytes.len() as u64).to_be_bytes());
        hasher.update(bytes);
    }

    let mut hasher = Sha256::new();
    field(&mut hasher, plan.contract.as_bytes());
    field(&mut hasher, plan.plan_id.as_bytes());
    field(&mut hasher, plan.seal_digest.as_bytes());
    field(
        &mut hasher,
        match plan.risk {
            ProposalRisk::Low => b"low",
            ProposalRisk::Medium => b"medium",
            ProposalRisk::High => b"high",
        },
    );
    hasher.update(plan.created_at.to_be_bytes());
    hasher.update(plan.expires_at.to_be_bytes());
    hasher.update(plan.expected_target_version.to_be_bytes());
    hex::encode(hasher.finalize())
}

fn plan_seal_valid(plan: &ProposalPlanV1) -> bool {
    verify_seal(&plan.semantic_payload, &plan.seal_digest).is_ok()
        && plan.proposal_seal_sha256 == proposal_seal(plan)
}

fn approval_valid(plan: &ProposalPlanV1) -> bool {
    plan.approval.as_ref().is_some_and(|approval| {
        !approval.reviewer_id.trim().is_empty()
            && approval.max_risk >= plan.risk
            && approval.approved_at >= plan.created_at
            && approval.approved_at < plan.expires_at
    })
}

fn verification_valid(plan: &ProposalPlanV1) -> bool {
    let Some(verification) = plan.verification.as_ref() else {
        return false;
    };
    let Some(approval) = plan.approval.as_ref() else {
        return false;
    };
    let Some(committed_at) = plan.committed_at else {
        return false;
    };
    !verification.receipt_id.trim().is_empty()
        && verification.passed
        && verification.plan_id == plan.plan_id
        && verification.proposal_seal_sha256 == plan.proposal_seal_sha256
        && verification.target_version == plan.expected_target_version
        && verification.checked_at >= approval.approved_at
        && verification.checked_at <= committed_at
        && committed_at < plan.expires_at
}

fn validate_file(file: &ProposalPlanFileV1) -> Result<(), ProposalStateError> {
    if file.schema_version != PROPOSAL_PLAN_SCHEMA_VERSION {
        return Err(ProposalStateError::Corrupt("unsupported schema version".into()));
    }
    for (key, plan) in &file.plans {
        validate_plan_id(key)?;
        if plan.contract != PROPOSAL_PLAN_CONTRACT
            || plan.plan_id != *key
            || plan.expires_at <= plan.created_at
            || !plan_seal_valid(plan)
        {
            return Err(ProposalStateError::Corrupt(format!(
                "invalid sealed plan {key}"
            )));
        }
        match plan.state {
            ProposalPlanState::Proposed => {
                if plan.approval.is_some()
                    || plan.verification.is_some()
                    || plan.committed_at.is_some()
                {
                    return Err(ProposalStateError::Corrupt(format!(
                        "proposed plan {key} carries later-state fields"
                    )));
                }
            }
            ProposalPlanState::Approved => {
                if !approval_valid(plan)
                    || plan.verification.is_some()
                    || plan.committed_at.is_some()
                {
                    return Err(ProposalStateError::Corrupt(format!(
                        "approved plan {key} has invalid state fields"
                    )));
                }
            }
            ProposalPlanState::Committed => {
                if !approval_valid(plan) || !verification_valid(plan) {
                    return Err(ProposalStateError::Corrupt(format!(
                        "committed plan {key} lacks approval or verification"
                    )));
                }
            }
            ProposalPlanState::Expired => {
                if plan.verification.is_some()
                    || plan.committed_at.is_some()
                    || plan.approval.is_some() && !approval_valid(plan)
                {
                    return Err(ProposalStateError::Corrupt(format!(
                        "expired plan {key} has invalid state fields"
                    )));
                }
            }
        }
    }
    Ok(())
}
