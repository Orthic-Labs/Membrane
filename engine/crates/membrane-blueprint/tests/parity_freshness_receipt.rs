// Parity port for blueprint/src/graph/freshness-receipt.mjs.
//
// Legacy source: blueprint/src/graph/freshness-receipt.mjs, exercised by
// blueprint/tests/freshness-receipt.test.mjs. That module has two families
// of behavior:
//
//   1. A pure decision core: `evaluateFreshness` (the freshness axis) and
//      `assertGenerationCoherence` (the orthogonal generation-coherence
//      axis, which fails closed at every freshness state, including
//      `fresh`). These already have an EQUIVALENT native port in
//      `engine/crates/membrane-blueprint/src/freshness.rs`
//      (`evaluate_freshness`, `assert_generation_coherence`,
//      `GenerationFreshnessBasis`, `CurrentSourceState`) predating this
//      lane. This file ports the legacy pure-axis test cases against that
//      existing code, unchanged, to prove parity rather than duplicate it.
//
//   2. Git/sqlite-backed orchestration (`observeCurrentVcsState`,
//      `changedPathsSinceGeneration`, `buildFreshnessReceipt`) that shells
//      out to git and reads a store envelope. `freshness.rs` is
//      deliberately process/shell-free by design (see its module doc), so
//      this lane adds `freshness_receipt.rs`'s `build_freshness_receipt`,
//      which reproduces the legacy receipt-assembly and suppression-mode
//      derivation rules from ALREADY-OBSERVED inputs (the caller performs
//      git/store I/O, exactly as `freshness.rs` already requires for its own
//      inputs). The `buildFreshnessReceipt` test cases below are ported
//      against synthetic (non-git) observations rather than a real git
//      fixture, since the git enumeration step itself has no native
//      equivalent in this crate.

use membrane_blueprint::freshness::{
    assert_generation_coherence, evaluate_freshness, CurrentSourceState, FreshnessState,
    GenerationFreshnessBasis,
};
use membrane_blueprint::freshness_receipt::{
    build_freshness_receipt, ChangedPaths, SuppressionMode, FRESHNESS_RECEIPT_SCHEMA,
};

fn basis(revision: &str, fingerprint: &str) -> GenerationFreshnessBasis {
    GenerationFreshnessBasis {
        indexed_revision: Some(revision.to_string()),
        indexed_worktree_fingerprint: Some(fingerprint.to_string()),
    }
}

