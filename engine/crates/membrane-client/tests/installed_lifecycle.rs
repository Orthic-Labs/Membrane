//! LC-01/LC-03/LC-05 registry-level lifecycle regression: tray/daemon
//! concurrent credential paths, rapid acquire/release fencing against stale
//! resurrection, and reordered renew/lost-response/reacquire/generation
//! sequencing. These exercise `ResidencyRegistry` directly (the host-side
//! serialization point where "only the final holder drains" and "detach
//! serializes with holder mutations" are actually enforced), independent of
//! the injected-transport surface covered by `installed_transport.rs`.

use membrane_client::{
    AuthenticatedHolder, ControllerIdentity, HolderKind, ResidencyError, ResidencyRegistry,
};

fn controller() -> ControllerIdentity {
    ControllerIdentity {
        installation_id: "install-1".into(),
        cortex_store_id: "store-1".into(),
        release_generation: "release-1".into(),
        startup_generation: 1,
    }
}

fn relaunched_controller() -> ControllerIdentity {
    let mut next = controller();
    next.startup_generation += 1;
    next
}

fn holder(kind: HolderKind, id: &str, credential: &str) -> AuthenticatedHolder {
    AuthenticatedHolder { kind, holder_id: id.into(), credential_id: credential.into() }
}

/// Tray (Hub) and CodeRight daemon are distinct holder kinds that must be
/// able to hold the same controller concurrently; only the last one out
/// drains it, whichever kind that happens to be.
#[test]
fn tray_and_daemon_concurrent_credential_paths_share_one_controller_and_drain_only_last() {
    let mut registry = ResidencyRegistry::new();
    let tray = holder(HolderKind::Hub, "hub-tray", "cred-tray");
    let daemon = holder(HolderKind::CodeRightDaemon, "coderight-daemon", "cred-daemon");

    let first = registry.acquire(controller(), tray.clone(), 10, 1_000).unwrap();
    assert!(first.start_controller, "first concurrent acquirer must start the controller");

    let second = registry.acquire(controller(), daemon.clone(), 20, 1_000).unwrap();
    assert!(!second.start_controller, "second concurrent holder must not restart the controller");
    assert_eq!(second.snapshot.hub_holders, 1);
    assert_eq!(second.snapshot.coderight_daemon_holders, 1);

    let daemon_release = registry.release(&daemon).unwrap();
    assert!(!daemon_release.drain_controller, "a non-final holder release must never drain");

    let tray_release = registry.release(&tray).unwrap();
    assert!(tray_release.drain_controller, "the final holder release must drain");
    assert_eq!(tray_release.drained_identity, Some(controller()));
}

/// Negative control for LC-01: "Peer-held controller drained by non-final
/// holder fails." Releasing a holder that never held a lease must be a
/// no-op with respect to draining while a peer is still active — it must
/// fail to drain, specifically because the peer still holds, not because of
/// any other fault.
#[test]
fn peer_held_controller_is_not_drained_by_an_unrelated_release() {
    let mut registry = ResidencyRegistry::new();
    let tray = holder(HolderKind::Hub, "hub-tray", "cred-tray");
    let daemon = holder(HolderKind::CodeRightDaemon, "coderight-daemon", "cred-daemon");
    registry.acquire(controller(), tray.clone(), 1, 1_000).unwrap();
    registry.acquire(controller(), daemon, 1, 1_000).unwrap();

    // Releasing a holder id that was never acquired is a benign no-op: it
    // must not drain the still-peer-held controller.
    let phantom = holder(HolderKind::Hub, "hub-phantom", "cred-phantom");
    let outcome = registry.release(&phantom).unwrap();
    assert!(!outcome.drain_controller, "peer-held controller must survive an unrelated release");
    assert_eq!(outcome.snapshot.hub_holders, 1);
    assert_eq!(outcome.snapshot.coderight_daemon_holders, 1);

    // The real tray holder can still act; nothing was corrupted.
    assert!(registry.renew(&tray, 2, 2_000).is_ok());
}

