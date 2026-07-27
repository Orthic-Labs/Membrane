use memright::doc_shadow::{
    evaluate_shadow_replay, DocumentClass, ReplayCandidateV1, ShadowReplayCaseV1,
    ShadowReplayDisposition,
};

fn candidate(doc_id: &str, section_id: Option<&str>, class: DocumentClass) -> ReplayCandidateV1 {
    ReplayCandidateV1 {
        doc_id: doc_id.into(),
        section_id: section_id.map(str::to_owned),
        document_class: class,
        superseded: false,
        duplicate: false,
    }
}

#[test]
fn reports_baseline_and_doc_candidate_quality_metrics() {
    let report = evaluate_shadow_replay(&[
        ShadowReplayCaseV1 {
            expected_doc_id: "runbook".into(),
            expected_section_id: Some("install".into()),
            baseline: vec![candidate("old", None, DocumentClass::Knowledge), candidate("runbook", Some("install"), DocumentClass::Runbook)],
            with_docs: vec![candidate("runbook", Some("install"), DocumentClass::Runbook), candidate("old", None, DocumentClass::Knowledge)],
        },
        ShadowReplayCaseV1 {
            expected_doc_id: "decision".into(),
            expected_section_id: Some("record".into()),
            baseline: vec![candidate("decision", Some("other"), DocumentClass::Decision)],
            with_docs: vec![candidate("decision", Some("record"), DocumentClass::Decision)],
        },
    ]);

    assert_eq!(report.baseline.mean_rank, 1.5);
    assert_eq!(report.with_docs.mean_rank, 1.0);
    assert_eq!(report.baseline.correct_doc_rate, 1.0);
    assert_eq!(report.baseline.correct_section_rate, 0.5);
    assert_eq!(report.with_docs.correct_doc_rate, 1.0);
    assert_eq!(report.with_docs.correct_section_rate, 1.0);
    assert_eq!(report.displacement_count, 0);
    assert_eq!(report.superseded_leakage_count, 0);
    assert_eq!(report.duplicate_leakage_count, 0);
    assert_eq!(report.disposition, ShadowReplayDisposition::ShadowOnly);
}

#[test]
fn conservative_fallback_narrows_once_then_stays_registration_only() {
    let bad_case = ShadowReplayCaseV1 {
        expected_doc_id: "known-good".into(),
        expected_section_id: None,
        baseline: vec![candidate("known-good", None, DocumentClass::Knowledge)],
        with_docs: vec![
            candidate("bad-doc", None, DocumentClass::Content),
            candidate("known-good", None, DocumentClass::Knowledge),
        ],
    };
    let first = evaluate_shadow_replay(&[bad_case.clone()]);
    assert_eq!(first.displacement_count, 1);
    assert_eq!(first.disposition, ShadowReplayDisposition::NarrowToRunbookAndDecision);

    let retry = evaluate_shadow_replay(&[bad_case]);
    assert_eq!(retry.disposition, ShadowReplayDisposition::NarrowToRunbookAndDecision);
    assert_eq!(retry.retry_after_narrowing().disposition, ShadowReplayDisposition::RegistrationOnly);
}

#[test]
fn detects_superseded_and_duplicate_candidates_as_safety_leakage() {
    let mut superseded = candidate("old", None, DocumentClass::Decision);
    superseded.superseded = true;
    let mut duplicate = candidate("same", None, DocumentClass::Runbook);
    duplicate.duplicate = true;

    let report = evaluate_shadow_replay(&[ShadowReplayCaseV1 {
        expected_doc_id: "known-good".into(),
        expected_section_id: None,
        baseline: vec![candidate("known-good", None, DocumentClass::Knowledge)],
        with_docs: vec![candidate("known-good", None, DocumentClass::Knowledge), superseded, duplicate],
    }]);

    assert_eq!(report.superseded_leakage_count, 1);
    assert_eq!(report.duplicate_leakage_count, 1);
    assert_eq!(report.disposition, ShadowReplayDisposition::NarrowToRunbookAndDecision);
}
