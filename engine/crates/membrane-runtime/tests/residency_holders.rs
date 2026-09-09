use membrane_protocol::{
    ResidentControllerIdentityV1, ResidentHolderOperationV1, ResidentHolderRequestV1,
    RESIDENT_HOLDER_SCHEMA_VERSION,
};
use membrane_runtime::residency::{Holder, HolderKind, Identity, ResidentController};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Barrier, Mutex,
};

fn identity() -> Identity {
    Identity {
        installation_id: "install-1".into(),
        cortex_store_id: "store-1".into(),
        release_generation: "release-1".into(),
        startup_generation: 7,
    }
}

fn holder(kind: HolderKind, holder_id: &str) -> Holder {
    Holder {
        kind,
        holder_id: holder_id.into(),
        credential_id: format!("credential-{holder_id}"),
    }
}

#[test]
fn neither_holder_leaves_explicit_execution_independent() {
    let controller = ResidentController::new();
    assert!(!controller.snapshot().residents_required());
    assert!(controller.snapshot().controller.is_none());
}

#[test]
fn either_holder_starts_one_controller_and_both_deduplicate() {
    let mut controller = ResidentController::new();
    let hub = holder(HolderKind::Hub, "hub");
    let coderight = holder(HolderKind::CodeRightDaemon, "coderight");
    assert!(controller.acquire(identity(), hub.clone(), 1, 100).unwrap().start_controller);
    let both = controller.acquire(identity(), coderight.clone(), 2, 100).unwrap();
    assert!(!both.start_controller);
    assert_eq!((both.snapshot.hub_holders, both.snapshot.coderight_daemon_holders), (1, 1));
    assert!(!controller.release(&hub).unwrap().drain_controller);
    assert!(controller.release(&coderight).unwrap().drain_controller);
}

#[test]
fn authenticated_renewal_and_expiry_preserve_final_drain_rule() {
    let mut controller = ResidentController::new();
    let hub = holder(HolderKind::Hub, "hub");
    assert!(controller.acquire(identity(), hub.clone(), 1, 10).unwrap().start_controller);
    assert_eq!(controller.renew(&hub, 2, 20).unwrap().hub_holders, 1);
    assert!(!controller.reconcile_expired(19).drain_controller);
    assert!(controller.reconcile_expired(20).drain_controller);
}

#[test]
fn authoritative_final_release_reports_inactive_while_peer_release_stays_ready() {
    let mut controller = ResidentController::new();
    let hub = holder(HolderKind::Hub, "hub");
    let coderight = holder(HolderKind::CodeRightDaemon, "coderight");
    let controller_identity = ResidentControllerIdentityV1 {
        installation_id: "install-1".into(),
        cortex_store_id: "store-1".into(),
        release_generation: "release-1".into(),
        startup_generation: 7,
        stable_current: "C:/Membrane/current".into(),
    };
    for holder in [hub.clone(), coderight.clone()] {
        let request = ResidentHolderRequestV1 {
            schema_version: RESIDENT_HOLDER_SCHEMA_VERSION,
            operation: ResidentHolderOperationV1::Acquire,
            controller: controller_identity.clone(),
            holder: Some(membrane_protocol::ResidentHolderCredentialV1 {
                holder_kind: match holder.kind { HolderKind::Hub => "hub", HolderKind::CodeRightDaemon => "coderight_daemon" }.into(),
                holder_id: holder.holder_id.clone(),
                credential_id: holder.credential_id.clone(),
            }),
            observed_at_unix_ms: 1,
            expires_at_unix_ms: Some(10),
            loss_cursor: None,
        };
        assert!(controller.dispatch_authoritative(controller_identity.clone(), 1, request, true).unwrap().status.controller_active);
    }
    for (holder, expected_active) in [(hub, true), (coderight, false)] {
        let request = ResidentHolderRequestV1 {
            schema_version: RESIDENT_HOLDER_SCHEMA_VERSION,
            operation: ResidentHolderOperationV1::Release,
            controller: controller_identity.clone(),
            holder: Some(membrane_protocol::ResidentHolderCredentialV1 {
                holder_kind: match holder.kind { HolderKind::Hub => "hub", HolderKind::CodeRightDaemon => "coderight_daemon" }.into(),
                holder_id: holder.holder_id,
                credential_id: holder.credential_id,
            }),
            observed_at_unix_ms: 0,
            expires_at_unix_ms: None,
            loss_cursor: None,
        };
        assert_eq!(controller.dispatch_authoritative(controller_identity.clone(), 2, request, true).unwrap().status.controller_active, expected_active);
    }
}

#[test]
fn final_release_closes_admission_before_waiting_acquire_can_rebind() {
    let controller = Arc::new(Mutex::new(ResidentController::new()));
    let draining = Arc::new(AtomicBool::new(false));
    let barrier = Arc::new(Barrier::new(2));
    let hub = holder(HolderKind::Hub, "hub");
    controller.lock().unwrap().acquire(identity(), hub.clone(), 1, 10).unwrap();

    let release_controller = Arc::clone(&controller);
    let release_draining = Arc::clone(&draining);
    let release_barrier = Arc::clone(&barrier);
    let release = std::thread::spawn(move || {
        let mut controller = release_controller.lock().unwrap();
        assert!(controller.release(&hub).unwrap().drain_controller);
        release_barrier.wait();
        release_draining.store(true, Ordering::Release);
    });

    let acquire_controller = Arc::clone(&controller);
    let acquire_draining = Arc::clone(&draining);
    let acquire_barrier = Arc::clone(&barrier);
    let acquire = std::thread::spawn(move || {
        // Model handler's optimistic pre-lock admission check.
        assert!(!acquire_draining.load(Ordering::Acquire));
        acquire_barrier.wait();
        let mut controller = acquire_controller.lock().unwrap();
        // Model authoritative handler-boundary recheck after lock acquisition.
        (!acquire_draining.load(Ordering::Acquire))
            .then(|| controller.acquire(identity(), holder(HolderKind::CodeRightDaemon, "coderight"), 2, 10).unwrap().start_controller)
    });
    release.join().unwrap();
    assert_eq!(acquire.join().unwrap(), None);
}

