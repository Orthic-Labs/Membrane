//! M9 acceptance for deterministic compaction, fallback isolation, and cache accounting.

use membrane_core::compaction::{compact, CompactionConfig, CompactionInput, CompactionItem, ResidualNarrativeSummarizer, CATEGORY_NARRATIVE, CATEGORY_OBLIGATION};
use membrane_core::compaction_receipt::{
    CacheImpactV1, CompactionReceiptV1, CompactionSourceCursor,
};
use std::time::Instant;

fn input() -> CompactionInput {
    CompactionInput {
        source_cursor: CompactionSourceCursor { session_id: "s1".into(), last_seq: 9 },
        obligations: vec![CompactionItem { id: "obligation-1".into(), category: CATEGORY_OBLIGATION.into(), content: "must preserve exact identifier ABC-123".into(), priority: 100, source_seq: 1, protected: true }],
        narrative: vec![CompactionItem::new("narrative-1", CATEGORY_NARRATIVE, "old exploratory detail that can be reduced")],
        ..Default::default()
    }
}

#[derive(Default)]
struct Spy { calls: std::cell::Cell<usize> }
impl ResidualNarrativeSummarizer for Spy {
    fn summarize(&self, _residual: &[CompactionItem], _budget_tokens: u32) -> Option<String> { self.calls.set(self.calls.get() + 1); Some("residual".into()) }
    fn provider(&self) -> &str { "acceptance-provider" }
}

#[test]
fn projection_is_read_time_only_and_receipt_records_cache_impact() {
    let source = input();
    let before = source.clone();
    let result = compact(&source, &CompactionConfig { budget_tokens: 64, cache_impact: CacheImpactV1 { cache_hit: true, cache_key: Some("k".into()), reused_tokens: 12, invalidated: false } }).unwrap();
    assert_eq!(source, before);
    assert!(result.projection.retained.iter().any(|item| item.id == "obligation-1"));
    assert_eq!(result.receipt.source_cursor.last_seq, 9);
    assert_eq!(result.receipt.cache_impact.reused_tokens, 12);
    assert!(!result.receipt.fallback_used);
}

#[test]
fn fallback_is_optional_and_never_rewrites_protected_material() {
    let spy = Spy::default();
    let result = membrane_core::compaction::assemble(&input(), &CompactionConfig { budget_tokens: 10, ..Default::default() }, Some(&spy)).unwrap();
    assert_eq!(spy.calls.get(), 1);
    assert!(result.receipt.fallback_used);
    assert!(result.projection.rendered_text.contains("ABC-123"));
    assert_eq!(result.receipt.fallback_provider.as_deref(), Some("acceptance-provider"));
    assert!(result.receipt.omitted_categories.contains(&CATEGORY_NARRATIVE.to_string()));
}

#[test]
fn duplicate_ids_fail_closed() {
    let mut source = input();
    source.session.push(CompactionItem::new("obligation-1", "session", "duplicate"));
    assert!(compact(&source, &CompactionConfig::default()).is_err());
}

#[test]
fn compaction_replay_is_deterministic_across_receipt_restart() {
    let config = CompactionConfig {
        budget_tokens: 64,
        cache_impact: CacheImpactV1 {
            cache_hit: true,
            cache_key: Some("replay-key".into()),
            reused_tokens: 8,
            invalidated: false,
        },
    };
    let started = Instant::now();
    let first = compact(&input(), &config).unwrap();
    let first_ns = started.elapsed().as_nanos();
    let replay_started = Instant::now();
    let replay = compact(&input(), &config).unwrap();
    let replay_ns = replay_started.elapsed().as_nanos();
    let persisted = serde_json::to_vec(&first.receipt).unwrap();
    let path = std::env::temp_dir().join(format!(
        "membrane-mem-i02-compaction-{}.json",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(&path, &persisted).unwrap();
    let restored: CompactionReceiptV1 = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    std::fs::remove_file(&path).unwrap();
    eprintln!(
        "MEM-I02 compaction replay measurement firstNs={first_ns} replayNs={replay_ns} bytes={}",
        persisted.len()
    );
    assert_eq!(first.projection, replay.projection);
    assert_eq!(first.receipt, replay.receipt);
    assert_eq!(restored, first.receipt);
    assert!(restored.retained_obligations.contains(&"obligation-1".to_string()));
}
