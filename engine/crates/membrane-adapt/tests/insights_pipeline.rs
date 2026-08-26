//! Scenarios 4: insights — detectors, guards, deterministic recurrence,
//! hybrid merge proposals that never auto-admit.

use membrane_adapt::insights::detectors::{repeated_ask_signature, run_all_detectors};
use membrane_adapt::insights::guards::{guard_user_span, has_positive_verification_claim};
use membrane_adapt::insights::recurrence::{
    form_issues, mine_issues, transition_issue, HybridMergeProposal,
};
use membrane_adapt::insights::{EventKind, IssueState, TranscriptEventV1};
use membrane_adapt::model_boundary::ModelProposalError;

fn ev(id: &str, session: &str, provenance: &str, kind: EventKind, text: &str) -> TranscriptEventV1 {
    TranscriptEventV1 {
        event_id: id.into(),
        session_id: session.into(),
        host: "pi".into(),
        provenance: provenance.into(),
        kind,
        text: text.into(),
        timestamp: Some("2026-08-24T01:00:00Z".into()),
        byte_start: 0,
        byte_end: text.len() as i64,
        call_id: None,
        occurrence: 0,
        evidence_eligible: true,
    }
}

fn user(id: &str, session: &str, text: &str) -> TranscriptEventV1 {
    ev(id, session, "external_user", EventKind::UserMessage, text)
}

#[test]
fn guards_suppress_quoted_tool_hypothetical_spans() {
    let quoted = user("q", "s", r#"the log said "verified passing done green""#);
    assert!(matches!(
        guard_user_span(&quoted),
        membrane_adapt::insights::guards::SpanGuard::Suppress("quoted")
    ));
    let tool = ev(
        "t",
        "s",
        "tool",
        EventKind::ToolResult,
        "error: tests failed",
    );
    assert!(matches!(
        guard_user_span(&tool),
        membrane_adapt::insights::guards::SpanGuard::Suppress("tool-carried")
    ));
    let hypothetical = user(
        "h",
        "s",
        "suppose you had verified everything and it works, what next",
    );
    assert!(matches!(
        guard_user_span(&hypothetical),
        membrane_adapt::insights::guards::SpanGuard::Suppress("hypothetical")
    ));
}

#[test]
fn negated_claims_are_not_positive() {
    assert!(has_positive_verification_claim("It is not verified yet").is_empty());
    assert!(!has_positive_verification_claim("I verified the fix works").is_empty());
}

#[test]
fn ignored_tool_failure_is_detected() {
    let events = vec![
        user("a", "s", "run the tests"),
        ev("b", "s", "assistant", EventKind::ToolCall, "cargo test"),
        ev(
            "c",
            "s",
            "tool",
            EventKind::ToolResult,
            "error: test failed",
        ),
        ev(
            "d",
            "s",
            "assistant",
            EventKind::AssistantMessage,
            "All done — I verified the fix works.",
        ),
    ];
    let eps = run_all_detectors(&events);
    assert!(eps.iter().any(|e| e.family == "ignored_tool_failure"));
}

#[test]
fn recurrence_is_deterministic_and_cross_session() {
    let events = vec![
        user(
            "a",
            "s1",
            "please run the full test suite before claiming done",
        ),
        user(
            "b",
            "s2",
            "Please run the FULL test suite before claiming done.",
        ),
    ];
    let issues1 = mine_issues(&events, 2);
    let issues2 = mine_issues(
        &events
            .clone()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .as_slice(),
        2,
    );
    // Order-independent convergence of issue formation.
    let mut a: Vec<String> = issues1.iter().map(serialize).collect();
    let mut b: Vec<String> = issues2.iter().map(serialize).collect();
    a.sort();
    b.sort();
    assert_eq!(a, b);
    assert!(issues1.iter().any(|i| i.family == "repeated_ask"));
    assert_eq!(issues1[0].state, IssueState::Observed);
}

fn serialize(i: &membrane_adapt::insights::InsightIssueV1) -> String {
    serde_json::to_string(i).unwrap()
}

#[test]
fn single_episode_never_forms_an_issue() {
    let events = vec![user(
        "a",
        "s1",
        "please run the full test suite before claiming done",
    )];
    assert!(mine_issues(&events, 2).is_empty());
}

#[test]
fn hybrid_merge_proposals_verify_but_never_auto_admit() {
    let events = vec![
        user(
            "a",
            "s1",
            "please run the full test suite before claiming done",
        ),
        user(
            "b",
            "s2",
            "Please run the FULL test suite before claiming done.",
        ),
    ];
    let eps = run_all_detectors(&events);
    let proposal = HybridMergeProposal {
        proposer_id: "model".into(),
        target_issue_id: None,
        episode_ids: eps.iter().map(|e| e.episode_id.clone()).collect(),
        rationale: "same theme".into(),
    };
    // Verified = validated plan only. No admission API is even reachable.
    assert!(proposal.verify(&eps, &[]).is_ok());
    let phantom = HybridMergeProposal {
        proposer_id: "model".into(),
        target_issue_id: None,
        episode_ids: vec!["ghost-episode".into()],
        rationale: "".into(),
    };
    assert_eq!(
        phantom.verify(&eps, &[]),
        Err(ModelProposalError::UnboundEvidence)
    );
}

#[test]
fn illegal_issue_transitions_are_refused() {
    let events = vec![
        user(
            "a",
            "s1",
            "please run the full test suite before claiming done",
        ),
        user(
            "b",
            "s2",
            "Please run the FULL test suite before claiming done.",
        ),
    ];
    let issues = form_issues(&run_all_detectors(&events), 2);
    let observed = &issues[0];
    // Observed cannot jump to Mitigated.
    assert!(transition_issue(observed, IssueState::Mitigated).is_err());
    assert!(transition_issue(observed, IssueState::Recurring).is_ok());
}

#[test]
fn signature_normalization_ignores_politeness_and_case() {
    assert_eq!(
        repeated_ask_signature("Please run the tests now"),
        repeated_ask_signature("please run the tests NOW")
    );
}
