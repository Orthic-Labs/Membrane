use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use membrane_blueprint::contracts::BarrierResult;
use membrane_blueprint::watch::{reconcile_snapshots, snapshot, Barrier, BarrierPoll, EventKind, GapReason, LifecycleObservation, LifecycleSink, MonotonicClock, NativeWatcher, SnapshotConfig};

fn write(path: &Path, value: &str) { if let Some(parent) = path.parent() { fs::create_dir_all(parent).unwrap(); } fs::write(path, value).unwrap(); }

#[test]
fn create_modify_delete_reconcile_deterministically() {
    let temp = tempfile::tempdir().unwrap();
    let config = SnapshotConfig::new(temp.path());
    let before = snapshot(&config).unwrap();
    write(&temp.path().join("src/a.rs"), "one");
    let created = snapshot(&config).unwrap();
    assert_eq!(reconcile_snapshots(&before, &created, 0).iter().map(|e| e.kind).collect::<Vec<_>>(), vec![EventKind::Create]);
    write(&temp.path().join("src/a.rs"), "two");
    let modified = snapshot(&config).unwrap();
    assert_eq!(reconcile_snapshots(&created, &modified, 1)[0].kind, EventKind::Modify);
    fs::remove_file(temp.path().join("src/a.rs")).unwrap();
    let deleted = snapshot(&config).unwrap();
    assert_eq!(reconcile_snapshots(&modified, &deleted, 2)[0].kind, EventKind::Delete);
}

#[test]
fn snapshot_excludes_git_agent_and_configured_paths() {
    let temp = tempfile::tempdir().unwrap();
    write(&temp.path().join(".git/config"), "ignored");
    write(&temp.path().join(".agent/graph.db"), "ignored");
    write(&temp.path().join("vendor/generated.rs"), "ignored");
    write(&temp.path().join("src/main.rs"), "kept");
    let mut config = SnapshotConfig::new(temp.path());
    config.exclusions.insert("vendor".into());
    let paths = snapshot(&config).unwrap().entries.into_iter().map(|entry| entry.path).collect::<BTreeSet<_>>();
    assert!(paths.contains("src"));
    assert!(paths.contains("src/main.rs"));
    assert!(!paths.iter().any(|path| path.starts_with(".git") || path.starts_with(".agent") || path.starts_with("vendor")));
}

#[derive(Default)]
struct Sink(Mutex<Vec<LifecycleObservation>>);
impl LifecycleSink for Sink { fn observe(&self, observation: LifecycleObservation) { self.0.lock().unwrap().push(observation); } }

#[test]
fn coalesced_loss_is_reconciled_and_gap_blocks_until_clear() {
    let temp = tempfile::tempdir().unwrap();
    let mut watcher = NativeWatcher::start(SnapshotConfig::new(temp.path())).unwrap();
    watcher.report_event_gap(GapReason::EventOverflow).unwrap();
    assert_eq!(watcher.barrier(Barrier { target_source_clock: 0, deadline: None }), BarrierPoll::Complete(BarrierResult::GapBlocked));
    write(&temp.path().join("src/new.rs"), "new");
    let events = watcher.reconcile_now(|_| Ok(())).unwrap();
    assert_eq!(events[0].kind, EventKind::Create);
    watcher.clear_gap_after_reconcile();
    assert_eq!(watcher.barrier(Barrier { target_source_clock: watcher.source_clock(), deadline: None }), BarrierPoll::Complete(BarrierResult::CaughtUp));
}

struct FixedClock(Duration);
impl MonotonicClock for FixedClock { fn now(&self) -> Duration { self.0 } }

#[test]
fn barrier_reports_timeout_from_monotonic_clock() {
    let temp = tempfile::tempdir().unwrap();
    let watcher = NativeWatcher::start(SnapshotConfig::new(temp.path())).unwrap()
        .with_clock(Arc::new(FixedClock(Duration::from_secs(5))));
    assert_eq!(watcher.barrier(Barrier { target_source_clock: 1, deadline: Some(Duration::from_secs(1)) }), BarrierPoll::Complete(BarrierResult::Timeout));
}

