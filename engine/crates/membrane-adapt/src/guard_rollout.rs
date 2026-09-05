//! ADP-077: a learned guard's evidence eligibility is not a host permission.
use crate::comparison::{bounded_id, digest, valid_digest};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuardStage {
    Reviewed,
    Shadow,
    Advisory,
    ScopedBlocking,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuardEvidenceV1 {
    pub kind: String,
    pub receipt_id: String,
    pub receipt_sha256: String,
    pub subject_sha256: String,
    pub scope: String,
    pub valid_until_ms: u64,
    pub passed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuardTransitionRequestV1 {
    pub schema_version: u32,
    pub issue_id: String,
    pub mitigation_sha256: String,
    pub target: String,
    pub target_sha256: String,
    pub host_configuration_sha256: String,
    pub current_scope: String,
    pub proposed_scope: String,
    pub current_stage: GuardStage,
    pub proposed_stage: GuardStage,
    pub now_ms: u64,
    pub comparable_exposures: u64,
    pub evaluated_exposures: u64,
    pub false_blocks: u64,
    pub minimum_exposures: u64,
    pub maximum_false_block_bps: u32,
    pub rollback_ref: String,
    pub evidence: Vec<GuardEvidenceV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardEligibilityV1 {
    pub contract: String,
    pub request_sha256: String,
    pub eligible: bool,
    pub reasons: Vec<String>,
    pub proposed_stage: GuardStage,
    pub host_authorization_required: bool,
    pub activation_authorized: bool,
    pub evidence_basis: String,
    pub decision_sha256: String,
}

pub fn evaluate(r: &GuardTransitionRequestV1) -> Result<GuardEligibilityV1, String> {
    if r.schema_version != 1
        || !bounded_id(&r.issue_id)
        || !bounded_id(&r.target)
        || !bounded_id(&r.current_scope)
        || !bounded_id(&r.proposed_scope)
        || ![
            &r.mitigation_sha256,
            &r.target_sha256,
            &r.host_configuration_sha256,
        ]
        .iter()
        .all(|d| valid_digest(d))
        || r.now_ms == 0
        || r.evidence.len() > 32
        || r.maximum_false_block_bps > 10000
        || r.false_blocks > r.evaluated_exposures
        || r.evaluated_exposures > r.comparable_exposures
    {
        return Err("invalid guard transition bindings/bounds".into());
    }
    let mut reasons = Vec::new();
    let normal = matches!(
        (r.current_stage, r.proposed_stage),
        (GuardStage::Reviewed, GuardStage::Shadow)
            | (GuardStage::Shadow, GuardStage::Advisory)
            | (GuardStage::Advisory, GuardStage::ScopedBlocking)
    );
    if !normal {
        reasons.push("stage_transition_not_sequential".into());
    }
    if r.current_scope != r.proposed_scope {
        reasons.push("scope_change_requires_separate_review".into());
    }
    if !bounded_id(&r.rollback_ref) {
        reasons.push("rollback_unavailable".into());
    }
    let mut ids = BTreeSet::new();
    for e in &r.evidence {
        if !bounded_id(&e.receipt_id)
            || !valid_digest(&e.receipt_sha256)
            || !valid_digest(&e.subject_sha256)
            || !ids.insert(&e.receipt_id)
        {
            return Err("invalid/duplicate guard evidence".into());
        }
    }
    for kind in [
        "review",
        "detector",
        "attribution",
        "target",
        "host_configuration",
    ] {
        let subject = match kind {
            "target" => &r.target_sha256,
            "host_configuration" => &r.host_configuration_sha256,
            _ => &r.mitigation_sha256,
        };
        if !r.evidence.iter().any(|e| {
            e.kind == kind
                && e.passed
                && &e.subject_sha256 == subject
                && e.scope == r.proposed_scope
                && e.valid_until_ms > r.now_ms
        }) {
            reasons.push(format!("missing_current_{kind}_evidence"));
        }
    }
    if r.proposed_stage != GuardStage::Shadow {
        let prior = if r.proposed_stage == GuardStage::Advisory {
            "shadow_evaluation"
        } else {
            "advisory_evaluation"
        };
        if !r.evidence.iter().any(|e| {
            e.kind == prior
                && e.passed
                && e.subject_sha256 == r.mitigation_sha256
                && e.scope == r.proposed_scope
                && e.valid_until_ms > r.now_ms
        }) {
            reasons.push(format!("missing_current_{prior}"));
        }
        if r.minimum_exposures == 0
            || r.evaluated_exposures < r.minimum_exposures
            || r.evaluated_exposures != r.comparable_exposures
        {
            reasons.push("insufficient_comparable_coverage".into());
        }
        if (r.false_blocks as u128) * 10000
            > (r.maximum_false_block_bps as u128) * (r.evaluated_exposures as u128)
        {
            reasons.push("false_block_limit_exceeded".into());
        }
    }
    let mut out = GuardEligibilityV1 {
        contract: "adapt.guard-eligibility.v1".into(),
        request_sha256: digest(r),
        eligible: reasons.is_empty(),
        reasons,
        proposed_stage: r.proposed_stage,
        host_authorization_required: true,
        activation_authorized: false,
        evidence_basis: "host_supplied_qualification_receipts".into(),
        decision_sha256: String::new(),
    };
    out.decision_sha256 = digest(&out);
    Ok(out)
}
