use membrane_adapt::clarification::*;

fn h(c: char) -> String {
    c.to_string().repeat(64)
}

fn need() -> ClarificationNeedV1 {
    let mut need = ClarificationNeedV1 {
        schema_version: 1,
        clarification_id: String::new(),
        lineage_id: "lineage-1".into(),
        scope: "repo".into(),
        semantic_target: "taste:verification-style".into(),
        target_version: 7,
        evidence_sha256: h('a'),
        question: "Should verification run before every completion claim?".into(),
        missing_evidence: vec!["preference_scope".into()],
        opened_at_ms: 100,
        expires_at_ms: 10_000,
    };
    need.clarification_id = need.expected_id();
    need
}

fn answer(need: &ClarificationNeedV1) -> ClarificationAnswerV1 {
    ClarificationAnswerV1 {
        schema_version: 1,
        clarification_id: need.clarification_id.clone(),
        human_actor_id: "local-user".into(),
        source: HumanAnswerSourceV1::LocalOperator,
        human_receipt_id: "operator-receipt-1".into(),
        human_receipt_sha256: h('b'),
        answer: "Yes, for completion claims in this repository.".into(),
        answered_at_ms: 200,
        observed_target_version: need.target_version,
        observed_evidence_sha256: need.evidence_sha256.clone(),
    }
}

#[test]
fn clarification_binds_one_answer_and_same_lineage_resume() {
    let opened = open(need()).unwrap();
    assert_eq!(opened.state, ClarificationStateV1::PendingHumanAnswer);

    let answered = submit_answer(&opened, answer(&opened.need)).unwrap();
    assert!(answered.accepted);
    assert_eq!(answered.snapshot.state, ClarificationStateV1::Answered);

    let resumed = resume(
        &answered.snapshot,
        ClarificationResumeRequestV1 {
            schema_version: 1,
            clarification_id: opened.need.clarification_id.clone(),
            resumed_at_ms: 300,
            observed_target_version: opened.need.target_version,
            observed_evidence_sha256: opened.need.evidence_sha256.clone(),
        },
    )
    .unwrap();
    assert!(resumed.accepted);
    assert_eq!(resumed.snapshot.state, ClarificationStateV1::Resumed);
    let binding = resumed.resume_binding.unwrap();
    assert_eq!(binding.lineage_id, opened.need.lineage_id);
    assert_eq!(binding.target_version, 7);
    assert_eq!(binding.human_receipt_id, "operator-receipt-1");
}

#[test]
fn stale_evidence_cannot_resume_or_rebind_answer() {
    let opened = open(need()).unwrap();
    let mut stale_answer = answer(&opened.need);
    stale_answer.observed_evidence_sha256 = h('c');
    let decision = submit_answer(&opened, stale_answer).unwrap();
    assert!(!decision.accepted);
    assert_eq!(decision.snapshot.state, ClarificationStateV1::Stale);

    let resumed = resume(
        &decision.snapshot,
        ClarificationResumeRequestV1 {
            schema_version: 1,
            clarification_id: opened.need.clarification_id,
            resumed_at_ms: 300,
            observed_target_version: 7,
            observed_evidence_sha256: h('a'),
        },
    )
    .unwrap();
    assert!(!resumed.accepted);
    assert_eq!(resumed.snapshot.state, ClarificationStateV1::Stale);
}

#[test]
fn target_version_change_and_expiry_fail_closed() {
    let opened = open(need()).unwrap();
    let mut changed = answer(&opened.need);
    changed.observed_target_version = 8;
    let decision = submit_answer(&opened, changed).unwrap();
    assert!(!decision.accepted);
    assert_eq!(decision.snapshot.state, ClarificationStateV1::Stale);

    let opened = open(need()).unwrap();
    let mut late = answer(&opened.need);
    late.answered_at_ms = opened.need.expires_at_ms;
    let decision = submit_answer(&opened, late).unwrap();
    assert!(!decision.accepted);
    assert_eq!(decision.snapshot.state, ClarificationStateV1::Expired);
}

#[test]
fn clarification_identity_is_target_evidence_bound() {
    let original = need();
    let mut changed_question = original.clone();
    changed_question.question = "Should this be global?".into();
    assert_eq!(original.expected_id(), changed_question.expected_id());

    let mut changed_evidence = original.clone();
    changed_evidence.evidence_sha256 = h('d');
    assert_ne!(original.expected_id(), changed_evidence.expected_id());

    let mut forged = original;
    forged.clarification_id = "arbitrary".into();
    assert!(open(forged).is_err());
}

#[test]
fn cancellation_and_unsupported_host_are_terminal() {
    let opened = open(need()).unwrap();
    let cancelled = cancel(&opened, 150, "user declined clarification").unwrap();
    assert_eq!(cancelled.state, ClarificationStateV1::Cancelled);
    let decision = submit_answer(&cancelled, answer(&cancelled.need)).unwrap();
    assert!(!decision.accepted);

    let opened = open(need()).unwrap();
    let unavailable = unsupported(&opened, 150, "host has no authenticated-human surface").unwrap();
    assert_eq!(unavailable.state, ClarificationStateV1::Unsupported);
    let decision = submit_answer(&unavailable, answer(&unavailable.need)).unwrap();
    assert!(!decision.accepted);
}

#[test]
fn persisted_store_survives_restart_and_rejects_question_rewrite() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("clarifications.json");
    let mut store = ClarificationStore::open(&path).unwrap();
    let created = store.create(need()).unwrap().clone();
    assert_eq!(store.revision(), 1);

    let mut rewritten = created.need.clone();
    rewritten.question = "Should this preference be global?".into();
    assert!(matches!(
        store.create(rewritten),
        Err(ClarificationError::IdentityConflict(_))
    ));
    assert_eq!(store.revision(), 1);

    drop(store);
    let mut store = ClarificationStore::open(&path).unwrap();
    assert_eq!(store.get(&created.need.clarification_id), Some(&created));
    let decision = store
        .answer(&created.need.clarification_id, answer(&created.need))
        .unwrap();
    assert!(decision.accepted);
    assert_eq!(store.revision(), 2);

    drop(store);
    let mut store = ClarificationStore::open(&path).unwrap();
    let decision = store
        .resume(
            &created.need.clarification_id,
            ClarificationResumeRequestV1 {
                schema_version: 1,
                clarification_id: created.need.clarification_id.clone(),
                resumed_at_ms: 300,
                observed_target_version: 7,
                observed_evidence_sha256: h('a'),
            },
        )
        .unwrap();
    assert!(decision.accepted);
    assert_eq!(decision.snapshot.state, ClarificationStateV1::Resumed);
    assert_eq!(store.revision(), 3);
}

#[test]
fn stale_writers_fail_closed() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("clarifications.json");
    let mut first = ClarificationStore::open(&path).unwrap();
    let mut stale = ClarificationStore::open(&path).unwrap();
    let created = first.create(need()).unwrap().clone();

    let mut second_need = created.need.clone();
    second_need.lineage_id = "lineage-2".into();
    second_need.clarification_id = second_need.expected_id();
    assert!(matches!(
        stale.create(second_need),
        Err(ClarificationError::ConcurrentModification { .. })
    ));
}