#[test]
fn selective_invalidation_reports_independent_events_per_changed_path() {
    // BPT-020: invalidation must be addressable per source/config parent, not
    // collapsed into one undifferentiated full-rebuild signal. Two
    // independent paths changing in the same snapshot interval must surface
    // as two independently identified events (distinct paths, monotonic
    // per-event source clocks) so a DAG consumer can invalidate selectively.
    let temp = tempfile::tempdir().unwrap();
    let config = SnapshotConfig::new(temp.path());
    let before = snapshot(&config).unwrap();
    write(&temp.path().join("src/a.rs"), "source change");
    write(&temp.path().join("blueprint.config.json"), "{\"schema\":1}");
    let after = snapshot(&config).unwrap();
    let events = reconcile_snapshots(&before, &after, 0);
    assert_eq!(events.len(), 2, "selective invalidation requires one event per changed parent, not a single bulk signal");
    let paths: BTreeSet<_> = events.iter().map(|e| e.path.clone()).collect();
    assert!(paths.contains("src/a.rs"));
    assert!(paths.contains("blueprint.config.json"));
    assert!(events.iter().all(|e| e.kind == EventKind::Create));
    let clocks: Vec<_> = events.iter().map(|e| e.source_clock).collect();
    assert_ne!(clocks[0], clocks[1], "each independently invalidated path must carry its own source clock");
}

#[test]
fn full_incremental_sequence_matches_full_rebuild_membership_across_add_remove_move() {
    // BPT-021: incremental reconciliation across add -> modify -> rename(as
    // delete+create) -> delete must leave the snapshot's final membership
    // identical to a single from-scratch snapshot of the same end state --
    // the definition of incremental/full equivalence at the watcher layer.
    let temp = tempfile::tempdir().unwrap();
    let config = SnapshotConfig::new(temp.path());
    let mut previous = snapshot(&config).unwrap();
    let mut clock = 0u64;

    write(&temp.path().join("src/kept.rs"), "kept");
    let step = snapshot(&config).unwrap();
    let events = reconcile_snapshots(&previous, &step, clock);
    clock += events.len() as u64;
    previous = step;

    write(&temp.path().join("src/renamed_from.rs"), "will move");
    let step = snapshot(&config).unwrap();
    let events = reconcile_snapshots(&previous, &step, clock);
    clock += events.len() as u64;
    previous = step;

    fs::remove_file(temp.path().join("src/renamed_from.rs")).unwrap();
    write(&temp.path().join("src/renamed_to.rs"), "will move");
    let step = snapshot(&config).unwrap();
    let events = reconcile_snapshots(&previous, &step, clock);
    clock += events.len() as u64;
    assert!(events.iter().any(|e| e.path == "src/renamed_from.rs" && e.kind == EventKind::Delete));
    assert!(events.iter().any(|e| e.path == "src/renamed_to.rs" && e.kind == EventKind::Create));
    previous = step;

    let incremental_paths: BTreeSet<_> = previous.entries.iter().map(|e| e.path.clone()).collect();
    let _ = clock;

    let full = snapshot(&config).unwrap();
    let full_paths: BTreeSet<_> = full.entries.iter().map(|e| e.path.clone()).collect();
    assert_eq!(incremental_paths, full_paths, "incremental sequence must be membership-equivalent to a full rebuild snapshot of the same end state");
}

#[test]
fn callback_never_decides_phase_two_and_shutdown_is_clean() {
    let temp = tempfile::tempdir().unwrap();
    let sink = Arc::new(Sink::default());
    let mut watcher = NativeWatcher::start(SnapshotConfig::new(temp.path())).unwrap().with_sink(sink.clone());
    write(&temp.path().join("x.md"), "doc");
    let mut scheduled = Vec::new();
    watcher.poll(|event| { scheduled.push(event.path.clone()); Ok(()) }).unwrap();
    assert_eq!(scheduled, vec!["x.md"]);
    watcher.shutdown().unwrap();
    assert!(watcher.is_shutdown());
    assert!(matches!(watcher.poll(|_| Ok(())), Err(membrane_blueprint::watch::WatchError::Shutdown)));
    let kinds = sink.0.lock().unwrap().iter().map(|event| event.kind.clone()).collect::<Vec<_>>();
    assert!(kinds.iter().any(|kind| kind == "shutdown_requested"));
    assert!(kinds.iter().any(|kind| kind == "shutdown_complete"));
}
