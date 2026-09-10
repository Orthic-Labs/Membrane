//! Watch-loop parity test (lane V4, NCL-02).
//!
//! Per `audit/qualification/windows-r5/ncl-02/watch-loop-parity.json`, no
//! production call site currently wires `membrane_blueprint::service`'s
//! `Supervisor`/`BlueprintService::resident` into a Hub- or tray-owned
//! resident holder (confirmed by a repo-wide grep for non-test callers), so
//! there is no Hub/tray-level residency to exercise here. This test instead
//! exercises the underlying policy watch.rs itself owns directly: taking a
//! bounded, deterministic snapshot of a real temp repository, mutating a
//! file, and polling (bounded to 10s, matching legacy debounce-then-
//! reconcile cadence) until a fresh snapshot's fingerprint has advanced and
//! `reconcile_snapshots` reports the expected per-path event -- the same
//! selective-invalidation contract a resident holder would rely on once one
//! exists. Additional cases cover native debounce, overflow, cancellation,
//! multi-root barriers, & additive holder cleanup.

use membrane_blueprint::watch::{
    reconcile_snapshots, snapshot, Barrier, BarrierPoll, EventKind, GapReason, NativeWatcher,
    SnapshotConfig,
};
use membrane_blueprint::api::{BlueprintOperation, BlueprintRequest, CancellationToken, RequestContext};
use membrane_blueprint::service::{HolderKind, NativeService, ServiceConfig, ServiceStatus};
use membrane_blueprint::BlueprintError;
use serde_json::Value;
use std::fs;
use std::sync::Arc;
use std::time::{Duration, Instant};

struct Noop;
impl BlueprintOperation for Noop {
    fn execute(&self, _: &BlueprintRequest, _: &RequestContext) -> Result<Value, BlueprintError> {
        Ok(serde_json::json!({"complete": true, "generationId": "test-generation"}))
    }
}

