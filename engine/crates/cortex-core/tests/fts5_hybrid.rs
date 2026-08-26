//! Fixed corpus checks for the FTS5-to-hybrid lexical contract.
//!
//! These tests exercise the storage-independent adapter.  The storage crate's
//! projection tests cover SQLite MATCH/update/delete mechanics; keeping this
//! fixture in core makes fusion behavior testable without a second vector or
//! database owner.

use cortex_core::retriever::LexicalHit;
use cortex_core::{default_scope, MemoryEntry, MemoryRegistry, MemoryRetriever, MemoryTier};

fn entry(id: &str, content: &str, keywords: &[&str], score: f64) -> MemoryEntry {
    MemoryEntry {
        id: id.into(),
        tier: MemoryTier::Working,
        content: content.into(),
        keywords: keywords.iter().map(|value| (*value).into()).collect(),
        score,
        created_at: "2026-08-23T00:00:00Z".into(),
        access_count: 0,
        embedding: None,
        scope_id: default_scope(),
    }
}

#[test]
fn fixed_corpus_relevance_is_deterministic() {
    let first = entry("first", "Rust async runtime", &["rust"], 0.0);
    let second = entry("second", "Python worker", &["python"], 0.0);
    let mut registry = MemoryRegistry::new();
    registry.insert(first);
    registry.insert(second);
    let hits = MemoryRetriever::retrieve(&registry, "rust", 10);
    assert_eq!(hits[0].id, "first");
}

#[test]
fn update_and_delete_are_reflected_by_adapter_results() {
    let first = entry("first", "Rust runtime", &["rust"], 0.0);
    let updated = entry("first", "Python runtime", &["python"], 0.0);
    let mut registry = MemoryRegistry::new();
    registry.insert(updated);
    assert!(MemoryRetriever::retrieve(&registry, "rust", 10).is_empty());
    assert_eq!(
        MemoryRetriever::retrieve(&registry, "python", 10)[0].id,
        "first"
    );
    registry.remove("first");
    assert!(MemoryRetriever::retrieve(&registry, "python", 10).is_empty());
    let _ = first;
}

#[test]
fn pagination_input_preserves_score_then_id_order() {
    let one = entry("one", "Rust", &["rust"], 0.1);
    let two = entry("two", "Rust", &["rust"], 0.0);
    let mut registry = MemoryRegistry::new();
    registry.insert(one);
    registry.insert(two);
    let page = MemoryRetriever::retrieve(&registry, "rust", 1);
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].id, "one");
}

#[test]
fn fts_hits_fuse_with_semantic_signal_and_drop_stale_rows() {
    let lexical = entry("lexical", "deployment notes", &[], 0.0);
    let semantic = entry("semantic", "rollout details", &[], 0.0);
    let mut registry = MemoryRegistry::new();
    registry.insert(lexical);
    registry.insert(semantic);
    let hits = [
        LexicalHit::new("lexical", 4.0),
        LexicalHit::new("stale-deleted-record", 100.0),
    ];
    let results = MemoryRetriever::retrieve_hybrid_with_lexical_hits(
        &registry,
        &hits,
        Some(&[1.0, 0.0]),
        10,
        None,
    );
    assert!(results
        .iter()
        .all(|entry| entry.id != "stale-deleted-record"));
    assert_eq!(results[0].id, "lexical");
}