fn current(revision: &str, fingerprint: &str) -> CurrentSourceState {
    CurrentSourceState {
        available: true,
        vcs_revision: Some(revision.to_string()),
        dirty: Some(false),
        worktree_fingerprint: Some(fingerprint.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Pure freshness axis (already-native; parity cases only)
// ---------------------------------------------------------------------------

#[test]
fn evaluate_freshness_fresh_when_current_matches_indexed_exactly() {
    assert_eq!(evaluate_freshness(&basis("abc123", "fp-1"), &current("abc123", "fp-1")), FreshnessState::Fresh);
}

#[test]
fn evaluate_freshness_changed_since_generation_on_revision_drift_is_a_truthful_success() {
    assert_eq!(
        evaluate_freshness(&basis("abc123", "fp-1"), &current("def456", "fp-2")),
        FreshnessState::ChangedSinceGeneration
    );
}

#[test]
fn evaluate_freshness_changed_since_generation_on_same_revision_but_dirty_overlay_drift() {
    assert_eq!(
        evaluate_freshness(&basis("abc123", "fp-clean"), &current("abc123", "fp-dirty")),
        FreshnessState::ChangedSinceGeneration
    );
}

#[test]
fn evaluate_freshness_unknown_when_generation_carries_no_indexed_basis() {
    let generation = GenerationFreshnessBasis { indexed_revision: None, indexed_worktree_fingerprint: None };
    assert_eq!(evaluate_freshness(&generation, &current("abc123", "fp-1")), FreshnessState::Unknown);
}

#[test]
fn evaluate_freshness_unavailable_when_current_state_observation_failed() {
    let unavailable = CurrentSourceState { available: false, ..Default::default() };
    assert_eq!(evaluate_freshness(&basis("abc123", "fp-1"), &unavailable), FreshnessState::Unavailable);
    // unavailable wins even when the generation basis is ALSO missing.
    let no_basis = GenerationFreshnessBasis { indexed_revision: None, indexed_worktree_fingerprint: None };
    assert_eq!(evaluate_freshness(&no_basis, &unavailable), FreshnessState::Unavailable);
}

#[test]
fn assert_generation_coherence_no_op_when_nothing_pinned_or_pin_matches() {
    assert!(assert_generation_coherence(None, Some("gen-1")).is_ok());
    assert!(assert_generation_coherence(Some("gen-1"), Some("gen-1")).is_ok());
}

#[test]
fn assert_generation_coherence_changed_since_generation_is_not_by_itself_a_mismatch() {
    // A consumer pinned to the generation STILL being served must not treat
    // staleness-only drift as incoherence.
    assert!(assert_generation_coherence(Some("gen-pinned"), Some("gen-pinned")).is_ok());
}

#[test]
fn assert_generation_coherence_fails_closed_on_mismatch_even_when_freshness_reports_fresh() {
    let receipt_freshness = evaluate_freshness(&basis("abc123", "fp-1"), &current("abc123", "fp-1"));
    assert_eq!(receipt_freshness, FreshnessState::Fresh, "the served generation is genuinely fresh");
    let error = assert_generation_coherence(Some("gen-earlier"), Some("gen-current")).unwrap_err();
    assert_eq!(error.pinned.as_deref(), Some("gen-earlier"));
    assert_eq!(error.served.as_deref(), Some("gen-current"));
}

#[test]
fn assert_generation_coherence_fails_closed_at_unknown_and_unavailable_too() {
    let unknown = GenerationFreshnessBasis { indexed_revision: None, indexed_worktree_fingerprint: None };
    let served = evaluate_freshness(&unknown, &current("abc123", "fp-1"));
    assert_eq!(served, FreshnessState::Unknown);
    assert!(assert_generation_coherence(Some("gen-other"), Some("gen-x")).is_err());

    let unavailable_current = CurrentSourceState { available: false, ..Default::default() };
    let served_unavailable = evaluate_freshness(&basis("abc123", "fp-1"), &unavailable_current);
    assert_eq!(served_unavailable, FreshnessState::Unavailable);
    assert!(assert_generation_coherence(Some("gen-other"), Some("gen-x")).is_err());
}

// ---------------------------------------------------------------------------
// build_freshness_receipt -- receipt assembly + suppression derivation
// ---------------------------------------------------------------------------

#[test]
fn build_freshness_receipt_fresh_requires_no_suppression_and_no_changed_enumeration() {
    let receipt = build_freshness_receipt(
        Some("gen-1".to_string()),
        Some("sha256:fixture".to_string()),
        basis("abc123", "fp-1"),
        current("abc123", "fp-1"),
        || panic!("must not enumerate changed paths when fresh"),
        |_path| true,
    );
    assert_eq!(receipt.schema, FRESHNESS_RECEIPT_SCHEMA);
    assert_eq!(receipt.freshness, FreshnessState::Fresh);
    assert_eq!(receipt.stale_sources, ChangedPaths::complete(Vec::new()));
    assert!(!receipt.suppression.required);
    assert_eq!(receipt.suppression.mode, SuppressionMode::None);
}

#[test]
fn build_freshness_receipt_changed_since_generation_reports_suppression_changed_paths_when_complete() {
    let receipt = build_freshness_receipt(
        Some("gen-1".to_string()),
        None,
        basis("abc123", "fp-1"),
        current("abc123", "fp-2"),
        || ChangedPaths::complete(vec!["src/app.js".to_string(), "src/other.js".to_string()]),
        |path| path == "src/app.js", // only app.js is present in the generation's indexed files
    );
    assert_eq!(receipt.freshness, FreshnessState::ChangedSinceGeneration);
    assert!(receipt.suppression.required);
    assert_eq!(receipt.suppression.mode, SuppressionMode::ChangedPaths);
    // Filtered to paths actually indexed by the generation.
    assert_eq!(receipt.stale_sources.paths, vec!["src/app.js".to_string()]);
    assert!(receipt.stale_sources.complete);
}

#[test]
fn build_freshness_receipt_changed_since_generation_falls_back_to_whole_generation_suppression_when_enumeration_incomplete() {
    let receipt = build_freshness_receipt(
        Some("gen-1".to_string()),
        None,
        basis("abc123", "fp-1"),
        current("def456", "fp-2"),
        || ChangedPaths::unavailable("comparison_failed"),
        |_path| true,
    );
    assert_eq!(receipt.freshness, FreshnessState::ChangedSinceGeneration);
    assert!(receipt.suppression.required);
    assert_eq!(receipt.suppression.mode, SuppressionMode::WholeGeneration);
    assert!(!receipt.stale_sources.complete);
    assert_eq!(receipt.stale_sources.reason.as_deref(), Some("comparison_failed"));
}

#[test]
fn build_freshness_receipt_unknown_generation_never_enumerates_changed_paths() {
    let receipt = build_freshness_receipt(
        Some("gen-1".to_string()),
        None,
        GenerationFreshnessBasis { indexed_revision: None, indexed_worktree_fingerprint: None },
        current("abc123", "fp-1"),
        || panic!("must not enumerate changed paths when unknown"),
        |_path| true,
    );
    assert_eq!(receipt.freshness, FreshnessState::Unknown);
    assert!(!receipt.suppression.required);
    assert_eq!(receipt.suppression.mode, SuppressionMode::None);
    assert!(!receipt.stale_sources.complete);
    assert_eq!(receipt.stale_sources.reason.as_deref(), Some("unknown"));
}

#[test]
fn build_freshness_receipt_unavailable_repo_never_enumerates_changed_paths() {
    let unavailable_current = CurrentSourceState { available: false, ..Default::default() };
    let receipt = build_freshness_receipt(
        Some("gen-1".to_string()),
        None,
        basis("deadbeef", "fp-x"),
        unavailable_current,
        || panic!("must not enumerate changed paths when unavailable"),
        |_path| true,
    );
    assert_eq!(receipt.freshness, FreshnessState::Unavailable);
    assert!(!receipt.suppression.required);
    assert_eq!(receipt.stale_sources.reason.as_deref(), Some("unavailable"));
}