#[test]
fn snapshot_fingerprint_advances_and_reconcile_detects_the_change_within_bound() {
    let temp = tempfile::tempdir().expect("create temp repo dir");
    let root = temp.path();
    fs::write(root.join("a.txt"), b"initial content").expect("seed file");

    let config = SnapshotConfig::new(root);
    let before = snapshot(&config).expect("initial snapshot");
    assert!(!before.fingerprint.is_empty());

    // Mutate the tracked file -- this is the "make a file change" step.
    fs::write(root.join("a.txt"), b"changed content, longer than before").expect("mutate file");

    // Bounded poll: a resident holder built on this primitive would re-scan
    // on some cadence; 10s bounds this test regardless of that cadence.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut after = before.clone();
    loop {
        after = snapshot(&config).expect("rescan snapshot");
        if after.fingerprint != before.fingerprint {
            break;
        }
        assert!(Instant::now() < deadline, "snapshot fingerprint did not advance within the 10s bound");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_ne!(after.fingerprint, before.fingerprint, "fingerprint must advance after a tracked file changes");

    let events = reconcile_snapshots(&before, &after, 0);
    let modified = events
        .iter()
        .find(|event| event.path == "a.txt")
        .expect("reconcile must report a.txt as changed");
    assert_eq!(modified.kind, EventKind::Modify);
}

#[test]
fn snapshot_fingerprint_advances_on_new_file_within_bound() {
    let temp = tempfile::tempdir().expect("create temp repo dir");
    let root = temp.path();
    fs::write(root.join("existing.txt"), b"stays the same").expect("seed file");

    let config = SnapshotConfig::new(root);
    let before = snapshot(&config).expect("initial snapshot");

    fs::write(root.join("new.txt"), b"brand new file").expect("create new file");

    let deadline = Instant::now() + Duration::from_secs(10);
    let mut after = before.clone();
    loop {
        after = snapshot(&config).expect("rescan snapshot");
        if after.fingerprint != before.fingerprint {
            break;
        }
        assert!(Instant::now() < deadline, "snapshot fingerprint did not advance within the 10s bound");
        std::thread::sleep(Duration::from_millis(50));
    }

    let events = reconcile_snapshots(&before, &after, 0);
    let created = events
        .iter()
        .find(|event| event.path == "new.txt")
        .expect("reconcile must report new.txt as created");
    assert_eq!(created.kind, EventKind::Create);
}

#[test]
fn watcher_barrier_reports_caught_up_then_gap_blocked() {
    let temp = tempfile::tempdir().expect("create temp repo dir");
    let root = temp.path();
    fs::write(root.join("a.txt"), b"initial content").expect("seed file");

    let mut watcher = NativeWatcher::start(SnapshotConfig::new(root)).expect("start watcher");
    assert_eq!(watcher.barrier(Barrier { target_source_clock: 0, deadline: None }), BarrierPoll::Complete(membrane_blueprint::contracts::BarrierResult::CaughtUp));

    fs::write(root.join("a.txt"), b"changed content").expect("mutate file");
    let events = watcher.poll(|_| Ok(())).expect("reconcile mutation");
    assert_eq!(events.len(), 1);
    assert_eq!(watcher.source_clock(), watcher.applied_clock());
    assert_eq!(watcher.barrier(Barrier { target_source_clock: watcher.source_clock(), deadline: None }), BarrierPoll::Complete(membrane_blueprint::contracts::BarrierResult::CaughtUp));

    watcher.report_event_gap(GapReason::CallbackFailed).expect("record event gap");
    assert_eq!(watcher.barrier(Barrier { target_source_clock: watcher.source_clock().saturating_add(1), deadline: None }), BarrierPoll::Complete(membrane_blueprint::contracts::BarrierResult::GapBlocked));
}

#[test]
fn native_poll_debounce_is_bounded_and_coalesces_a_save_burst() {
    let temp = tempfile::tempdir().expect("create temp repo dir");
    let root = temp.path();
    fs::write(root.join("a.txt"), b"initial").expect("seed file");
    let mut watcher = NativeWatcher::start(SnapshotConfig::new(root).debounce_ms(25)).expect("start watcher");
    watcher.poll(|_| Ok(())).expect("initial poll");
    fs::write(root.join("a.txt"), b"save one").expect("first save");
    fs::write(root.join("a.txt"), b"save two").expect("second save");
    let started = Instant::now();
    let events = watcher.poll_debounced(|_| Ok(()), &CancellationToken::new()).expect("debounced poll");
    assert_eq!(events.len(), 1, "a burst must produce one path event");
    assert!(started.elapsed() >= Duration::from_millis(20), "debounce must be applied");
    assert!(started.elapsed() < Duration::from_millis(500), "debounce must remain bounded");
}

#[test]
fn event_overflow_latches_gap_without_acknowledging_dropped_events() {
    let temp = tempfile::tempdir().expect("create temp repo dir");
    let root = temp.path();
    let mut watcher = NativeWatcher::start(SnapshotConfig::new(root).max_events(1)).expect("start watcher");
    fs::write(root.join("one.txt"), b"one").expect("create one");
    fs::write(root.join("two.txt"), b"two").expect("create two");
    assert!(matches!(watcher.poll(|_| Ok(())), Err(membrane_blueprint::watch::WatchError::EventOverflow { .. })));
    assert_eq!(watcher.gap().map(|gap| gap.reason), Some(GapReason::EventOverflow));
    assert_eq!(watcher.applied_clock(), 0, "overflow cannot publish a partial batch");
}

#[test]
fn cancellation_does_not_latch_snapshot_gap_or_leave_resident_work() {
    let temp = tempfile::tempdir().expect("create temp repo dir");
    let root = temp.path();
    let mut watcher = NativeWatcher::start(SnapshotConfig::new(root)).expect("start watcher");
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    assert!(matches!(watcher.poll_with_cancellation(|_| Ok(()), &cancellation), Err(membrane_blueprint::watch::WatchError::Snapshot(membrane_blueprint::watch::SnapshotError::Cancelled))));
    assert!(watcher.gap().is_none());
}

#[test]
fn multi_root_barrier_is_independent_and_final_holder_drains() {
    let first = tempfile::tempdir().expect("first root");
    let second = tempfile::tempdir().expect("second root");
    let config = ServiceConfig::new(first.path()).with_watchers(vec![
        SnapshotConfig::new(first.path()),
        SnapshotConfig::new(second.path()),
    ]);
    let service = NativeService::new(Arc::new(Noop), config);
    service.acquire_holder(HolderKind::Hub).expect("hub holder");
    service.acquire_holder(HolderKind::CodeRight).expect("coderight holder");
    assert_eq!(service.enrolled_roots().len(), 2);
    assert_eq!(service.barrier_all(None).len(), 2);
    assert!(service.barrier_all(None).iter().all(|receipt| !receipt.generation_complete && receipt.barrier_result == membrane_blueprint::contracts::BarrierResult::GapBlocked));
    service.release_holder(HolderKind::Hub).expect("release hub");
    assert_eq!(service.status(), ServiceStatus::Running, "peer holder keeps resident service alive");
    service.release_holder(HolderKind::CodeRight).expect("release coderight");
    assert_eq!(service.status(), ServiceStatus::Stopped, "final holder drains resident service");
}
