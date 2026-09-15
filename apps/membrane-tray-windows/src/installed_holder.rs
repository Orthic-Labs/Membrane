//! Hub holder lease for installed tray lifetime.
use crate::{snapshot, startup, workspace::Workspace};
use membrane_protocol::{ResidentHolderCredentialV1, ResidentHolderOperationV1, ResidentHolderRequestV1, RESIDENT_HOLDER_SCHEMA_VERSION};
use std::{sync::{atomic::{AtomicBool, Ordering}, Arc, mpsc::{self, Receiver, Sender}}, thread, time::Duration};

const TTL_MS: u64 = 30_000;
const RENEW_MS: u64 = 10_000;

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
        let Ok(observed) = snapshot::fetch_remote_holder_status(&endpoint, &token) else { controller = None; continue; };
        let renew = request(ResidentHolderOperationV1::Renew, previous.clone(), holder.clone(), TTL_MS);
        if observed.controller != previous { controller = None; continue; }
        match snapshot::dispatch_resident_holder(&endpoint, &token, &renew) {
            Ok(response) if response.controller == renew.controller && response.status.controller_active => {}
            _ => { controller = None; crate::supervisor::lifecycle_event("tray_hub_holder_renew_failed", serde_json::json!({"startupGeneration": renew.controller.startup_generation})); }
        }
    }
    if let Some(controller) = controller {
        let released = snapshot::dispatch_resident_holder(&endpoint, &token, &request(ResidentHolderOperationV1::Release, controller.clone(), holder, 0)).is_ok();
        crate::supervisor::lifecycle_event(if released { "tray_hub_holder_released" } else { "tray_hub_holder_release_failed" }, serde_json::json!({"startupGeneration": controller.startup_generation}));
    }
}

impl Drop for InstalledHubLease {
    fn drop(&mut self) { self.stop.store(true, Ordering::Release); if let Some(wake) = self.wake.take() { let _ = wake.send(()); } if let Some(worker) = self.worker.take() { let _ = worker.join(); } }
}

fn request(operation: ResidentHolderOperationV1, controller: membrane_protocol::ResidentControllerIdentityV1, holder: ResidentHolderCredentialV1, ttl_ms: u64) -> ResidentHolderRequestV1 {
    let observed_at_unix_ms = now_ms();
    ResidentHolderRequestV1 { schema_version: RESIDENT_HOLDER_SCHEMA_VERSION, operation, controller, holder: Some(holder), expires_at_unix_ms: (ttl_ms != 0).then_some(observed_at_unix_ms.saturating_add(ttl_ms)), observed_at_unix_ms, loss_cursor: None }
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
}
