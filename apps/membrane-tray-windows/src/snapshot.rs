//! Bounded, authenticated loopback snapshot polling for tray evidence.
//!
//! The tray never invents aggregate values. If resident snapshot admission
//! data is missing, malformed, unauthenticated, or unavailable, every metric
//! remains explicitly `Unknown` with its typed reason.

use std::{
    io::{self, Read, Write},
    net::{SocketAddr, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

use membrane_protocol::{
    HubSnapshotV1, ResidentControllerIdentityV1, ResidentHolderOperationV1,
    ResidentHolderRequestV1, ResidentHolderResponseV1, ResidentHolderStatusV1,
    ResidentServicesUnavailableV1,
    HUB_ADMISSION_SCHEMA_VERSION, HUB_SCHEMA_VERSION, RESIDENT_HOLDER_SCHEMA_VERSION,
};
use membrane_client::{
    build_loopback_request_headers, build_loopback_response_headers, verify_loopback_response_headers, LoopbackAuthSigner,
    LoopbackIdentityFields, LoopbackRequestFields,
};
use sha2::Digest;

const REQUEST_TIMEOUT: Duration = Duration::from_millis(500);
const POLL_INTERVAL: Duration = Duration::from_secs(2);
const MAX_RESPONSE_BYTES: usize = 1024 * 1024 + 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotValues {
    pub admitted: String,
    pub withheld: String,
    pub budget: String,
    pub observed: String,
}

impl SnapshotValues {
    pub fn unknown(reason: &str) -> Self {
        let value = format!("Unknown · {reason}");
        Self {
            admitted: value.clone(),
            withheld: value.clone(),
            budget: value,
            observed: format!("Unknown · {reason}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotUpdate {
    pub generation: u64,
    pub values: SnapshotValues,
    /// Fenced status observed from the installed daemon's resident-holder
    /// authority. `None` means health/status admission failed and must never
    /// be treated as a holder.
    pub resident_holder: Option<RemoteHolderObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteHolderObservation {
    pub controller: ResidentControllerIdentityV1,
    pub status: ResidentHolderStatusV1,
}

/// Start one bounded poller per daemon generation. Token stays in this
/// in-memory worker and is used only by SDK-owned loopback HMAC signing.
pub fn start_polling(
    endpoint: String,
    api_token: String,
    generation: u64,
) -> (Receiver<SnapshotUpdate>, Arc<AtomicBool>) {
    let (sender, receiver) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = Arc::clone(&stop);
    thread::Builder::new()
        .name("membrane-tray-snapshot".into())
        .spawn(move || poll_loop(endpoint, api_token, generation, worker_stop, sender))
        .expect("snapshot worker thread must start");
    (receiver, stop)
}

fn poll_loop(
    endpoint: String,
    api_token: String,
    generation: u64,
    stop: Arc<AtomicBool>,
    sender: Sender<SnapshotUpdate>,
) {
    loop {
        if stop.load(Ordering::Acquire) {
            return;
        }
        let values = fetch_snapshot(&endpoint, &api_token)
            .unwrap_or_else(|reason| SnapshotValues::unknown(reason));
        let resident_holder = fetch_remote_holder_status(&endpoint, &api_token).ok();
        if sender
            .send(SnapshotUpdate {
                generation,
                values,
                resident_holder,
            })
            .is_err()
        {
            return;
        }
        let mut elapsed = Duration::ZERO;
        while elapsed < POLL_INTERVAL {
            if stop.load(Ordering::Acquire) {
                return;
            }
            let step = Duration::from_millis(100);
            thread::sleep(step);
            elapsed += step;
        }
    }
}

/// Read the installed health identity, then ask that same fenced controller
/// for status. Health is only a discovery/fence; this helper never acquires,
/// renews, releases, or creates holder authority.
pub fn fetch_remote_holder_status(
    endpoint: &str,
    api_token: &str,
) -> Result<RemoteHolderObservation, &'static str> {
    let (status, body) = get_json(endpoint, api_token, "/health")?;
    if !matches!(status, 200 | 503) {
        return Err("resident_health_unavailable");
    }
    let health: serde_json::Value =
        serde_json::from_slice(&body).map_err(|_| "resident_health_invalid")?;
    let controller = controller_identity_from_health(&health)?;
    if status == 200 && health.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
        return Err("resident_health_unavailable");
    }
    let request = ResidentHolderRequestV1 {
        schema_version: RESIDENT_HOLDER_SCHEMA_VERSION,
        operation: ResidentHolderOperationV1::Status,
        controller: controller.clone(),
        holder: None,
        expires_at_unix_ms: None,
        observed_at_unix_ms: now_unix_ms(),
        loss_cursor: None,
    };
    let response = dispatch_resident_holder(endpoint, api_token, &request)?;
    if response.schema_version != RESIDENT_HOLDER_SCHEMA_VERSION
        || response.operation != ResidentHolderOperationV1::Status
        || response.controller != controller
    {
        return Err("resident_controller_mismatch");
    }
    Ok(RemoteHolderObservation {
        controller,
        status: response.status,
    })
}

fn controller_identity_from_health(
    health: &serde_json::Value,
) -> Result<ResidentControllerIdentityV1, &'static str> {
    if health.get("serviceId").and_then(serde_json::Value::as_str) != Some("membrane-hub")
        || health.get("runtimeOrigin").and_then(serde_json::Value::as_str) != Some("installed")
        || health.get("nativeOnly").and_then(serde_json::Value::as_bool) != Some(true)
    {
        return Err("resident_health_identity_invalid");
    }
    let installation_id = health
        .get("installationId")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or("resident_health_identity_invalid")?;
    let cortex_store_id = health
        .get("cortexStoreId")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or("resident_health_identity_invalid")?;
    let release_generation = health
        .get("releaseGeneration")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or("resident_health_identity_invalid")?;
    let startup_generation = health
        .get("startupGeneration")
        .and_then(serde_json::Value::as_u64)
        .filter(|value| *value != 0)
        .ok_or("resident_health_identity_invalid")?;
    let stable_current = health
        .get("stableInstallRoot")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or("resident_health_identity_invalid")?;
    Ok(ResidentControllerIdentityV1 {
        installation_id: installation_id.to_owned(),
        cortex_store_id: cortex_store_id.to_owned(),
        release_generation: release_generation.to_owned(),
        startup_generation,
        stable_current: stable_current.to_owned(),
    })
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn fetch_snapshot(endpoint: &str, api_token: &str) -> Result<SnapshotValues, &'static str> {
    let (status, _headers, body) = loopback_request(endpoint, api_token, "GET", "/hub/snapshot", "", &[])?;
    if status != 200 { return Err("snapshot_unavailable"); }
    let snapshot: HubSnapshotV1 = serde_json::from_slice(&body).map_err(|_| "snapshot_invalid")?;
    if snapshot.schema_version != HUB_SCHEMA_VERSION { return Err("snapshot_schema_unsupported"); }
    let admission = snapshot.admission.ok_or("snapshot_admission_unavailable")?;
    if admission.schema_version != HUB_ADMISSION_SCHEMA_VERSION { return Err("snapshot_admission_schema_unsupported"); }
    let admitted = admission.decisions_total.checked_sub(admission.omissions_total).ok_or("snapshot_admission_invalid")?;
    Ok(SnapshotValues { admitted: admitted.to_string(), withheld: admission.omissions_total.to_string(), budget: admission.budget_pressure_total.to_string(), observed: format_observed(admission.window_hours, snapshot.observed_at_unix_ms) })
}

fn get_json(endpoint: &str, api_token: &str, path: &str) -> Result<(u16, Vec<u8>), &'static str> {
    let (status, _headers, body) = loopback_request(endpoint, api_token, "GET", path, "", &[])?;
    Ok((status, body))
}

fn loopback_request(endpoint: &str, api_token: &str, method: &str, path: &str, content_type: &str, body: &[u8]) -> Result<(u16, Vec<(String, String)>, Vec<u8>), &'static str> {
    let address = parse_loopback_endpoint(endpoint)?;
    if api_token.len() != 64 || !api_token.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) { return Err("snapshot_auth_invalid"); }
    let identity = if path == "/livez" { None } else { Some(discover_identity(endpoint, api_token)?) };
    let signer = LoopbackAuthSigner::from_hex_token(api_token).map_err(|_| "snapshot_auth_invalid")?;
    let now = now_unix_ms() / 1000;
    let (head, request_fields) = if let Some(identity) = identity {
        let expiry = LoopbackAuthSigner::bounded_expiry(now, 10);
        let nonce = LoopbackAuthSigner::generate_nonce().map_err(|_| "snapshot_auth_unavailable")?;
        let headers = build_loopback_request_headers(&signer, &identity, method, path, "127.0.0.1", content_type, body, nonce, expiry).map_err(|_| "snapshot_auth_invalid")?;
        let fields = LoopbackRequestFields { method: method.into(), target: path.into(), host: "127.0.0.1".into(), content_type: content_type.into(), body_sha256: sha2::Sha256::digest(body).into(), identity, nonce, expiry_unix_secs: expiry };
        (format_headers(method, path, &headers), Some(fields))
    } else { (format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"), None) };
    let deadline = Instant::now().checked_add(REQUEST_TIMEOUT).unwrap_or_else(Instant::now);
    let mut stream = TcpStream::connect_timeout(&address, remaining_timeout(deadline)?).map_err(snapshot_io_reason)?;
    write_all_deadline(&mut stream, head.as_bytes(), deadline)?;
    if !body.is_empty() { write_all_deadline(&mut stream, body, deadline)?; }
    let mut raw = Vec::new(); let mut chunk = [0_u8; 8 * 1024];
    loop { stream.set_read_timeout(Some(remaining_timeout(deadline)?)).map_err(|_| "snapshot_unavailable")?; match stream.read(&mut chunk) { Ok(0) => break, Ok(n) => { raw.extend_from_slice(&chunk[..n]); if raw.len() > MAX_RESPONSE_BYTES { return Err("snapshot_too_large"); } }, Err(e) => return Err(snapshot_io_reason(e)) } }
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").ok_or("snapshot_invalid_http")?;
    let header = std::str::from_utf8(&raw[..split]).map_err(|_| "snapshot_invalid_http")?;
    let status = header.lines().next().and_then(|line| line.split_whitespace().nth(1)).and_then(|v| v.parse().ok()).ok_or("snapshot_invalid_http")?;
    let headers = parse_response_headers(header)?;
    let length = content_length(header)?; let end = split.checked_add(4).and_then(|v| v.checked_add(length)).ok_or("snapshot_too_large")?;
    let response_body = raw.get(split+4..end).ok_or("snapshot_invalid_http")?.to_vec();
    if let Some(request) = request_fields { verify_loopback_response_headers(&signer, &headers, &request, status, &response_body, &request.identity, now_unix_ms() / 1000).map_err(|_| "snapshot_response_auth_invalid")?; }
    Ok((status, headers, response_body))
}

fn discover_identity(endpoint: &str, api_token: &str) -> Result<LoopbackIdentityFields, &'static str> {
    let (status, _, body) = loopback_request(endpoint, api_token, "GET", "/livez", "", &[])?;
    if !matches!(status, 200 | 503) { return Err("resident_health_unavailable"); }
    let health: serde_json::Value = serde_json::from_slice(&body).map_err(|_| "resident_health_invalid")?;
    let field = |name: &str| health.get(name).and_then(serde_json::Value::as_str).filter(|v| !v.trim().is_empty()).ok_or("resident_health_identity_invalid");
    let installation_id = field("installationId")?.to_owned();
    let cortex_store_id = field("cortexStoreId")?.to_owned();
    let release_generation = field("releaseGeneration")?.to_owned();
    let startup_generation = health.get("startupGeneration").and_then(serde_json::Value::as_u64).filter(|v| *v != 0).ok_or("resident_health_identity_invalid")?;
    let stable_install_root = field("stableInstallRoot")?.to_owned();
    Ok(LoopbackIdentityFields { installation_id, cortex_store_id, release_generation, startup_generation, stable_install_root })
}

fn format_headers(method: &str, path: &str, headers: &[(String, String)]) -> String { let mut out = format!("{method} {path} HTTP/1.1\r\n"); for (n,v) in headers { out.push_str(n); out.push_str(": "); out.push_str(v); out.push_str("\r\n"); } out.push_str("\r\n"); out }
fn write_all_deadline(stream: &mut TcpStream, bytes: &[u8], deadline: Instant) -> Result<(), &'static str> { let mut offset=0; while offset < bytes.len() { stream.set_write_timeout(Some(remaining_timeout(deadline)?)).map_err(|_| "snapshot_unavailable")?; match stream.write(&bytes[offset..]) { Ok(0)=>return Err("snapshot_unavailable"), Ok(n)=>offset+=n, Err(e)=>return Err(snapshot_io_reason(e)) } } Ok(()) }
fn parse_response_headers(header: &str) -> Result<Vec<(String,String)>, &'static str> { header.lines().skip(1).map(|line| line.split_once(':').map(|(n,v)|(n.trim().to_ascii_lowercase(),v.trim().to_owned())).ok_or("snapshot_invalid_http")).collect() }
/// Dispatch one typed lifecycle request through tray's existing authenticated
/// loopback channel. This function does not create credentials, spawn a
/// daemon, or fall back to a local registry.
pub fn dispatch_resident_holder(endpoint: &str, api_token: &str, request: &ResidentHolderRequestV1) -> Result<ResidentHolderResponseV1, &'static str> {
    let body = serde_json::to_vec(request).map_err(|_| "resident_holder_invalid")?;
    let (status, _, response_body) = loopback_request(endpoint, api_token, "POST", "/resident-holder", "application/json", &body)?;
    if status != 200 { return Err("resident_holder_rejected"); }
    serde_json::from_slice(&response_body).map_err(|_| "resident_holder_invalid")
}

