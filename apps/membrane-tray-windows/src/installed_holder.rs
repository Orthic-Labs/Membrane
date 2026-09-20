//! Hub holder lease for installed tray lifetime.
use crate::{snapshot, startup, workspace::Workspace};
use membrane_protocol::{ResidentHolderCredentialV1, ResidentHolderOperationV1, ResidentHolderRequestV1, RESIDENT_HOLDER_SCHEMA_VERSION};
use std::{sync::{atomic::{AtomicBool, Ordering}, Arc, mpsc::{self, Receiver, Sender}}, thread, time::Duration};

const TTL_MS: u64 = 30_000;
const RENEW_MS: u64 = 10_000;
const RELEASE_ATTEMPTS: usize = 3;
const RELEASE_RETRY_DELAY: Duration = Duration::from_millis(100);

pub struct InstalledHubLease { stop: Arc<AtomicBool>, wake: Option<Sender<()>>, worker: Option<thread::JoinHandle<()>> }

impl InstalledHubLease {
    pub fn acquire(workspace: &Workspace) -> Result<Self, &'static str> {
        if workspace.origin != crate::workspace::RuntimeOrigin::Installed { return Err("installed_holder_requires_installed_origin"); }
        let endpoint = format!("http://127.0.0.1:{}", workspace.http_port);
        let token = crate::workspace::api_token(&workspace.root)?;
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let (wake, wake_rx) = mpsc::channel();
        let client = workspace.client_path();
        let current = workspace.stable_current.clone();
        let worker = thread::Builder::new().name("membrane-tray-hub-holder".into()).spawn(move || {
            run_worker(worker_stop, wake_rx, endpoint, token, client, current);
        }).map_err(|_| "resident_holder_worker_unavailable")?;
        Ok(Self { stop, wake: Some(wake), worker: Some(worker) })
    }
}

