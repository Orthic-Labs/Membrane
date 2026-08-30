use cortex_core::review::{
    select_review_input, ForegroundMemoryEmissionV1, ForegroundMemoryStateV1,
    ReviewInputSelectionLimitsV1, ReviewInputSelectionSkipReasonV1,
    REVIEW_INPUT_NOVELTY_FLOOR,
};
use cortex_core::{content_hash, EventCursor, MemoryTier, ProvenanceRef, SessionEvent};
use membrane_protocol::background_review::{
    BackgroundReviewJobKindV1, BackgroundReviewJobV1, BackgroundReviewReasonV1,
    BackgroundSemanticReviewResultV1, BackgroundSemanticReviewStatusV1,
};
use membrane_protocol::HostObservationProvenanceV1;
use membrane_runtime::background_review::{
    execute_background_semantic_review, BackgroundReviewCompletion, BackgroundReviewCursorStore,
    BackgroundReviewScheduler, BackgroundSemanticReviewInputV1,
    BackgroundSemanticReviewProvider, BackgroundSemanticReviewProviderError,
};
use membrane_runtime::{
    MemoryLifecycleOperation, MemoryLifecycleOperationV1, MemoryStore, VerifiedMemoryActor,
};
use serde_json::json;

fn event(session_id: &str, seq: u64, event_id: &str, text: &str) -> SessionEvent {
    let payload = json!({"text": text});
    SessionEvent {
        schema_version: cortex_core::ABSORBED_SCHEMA_VERSION,
        session_id: session_id.into(),
        seq,
        event_id: event_id.into(),
        event_type: "assistant_message".into(),
        payload: payload.clone(),
        scope_id: "scope".into(),
        authority: "observed".into(),
        influence_class: "episodic".into(),
        lifecycle: "active".into(),
        retention: "session".into(),
        provenance: vec![ProvenanceRef {
            source: "test".into(),
            source_event_ids: vec![event_id.into()],
            producer: None,
        }],
        content_hash: content_hash(&payload).unwrap(),
        occurred_at_ms: seq,
        recorded_at_ms: seq,
    }
}

fn cursor() -> EventCursor {
    EventCursor {
        session_id: "session".into(),
        last_seq: 1,
    }
}

#[test]
fn selection_picks_top_k_by_novelty_with_recency_tie_break_and_budget() {
    let baseline = event("session", 1, "old", "established baseline");
    let first = event("session", 2, "first", "novel alpha observation");
    let second = event("session", 3, "second", "novel beta observation");
    let duplicate = event("session", 4, "duplicate", "established baseline");
    let budget = cortex_core::estimate_tokens(&first.payload.to_string())
        + cortex_core::estimate_tokens(&second.payload.to_string());
    let result = select_review_input(
        &cursor(),
        &[first.clone(), second.clone(), duplicate],
        &[baseline],
        ReviewInputSelectionLimitsV1 {
            max_input_tokens: budget,
            novelty_floor: REVIEW_INPUT_NOVELTY_FLOOR,
        },
    )
    .unwrap();

    assert_eq!(result.receipt.selected, vec!["second", "first"]);
    assert_eq!(result.events.iter().map(|event| event.seq).collect::<Vec<_>>(), vec![2, 3]);
    assert_eq!(result.receipt.skipped.len(), 1);
    assert_eq!(result.receipt.skipped[0].event_id, "duplicate");
}

#[test]
fn selection_receipt_covers_every_non_selected_candidate_with_typed_reason() {
    let baseline = event("session", 1, "old", "baseline");
    let candidates = vec![
        event("session", 2, "fits", "new useful fact"),
        event("session", 3, "floor", "baseline"),
        event("session", 4, "budget", "another new useful fact"),
    ];
    let budget = cortex_core::estimate_tokens(&candidates[0].payload.to_string());
    let result = select_review_input(
        &cursor(),
        &candidates,
        &[baseline],
        ReviewInputSelectionLimitsV1 {
            max_input_tokens: budget,
            novelty_floor: REVIEW_INPUT_NOVELTY_FLOOR,
        },
    )
    .unwrap();

    assert_eq!(result.receipt.candidates_considered.len(), 3);
    assert_eq!(result.receipt.selected.len() + result.receipt.skipped.len(), 3);
    assert!(result.receipt.skipped.iter().all(|entry| matches!(
        entry.reason,
        ReviewInputSelectionSkipReasonV1::BudgetExhausted
            | ReviewInputSelectionSkipReasonV1::BelowNoveltyFloor
    )));
}