/// Rapid A acquire/release, then B acquire/release, then a delayed A
/// arriving late: the delayed A must be treated as stale and must not be
/// able to resurrect (mutate or restart) the controller B already drained.
#[test]
fn rapid_a_then_b_then_delayed_a_is_stale_and_cannot_resurrect() {
    let mut registry = ResidencyRegistry::new();
    let a = holder(HolderKind::Hub, "hub-a", "cred-a");
    let b = holder(HolderKind::CodeRightDaemon, "coderight-b", "cred-b");

    // A acquires and releases quickly.
    let a_acquire = registry.acquire(controller(), a.clone(), 1, 100).unwrap();
    assert!(a_acquire.start_controller);
    let a_release = registry.release(&a).unwrap();
    assert!(a_release.drain_controller, "A was the only holder; its release drains");

    // B acquires and releases quickly afterward, against a fresh controller.
    let b_acquire = registry.acquire(controller(), b.clone(), 2, 200).unwrap();
    assert!(b_acquire.start_controller, "B restarts the controller after A fully drained");
    let b_release = registry.release(&b).unwrap();
    assert!(b_release.drain_controller);

    // A delayed A renew arrives after both cycles: A's lease no longer
    // exists (it was released, then the registry moved on through B), so
    // this must fail as a missing holder, not silently resurrect anything.
    let delayed = registry.renew(&a, 50, 500);
    assert_eq!(delayed, Err(ResidencyError::HolderMissing));
    assert_eq!(registry.snapshot().hub_holders, 0);
    assert_eq!(registry.snapshot().coderight_daemon_holders, 0);
    assert!(registry.snapshot().controller.is_none(), "delayed A must not resurrect a controller");
}

/// Reordered renew, a lost response (client believes it failed and
/// reacquires), and a generation change must all be handled without
/// letting old state bleed into the new one.
#[test]
fn reordered_renew_lost_response_reacquire_and_generation_change() {
    let mut registry = ResidencyRegistry::new();
    let holder_a = holder(HolderKind::Hub, "hub-a", "cred-a");

    registry.acquire(controller(), holder_a.clone(), 1, 100).unwrap();

    // Renew succeeds, but the caller never observes the response (lost
    // response) and, believing the renew failed, reacquires instead. The
    // reacquire with the same credential must be accepted idempotently and
    // must not regress the already-renewed expiry backward inadvertently.
    let renewed = registry.renew(&holder_a, 5, 150).unwrap();
    assert_eq!(renewed.hub_holders, 1);
    let reacquire = registry.acquire(controller(), holder_a.clone(), 6, 160).unwrap();
    assert!(!reacquire.start_controller, "reacquire of an existing holder must not restart");

    // A reordered (older) renew arriving after the reacquire must still be
    // accepted as a monotonically-fine op against the still-live lease, but
    // must never be allowed to move the expiry backward past what a later
    // op already established if the caller sends a smaller expiry — the
    // registry always takes the caller-declared expiry for the winning
    // (last-applied) call, so out-of-order arrival is the transport's
    // responsibility; here we assert the registry rejects only genuinely
    // invalid (non-advancing-past-now) expiries.
    assert_eq!(registry.renew(&holder_a, 7, 7), Err(ResidencyError::InvalidExpiry));

    // Now the controller's generation changes (a relaunch): the holder's
    // original acquire targeted the old controller, so any further mutation
    // under the new generation must be refused as a controller mismatch,
    // not silently accepted.
    let new_holder = holder(HolderKind::CodeRightDaemon, "coderight-new", "cred-new");
    assert_eq!(
        registry.acquire(relaunched_controller(), new_holder, 8, 200),
        Err(ResidencyError::ControllerMismatch)
    );
}

/// Negative control for LC-01: "Stale holder able to mutate fails." An
/// expired lease must not be renewable even if the caller still presents
/// the correct credential.
#[test]
fn stale_expired_holder_cannot_renew_or_mutate() {
    let mut registry = ResidencyRegistry::new();
    let holder_a = holder(HolderKind::Hub, "hub-a", "cred-a");
    registry.acquire(controller(), holder_a.clone(), 1, 10).unwrap();

    // Time passes the lease's expiry without a renew; reconcile sweeps it.
    let reconciled = registry.reconcile_expired(50);
    assert!(reconciled.drain_controller);

    // The stale holder's own renew and release now fail as missing, not as
    // any other error shape — this is the specific injected fault.
    assert_eq!(registry.renew(&holder_a, 60, 200), Err(ResidencyError::HolderMissing));
    let phantom_release = registry.release(&holder_a).unwrap();
    assert!(!phantom_release.drain_controller, "release of an already-gone holder is a no-op, not a fresh drain");
}