fn run_worker(stop: Arc<AtomicBool>, wake: Receiver<()>, endpoint: String, token: String, client: Option<std::path::PathBuf>, current: Option<std::path::PathBuf>) {
    let holder = ResidentHolderCredentialV1 { holder_kind: "hub".into(), holder_id: format!("tray-hub-{}", std::process::id()), credential_id: format!("tray-hub-{}-credential", std::process::id()) };
    let mut controller = None;
    let mut last_activation = std::time::Instant::now();
    while !stop.load(Ordering::Acquire) {
        if controller.is_none() {
            if let Ok(observed) = snapshot::fetch_remote_holder_status(&endpoint, &token) {
                if stop.load(Ordering::Acquire) { break; }
                let acquire = request(ResidentHolderOperationV1::Acquire, observed.controller.clone(), holder.clone(), TTL_MS);
                if let Ok(response) = snapshot::dispatch_resident_holder(&endpoint, &token, &acquire) {
                    if response.controller == acquire.controller && response.status.controller_active {
                        crate::supervisor::lifecycle_event("tray_hub_holder_acquired", serde_json::json!({"startupGeneration": acquire.controller.startup_generation}));
                        if stop.load(Ordering::Acquire) { let _ = snapshot::dispatch_resident_holder(&endpoint, &token, &request(ResidentHolderOperationV1::Release, acquire.controller, holder.clone(), 0)); break; }
                    controller = Some(observed.controller);
                    continue;
                    }
                }
            } else if last_activation.elapsed() >= Duration::from_secs(10) {
                if let (Some(client), Some(current)) = (client.as_deref(), current.as_deref()) {
                    if !stop.load(Ordering::Acquire) { let _ = startup::request_activation(client, current); last_activation = std::time::Instant::now(); }
                }
            }
            if wake.recv_timeout(Duration::from_millis(500)).is_ok() { break; }
            continue;
        }
        if wake.recv_timeout(Duration::from_millis(RENEW_MS)).is_ok() { break; }
        if stop.load(Ordering::Acquire) { break; }
        let Some(previous) = controller.clone() else { continue; };
        // A failed status/renew request is transport uncertainty, not proof
        // that this holder disappeared. Keep the credential so Drop can
        // still issue an explicit release after a later retry. Clearing it
        // here strands a live lease until TTL expiry when the tray exits
        // during a transient loopback failure.
        let Ok(observed) = snapshot::fetch_remote_holder_status(&endpoint, &token) else {
            controller = controller_after_renew(&previous, RenewOutcome::StatusUnavailable);
            crate::supervisor::lifecycle_event("tray_hub_holder_status_failed", serde_json::json!({"startupGeneration": previous.startup_generation}));
            continue;
        };
        let renew = request(ResidentHolderOperationV1::Renew, previous.clone(), holder.clone(), TTL_MS);
        if observed.controller != previous {
            controller = controller_after_renew(&previous, RenewOutcome::ControllerChanged);
            continue;
        }
        let outcome = match snapshot::dispatch_resident_holder(&endpoint, &token, &renew) {
            Ok(response) if response.controller == renew.controller && response.status.controller_active => RenewOutcome::Renewed,
            Err("resident_holder_rejected") if reacquire_holder(&endpoint, &token, &previous, &holder) => RenewOutcome::Renewed,
            Ok(_) | Err(_) => RenewOutcome::RenewRejected,
        };
        controller = controller_after_renew(&previous, outcome);
        if outcome == RenewOutcome::RenewRejected {
            // Preserve controller identity across an ambiguous renewal
            // result. A later status response can prove controller
            // replacement; until then, explicit shutdown must release
            // this holder rather than relying on lease expiry.
            crate::supervisor::lifecycle_event("tray_hub_holder_renew_failed", serde_json::json!({"startupGeneration": renew.controller.startup_generation}));
        }
    }
    if let Some(controller) = controller {
        let released = release_holder(&endpoint, &token, controller.clone(), holder);
        crate::supervisor::lifecycle_event(if released { "tray_hub_holder_released" } else { "tray_hub_holder_release_failed" }, serde_json::json!({"startupGeneration": controller.startup_generation}));
    }
    crate::supervisor::lifecycle_event("tray_hub_holder_worker_exit", serde_json::json!({"stopped": stop.load(Ordering::Acquire)}));
}

impl Drop for InstalledHubLease {
    fn drop(&mut self) { self.stop.store(true, Ordering::Release); if let Some(wake) = self.wake.take() { let _ = wake.send(()); } if let Some(worker) = self.worker.take() { let _ = worker.join(); } }
}

