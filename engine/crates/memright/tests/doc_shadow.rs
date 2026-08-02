use memright::doc_shadow::{
    evaluate_frozen_shadow_replay, evaluate_shadow_replay, select_doc_candidates_for_shadow,
    DocCandidateFreshnessV1, DocCandidateProviderCandidateV1, DocCandidateProviderPolicyV1,
    DocTaskClassV1, DocumentClass, FrozenShadowReplayReceiptV1, ReplayCandidateV1,
    ShadowReplayCaseV1, ShadowReplayDisposition,
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
            baseline: vec![
                candidate("old", None, DocumentClass::Knowledge),
                candidate("runbook", Some("install"), DocumentClass::Runbook),
            ],
            with_docs: vec![
                candidate("runbook", Some("install"), DocumentClass::Runbook),
                candidate("old", None, DocumentClass::Knowledge),
            ],
        },
        ShadowReplayCaseV1 {
            expected_doc_id: "decision".into(),
            expected_section_id: Some("record".into()),
            baseline: vec![candidate(
                "decision",
                Some("other"),
                DocumentClass::Decision,
            )],
            with_docs: vec![candidate(
                "decision",
                Some("record"),
                DocumentClass::Decision,
            )],
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
    assert_eq!(
        first.disposition,
        ShadowReplayDisposition::NarrowToRunbookAndDecision
    );

    let retry = evaluate_shadow_replay(&[bad_case]);
    assert_eq!(
        retry.disposition,
        ShadowReplayDisposition::NarrowToRunbookAndDecision
    );
    assert_eq!(
        retry.retry_after_narrowing().disposition,
        ShadowReplayDisposition::RegistrationOnly
    );
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
        with_docs: vec![
            candidate("known-good", None, DocumentClass::Knowledge),
            superseded,
            duplicate,
        ],
    }]);

    assert_eq!(report.superseded_leakage_count, 1);
    assert_eq!(report.duplicate_leakage_count, 1);
    assert_eq!(
        report.disposition,
        ShadowReplayDisposition::NarrowToRunbookAndDecision
    );
}

fn provider_candidate(
    doc_id: &str,
    class: DocumentClass,
    hash: &str,
    revision: &str,
    generation: i64,
) -> DocCandidateProviderCandidateV1 {
    DocCandidateProviderCandidateV1 {
        doc_id: doc_id.into(),
        document_class: class,
        freshness: DocCandidateFreshnessV1 {
            content_hash: hash.into(),
            revision: revision.into(),
            index_generation: generation,
        },
    }
}

#[test]
fn frozen_replay_receipt_round_trips_cases_and_report() {
    let receipt = evaluate_frozen_shadow_replay(vec![ShadowReplayCaseV1 {
        expected_doc_id: "runbook".into(),
        expected_section_id: Some("install".into()),
        baseline: vec![candidate(
            "runbook",
            Some("install"),
            DocumentClass::Runbook,
        )],
        with_docs: vec![candidate(
            "runbook",
            Some("install"),
            DocumentClass::Runbook,
        )],
    }]);

    let serialized = serde_json::to_string(&receipt).expect("receipt serializes");
    let restored: FrozenShadowReplayReceiptV1 =
        serde_json::from_str(&serialized).expect("receipt deserializes");
    assert_eq!(restored, receipt);
    assert_eq!(restored.schema_version, "memright.doc_shadow_receipt.v1");
    assert_eq!(
        restored.report.disposition,
        ShadowReplayDisposition::ShadowOnly
    );
}

#[test]
fn frozen_replay_receipt_is_byte_deterministic_when_case_input_order_changes() {
    let runbook = ShadowReplayCaseV1 {
        expected_doc_id: "runbook".into(),
        expected_section_id: Some("install".into()),
        baseline: vec![candidate(
            "runbook",
            Some("install"),
            DocumentClass::Runbook,
        )],
        with_docs: vec![candidate(
            "runbook",
            Some("install"),
            DocumentClass::Runbook,
        )],
    };
    let decision = ShadowReplayCaseV1 {
        expected_doc_id: "decision".into(),
        expected_section_id: Some("record".into()),
        baseline: vec![candidate(
            "decision",
            Some("record"),
            DocumentClass::Decision,
        )],
        with_docs: vec![candidate(
            "decision",
            Some("record"),
            DocumentClass::Decision,
        )],
    };

    let forward = evaluate_frozen_shadow_replay(vec![runbook.clone(), decision.clone()]);
    let reversed = evaluate_frozen_shadow_replay(vec![decision, runbook]);

    assert_eq!(
        serde_json::to_vec(&forward).expect("forward receipt serializes"),
        serde_json::to_vec(&reversed).expect("reversed receipt serializes"),
        "frozen replay evidence must not depend on input enumeration order"
    );
}

