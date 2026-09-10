//! Parity coverage for `blueprint/src/graph/barrier.mjs`
//! (legacy test: `blueprint/tests/freshness-barrier.test.mjs`).
//!
//! The legacy barrier drives a Node `reconcile()` against a `watch_state`
//! SQLite table and reports one of three outcomes — `caught_up`,
//! `gap_blocked`, `timeout` — as a generation receipt. The native crate
//! already carries an equivalent generation write barrier:
//! `contracts::BarrierResult` is the same three-value outcome, and
//! `watch::NativeWatcher::barrier` implements the same decision rule
//! (`gap_blocked` if a gap is latched, `caught_up` once `applied_clock`
//! reaches the target, else `timeout` once the deadline has passed) over
//! its own `source_clock`/`applied_clock` polling model. This file proves
//! that decision rule matches the legacy barrier's contract without
//! touching `watch.rs` or spawning the legacy CLI process the JS test does.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use membrane_blueprint::contracts::BarrierResult;
use membrane_blueprint::watch::{Barrier, BarrierPoll, GapReason, MonotonicClock, NativeWatcher, SnapshotConfig};

/// A clock whose `now()` is set explicitly, so timeout behavior is
/// deterministic instead of depending on wall-clock races.
struct FakeClock(Mutex<Duration>);
impl FakeClock {
    fn new() -> Arc<Self> {
        Arc::new(Self(Mutex::new(Duration::ZERO)))
    }
    fn advance_to(&self, d: Duration) {
        *self.0.lock().unwrap() = d;
    }
}
impl MonotonicClock for FakeClock {
    fn now(&self) -> Duration {
        *self.0.lock().unwrap()
    }
}

fn watcher_at(root: &Path, clock: Arc<FakeClock>) -> NativeWatcher {
    NativeWatcher::start(SnapshotConfig::new(root)).unwrap().with_clock(clock)
}

#[test]
fn barrier_reports_caught_up_when_applied_clock_meets_target() {
    let dir = tempfile::tempdir().unwrap();
    let clock = FakeClock::new();
    let watcher = watcher_at(dir.path(), clock);
    // A fresh watcher starts with applied_clock == source_clock == 0, so a
    // barrier targeting clock 0 is already satisfied — the one-shot "no
    // drift to apply" case the legacy test calls barrierResult: "caught_up".
    let barrier = Barrier { target_source_clock: 0, deadline: None };
    assert_eq!(watcher.barrier(barrier), BarrierPoll::Complete(BarrierResult::CaughtUp));
}

#[test]
fn barrier_reports_gap_blocked_once_a_gap_is_latched() {
    let dir = tempfile::tempdir().unwrap();
    let clock = FakeClock::new();
    let mut watcher = watcher_at(dir.path(), clock);
    watcher.report_event_gap(GapReason::CallbackFailed).unwrap();
    // A gap takes priority over an otherwise-satisfied target clock, exactly
    // as the legacy `event_gap === "1"` check runs before the applied-clock
    // comparison in `syncToCurrentSource`.
    let barrier = Barrier { target_source_clock: 0, deadline: None };
    assert_eq!(watcher.barrier(barrier), BarrierPoll::Complete(BarrierResult::GapBlocked));
}

#[test]
fn barrier_reports_timeout_when_target_is_unmet_past_the_deadline() {
    let dir = tempfile::tempdir().unwrap();
    let clock = FakeClock::new();
    let watcher = watcher_at(dir.path(), Arc::clone(&clock));
    // Target clock ahead of what has actually been applied (nothing has
    // been applied yet), with a deadline already in the past — the same
    // "corrupt/behind clocks" shape the legacy test drives via
    // `watch_state.source_clock=5, applied_clock=0` and a short
    // `--timeout-ms`.
    clock.advance_to(Duration::from_millis(100));
    let barrier = Barrier { target_source_clock: 5, deadline: Some(Duration::from_millis(50)) };
    assert_eq!(watcher.barrier(barrier), BarrierPoll::Complete(BarrierResult::Timeout));
}

#[test]
fn barrier_waits_when_target_is_unmet_and_deadline_has_not_passed() {
    let dir = tempfile::tempdir().unwrap();
    let clock = FakeClock::new();
    let watcher = watcher_at(dir.path(), Arc::clone(&clock));
    clock.advance_to(Duration::from_millis(10));
    let barrier = Barrier { target_source_clock: 5, deadline: Some(Duration::from_millis(50)) };
    assert_eq!(watcher.barrier(barrier), BarrierPoll::Waiting);
}