fn request(operation: ResidentHolderOperationV1, controller: membrane_protocol::ResidentControllerIdentityV1, holder: ResidentHolderCredentialV1, ttl_ms: u64) -> ResidentHolderRequestV1 {
    let observed_at_unix_ms = now_ms();
    ResidentHolderRequestV1 { schema_version: RESIDENT_HOLDER_SCHEMA_VERSION, operation, controller, holder: Some(holder), expires_at_unix_ms: (ttl_ms != 0).then_some(observed_at_unix_ms.saturating_add(ttl_ms)), observed_at_unix_ms, loss_cursor: None }
}
fn release_request(controller: membrane_protocol::ResidentControllerIdentityV1, holder: ResidentHolderCredentialV1) -> ResidentHolderRequestV1 {
    request(ResidentHolderOperationV1::Release, controller, holder, 0)
}
fn release_holder(endpoint: &str, token: &str, controller: membrane_protocol::ResidentControllerIdentityV1, holder: ResidentHolderCredentialV1) -> bool {
    for attempt in 0..RELEASE_ATTEMPTS {
        let response = snapshot::dispatch_resident_holder(endpoint, token, &release_request(controller.clone(), holder.clone()));
        if response.as_ref().is_ok_and(|response| release_succeeded(response, &controller)) {
            return true;
        }
        if attempt + 1 < RELEASE_ATTEMPTS {
            thread::sleep(RELEASE_RETRY_DELAY);
        }
    }
    false
}
fn reacquire_holder(endpoint: &str, token: &str, controller: &membrane_protocol::ResidentControllerIdentityV1, holder: &ResidentHolderCredentialV1) -> bool {
    let request = request(ResidentHolderOperationV1::Acquire, controller.clone(), holder.clone(), TTL_MS);
    snapshot::dispatch_resident_holder(endpoint, token, &request)
        .is_ok_and(|response| response.operation == ResidentHolderOperationV1::Acquire && response.controller == *controller && response.status.controller_active)
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RenewOutcome { Renewed, RenewRejected, StatusUnavailable, ControllerChanged }
fn controller_after_renew(previous: &membrane_protocol::ResidentControllerIdentityV1, outcome: RenewOutcome) -> Option<membrane_protocol::ResidentControllerIdentityV1> {
    (!matches!(outcome, RenewOutcome::ControllerChanged)).then_some(previous.clone())
}
fn release_succeeded(response: &membrane_protocol::ResidentHolderResponseV1, controller: &membrane_protocol::ResidentControllerIdentityV1) -> bool {
    response.operation == ResidentHolderOperationV1::Release && response.controller == *controller
}
fn now_ms() -> u64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64 }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stopped_worker_never_contacts_listener_or_activates() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let (_sender, receiver) = mpsc::channel();
        run_worker(Arc::new(AtomicBool::new(true)), receiver, endpoint, String::new(), None, None);
        assert_eq!(listener.accept().unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
    }

    #[test]
    fn drop_wakes_waiting_worker_without_renewal_delay() {
        let stop = Arc::new(AtomicBool::new(false));
        let (wake, receiver) = mpsc::channel();
        let worker_stop = stop.clone();
        let worker = thread::spawn(move || {
            receiver.recv_timeout(Duration::from_secs(10)).unwrap();
            assert!(worker_stop.load(Ordering::Acquire));
        });
        let started = std::time::Instant::now();
        drop(InstalledHubLease { stop, wake: Some(wake), worker: Some(worker) });
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    fn identity(startup_generation: u64) -> membrane_protocol::ResidentControllerIdentityV1 {
        membrane_protocol::ResidentControllerIdentityV1 {
            installation_id: "install".into(), cortex_store_id: "store".into(),
            release_generation: "release".into(), startup_generation,
            stable_current: "current".into(),
        }
    }

    fn holder() -> ResidentHolderCredentialV1 {
        ResidentHolderCredentialV1 {
            holder_kind: "hub".into(), holder_id: "tray-hub-test".into(),
            credential_id: "tray-hub-test-credential".into(),
        }
    }

    #[test]
    fn transient_renew_failure_keeps_controller_for_explicit_shutdown_release() {
        let controller = identity(7);
        // Ambiguous renew failure leaves controller present for Drop.
        let retained = controller_after_renew(&controller, RenewOutcome::StatusUnavailable);
        let release = release_request(retained.expect("controller retained"), holder());
        assert_eq!(release.operation, ResidentHolderOperationV1::Release);
        assert_eq!(release.controller, controller);
        let response = membrane_protocol::ResidentHolderResponseV1 {
            schema_version: RESIDENT_HOLDER_SCHEMA_VERSION,
            operation: ResidentHolderOperationV1::Release,
            controller: controller.clone(),
            status: membrane_protocol::ResidentHolderStatusV1 {
                controller_active: false, services_ready: false,
                services_unavailable_reason: None, hub_holders: 0,
                coderight_daemon_holders: 0, harness_holders: 0,
            },
            loss: None,
        };
        assert!(release_succeeded(&response, &controller));
        assert!(controller_after_renew(&controller, RenewOutcome::RenewRejected).is_some());
    }

    #[test]
    fn confirmed_controller_replacement_does_not_release_old_identity() {
        let previous = identity(7);
        let retained = controller_after_renew(&previous, RenewOutcome::ControllerChanged);
        assert!(retained.is_none());
    }
}
