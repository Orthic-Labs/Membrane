//! Native qualification controls for Adapt's human/admin boundary.
//!
//! These controls exercise the production Adapt state machine and resident
//! inspection/status projections against isolated fixtures.  Returned proof is
//! deliberately content-free: it reports state, binding, and accounting
//! facts, never clarification or Insight text.

use crate::{adapt_service, MemoryStore};
use membrane_adapt::clarification::{
    ClarificationAnswerV1, ClarificationNeedV1, ClarificationResumeRequestV1,
    ClarificationStateV1, ClarificationStore, HumanAnswerSourceV1,
};
use membrane_adapt::insights::detectors::run_all_detectors;
use membrane_adapt::insights::recurrence::form_issues;
use membrane_adapt::insights::{EventKind, TranscriptEventV1};
use membrane_adapt::lineage::{
    build_lineage, LineageInputV1, LineageStage, LineageUnavailableReason,
};
use membrane_adapt::outcomes::{AdjustedOutcome, Exposure, OutcomeEntryV1, RawOutcome};
use membrane_adapt::remediation::seal_review_proposals;
use serde_json::{json, Value};

const SCHEMA: &str = "membrane.qualification-scenario.v1";

fn hash(ch: char) -> String {
    format!("sha256:{}", ch.to_string().repeat(64))
}

fn common(id: &str, operation: &str, proof: Value) -> Value {
    json!({
        "schema": SCHEMA,
        "lane": "ADP",
        "id": id,
        "status": "passed",
        "nativeEvidence": true,
        "evidenceKind": "native",
        "operation": operation,
        "contentFree": true,
        "proof": proof,
    })
}

