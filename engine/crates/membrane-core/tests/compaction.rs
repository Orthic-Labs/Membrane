use membrane_core::compaction::{
    assemble, check_federation_liveness, compact, CompactionConfig,
    CompactionFederationDecision, CompactionInput, CompactionItem, FederationCommitLedger,
    FederationLivenessError, ResidualNarrativeSummarizer, CATEGORY_NARRATIVE, CATEGORY_OBLIGATION,
    COMPACTION_FEDERATION_V1_SCHEMA,
};
use membrane_core::compaction_receipt::CompactionSourceCursor;

const GOLDEN_FIXTURE: &str =
    include_str!("fixtures/compaction-federation-v1.golden.json");

fn golden_decision() -> CompactionFederationDecision {
    serde_json::from_str(GOLDEN_FIXTURE).expect("golden fixture must parse as decision")
}

fn input() -> CompactionInput {
    CompactionInput {
        source_cursor: CompactionSourceCursor {
            session_id: "session-1".into(),
            last_seq: 7,
        },
        obligations: vec![CompactionItem {
            id: "obligation-1".into(),
            category: CATEGORY_OBLIGATION.into(),
            content: "must preserve exact identifier ABC-123".into(),
            priority: 100,
            source_seq: 1,
            protected: true,
        }],
        narrative: vec![CompactionItem::new(
            "narrative-1",
            CATEGORY_NARRATIVE,
            "old exploratory detail may be reduced",
        )],
        ..Default::default()
    }
}

