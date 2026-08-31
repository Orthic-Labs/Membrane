//! Production Adapt proposal-plan service.
//!
//! Model output supplies wording plus evidence claims only. This service
//! deterministically binds authority, scope, effect, receipts, & lifecycle,
//! then persists each sealed transition through `ProposalPlanStore`. It never
//! writes Cortex or treats committed plan state as durable-truth admission.

use std::path::Path;

use membrane_adapt::model_boundary::{
    DeterministicProposalBindingV1, ModelExtractionProposal, ModelProposalError,
};
use membrane_adapt::proposal_state::{
    ProposalPlanStore, ProposalPlanV1, ProposalRisk, ProposalStateError,
    ProposalVerificationV1,
};
use serde::{Deserialize, Serialize};

pub const ADAPT_PROPOSAL_SERVICE_CONTRACT: &str = "adapt.proposal-service.v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum AdaptProposalPlanRequestV1 {
    Propose {
        plan_id: String,
        model_proposal: ModelExtractionProposal,
        deterministic_binding: DeterministicProposalBindingV1,
        now: u64,
        ttl_seconds: u64,
        expected_target_version: u64,
    },
    Approve {
        plan_id: String,
        reviewer_id: String,
        max_risk: ProposalRisk,
        now: u64,
        observed_target_version: u64,
    },
    Commit {
        plan_id: String,
        now: u64,
        observed_target_version: u64,
        verification: ProposalVerificationV1,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AdaptProposalServiceError {
    #[error("model proposal failed deterministic binding: {0}")]
    ModelBinding(ModelProposalError),
    #[error(transparent)]
    State(#[from] ProposalStateError),
}

/// Execute one production proposal lifecycle transition against persisted
/// state. Every call reopens & validates store before applying transition.
pub fn execute_adapt_proposal_plan(
    store_path: &Path,
    request: AdaptProposalPlanRequestV1,
) -> Result<ProposalPlanV1, AdaptProposalServiceError> {
    let mut store = ProposalPlanStore::open(store_path)?;
    let plan = match request {
        AdaptProposalPlanRequestV1::Propose {
            plan_id,
            model_proposal,
            deterministic_binding,
            now,
            ttl_seconds,
            expected_target_version,
        } => {
            let payload = model_proposal
                .bind_deterministically(&deterministic_binding)
                .map_err(AdaptProposalServiceError::ModelBinding)?;
            let risk = match payload.authority_effect {
                membrane_adapt::authority::AuthorityEffect::Neutral => ProposalRisk::Low,
                membrane_adapt::authority::AuthorityEffect::Restrictive => ProposalRisk::Medium,
                membrane_adapt::authority::AuthorityEffect::PermissionExpanding
                | membrane_adapt::authority::AuthorityEffect::SecurityWeakening => {
                    ProposalRisk::High
                }
            };
            store.propose(
                &plan_id,
                payload,
                risk,
                now,
                ttl_seconds,
                expected_target_version,
            )?
        }
        AdaptProposalPlanRequestV1::Approve {
            plan_id,
            reviewer_id,
            max_risk,
            now,
            observed_target_version,
        } => store.approve(
            &plan_id,
            &reviewer_id,
            max_risk,
            now,
            observed_target_version,
        )?,
        AdaptProposalPlanRequestV1::Commit {
            plan_id,
            now,
            observed_target_version,
            verification,
        } => store.commit(
            &plan_id,
            now,
            observed_target_version,
            verification,
        )?,
    };
    Ok(plan.clone())
}