#[test]
fn hub_only_residency_matrix_scenario_reports_active_controller_and_final_drain() {
    let mut controller = ResidentController::new();
    let hub = holder(HolderKind::Hub, "hub");
    let decision = controller.acquire(identity(), hub.clone(), 1, 100).unwrap();
    assert!(decision.start_controller);
    assert_eq!((decision.snapshot.hub_holders, decision.snapshot.coderight_daemon_holders), (1, 0));
    let release = controller.release(&hub).unwrap();
    assert!(release.drain_controller);
    assert!(controller.snapshot().controller.is_none());
}

#[test]
fn coderight_only_residency_matrix_scenario_reports_active_controller_and_final_drain() {
    let mut controller = ResidentController::new();
    let coderight = holder(HolderKind::CodeRightDaemon, "coderight");
    let decision = controller.acquire(identity(), coderight.clone(), 1, 100).unwrap();
    assert!(decision.start_controller);
    assert_eq!((decision.snapshot.hub_holders, decision.snapshot.coderight_daemon_holders), (0, 1));
    let release = controller.release(&coderight).unwrap();
    assert!(release.drain_controller);
    assert!(controller.snapshot().controller.is_none());
}

/// Negative control: a non-final holder release must never drain a
/// peer-held controller. Covers the LC-01 negative control
/// "Peer-held controller drained by non-final holder fails."
#[test]
fn peer_held_controller_is_never_drained_by_non_final_holder_release() {
    let mut controller = ResidentController::new();
    let hub = holder(HolderKind::Hub, "hub");
    let coderight = holder(HolderKind::CodeRightDaemon, "coderight");
    controller.acquire(identity(), hub.clone(), 1, 100).unwrap();
    controller.acquire(identity(), coderight.clone(), 2, 100).unwrap();
    // Releasing one of two live holders must report no drain and keep the
    // controller active for the surviving peer.
    let release = controller.release(&hub).unwrap();
    assert!(!release.drain_controller);
    assert!(controller.snapshot().controller.is_some());
    assert_eq!(controller.snapshot().coderight_daemon_holders, 1);
}

/// Negative control: a stale/duplicate release for a holder already removed
/// (crash-then-graceful-exit double signal, or a delayed release retransmit)
/// must be a no-op — it cannot resurrect a drained controller or mutate a
/// controller it is no longer part of. Covers "Stale holder able to mutate
/// fails."
#[test]
fn stale_duplicate_release_after_final_drain_cannot_resurrect_or_mutate() {
    let mut controller = ResidentController::new();
    let hub = holder(HolderKind::Hub, "hub");
    assert!(controller.acquire(identity(), hub.clone(), 1, 100).unwrap().start_controller);
    assert!(controller.release(&hub).unwrap().drain_controller);
    assert!(controller.snapshot().controller.is_none());

    // Retransmitted/duplicate release of the same already-drained holder
    // must not report a second drain and must not mutate a fresh holder's
    // occupancy of the controller.
    let stale_release = controller.release(&hub).unwrap();
    assert!(!stale_release.drain_controller);
    assert!(controller.snapshot().controller.is_none());

    let coderight = holder(HolderKind::CodeRightDaemon, "coderight");
    assert!(controller.acquire(identity(), coderight.clone(), 2, 100).unwrap().start_controller);
    // A second stale release of the long-gone hub holder must not touch the
    // freshly acquired, unrelated controller occupant.
    let stale_release_after_reacquire = controller.release(&hub).unwrap();
    assert!(!stale_release_after_reacquire.drain_controller);
    assert!(controller.snapshot().controller.is_some());
    assert_eq!(controller.snapshot().coderight_daemon_holders, 1);
}

#[test]
fn expiry_closes_admission_before_waiting_acquire_can_rebind() {
    let controller = Arc::new(Mutex::new(ResidentController::new()));
    let draining = Arc::new(AtomicBool::new(false));
    let barrier = Arc::new(Barrier::new(2));
    controller.lock().unwrap().acquire(identity(), holder(HolderKind::Hub, "hub"), 1, 2).unwrap();

    let expiry_controller = Arc::clone(&controller);
    let expiry_draining = Arc::clone(&draining);
    let expiry_barrier = Arc::clone(&barrier);
    let expiry = std::thread::spawn(move || {
        let mut controller = expiry_controller.lock().unwrap();
        assert!(controller.reconcile_expired(2).drain_controller);
        expiry_barrier.wait();
        expiry_draining.store(true, Ordering::Release);
    });

    let acquire_controller = Arc::clone(&controller);
    let acquire_draining = Arc::clone(&draining);
    let acquire_barrier = Arc::clone(&barrier);
    let acquire = std::thread::spawn(move || {
        // A stale pre-lock observation must not bypass this recheck.
        assert!(!acquire_draining.load(Ordering::Acquire));
        acquire_barrier.wait();
        let mut controller = acquire_controller.lock().unwrap();
        (!acquire_draining.load(Ordering::Acquire))
            .then(|| controller.acquire(identity(), holder(HolderKind::CodeRightDaemon, "coderight"), 2, 10).unwrap().start_controller)
    });
    expiry.join().unwrap();
    assert_eq!(acquire.join().unwrap(), None);
}