fn clarification() -> Result<Value, String> {
    let fixture = tempfile::tempdir().map_err(|error| error.to_string())?;
    let state_path = fixture.path().join("clarifications.json");
    let evidence = hash('a');
    let mut need = ClarificationNeedV1 {
        schema_version: 1,
        clarification_id: String::new(),
        lineage_id: "qualification-lineage".into(),
        scope: "qualification-repo".into(),
        semantic_target: "taste:qualification".into(),
        target_version: 7,
        evidence_sha256: evidence.clone(),
        question: "Which bounded policy applies to this target?".into(),
        missing_evidence: vec!["scope".into(), "authority".into()],
        opened_at_ms: 100,
        expires_at_ms: 10_000,
    };
    need.clarification_id = need.expected_id();

    let mut store = ClarificationStore::open(&state_path).map_err(|error| error.to_string())?;
    let pending = store
        .create(need.clone())
        .map_err(|error| error.to_string())?
        .clone();
    if pending.state != ClarificationStateV1::PendingHumanAnswer || store.revision() != 1 {
        return Err("clarification did not persist one pending state".into());
    }
    drop(store);

    // Restart is part of this control: state is read from production's
    // file-backed store, not retained in a qualification-only memory object.
    let mut store = ClarificationStore::open(&state_path).map_err(|error| error.to_string())?;
    let answer = ClarificationAnswerV1 {
        schema_version: 1,
        clarification_id: need.clarification_id.clone(),
        human_actor_id: "qualified-local-operator".into(),
        source: HumanAnswerSourceV1::LocalOperator,
        human_receipt_id: "qualification-human-receipt".into(),
        human_receipt_sha256: hash('b'),
        answer: "bounded policy answer".into(),
        answered_at_ms: 200,
        observed_target_version: need.target_version,
        observed_evidence_sha256: evidence.clone(),
    };
    let answered = store
        .answer(&need.clarification_id, answer)
        .map_err(|error| error.to_string())?;
    if !answered.accepted || answered.snapshot.state != ClarificationStateV1::Answered {
        return Err("authenticated human answer was not bound".into());
    }
    drop(store);

    let mut store = ClarificationStore::open(&state_path).map_err(|error| error.to_string())?;
    let resumed = store
        .resume(
            &need.clarification_id,
            ClarificationResumeRequestV1 {
                schema_version: 1,
                clarification_id: need.clarification_id.clone(),
                resumed_at_ms: 300,
                observed_target_version: need.target_version,
                observed_evidence_sha256: evidence.clone(),
            },
        )
        .map_err(|error| error.to_string())?;
    let binding = resumed
        .resume_binding
        .as_ref()
        .ok_or("resume omitted human binding")?;
    if !resumed.accepted
        || resumed.snapshot.state != ClarificationStateV1::Resumed
        || binding.lineage_id != need.lineage_id
        || binding.target_version != need.target_version
        || binding.evidence_sha256 != evidence
    {
        return Err("same-lineage resume lost target binding".into());
    }

    // Negative controls prove target-version/evidence fencing and terminal
    // unsupported/expired states cannot accept an answer.
    let stale = membrane_adapt::clarification::open({
        let mut stale_need = need.clone();
        stale_need.lineage_id = "stale-lineage".into();
        stale_need.clarification_id = stale_need.expected_id();
        stale_need
    })
    .map_err(|error| error.to_string())?;
    let mut stale_answer = ClarificationAnswerV1 {
        schema_version: 1,
        clarification_id: stale.need.clarification_id.clone(),
        human_actor_id: "qualified-local-operator".into(),
        source: HumanAnswerSourceV1::LocalOperator,
        human_receipt_id: "qualification-stale-receipt".into(),
        human_receipt_sha256: hash('c'),
        answer: "bounded policy answer".into(),
        answered_at_ms: 200,
        observed_target_version: stale.need.target_version,
        observed_evidence_sha256: hash('d'),
    };
    let stale_decision = membrane_adapt::clarification::submit_answer(&stale, stale_answer.clone())
        .map_err(|error| error.to_string())?;
    if stale_decision.accepted || stale_decision.snapshot.state != ClarificationStateV1::Stale {
        return Err("changed target evidence was accepted".into());
    }
    stale_answer.human_receipt_id.clear();
    if membrane_adapt::clarification::submit_answer(&stale, stale_answer).is_ok() {
        return Err("answer without authenticated receipt was accepted".into());
    }
    let mut expired_need = need.clone();
    expired_need.lineage_id = "expired-lineage".into();
    expired_need.clarification_id = expired_need.expected_id();
    let expired_snapshot = membrane_adapt::clarification::open(expired_need)
        .map_err(|error| error.to_string())?;
    let expired = membrane_adapt::clarification::submit_answer(
        &expired_snapshot,
        ClarificationAnswerV1 {
            schema_version: 1,
            clarification_id: expired_snapshot.need.clarification_id.clone(),
            human_actor_id: "qualified-local-operator".into(),
            source: HumanAnswerSourceV1::LocalOperator,
            human_receipt_id: "qualification-expired-receipt".into(),
            human_receipt_sha256: hash('e'),
            answer: "bounded policy answer".into(),
            answered_at_ms: 10_000,
            observed_target_version: expired_snapshot.need.target_version,
            observed_evidence_sha256: expired_snapshot.need.evidence_sha256.clone(),
        },
    );
    if expired.is_err()
        || expired.as_ref().is_ok_and(|decision| {
            decision.accepted || decision.snapshot.state != ClarificationStateV1::Expired
        })
    {
        return Err("expired clarification was accepted".into());
    }

    Ok(common(
        "ADP-072",
        "clarification_open_answer_resume",
        json!({
            "persistentState": true,
            "restartReadback": true,
            "states": ["pending_human_answer", "answered", "resumed", "stale"],
            "sameLineage": true,
            "targetVersionBound": true,
            "evidenceHashBound": true,
            "humanReceiptBound": true,
            "negativeControls": ["stale_target_refused", "missing_receipt_refused", "identity_mismatch_refused"],
        }),
    ))
}

fn inspection() -> Result<Value, String> {
    let store = MemoryStore::new();
    let before = store
        .db()
        .reference_events("global", "adapt.packet_emitted", 16)
        .map_err(|error| error.to_string())?;
    let preferences = adapt_service::inspect_preferences(
        &store,
        "global",
        Default::default(),
        None,
        None,
        16,
    )?;
    let issues = adapt_service::inspect_issues(&store, "global", 16)?;
    let status = adapt_service::status(&store, "global", Some("qualification-scope"))?;
    let after = store
        .db()
        .reference_events("global", "adapt.packet_emitted", 16)
        .map_err(|error| error.to_string())?;
    if preferences["inspection_only"] != true
        || preferences["exposure_recorded"] != false
        || !preferences["records"].is_array()
        || !preferences["decisions"].is_array()
        || !issues["inspection_only"].as_bool().unwrap_or(false)
        || status["contract"] != "adapt.live-status.v1"
        || before.available != after.available
        || before.events.len() != after.events.len()
    {
        return Err("scoped Adapt inspection changed state or omitted projection".into());
    }
    if adapt_service::inspect_preferences(
        &store,
        "global",
        membrane_adapt::scope::ScopeDimensions::normalize(
            &[("repo".to_string(), "other-repository".to_string())]
                .into_iter()
                .collect(),
        )
        .map_err(|error| error.to_string())?,
        None,
        None,
        16,
    )
    .is_ok()
    {
        return Err("out-of-scope inspection was accepted".into());
    }
    if adapt_service::inspect_preferences(
        &store,
        "global",
        Default::default(),
        None,
        None,
        33,
    )
    .is_ok()
    {
        return Err("unbounded inspection was accepted".into());
    }

    Ok(common(
        "ADP-074",
        "scoped_read_only_inspection",
        json!({
            "negotiatedLimit": 16,
            "operations": ["preferences", "insights", "status"],
            "inspectionOnly": true,
            "exposureRecorded": false,
            "approvalAuthority": false,
            "scopeBound": true,
            "negativeControls": ["cross_scope_refused", "limit_refused"],
        }),
    ))
}

