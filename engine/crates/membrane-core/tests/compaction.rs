use membrane_core::compaction::{
    assemble, compact, CompactionConfig, CompactionInput, CompactionItem,
    ResidualNarrativeSummarizer, CATEGORY_NARRATIVE, CATEGORY_OBLIGATION,
};
use membrane_core::compaction_receipt::CompactionSourceCursor;

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