fn content_length(head: &str) -> Result<usize, &'static str> {
    let mut value = None;
    for line in head.lines().skip(1) {
        let Some((name, raw_value)) = line.split_once(':') else { continue; };
        if name.eq_ignore_ascii_case("content-length") {
            let parsed = raw_value.trim().parse::<usize>().map_err(|_| "snapshot_invalid_http")?;
            if value.replace(parsed).is_some() { return Err("snapshot_invalid_http"); }
        }
    }
    value.ok_or("snapshot_invalid_http")
}

fn remaining_timeout(deadline: Instant) -> Result<Duration, &'static str> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() { Err("snapshot_timeout") } else { Ok(remaining) }
}

fn snapshot_io_reason(error: io::Error) -> &'static str {
    match error.kind() { io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => "snapshot_timeout", _ => "snapshot_unavailable" }
}
fn parse_loopback_endpoint(endpoint: &str) -> Result<SocketAddr, &'static str> {
    let authority = endpoint
        .strip_prefix("http://")
        .ok_or("snapshot_endpoint_invalid")?
        .split('/')
        .next()
        .ok_or("snapshot_endpoint_invalid")?;
    let address: SocketAddr = authority.parse().map_err(|_| "snapshot_endpoint_invalid")?;
    if !address.ip().is_loopback() {
        return Err("snapshot_endpoint_not_loopback");
    }
    Ok(address)
}

