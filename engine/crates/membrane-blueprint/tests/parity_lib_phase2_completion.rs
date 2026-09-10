//! Parity tests for `lib_phase2_completion` (native port of the pure
//! generation-fencing decision logic in
//! `blueprint/src/lib/phase2-completion.mjs`), lane LIB4 (r5 closure). See
//! that module's doc comment for why the SQLite/delta-store orchestration
//! itself is out of scope for this pass.

use membrane_blueprint::lib_phase2_completion::{
    decide_completion, phase2_paths, CompletionState, Phase2Plan, PendingCounts,
};
use std::path::PathBuf;

#[test]
fn phase2_paths_layout_matches_legacy_join_order() {
    #[cfg(windows)]
    let root = PathBuf::from(r"C:\repo");
    #[cfg(not(windows))]
    let root = PathBuf::from("/repo");
    let paths = phase2_paths(&root, ".agent");
    assert_eq!(paths.base, root.join(".agent"));
    assert_eq!(paths.queue, root.join(".agent").join("queue.json"));
    assert_eq!(paths.verdicts, root.join(".agent").join("verdicts.json"));
    assert_eq!(paths.understanding, root.join(".agent").join("understanding.json"));
    assert_eq!(paths.plan, root.join(".agent").join("phase2-plan.json"));
}

#[test]
fn noop_when_doc_not_pending() {
    let outcome = decide_completion(false, Some("g1"), Some("g1"), Some("g1"), Some("g1"), None);
    assert_eq!(outcome.state, CompletionState::Noop);
    assert_eq!(outcome.phase2_complete, None);
}

#[test]
fn missing_generation_when_graph_has_no_generation_id() {
    let outcome = decide_completion(true, Some("g1"), None, Some("g1"), Some("g1"), None);
    assert_eq!(outcome.state, CompletionState::MissingGeneration);
}

#[test]
fn superseded_when_store_generation_moved_before_planning() {
    let outcome = decide_completion(true, Some("g0"), Some("g1"), Some("g1"), Some("g1"), None);
    assert_eq!(outcome.state, CompletionState::Superseded);
    assert_eq!(outcome.generation_id.as_deref(), Some("g1"));
}

#[test]
fn superseded_when_store_generation_moved_after_sealing() {
    let plan = Phase2Plan { complete: true, pending: PendingCounts::default() };
    let outcome = decide_completion(
        true,
        Some("g1"),
        Some("g1"),
        Some("g2"), // advanced after plan built / before seal check
        Some("g2"),
        Some(&plan),
    );
    assert_eq!(outcome.state, CompletionState::Superseded);
    assert_eq!(outcome.phase2_complete, Some(true));
}

#[test]
fn superseded_when_store_generation_moved_at_commit() {
    let plan = Phase2Plan { complete: false, pending: PendingCounts { verify: 2, synthesize: 1 } };
    let outcome = decide_completion(
        true,
        Some("g1"),
        Some("g1"),
        Some("g1"),
        Some("g2"), // advanced right before the transaction commit
        Some(&plan),
    );
    assert_eq!(outcome.state, CompletionState::Superseded);
    assert_eq!(outcome.phase2_complete, Some(false));
    // pending counts are only populated on a successful (non-superseded)
    // outcome, mirroring the legacy result object which omits verify/
    // synthesize entirely on the superseded branches.
    assert_eq!(outcome.verify, 0);
    assert_eq!(outcome.synthesize, 0);
}

#[test]
fn complete_when_plan_fully_reusable() {
    let plan = Phase2Plan { complete: true, pending: PendingCounts::default() };
    let outcome = decide_completion(true, Some("g1"), Some("g1"), Some("g1"), Some("g1"), Some(&plan));
    assert_eq!(outcome.state, CompletionState::Complete);
    assert_eq!(outcome.phase2_complete, Some(true));
    assert_eq!(outcome.state.watch_state_value(), Some("reused"));
}

#[test]
fn complete_state_maps_to_reused_watch_state_value() {
    assert_eq!(CompletionState::Complete.watch_state_value(), Some("reused"));
    assert_eq!(
        CompletionState::DocCurrentPhase2Pending.watch_state_value(),
        Some("pending_judgment")
    );
    assert_eq!(CompletionState::Noop.watch_state_value(), None);
    assert_eq!(CompletionState::Superseded.watch_state_value(), None);
}

#[test]
fn doc_current_phase2_pending_when_plan_requires_judgment() {
    let plan = Phase2Plan {
        complete: false,
        pending: PendingCounts { verify: 3, synthesize: 5 },
    };
    let outcome = decide_completion(true, Some("g1"), Some("g1"), Some("g1"), Some("g1"), Some(&plan));
    assert_eq!(outcome.state, CompletionState::DocCurrentPhase2Pending);
    assert_eq!(outcome.phase2_complete, Some(false));
    assert_eq!(outcome.verify, 3);
    assert_eq!(outcome.synthesize, 5);
}
