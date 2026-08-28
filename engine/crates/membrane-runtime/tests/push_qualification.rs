//! Deterministic Push qualification harness.
//!
//! This harness exercises the production Push preparation route at matched
//! attention budgets. Fixture mechanics are test evidence only; host-backed
//! correctness, latency, correction, and resolver-restore fields remain typed
//! unavailable until the root lane runs the real production path.

use membrane_runtime::push::{compress, prep, PushPolicy};
use serde::Deserialize;
use std::collections::HashSet;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    schema_version: u8,
    fixture_id: String,
    attention_budget_tokens: usize,
    development: Vec<Case>,
    held_out: Vec<Case>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    id: String,
    path: String,
    query: String,
    resolver: String,
    required_evidence: Vec<String>,
    protected_spans: Vec<String>,
    source: String,
}

fn fixture() -> Fixture {
    serde_json::from_str(include_str!("fixtures/push-qualification.v1.json"))
        .expect("Push qualification fixture must be valid JSON")
}

fn production_query_output(case: &Case, budget: usize) -> (String, usize) {
    let temp = tempfile::tempdir().expect("qualification tempdir");
    let source_path = temp.path().join(&case.path);
    std::fs::write(&source_path, &case.source).expect("write qualification source");
    let out_dir = temp.path().join("prepared");
    let manifest = prep::prep_files_with_budget_and_policy(
        &out_dir,
        std::slice::from_ref(&source_path),
        0.5,
        0,
        Some(budget),
        PushPolicy::query_aware(case.query.clone(), true, true),
    );
    let entry = manifest.first().expect("production Push manifest entry");
    let prepared = entry.prepared.as_ref().expect("production Push output");
    let text = std::fs::read_to_string(prepared).expect("read production Push output");
    (text.clone(), compress::estimate_tokens(&text))
}

fn all_cases(fixture: &Fixture) -> impl Iterator<Item = &Case> {
    fixture.development.iter().chain(fixture.held_out.iter())
}

#[test]
fn qualification_fixture_is_frozen_and_corpus_is_disjoint() {
    let fixture = fixture();
    assert_eq!(fixture.schema_version, 1);
    assert_eq!(fixture.fixture_id, "push-qualification-v1");
    assert!(!fixture.development.is_empty());
    assert!(!fixture.held_out.is_empty());
    let mut ids = HashSet::new();
    for case in all_cases(&fixture) {
        assert!(ids.insert(&case.id), "duplicate fixture case: {}", case.id);
        assert!(!case.resolver.trim().is_empty());
        assert!(!case.required_evidence.is_empty());
        assert!(!case.protected_spans.is_empty());
    }
    let development_sources = fixture
        .development
        .iter()
        .map(|case| case.source.as_str())
        .collect::<HashSet<_>>();
    let development_paths = fixture
        .development
        .iter()
        .map(|case| case.path.as_str())
        .collect::<HashSet<_>>();
    assert!(fixture
        .held_out
        .iter()
        .all(|case| !development_sources.contains(case.source.as_str())));
    assert!(fixture
        .held_out
        .iter()
        .all(|case| !development_paths.contains(case.path.as_str())));
}

#[test]
fn production_query_aware_lane_is_reachable_and_preserves_required_evidence() {
    let fixture = fixture();
    let mut query_aware_differs = false;
    for case in all_cases(&fixture) {
        let raw = &case.source;
        let structural =
            compress::compress_to_budget_with_options(raw, fixture.attention_budget_tokens, true);
        let (query, query_tokens) = production_query_output(case, fixture.attention_budget_tokens);
        assert!(query_tokens <= fixture.attention_budget_tokens);
        for evidence in &case.required_evidence {
            assert!(
                query.contains(evidence),
                "missing {evidence} in {}",
                case.id
            );
        }
        for span in &case.protected_spans {
            assert!(
                query.contains(span),
                "protected span dropped in {}",
                case.id
            );
        }
        query_aware_differs |= structural.text != query && query.as_str() != raw.as_str();
    }
    assert!(
        query_aware_differs,
        "query-aware provider was unreachable or inert"
    );
}

#[test]
fn control_remains_default_and_matched_budget_is_explicit() {
    let fixture = fixture();
    assert_eq!(PushPolicy::default(), PushPolicy::Control);
    assert!(fixture.attention_budget_tokens > 0);
    for case in all_cases(&fixture) {
        let raw_tokens = compress::estimate_tokens(&case.source);
        let structural = compress::compress_to_budget_with_options(
            &case.source,
            fixture.attention_budget_tokens,
            true,
        );
        assert!(structural.output_tokens <= raw_tokens);
    }
}
