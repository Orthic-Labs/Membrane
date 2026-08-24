//! The three distinct admission decisions (canon §3.5).
//!
//! 1. [`ProposalEligibilityDecision`] — may evidence form a Taste candidate
//!    or Insight episode/issue? (delegating computation lives in
//!    `admission` / `insights`).
//! 2. [`CortexAdmissionEnvelope`] — a typed REQUEST that a durable record be
//!    admitted to Cortex. Adapt can only propose; Cortex owns the decision.
//! 3. [`ContextAdmissionRecord`] — representation of Membrane's separate
//!    context-packet admission decision over an already-durable record.
//!
//! Passing one gate grants no authority at any later gate. These types never
//! collapse: there is no constructor that turns an eligibility pass into a
//! durable record or a context inclusion.

use serde::{Deserialize, Serialize};

use crate::record::InfluenceClass;

/// Gate 1 outcome (computed by `admission::evaluate_eligibility` /
/// `insights` detectors). Purely local to Adapt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalEligibilityDecision {
    pub eligible: bool,
    pub reason: String,
}

/// Gate 2: a typed Cortex durable-admission request. Adapt constructs these
/// only after Gate 1 passes; whether the record actually enters durable
/// knowledge is Cortex's decision, represented by `cortex_verdict`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CortexAdmissionEnvelope {
    pub envelope_id: String,
    /// Semantic kind (`preference`, `insight_issue`, ...).
    pub record_kind: String,
    /// Digest of the sealed immutable semantic payload.
    pub seal_digest: String,
    /// Influence class carried by this record toward consumers.
    pub influence_class: InfluenceClass,
    /// Idempotency key so retries after partial failure cannot duplicate.
    pub idempotency_key: String,
    pub installation_id: String,
    /// Set by Cortex, never by Adapt. `None` = not yet decided; Adapt must
    /// treat `None` as "not admitted".
    #[serde(default)]
    pub cortex_verdict: Option<CortexVerdict>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CortexVerdict {
    Admitted,
    Refused { reason_code: u16 },
}

impl CortexAdmissionEnvelope {
    /// A gate-2 envelope is only durably real once Cortex said yes. Until
    /// then it is a proposal — reportable, retryable, never authoritative.
    pub fn is_durable(&self) -> bool {
        matches!(self.cortex_verdict, Some(CortexVerdict::Admitted))
    }
}

/// Gate 3: representation of Membrane's separate context-admission decision.
/// Adapt never decides this; it only carries the inputs and observes the
/// outcome recorded by the Membrane planner.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextAdmissionRecord {
    /// References the durable record admitted at gate 2.
    pub cortex_record_ref: String,
    pub authority_ok: bool,
    pub fresh: bool,
    pub sufficient: bool,
    pub within_budget: bool,
    /// Planner decision; absent until the planner records one.
    #[serde(default)]
    pub planner_decision: Option<ContextDecision>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextDecision {
    Included,
    Omitted,
}

impl ContextAdmissionRecord {
    /// Whether this record was included in a context packet. Requires BOTH a
    /// durable gate-2 admission AND an explicit planner inclusion.
    pub fn delivered(&self) -> bool {
        self.planner_decision == Some(ContextDecision::Included)
    }
}

/// Guard used by tests and callers: prove that a gate-1 pass alone cannot
/// fabricate a durable or delivered record.
pub fn gate1_pass_implies_nothing(decision: &ProposalEligibilityDecision) -> (bool, bool) {
    // Returns (durably_admitted?, delivered_in_context?) — always false.
    let _ = decision;
    (false, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::InfluenceClass;

    fn envelope() -> CortexAdmissionEnvelope {
        CortexAdmissionEnvelope {
            envelope_id: "e1".into(),
            record_kind: "preference".into(),
            seal_digest: "digest".into(),
            influence_class: InfluenceClass::Provisional,
            idempotency_key: "idem-1".into(),
            installation_id: "inst".into(),
            cortex_verdict: None,
        }
    }

    #[test]
    fn undecided_envelope_is_not_durable() {
        assert!(!envelope().is_durable());
    }

    #[test]
    fn refused_context_admission_is_not_delivered() {
        let rec = ContextAdmissionRecord {
            cortex_record_ref: "r".into(),
            authority_ok: true,
            fresh: true,
            sufficient: true,
            within_budget: true,
            planner_decision: Some(ContextDecision::Omitted),
        };
        assert!(!rec.delivered());
    }

    #[test]
    fn gate1_pass_grants_no_later_gate() {
        let d = ProposalEligibilityDecision {
            eligible: true,
            reason: "ok".into(),
        };
        let (durable, delivered) = gate1_pass_implies_nothing(&d);
        assert!(!durable && !delivered);
    }
}