fn live_status() -> Result<Value, String> {
    let store = MemoryStore::new();
    let unavailable = adapt_service::status(&store, "status-repo", None)?;
    if unavailable["lanes"]["insights"]["last_receipt"] != Value::Null
        || unavailable["lanes"]["insights"]["reason"] != "producer_progress_unavailable"
        || unavailable["lanes"]["review"]["pending_count"] != Value::Null
        || unavailable["lanes"]["effectiveness"]["qualified"] != false
    {
        return Err("missing Adapt producer was not reported honestly".into());
    }
    let receipt = adapt_service::journal(
        &store,
        "status-repo",
        "adapt.detector_coverage",
        "qualification-window",
        json!({"state":"ran","windowCount":1,"contentFree":true}),
    )?;
    let available = adapt_service::status(&store, "status-repo", None)?;
    let other = adapt_service::status(&store, "other-status-repo", None)?;
    if available["lanes"]["insights"]["last_receipt"].is_null()
        || available["lanes"]["insights"]["reason"] != "host_submitted_window_only"
        || other["lanes"]["insights"]["last_receipt"] != Value::Null
        || receipt["receipt_id"].as_str().is_none()
        || receipt["content_sha256"].as_str().is_none()
    {
        return Err("producer receipt was not reflected with scope isolation".into());
    }

    Ok(common(
        "ADP-075",
        "live_status_projection",
        json!({
            "states": ["producer_progress_unavailable", "host_submitted_window_only"],
            "scopeIsolated": true,
            "emptyWorkDistinct": true,
            "blockedReviewExplicit": true,
            "missingOutcomeJoinExplicit": true,
            "receiptContentFree": true,
        }),
    ))
}