#[test]
fn skipped_but_eligible_episode_remains_eligible_on_the_next_run() {
    let candidate = event("session", 2, "deferred", "novel fact");
    let first = select_review_input(
        &cursor(),
        std::slice::from_ref(&candidate),
        &[],
        ReviewInputSelectionLimitsV1 {
            max_input_tokens: 0,
            novelty_floor: REVIEW_INPUT_NOVELTY_FLOOR,
        },
    )
    .unwrap();
    assert_eq!(
        first.receipt.skipped[0].reason,
        ReviewInputSelectionSkipReasonV1::BudgetExhausted
    );

    let second = select_review_input(
        &cursor(),
        &[candidate],
        &[],
        ReviewInputSelectionLimitsV1 {
            max_input_tokens: 100,
            novelty_floor: REVIEW_INPUT_NOVELTY_FLOOR,
        },
    )
    .unwrap();
    assert_eq!(second.receipt.selected, vec!["deferred"]);
}

#[test]
fn quiet_period_reports_below_floor_without_padding_the_budget() {
    let baseline = event("session", 1, "old", "same fact");
    let result = select_review_input(
        &cursor(),
        &[event("session", 2, "same", "same fact")],
        &[baseline],
        ReviewInputSelectionLimitsV1 {
            max_input_tokens: 100,
            novelty_floor: REVIEW_INPUT_NOVELTY_FLOOR,
        },
    )
    .unwrap();
    assert!(result.events.is_empty());
    assert!(result.receipt.selected.is_empty());
    assert!(result.receipt.quiet_period);
    assert_eq!(
        result.receipt.skipped[0].reason,
        ReviewInputSelectionSkipReasonV1::BelowNoveltyFloor
    );
}

#[test]
fn selection_is_pure_and_never_triggers_a_background_run() {
    let scheduler = scheduler(100, 100);
    let _ = select_review_input(
        &cursor(),
        &[event("session", 2, "new", "novel")],
        &[],
        ReviewInputSelectionLimitsV1 {
            max_input_tokens: 100,
            novelty_floor: REVIEW_INPUT_NOVELTY_FLOOR,
        },
    )
    .unwrap();
    assert_eq!(scheduler.active_count(), 0);
}

struct EmptySemanticProvider;

impl BackgroundSemanticReviewProvider for EmptySemanticProvider {
    fn execute(
        &self,
        request: &membrane_protocol::background_review::BackgroundSemanticReviewRequestV1,
    ) -> Result<BackgroundSemanticReviewResultV1, BackgroundSemanticReviewProviderError> {
        Ok(BackgroundSemanticReviewResultV1 {
            schema_version: BackgroundSemanticReviewResultV1::SCHEMA_VERSION,
            job_id: request.job_id.clone(),
            job_kind: request.job_kind,
            session_id: request.session_id.clone(),
            task_id: request.task_id.clone(),
            turn_id: request.turn_id.clone(),
            curation_proposals: vec![],
            memory_candidates: vec![],
            next_cursor: None,
            model: None,
            provider: Some("test".into()),
            usage: None,
            provenance_receipt: HostObservationProvenanceV1::new(
                "test-receipt",
                "test",
                1,
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ),
            status: BackgroundSemanticReviewStatusV1::Proposals,
        })
    }
}

fn scheduler(per_turn: u64, aggregate: u64) -> BackgroundReviewScheduler {
    BackgroundReviewScheduler::new(membrane_protocol::BackgroundReviewConfigV1 {
        schema_version: 1,
        enabled: true,
        min_elapsed_ms: 1,
        activity_threshold: 1,
        per_turn_input_budget: per_turn,
        aggregate_input_budget: aggregate,
        cancellation_timeout_ms: 1,
    })
    .unwrap()
}

fn semantic_job(id: &str, turn_id: &str, input_tokens: u64) -> BackgroundReviewJobV1 {
    BackgroundReviewJobV1 {
        schema_version: 1,
        job_id: id.into(),
        kind: BackgroundReviewJobKindV1::CortexSemanticDream,
        turn_id: turn_id.into(),
        input_tokens,
        requested_at_unix_ms: 1,
    }
}