#[test]
fn frozen_replay_receipt_is_byte_deterministic_for_distinct_cases_with_same_target() {
    let exact_section = ShadowReplayCaseV1 {
        expected_doc_id: "runbook".into(),
        expected_section_id: Some("install".into()),
        baseline: vec![candidate(
            "runbook",
            Some("install"),
            DocumentClass::Runbook,
        )],
        with_docs: vec![candidate(
            "runbook",
            Some("install"),
            DocumentClass::Runbook,
        )],
    };
    let displaced_section = ShadowReplayCaseV1 {
        expected_doc_id: "runbook".into(),
        expected_section_id: Some("install".into()),
        baseline: vec![candidate(
            "runbook",
            Some("install"),
            DocumentClass::Runbook,
        )],
        with_docs: vec![
            candidate("other", None, DocumentClass::Knowledge),
            candidate("runbook", Some("install"), DocumentClass::Runbook),
        ],
    };

    let forward =
        evaluate_frozen_shadow_replay(vec![exact_section.clone(), displaced_section.clone()]);
    let reversed = evaluate_frozen_shadow_replay(vec![displaced_section, exact_section]);

    assert_eq!(
        serde_json::to_vec(&forward).expect("forward receipt serializes"),
        serde_json::to_vec(&reversed).expect("reversed receipt serializes"),
        "frozen replay evidence must canonically order distinct cases sharing one target"
    );
}

#[test]
fn provider_policy_keeps_fresh_task_aligned_candidates_in_shadow_with_two_doc_cap() {
    let current = DocCandidateFreshnessV1 {
        content_hash: "sha256:current".into(),
        revision: "main@123".into(),
        index_generation: 7,
    };
    let selected = select_doc_candidates_for_shadow(
        &DocCandidateProviderPolicyV1::default(),
        DocTaskClassV1::Runbook,
        &current,
        &[
            provider_candidate(
                "runbook-1",
                DocumentClass::Runbook,
                "sha256:current",
                "main@123",
                7,
            ),
            provider_candidate(
                "decision-1",
                DocumentClass::Decision,
                "sha256:current",
                "main@123",
                7,
            ),
            provider_candidate(
                "runbook-2",
                DocumentClass::Runbook,
                "sha256:current",
                "main@123",
                7,
            ),
            provider_candidate(
                "content",
                DocumentClass::Content,
                "sha256:current",
                "main@123",
                7,
            ),
            provider_candidate("stale", DocumentClass::Runbook, "sha256:old", "main@123", 7),
        ],
    );

    assert!(selected.shadow_only);
    assert_eq!(
        selected
            .candidates
            .iter()
            .map(|candidate| candidate.doc_id.as_str())
            .collect::<Vec<_>>(),
        vec!["runbook-1", "decision-1"]
    );
}

#[test]
fn provider_policy_applies_task_class_filters_and_exact_freshness() {
    let current = DocCandidateFreshnessV1 {
        content_hash: "sha256:current".into(),
        revision: "main@123".into(),
        index_generation: 7,
    };
    let candidates = [
        provider_candidate(
            "wrong-hash",
            DocumentClass::Knowledge,
            "sha256:old",
            "main@123",
            7,
        ),
        provider_candidate(
            "wrong-revision",
            DocumentClass::Knowledge,
            "sha256:current",
            "main@124",
            7,
        ),
        provider_candidate(
            "wrong-generation",
            DocumentClass::Knowledge,
            "sha256:current",
            "main@123",
            8,
        ),
        provider_candidate(
            "knowledge",
            DocumentClass::Knowledge,
            "sha256:current",
            "main@123",
            7,
        ),
        provider_candidate(
            "decision",
            DocumentClass::Decision,
            "sha256:current",
            "main@123",
            7,
        ),
        provider_candidate(
            "runbook",
            DocumentClass::Runbook,
            "sha256:current",
            "main@123",
            7,
        ),
    ];

    let lookup = select_doc_candidates_for_shadow(
        &DocCandidateProviderPolicyV1::default(),
        DocTaskClassV1::DocLookup,
        &current,
        &candidates,
    );
    let orientation = select_doc_candidates_for_shadow(
        &DocCandidateProviderPolicyV1::default(),
        DocTaskClassV1::Orientation,
        &current,
        &candidates,
    );

    assert_eq!(lookup.candidates.len(), 2);
    assert_eq!(
        orientation
            .candidates
            .iter()
            .map(|candidate| candidate.doc_id.as_str())
            .collect::<Vec<_>>(),
        vec!["knowledge", "decision"]
    );
}