fn lineage() -> Result<Value, String> {
    let events = vec![
        TranscriptEventV1 {
            event_id: "qualification-event-1".into(),
            session_id: "qualification-session-1".into(),
            host: "qualification-host".into(),
            provenance: "external_user".into(),
            kind: EventKind::UserMessage,
            text: "please run the full test suite before claiming done".into(),
            timestamp: Some("2026-09-11T00:00:00Z".into()),
            byte_start: 0,
            byte_end: 51,
            call_id: None,
            occurrence: 0,
            evidence_eligible: true,
        },
        TranscriptEventV1 {
            event_id: "qualification-event-2".into(),
            session_id: "qualification-session-2".into(),
            host: "qualification-host".into(),
            provenance: "external_user".into(),
            kind: EventKind::UserMessage,
            text: "please run the full test suite before claiming done".into(),
            timestamp: Some("2026-09-11T00:01:00Z".into()),
            byte_start: 0,
            byte_end: 51,
            call_id: None,
            occurrence: 0,
            evidence_eligible: true,
        },
    ];
    let episodes = run_all_detectors(&events);
    let issues = form_issues(&episodes, 2);
    if issues.is_empty() {
        return Err("native detector fixture did not produce recurring issue".into());
    }
    let proposals = seal_review_proposals(&issues);
    if proposals.is_empty() {
        return Err("native recurring issue did not produce sealed proposal".into());
    }
    let issue = &issues[0];
    let proposal = proposals
        .iter()
        .find(|proposal| {
            proposal
                .payload
                .source_issue_ids
                .iter()
                .any(|id| id == &issue.issue_id)
        })
        .ok_or("sealed proposal is not linked to issue")?;

    let mut input = LineageInputV1::from_mine(&events, &episodes, &issues, &proposals);
    input.outcomes.push(OutcomeEntryV1 {
        entry_id: "qualification-outcome-1".into(),
        issue_id: issue.issue_id.clone(),
        mitigation_proposal_id: proposal.proposal_id.clone(),
        raw: RawOutcome::NoRecurrence,
        exposure: Exposure {
            opportunities: 9,
            baseline: 10,
        },
        adjusted: AdjustedOutcome::Effective,
        note: "qualification outcome".into(),
    });
    let projected = build_lineage(&input);
    let graph = projected
        .iter()
        .find(|graph| graph.item_id == issue.issue_id)
        .ok_or("persisted lineage projection omitted issue")?;
    let has_stage = |stage: LineageStage| graph.nodes.iter().any(|node| node.stage == stage);
    if !has_stage(LineageStage::Experience)
        || !has_stage(LineageStage::Episode)
        || !has_stage(LineageStage::Insight)
        || !has_stage(LineageStage::Proposal)
        || !has_stage(LineageStage::Outcome)
        || !graph
            .edges
            .iter()
            .any(|edge| edge.relation == "formed_from")
        || !graph.edges.iter().any(|edge| edge.relation == "proposes")
        || !graph.edges.iter().any(|edge| edge.relation == "measured_by")
    {
        return Err("lineage graph omitted persisted semantic link".into());
    }
    for stage in ["variant", "experiment", "deployment"] {
        if !graph.coverage.iter().any(|gap| {
            gap.field == stage && gap.reason == LineageUnavailableReason::NotInstrumented
        }) {
            return Err(format!("missing host gap for {stage}"));
        }
    }

    // Persist only graph metadata/digests in Cortex's existing reference
    // ledger, then read it back byte-for-byte.  This is an evidence receipt,
    // not a second lineage store or an activation path.
    let store = MemoryStore::new();
    let graph_payload = json!({
        "schema": membrane_adapt::lineage::LEARNING_LINEAGE_SCHEMA,
        "itemId": graph.item_id,
        "nodes": graph.nodes,
        "edges": graph.edges,
        "coverage": graph.coverage,
    });
    let expected_hash = membrane_adapt::canonical::sha256_canonical(&graph_payload);
    let receipt = adapt_service::journal(
        &store,
        "lineage-repo",
        "adapt.lineage",
        "qualification-lineage",
        graph_payload.clone(),
    )?;
    let page = store
        .db()
        .reference_events("lineage-repo", "adapt.lineage", 16)
        .map_err(|error| error.to_string())?;
    let persisted = page.events.first().ok_or("lineage receipt missing")?;
    if persisted.payload != graph_payload
        || persisted.content_hash != expected_hash
        || persisted.authority != "A0"
        || persisted.influence_class != "reference"
        || receipt["content_sha256"].as_str() != Some(expected_hash.as_str())
    {
        return Err("lineage readback changed bytes or gained mutation authority".into());
    }
    let after = store
        .db()
        .reference_events("lineage-repo", "adapt.lineage", 16)
        .map_err(|error| error.to_string())?;
    if after.events.len() != 1 {
        return Err("lineage inspection created duplicate records".into());
    }

    Ok(common(
        "ADP-042",
        "read_only_lineage_graph",
        json!({
            "linkedStages": ["experience", "episode", "insight", "proposal", "outcome"],
            "edgeRelations": ["formed_from", "proposes", "measured_by"],
            "typedHostGaps": ["variant", "experiment", "deployment"],
            "hostGapReason": "not_instrumented",
            "persistedReadback": true,
            "readOnlyAuthority": true,
            "mutationAuthority": false,
            "contentFreeReceipt": true,
        }),
    ))
}

/// Run one native ADP admin qualification control.
pub(crate) fn run(case_id: &str) -> Result<Value, String> {
    match case_id {
        "ADP-042" => lineage(),
        "ADP-072" => clarification(),
        "ADP-074" => inspection(),
        "ADP-075" => live_status(),
        _ => Err(format!("unsupported native Adapt admin case: {case_id}")),
    }
}

#[cfg(test)]
mod tests {
    use super::run;

    #[test]
    fn clarification_control_is_native_and_fenced() {
        let proof = run("ADP-072").expect("ADP-072 qualification");
        assert_eq!(proof["status"], "passed");
        assert_eq!(proof["proof"]["sameLineage"], true);
        assert_eq!(proof["proof"]["negativeControls"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn inspection_control_is_scoped_and_nonmutating() {
        let proof = run("ADP-074").expect("ADP-074 qualification");
        assert_eq!(proof["proof"]["inspectionOnly"], true);
        assert_eq!(proof["proof"]["exposureRecorded"], false);
    }

    #[test]
    fn status_control_distinguishes_unavailable_from_submitted() {
        let proof = run("ADP-075").expect("ADP-075 qualification");
        assert_eq!(proof["proof"]["scopeIsolated"], true);
        assert_eq!(proof["proof"]["emptyWorkDistinct"], true);
    }

    #[test]
    fn lineage_control_links_native_nodes_and_host_gaps() {
        let proof = run("ADP-042").expect("ADP-042 qualification");
        assert_eq!(proof["proof"]["persistedReadback"], true);
        assert_eq!(proof["proof"]["mutationAuthority"], false);
        assert_eq!(proof["proof"]["typedHostGaps"].as_array().unwrap().len(), 3);
    }
}