fn format_observed(window_hours: u32, observed_at_ms: u64) -> String {
    if observed_at_ms == 0 {
        return format!("Unknown · window {window_hours}h");
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let age = now.saturating_sub(observed_at_ms);
    let age_label = if age < 1_000 {
        "now".to_owned()
    } else if age < 60_000 {
        format!("{}s ago", age / 1_000)
    } else {
        format!("{}m ago", age / 60_000)
    };
    format!("window {window_hours}h · observed {age_label}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    fn fixture_health() -> serde_json::Value {
        serde_json::json!({
            "serviceId": "membrane-hub",
            "runtimeOrigin": "installed",
            "nativeOnly": true,
            "installationId": "installation",
            "cortexStoreId": "store",
            "releaseGeneration": "release",
            "startupGeneration": 4,
            "stableInstallRoot": "C:/Membrane/current",
            "ok": false
        })
    }

    fn serve_json(stream: &mut std::net::TcpStream, status: u16, body: &serde_json::Value) {
        let body = serde_json::to_vec(body).unwrap();
        let head = format!(
            "HTTP/1.1 {status} TEST\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(head.as_bytes()).unwrap();
        stream.write_all(&body).unwrap();
    }

    fn serve_signed_json(stream: &mut std::net::TcpStream, status: u16, body: &serde_json::Value, request: &str, token: &str, identity: &LoopbackIdentityFields) {
        let body = serde_json::to_vec(body).unwrap();
        let value = |name: &str| request.lines().find_map(|line| line.split_once(':').filter(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.trim()));
        let nonce_hex = value("x-membrane-loopback-nonce").unwrap();
        assert_eq!(nonce_hex.len(), 64);
        let mut nonce = [0_u8; 32];
        for (index, byte) in nonce.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&nonce_hex[index * 2..index * 2 + 2], 16).unwrap();
        }
        let expiry = value("x-membrane-loopback-expiry").unwrap().parse().unwrap();
        let signer = LoopbackAuthSigner::from_hex_token(token).unwrap();
        let headers = build_loopback_response_headers(&signer, identity, nonce, status, &body, expiry).unwrap();
        let mut head = format!("HTTP/1.1 {status} TEST\r\n");
        for (name, value) in headers { head.push_str(&format!("{name}: {value}\r\n")); }
        head.push_str("\r\n"); stream.write_all(head.as_bytes()).unwrap(); stream.write_all(&body).unwrap();
    }

    fn read_http_request(stream: &mut std::net::TcpStream) -> String {
        let mut raw = Vec::new();
        let mut chunk = [0_u8; 1024];
        let header_end;
        let content_length;
        loop {
            let read = stream.read(&mut chunk).unwrap();
            assert!(read > 0, "fixture client closed before request headers");
            raw.extend_from_slice(&chunk[..read]);
            if let Some(end) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
                header_end = end + 4;
                let headers = String::from_utf8_lossy(&raw[..end]);
                content_length = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("Content-Length:").or_else(|| line.strip_prefix("content-length:")))
                    .and_then(|value| value.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                break;
            }
            assert!(raw.len() < 64 * 1024, "fixture request headers exceeded bound");
        }
        while raw.len() < header_end + content_length {
            let read = stream.read(&mut chunk).unwrap();
            assert!(read > 0, "fixture client closed before request body");
            raw.extend_from_slice(&chunk[..read]);
            assert!(raw.len() <= header_end + content_length, "fixture request exceeded declared body length");
        }
        String::from_utf8_lossy(&raw).into_owned()
    }

    fn spawn_holder_fixture(status: ResidentHolderStatusV1, mismatched: bool) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        thread::spawn(move || {
            let health = fixture_health();
            let controller = controller_identity_from_health(&health).unwrap();
            let token = if mismatched { "b".repeat(64) } else { "a".repeat(64) };
            let identity = LoopbackIdentityFields { installation_id: "installation".into(), cortex_store_id: "store".into(), release_generation: "release".into(), startup_generation: 4, stable_install_root: "C:/Membrane/current".into() };
            for _ in 0..4 {
                let (mut stream, _) = listener.accept().unwrap();
                let request = read_http_request(&mut stream);
                if request.starts_with("GET /livez") {
                    serve_json(&mut stream, 200, &health);
                } else if request.starts_with("GET /health") {
                    serve_signed_json(&mut stream, 503, &health, &request, &token, &identity);
                } else {
                    let response_controller = if mismatched {
                        ResidentControllerIdentityV1 { startup_generation: 99, ..controller.clone() }
                    } else {
                        controller.clone()
                    };
                    let response = serde_json::json!({
                        "schemaVersion": RESIDENT_HOLDER_SCHEMA_VERSION,
                        "operation": "status",
                        "controller": response_controller,
                        "status": status,
                    });
                    serve_signed_json(&mut stream, 200, &response, &request, &token, &identity);
                }
            }
        });
        endpoint
    }

    #[test]
    fn rejects_non_loopback_endpoint() {
        assert_eq!(
            parse_loopback_endpoint("http://192.0.2.1:4317").unwrap_err(),
            "snapshot_endpoint_not_loopback"
        );
    }

    #[test]
    fn rejects_non_hex_or_wrong_length_token() {
        assert_eq!(
            fetch_snapshot("http://127.0.0.1:1", "short").unwrap_err(),
            "snapshot_auth_invalid"
        );
        assert_eq!(
            fetch_snapshot("http://127.0.0.1:1", &"A".repeat(64)).unwrap_err(),
            "snapshot_auth_invalid"
        );
    }

    #[test]
    fn unknown_values_keep_typed_reason() {
        let values = SnapshotValues::unknown("snapshot_timeout");
        assert!(values.admitted.starts_with("Unknown · snapshot_timeout"));
        assert!(values.withheld.starts_with("Unknown · snapshot_timeout"));
        assert!(values.budget.starts_with("Unknown · snapshot_timeout"));
    }

    #[test]
    fn health_identity_is_installed_native_and_complete() {
        let health = serde_json::json!({
            "serviceId": "membrane-hub",
            "runtimeOrigin": "installed",
            "nativeOnly": true,
            "installationId": "installation",
            "cortexStoreId": "store",
            "releaseGeneration": "release",
            "startupGeneration": 4,
            "stableInstallRoot": "C:/Membrane/current"
        });
        let identity = controller_identity_from_health(&health).unwrap();
        assert_eq!(identity.startup_generation, 4);
        assert_eq!(identity.stable_current, "C:/Membrane/current");
    }

    #[test]
    fn health_identity_rejects_non_installed_or_incomplete_fence() {
        let health = serde_json::json!({
            "serviceId": "membrane-hub",
            "runtimeOrigin": "development",
            "nativeOnly": true,
        });
        assert_eq!(
            controller_identity_from_health(&health).unwrap_err(),
            "resident_health_identity_invalid"
        );
    }

    #[test]
    fn health_503_valid_identity_then_status_returns_fenced_observation() {
        let status = ResidentHolderStatusV1 {
            controller_active: true,
            services_ready: false,
            services_unavailable_reason: Some(ResidentServicesUnavailableV1::BlueprintWatcherUnavailable),
            hub_holders: 1,
            coderight_daemon_holders: 0,
        };
        let endpoint = spawn_holder_fixture(status.clone(), false);
        let observation = fetch_remote_holder_status(&endpoint, &"a".repeat(64)).unwrap();
        assert_eq!(observation.controller.startup_generation, 4);
        assert_eq!(observation.status, status);
    }

    #[test]
    fn status_controller_mismatch_is_refused_after_valid_health_fence() {
        let status = ResidentHolderStatusV1 {
            controller_active: true,
            services_ready: true,
            services_unavailable_reason: None,
            hub_holders: 1,
            coderight_daemon_holders: 0,
        };
        let endpoint = spawn_holder_fixture(status, true);
        assert_eq!(
            fetch_remote_holder_status(&endpoint, &"b".repeat(64)).unwrap_err(),
            "resident_controller_mismatch"
        );
    }

    #[test]
    fn content_length_frames_keep_alive_response() {
        assert_eq!(
            content_length(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\ncontent-length: 42"
            ),
            Ok(42)
        );
        assert_eq!(
            content_length("HTTP/1.1 200 OK\r\ncontent-type: application/json"),
            Err("snapshot_invalid_http")
        );
        assert_eq!(
            content_length("HTTP/1.1 200 OK\r\ncontent-length: 1\r\nContent-Length: 1"),
            Err("snapshot_invalid_http")
        );
    }
}