#[test]
fn deterministic_projection_retains_obligations_and_does_not_mutate_input() {
    let source = input();
    let before = source.clone();
    let result = compact(
        &source,
        &CompactionConfig {
            budget_tokens: 64,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(source, before);
    assert_eq!(result.projection.source_cursor.last_seq, 7);
    assert!(result
        .projection
        .retained
        .iter()
        .any(|item| item.id == "obligation-1"));
    assert_eq!(result.receipt.retained_obligations, vec!["obligation-1"]);
    assert!(!result.receipt.fallback_used);
    assert!(result.receipt.budget_met);
}

#[derive(Default)]
struct SpySummarizer {
    calls: std::cell::Cell<usize>,
}

impl ResidualNarrativeSummarizer for SpySummarizer {
    fn summarize(&self, _residual: &[CompactionItem], _budget_tokens: u32) -> Option<String> {
        self.calls.set(self.calls.get() + 1);
        Some("residual summary".into())
    }

    fn provider(&self) -> &str {
        "fixture-provider"
    }
}

#[test]
fn fallback_is_injected_only_for_residual_narrative() {
    let summarizer = SpySummarizer::default();
    let result = assemble(
        &input(),
        &CompactionConfig {
            budget_tokens: 11,
            ..Default::default()
        },
        Some(&summarizer),
    )
    .unwrap();

    assert_eq!(summarizer.calls.get(), 1);
    assert!(result.receipt.fallback_used);
    assert_eq!(
        result.receipt.fallback_provider.as_deref(),
        Some("fixture-provider")
    );
    assert!(result
        .projection
        .retained
        .iter()
        .any(|item| item.category == "residual_narrative"));
}

#[test]
fn protected_material_survives_budget_overflow() {
    let mut source = input();
    source.obligations[0].content = "identifier ABC-123 constraint must never be elided".into();
    let result = compact(
        &source,
        &CompactionConfig {
            budget_tokens: 1,
            ..Default::default()
        },
    )
    .unwrap();

    assert!(!result.receipt.budget_met);
    assert!(result.projection.rendered_text.contains("ABC-123"));
    assert!(result
        .receipt
        .omitted_categories
        .contains(&CATEGORY_NARRATIVE.to_string()));
}

#[test]
fn duplicate_ids_fail_closed() {
    let mut source = input();
    source
        .session
        .push(CompactionItem::new("obligation-1", "session", "duplicate"));
    assert!(compact(&source, &CompactionConfig::default()).is_err());
}

// ---------------------------------------------------------------------------
// CRA-09 `compaction-federation-v1` consumer contract.
// ---------------------------------------------------------------------------

#[test]
fn federation_decision_round_trips_against_golden_fixture() {
    let decision = golden_decision();
    assert_eq!(decision.schema, COMPACTION_FEDERATION_V1_SCHEMA);
    assert_eq!(decision.identity, "cfed-decision-01a07e9f-1f88-7861-90a8-fc8ef04cb4b4");
    assert_eq!(decision.restart_epoch, 42);
    assert_eq!(decision.issued_monotonic_elapsed_ms, 184203771);
    assert_eq!(decision.expires_after_elapsed_ms, 30000);
    assert_eq!(
        decision.selection_receipt.selected_item_ids,
        vec!["ctx-item-0001", "ctx-item-0002", "ctx-item-0017"]
    );

    // Byte-identical round trip: re-serializing our copy of the golden fixture
    // and re-parsing it must reproduce the exact same typed value. We compare
    // against a value parsed from our own fixture file, never against the
    // CodeRight path.
    let serialized = serde_json::to_string_pretty(&decision).expect("serialize decision");
    let reparsed: CompactionFederationDecision =
        serde_json::from_str(&serialized).expect("reparse serialized decision");
    assert_eq!(decision, reparsed);
}

#[test]
fn replayed_after_expiry_is_refused() {
    let decision = golden_decision();
    // Fault: current monotonic elapsed is issued + expiry + 1ms, i.e. the
    // decision is replayed one millisecond after its allowance expired.
    let current_elapsed = decision.issued_monotonic_elapsed_ms
        + decision.expires_after_elapsed_ms
        + 1;

    let result = check_federation_liveness(&decision, decision.restart_epoch, current_elapsed);
    assert_eq!(
        result,
        Err(FederationLivenessError::Expired {
            elapsed_ms: decision.expires_after_elapsed_ms + 1,
            allowed_ms: decision.expires_after_elapsed_ms,
        })
    );
}

#[test]
fn second_commit_is_refused() {
    let decision = golden_decision();
    let mut ledger = FederationCommitLedger::new(decision.restart_epoch);
    let current_elapsed = decision.issued_monotonic_elapsed_ms + 1;

    ledger
        .commit(&decision, current_elapsed)
        .expect("first commit must succeed");

    // Fault: the same one-use decision is committed a second time.
    let result = ledger.commit(&decision, current_elapsed);
    assert_eq!(
        result,
        Err(FederationLivenessError::AlreadyCommitted(
            decision.identity.clone()
        ))
    );
}

#[test]
fn pending_across_restart_is_invalidated() {
    let decision = golden_decision();
    let mut ledger = FederationCommitLedger::new(decision.restart_epoch);

    // Fault: the process restarts (monotonic elapsed and pending decisions
    // reset) before the pending decision is committed. Liveness must be
    // judged by restart epoch first, never by wall clock or elapsed alone.
    ledger.restart(decision.restart_epoch + 1);

    let result = ledger.commit(&decision, decision.issued_monotonic_elapsed_ms + 1);
    assert_eq!(
        result,
        Err(FederationLivenessError::RestartEpochStale {
            decision_epoch: decision.restart_epoch,
            current_epoch: decision.restart_epoch + 1,
        })
    );
}

#[test]
fn diverging_from_golden_fails_round_trip() {
    let decision = golden_decision();

    // Fault: mutate one field so the value diverges from the golden fixture.
    let mut diverged = decision.clone();
    diverged.decision = "rejected".to_owned();

    assert_ne!(diverged, decision);

    let diverged_json = serde_json::to_string_pretty(&diverged).expect("serialize diverged");
    assert_ne!(diverged_json.trim(), GOLDEN_FIXTURE.trim());

    // The round trip on the *diverged* value must fail to equal the original
    // golden-parsed decision, proving the round-trip assertion actually
    // discriminates.
    let reparsed_diverged: CompactionFederationDecision =
        serde_json::from_str(&diverged_json).expect("reparse diverged");
    assert_ne!(reparsed_diverged, golden_decision());
}
