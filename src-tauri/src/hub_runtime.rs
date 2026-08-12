use crate::{manifest_scan::discover_manifests, schema_types::{ManifestV1, SectionState, SectionV1, SnapshotV1}, supervisor::{ProductStatus, Supervisor}};
use serde::Serialize;
use std::{collections::BTreeMap, io::{Read, Write}, net::{SocketAddr, TcpStream, ToSocketAddrs}, sync::Mutex, time::{Duration, SystemTime, UNIX_EPOCH}};

const MAX_SNAPSHOT_BYTES: usize = 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductTab {
    pub product_id: String,
    pub display_name: String,
    pub snapshot: SnapshotV1,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HubSnapshot {
    pub products: Vec<ProductTab>,
}

pub struct HubRuntime {
    manifests: Vec<ManifestV1>,
    snapshots: Mutex<BTreeMap<String, SnapshotV1>>,
    supervisor: Supervisor,
}

impl HubRuntime {
    pub fn discover() -> Self {
        Self::from_manifests(discover_manifests())
    }

    pub fn from_manifests(manifests: Vec<ManifestV1>) -> Self {
        Self { manifests, snapshots: Mutex::new(BTreeMap::new()), supervisor: Supervisor::new() }
    }

    pub fn start_all(&self) {
        for manifest in &self.manifests {
            if self.supervisor.start_product(manifest).unwrap_or(ProductStatus::Unavailable) == ProductStatus::Unavailable {
                self.store_failure(manifest, "service_unavailable");
            }
        }
    }

    pub fn stop_all(&self) {
        self.supervisor.stop_all();
    }

    pub fn poll_all(&self) -> HubSnapshot {
        for manifest in &self.manifests {
            if self.supervisor.supervise_product(manifest).unwrap_or(ProductStatus::Unavailable) == ProductStatus::Unavailable {
                self.store_failure(manifest, "service_unavailable");
                continue;
            }
            match fetch_snapshot(manifest) {
                Ok(snapshot) => self.store_snapshot(manifest, snapshot),
                Err(reason) => self.store_failure(manifest, &reason),
            }
        }
        self.snapshot()
    }

    pub fn snapshot(&self) -> HubSnapshot {
        let snapshots = self.snapshots.lock().expect("runtime snapshot lock poisoned");
        HubSnapshot {
            products: self.manifests.iter().map(|manifest| ProductTab {
                product_id: manifest.product_id.clone(),
                display_name: manifest.display_name.clone(),
                snapshot: snapshots.get(&manifest.product_id).cloned().unwrap_or_else(|| unavailable_snapshot(manifest, "snapshot_unavailable")),
            }).collect(),
        }
    }

    fn store_snapshot(&self, manifest: &ManifestV1, snapshot: SnapshotV1) {
        self.snapshots.lock().expect("runtime snapshot lock poisoned").insert(manifest.product_id.clone(), snapshot);
    }

    fn store_failure(&self, manifest: &ManifestV1, reason: &str) {
        let mut snapshots = self.snapshots.lock().expect("runtime snapshot lock poisoned");
        let snapshot = snapshots.entry(manifest.product_id.clone()).or_insert_with(|| unavailable_snapshot(manifest, reason));
        if snapshot.observed_at_unix_ms != now_ms() {
            snapshot.stale = Some(true);
            snapshot.cache_age_ms = Some(now_ms().saturating_sub(snapshot.observed_at_unix_ms));
            snapshot.sections.insert("hub".into(), SectionV1 {
                state: SectionState::Unavailable,
                reason: reason.into(), items: None, evidence: None, resolver: None, observed_at_unix_ms: Some(now_ms()),
            });
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

fn unavailable_snapshot(manifest: &ManifestV1, reason: &str) -> SnapshotV1 {
    SnapshotV1 {
        schema_version: 1, product_id: manifest.product_id.clone(), observed_at_unix_ms: now_ms(), stale: Some(true), cache_age_ms: None,
        sections: BTreeMap::from([("hub".into(), SectionV1 { state: SectionState::Unavailable, reason: reason.into(), items: None, evidence: None, resolver: None, observed_at_unix_ms: Some(now_ms()) })]).into_iter().collect(),
    }
}

fn fetch_snapshot(manifest: &ManifestV1) -> Result<SnapshotV1, String> {
    let endpoint = &manifest.status_endpoint;
    let address: SocketAddr = (endpoint.host.as_str(), endpoint.port).to_socket_addrs()
        .map_err(|_| "snapshot_connect_failed")?
        .find(|address| address.ip().is_loopback())
        .ok_or("snapshot_not_loopback")?;
    let mut stream = TcpStream::connect_timeout(&address, REQUEST_TIMEOUT).map_err(|_| "snapshot_connect_failed")?;
    stream.set_read_timeout(Some(REQUEST_TIMEOUT)).map_err(|_| "snapshot_timeout")?;
    stream.set_write_timeout(Some(REQUEST_TIMEOUT)).map_err(|_| "snapshot_timeout")?;
    let request = format!("GET /snapshot HTTP/1.1\r\nHost: {}\r\n{}: {}\r\nConnection: close\r\n\r\n", endpoint.host, endpoint.auth_header, endpoint.auth_token);
    stream.write_all(request.as_bytes()).map_err(|_| "snapshot_write_failed")?;
    let mut response = Vec::new();
    stream.take((MAX_SNAPSHOT_BYTES + 8192) as u64).read_to_end(&mut response).map_err(|_| "snapshot_read_failed")?;
    let separator = response.windows(4).position(|bytes| bytes == b"\r\n\r\n").ok_or("snapshot_http_invalid")?;
    let (head, body) = response.split_at(separator + 4);
    if !head.starts_with(b"HTTP/1.1 200") && !head.starts_with(b"HTTP/1.0 200") { return Err("snapshot_http_status".into()); }
    if body.len() > MAX_SNAPSHOT_BYTES { return Err("snapshot_too_large".into()); }
    let snapshot: SnapshotV1 = serde_json::from_slice(body).map_err(|_| "snapshot_schema_invalid")?;
    if snapshot.schema_version != 1 || snapshot.product_id != manifest.product_id { return Err("snapshot_schema_invalid".into()); }
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{net::TcpListener, thread};

    fn manifest(port: u16, token: &str) -> ManifestV1 {
        ManifestV1 { schema_version: 1, product_id: "sample".into(), display_name: "Sample".into(), product_version: "1".into(), hub_compat_range: ">=0".into(), install_root: "/tmp".into(), service_start: vec!["/bin/true".into()], service_stop: vec![], status_endpoint: crate::schema_types::StatusEndpoint { host: "127.0.0.1".into(), port, auth_header: "X-Orthic-Token".into(), auth_token: token.into() }, icon: "/tmp/icon".into() }
    }

    #[test]
    fn polls_authenticated_loopback_snapshot_for_each_manifest() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut bytes = [0; 2048];
            let read = socket.read(&mut bytes).unwrap();
            let request = String::from_utf8_lossy(&bytes[..read]);
            assert!(request.starts_with("GET /snapshot HTTP/1.1"));
            assert!(request.contains("X-Orthic-Token: expected"));
            let body = r#"{"schemaVersion":1,"productId":"sample","observedAtUnixMs":1,"sections":{"health":{"state":"available","reason":"ready"}}}"#;
            write!(socket, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        });
        let snapshot = fetch_snapshot(&manifest(port, "expected")).unwrap();
        assert_eq!(snapshot.sections["health"].state, SectionState::Available);
        server.join().unwrap();
    }

    #[test]
    fn polling_failure_is_typed_unavailable() {
        let runtime = HubRuntime::from_manifests(vec![manifest(9, "expected")]);
        let state = runtime.poll_all();
        assert_eq!(state.products[0].snapshot.sections["hub"].state, SectionState::Unavailable);
        assert_eq!(state.products[0].snapshot.sections["hub"].reason, "service_unavailable");
    }
}