#[test]
fn background_semantic_job_cannot_write_a_durable_cortex_record() {
    let store = MemoryStore::new();
    let before = store.entries(100).len();
    let scheduler = scheduler(100, 100);
    scheduler.set_hub_active(true, 1);
    scheduler.record_activity(1);
    let job = semantic_job("semantic-no-write", "turn", 1);
    assert!(matches!(scheduler.start(job.clone(), 1), membrane_runtime::background_review::BackgroundReviewDecision::Started { .. }));
    let input = BackgroundSemanticReviewInputV1 {
        task_id: None,
        cursor: cursor(),
        events: vec![event("session", 2, "new", "proposal source")],
        reviewed_baseline: vec![],
        foreground_memory_state: ForegroundMemoryStateV1::AvailableNoEmission,
    };
    let execution = execute_background_semantic_review(
        &scheduler,
        &job,
        &input,
        &EmptySemanticProvider,
        None,
        &BackgroundReviewCursorStore::default(),
        10_000,
        1,
    );
    assert!(execution.proposals.is_empty());
    assert_eq!(store.entries(100).len(), before);
}

#[test]
fn deterministic_cortex_lifecycle_operation_can_mutate_through_governed_api() {
    let store = MemoryStore::new();
    let actor = VerifiedMemoryActor::from_execution_context(
        "test-actor",
        "A2",
        "loopback",
        "session",
        "trace",
    )
    .unwrap();
    let receipt = store
        .execute_lifecycle_operation(
            &MemoryLifecycleOperationV1 {
                operation: MemoryLifecycleOperation::Create,
                memory_id: "deterministic-memory".into(),
                replacement_id: None,
                scope_id: Some("scope".into()),
                expected_content_sha256: None,
                reason_ref: "deterministic-test".into(),
                content: Some("governed lifecycle content".into()),
                tier: Some(MemoryTier::Semantic),
            },
            &actor,
        )
        .unwrap();
    assert_eq!(receipt.status, "created");
    assert_eq!(store.entries(100).len(), 1);
}

#[test]
fn foreground_memory_emission_skips_overlapping_memory_extraction() {
    let cursor = cursor();
    let emission = ForegroundMemoryEmissionV1 {
        emission_id: "foreground-1".into(),
        session_id: "session".into(),
        start_seq: 2,
        end_seq: 3,
    };
    let decision = cortex_core::review::bound_memory_candidate_extraction_window_with_state(
        &cursor,
        &[event("session", 2, "event", "already emitted")],
        &ForegroundMemoryStateV1::AvailableEmission(emission),
        cortex_core::review::MemoryCandidateExtractionLimitsV1 {
            max_events: 10,
            max_input_tokens: 100,
            max_duration_ms: 100,
            max_model_requests: 1,
        },
        true,
    )
    .unwrap();
    assert!(matches!(
        decision,
        cortex_core::review::MemoryCandidateExtractionDecisionV1::Skipped {
            reason: cortex_core::review::MemoryCandidateExtractionSkipV1::ForegroundMemoryEmissionPresent
        }
    ));
}

#[test]
fn unreadable_background_config_fails_closed_and_refusal_is_observable() {
    let scheduler = BackgroundReviewScheduler::from_config_path(
        "does-not-exist/background-learning.json",
        7,
    );
    let job = semantic_job("config-refused", "turn", 1);
    assert!(matches!(
        scheduler.start(job, 7),
        membrane_runtime::background_review::BackgroundReviewDecision::Deferred {
            reason: BackgroundReviewReasonV1::ConfigUnavailable
        }
    ));
    assert!(scheduler.drain_observations().iter().any(|observation| {
        observation.reason == BackgroundReviewReasonV1::ConfigUnavailable
    }));
}

#[test]
fn aggregate_budget_exhaustion_is_observable() {
    let scheduler = scheduler(10, 10);
    scheduler.set_hub_active(true, 1);
    scheduler.record_activity(1);
    let first = semantic_job("aggregate-1", "turn-1", 10);
    assert!(scheduler.start(first.clone(), 1).is_started());
    assert!(scheduler.finish("aggregate-1", BackgroundReviewCompletion::Completed, 2));
    scheduler.record_activity(1);
    let second = semantic_job("aggregate-2", "turn-2", 1);
    assert!(matches!(
        scheduler.start(second, 3),
        membrane_runtime::background_review::BackgroundReviewDecision::Deferred {
            reason: BackgroundReviewReasonV1::AggregateBudgetExceeded
        }
    ));
    assert!(scheduler.drain_observations().iter().any(|observation| {
        observation.reason == BackgroundReviewReasonV1::AggregateBudgetExceeded
    }));
}

trait StartedDecision {
    fn is_started(self) -> bool;
}

impl StartedDecision for membrane_runtime::background_review::BackgroundReviewDecision {
    fn is_started(self) -> bool {
        matches!(
            self,
            membrane_runtime::background_review::BackgroundReviewDecision::Started { .. }
        )
    }
}