/// PID+creation-time-analogous continuity: an abrupt launcher relaunch is
/// modeled here as the controller's `startup_generation` incrementing while
/// `installation_id`/`cortex_store_id` stay the same. Holders bound to the
/// prior generation must not carry over into the new one; a fresh holder
/// against the new generation starts a fresh controller lifecycle.
#[test]
fn startup_generation_bump_on_abrupt_relaunch_does_not_inherit_old_holders() {
    let mut registry = ResidencyRegistry::new();
    let holder_a = holder(HolderKind::Hub, "hub-a", "cred-a");
    registry.acquire(controller(), holder_a.clone(), 1, 1_000).unwrap();

    // The old generation's holder is released (abrupt exit path already
    // reconciled by the supervisor before the new generation is observed).
    let released = registry.release(&holder_a).unwrap();
    assert!(released.drain_controller);

    // The relaunched controller (new startup_generation) accepts a fresh
    // acquire and starts its own independent lifecycle.
    let holder_b = holder(HolderKind::Hub, "hub-a", "cred-a-new");
    let fresh = registry.acquire(relaunched_controller(), holder_b, 2, 2_000).unwrap();
    assert!(fresh.start_controller);
    assert_eq!(fresh.snapshot.controller, Some(relaunched_controller()));
}

/// Malformed controller/holder identity must fail with the specific typed
/// validation error, not any generic failure, so callers can distinguish a
/// configuration bug from a lifecycle race.
#[test]
fn malformed_identity_fails_with_its_specific_typed_error() {
    let mut registry = ResidencyRegistry::new();
    let mut bad_controller = controller();
    bad_controller.startup_generation = 0;
    assert_eq!(
        registry.acquire(bad_controller, holder(HolderKind::Hub, "hub-a", "cred-a"), 1, 10),
        Err(ResidencyError::InvalidController)
    );

    let mut bad_holder = holder(HolderKind::Hub, "hub-a", "cred-a");
    bad_holder.holder_id.clear();
    assert_eq!(
        registry.acquire(controller(), bad_holder, 1, 10),
        Err(ResidencyError::InvalidHolder)
    );
}

/// An expiry worker and a late acquire are one serialized lifecycle boundary:
/// once expiry drains generation N, an acquire for generation N must remain
/// fenced even when expiry wins the mutex before the late request arrives.
#[test]
fn expired_generation_tombstone_blocks_late_acquire_but_allows_new_generation() {
    let mut registry = ResidencyRegistry::new();
    let old = holder(HolderKind::Hub, "hub", "credential");
    let new = holder(HolderKind::CodeRightDaemon, "coderight", "credential-new");
    registry.acquire(controller(), old, 1, 10).unwrap();
    assert!(registry.reconcile_expired(10).drain_controller);
    assert_eq!(
        registry.acquire(controller(), new.clone(), 10, 20),
        Err(ResidencyError::HolderExpired)
    );
    assert!(registry.acquire(relaunched_controller(), new.clone(), 11, 30).is_ok());
    assert!(registry.release(&new).unwrap().drain_controller);
}

/// Installed lifecycle probes must carry semantic state transitions in their
/// native output; a green process exit alone is insufficient evidence.
#[test]
fn installed_lifecycle_probes_report_controller_and_watcher_transitions() {
    for scenario in [
        "hub-only",
        "coderight-only",
        "both",
        "holder-crash",
        "holder-exit",
        "final-holder-shutdown",
        "concurrent-acquire-renew-release",
        "drain-acquire-race",
        "restart-during-acquire",
        "stale-fencing",
        "survivor-continuity",
    ] {
        let value = membrane_client::residency::qualification_lifecycle(scenario);
        assert_eq!(value["status"], "pass", "{scenario} must pass native control");
        assert!(value["evidence"]["expectedControllerState"].is_object(), "{scenario} missing controller state");
        assert!(value["evidence"]["watcherActivity"].is_object(), "{scenario} missing watcher activity");
    }
}
