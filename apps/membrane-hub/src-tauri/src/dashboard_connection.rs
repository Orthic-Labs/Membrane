//! Native dashboard bootstrap & authenticated loopback transport.
//!
//! The resident tray launches this process with one inherited stdin pipe. The
//! pipe carries exactly one newline-delimited endpoint/token object. The token
//! is retained only in this native state; no command returns it to the
//! webview, and no environment or filesystem fallback is accepted.

use membrane_protocol::{
    HubSnapshotV1, ResidentControllerIdentityV1, ResidentHolderCredentialV1,
    ResidentHolderOperationV1, ResidentHolderRequestV1, ResidentHolderResponseV1,
    RESIDENT_HOLDER_SCHEMA_VERSION,
};
use membrane_client::{
    build_loopback_request_headers, build_loopback_response_headers, verify_loopback_response_headers, LoopbackAuthSigner,
    LoopbackIdentityFields, LoopbackRequestFields,
};
use serde::Deserialize;
use std::{
    io::{self, BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpStream},
    sync::{atomic::{AtomicBool, Ordering}, Arc},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use sha2::Digest;

pub const BOOTSTRAP_MAX_FRAME_BYTES: usize = 16 * 1024;
pub const HTTP_MAX_RESPONSE_BYTES: usize = 1024 * 1024 + 4096;

/// Default per-call deadline. Callers construct a fresh `Duration` for each
/// call site rather than storing a timeout on `DashboardConnection`; the
/// connection itself carries no construction-time deadline.
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const HUB_LEASE_TTL_MS: u64 = 60_000;
const HUB_LEASE_RENEW_LEAD_MS: u64 = 10_000;
const HUB_LEASE_RETRY_MS: u64 = 1_000;
const HUB_LEASE_RENEW_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DashboardBootstrap {
    endpoint: String,
    token: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResidentHealthIdentity {
    ok: bool,
    protocol_version: u32,
    schema_version: u32,
    native_only: bool,
    runtime_origin: String,
    installation_id: String,
    cortex_store_id: String,
    release_generation: String,
    startup_generation: u64,
    stable_install_root: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResidentLivezIdentity {
    installation_id: String,
    cortex_store_id: String,
    release_generation: String,
    startup_generation: u64,
    stable_install_root: String,
}

/// Authenticated resident connection held by the native backend.
///
/// This type deliberately has no `Serialize` implementation. Its loopback credential is
/// never part of a Tauri command result or any webview-facing state.
#[derive(Clone)]
pub struct DashboardConnection {
    endpoint: SocketAddr,
    api_token: String,
}

impl DashboardConnection {
    #[cfg(test)]
    pub fn endpoint(&self) -> SocketAddr {
        self.endpoint
    }

    /// Issue one bounded authenticated GET against resident loopback HTTP.
    pub fn get(&self, path: &str, timeout: Duration) -> Result<HttpResponse, String> {
        self.request("GET", path, None, timeout)
    }

    /// Forward a typed holder request over same authenticated loopback
    /// connection. Credentials remain in caller-owned request state; loopback credential
    /// never leaves this native connection.
    pub fn dispatch_resident_holder(
        &self,
        request: &ResidentHolderRequestV1,
        timeout: Duration,
    ) -> Result<ResidentHolderResponseV1, String> {
        let body = serde_json::to_vec(request).map_err(|_| "dashboard_request_invalid")?;
        let response = self.request("POST", "/resident-holder", Some(&body), timeout)?;
        if response.status != 200 {
            return Err("dashboard_resident_holder_rejected".into());
        }
        serde_json::from_slice(&response.body).map_err(|_| "dashboard_resident_holder_invalid".into())
    }

    /// Derive the installed controller fence from authenticated resident
    /// health. No bootstrap field or dashboard input can supply this identity.
    pub fn controller_identity(&self, timeout: Duration) -> Result<ResidentControllerIdentityV1, String> {
        let response = self.get("/health", timeout)?;
        // Holder acquisition precedes semantic catch-up. Authenticated 503
        // health still carries the installed controller fence; it does not
        // authorize semantic requests or claim services are ready.
        if !matches!(response.status, 200 | 503) {
            return Err("dashboard_resident_health_unavailable".into());
        }
        let health: ResidentHealthIdentity = serde_json::from_slice(&response.body)
            .map_err(|_| "dashboard_resident_health_invalid")?;
        if (response.status == 200 && !health.ok)
            || health.protocol_version != RESIDENT_HOLDER_SCHEMA_VERSION
            || health.schema_version != RESIDENT_HOLDER_SCHEMA_VERSION
            || !health.native_only
            || health.runtime_origin != "installed"
            || health.installation_id.trim().is_empty()
            || health.cortex_store_id.trim().is_empty()
            || health.release_generation.trim().is_empty()
            || health.startup_generation == 0
            || health.stable_install_root.trim().is_empty()
            || !std::path::Path::new(&health.stable_install_root).is_absolute()
            || [
                &health.installation_id,
                &health.cortex_store_id,
                &health.release_generation,
                &health.stable_install_root,
            ]
            .iter()
            .any(|value| value.chars().any(char::is_control))
        {
            return Err("dashboard_resident_identity_invalid".into());
        }
        Ok(ResidentControllerIdentityV1 {
            installation_id: health.installation_id,
            cortex_store_id: health.cortex_store_id,
            release_generation: health.release_generation,
            startup_generation: health.startup_generation,
            stable_current: health.stable_install_root,
        })
    }

    /// Read the resident's published hub snapshot over the same authenticated
    /// loopback connection. This is an audited read of resident-published
    /// state: the Hub never composes or republishes its own snapshot, it only
    /// forwards the resident's `GET /hub/snapshot` response to the webview.
    pub fn fetch_hub_snapshot(&self, timeout: Duration) -> Result<HubSnapshotV1, String> {
        let response = self.get("/hub/snapshot", timeout)?;
        if response.status != 200 {
            return Err("dashboard_hub_snapshot_unavailable".into());
        }
        serde_json::from_slice(&response.body).map_err(|_| "dashboard_hub_snapshot_invalid".into())
    }

    fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<&[u8]>,
        timeout: Duration,
    ) -> Result<HttpResponse, String> {
        if !path.starts_with('/') || path.bytes().any(|byte| matches!(byte, b'\r' | b'\n')) {
            return Err("dashboard_request_invalid".into());
        }
        if !matches!(method, "GET" | "POST") {
            return Err("dashboard_request_invalid".into());
        }
        let identity = if path == "/livez" {
            None
        } else {
            Some(self.discover_identity(timeout)?)
        };
        let mut stream = TcpStream::connect_timeout(&self.endpoint, timeout)
            .map_err(|_| "dashboard_resident_unavailable")?;
        stream
            .set_read_timeout(Some(timeout))
            .map_err(|_| "dashboard_request_timeout")?;
        stream
            .set_write_timeout(Some(timeout))
            .map_err(|_| "dashboard_request_timeout")?;
        let body = body.unwrap_or(&[]);
        let (request, request_fields) = if let Some(identity) = identity {
            let signer = LoopbackAuthSigner::from_hex_token(&self.api_token).map_err(|_| "dashboard_auth_invalid")?;
            let now = now_unix_ms() / 1000;
            let expiry = LoopbackAuthSigner::bounded_expiry(now, 10);
            let nonce = LoopbackAuthSigner::generate_nonce().map_err(|_| "dashboard_auth_unavailable")?;
            let content_type = if method == "POST" { "application/json" } else { "" };
            let headers = build_loopback_request_headers(&signer, &identity, method, path, "127.0.0.1", content_type, body, nonce, expiry)
                .map_err(|_| "dashboard_auth_invalid")?;
            let request_fields = LoopbackRequestFields { method: method.into(), target: path.into(), host: "127.0.0.1".into(), content_type: content_type.into(), body_sha256: sha2::Sha256::digest(body).into(), identity, nonce, expiry_unix_secs: expiry };
            (format_http_request(method, path, &headers), Some((signer, request_fields)))
        } else {
            (format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"), None)
        };
        stream
            .write_all(request.as_bytes())
            .map_err(|_| "dashboard_request_write_failed")?;
        if !body.is_empty() {
            stream
                .write_all(body)
                .map_err(|_| "dashboard_request_write_failed")?;
        }
        let raw = read_http_response(&mut stream)?;
        let response = parse_http_response(&raw)?;
        if let Some((signer, request)) = request_fields {
            let identity = request.identity.clone();
            verify_loopback_response_headers(&signer, &response.headers, &request, response.status, &response.body, &identity, now_unix_ms() / 1000)
                .map_err(|_| "dashboard_response_auth_invalid")?;
        }
        Ok(response)
    }

    fn discover_identity(&self, timeout: Duration) -> Result<LoopbackIdentityFields, String> {
        let response = self.request("GET", "/livez", None, timeout)?;
        let livez: ResidentLivezIdentity = serde_json::from_slice(&response.body).map_err(|_| "dashboard_resident_health_invalid")?;
        if livez.installation_id.trim().is_empty() || livez.cortex_store_id.trim().is_empty() || livez.release_generation.trim().is_empty() || livez.startup_generation == 0 || livez.stable_install_root.trim().is_empty() { return Err("dashboard_resident_identity_invalid".into()); }
        Ok(LoopbackIdentityFields { installation_id: livez.installation_id, cortex_store_id: livez.cortex_store_id, release_generation: livez.release_generation, startup_generation: livez.startup_generation, stable_install_root: livez.stable_install_root })
    }
}

fn format_http_request(method: &str, path: &str, headers: &[(String, String)]) -> String {
    let mut out = format!("{method} {path} HTTP/1.1\r\n");
    for (name, value) in headers { out.push_str(name); out.push_str(": "); out.push_str(value); out.push_str("\r\n"); }
    out.push_str("\r\n"); out
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn native_hub_holder() -> ResidentHolderCredentialV1 {
    // PID is process-local native state. It is never returned by a Tauri
    // command or serialized into webview state.
    let process_id = std::process::id();
    let holder_id = format!("hub-process-{process_id}");
    ResidentHolderCredentialV1 {
        holder_kind: "hub".into(),
        holder_id: holder_id.clone(),
        credential_id: format!("{holder_id}-credential"),
    }
}

fn holder_request(
    operation: ResidentHolderOperationV1,
    controller: &ResidentControllerIdentityV1,
    holder: Option<ResidentHolderCredentialV1>,
    observed_at_unix_ms: u64,
    expires_at_unix_ms: Option<u64>,
) -> ResidentHolderRequestV1 {
    ResidentHolderRequestV1 {
        schema_version: RESIDENT_HOLDER_SCHEMA_VERSION,
        operation,
        controller: controller.clone(),
        holder,
        expires_at_unix_ms,
        observed_at_unix_ms,
        loss_cursor: None,
    }
}

fn response_matches(
    response: &ResidentHolderResponseV1,
    operation: ResidentHolderOperationV1,
    controller: &ResidentControllerIdentityV1,
) -> bool {
    response.schema_version == RESIDENT_HOLDER_SCHEMA_VERSION
        && response.operation == operation
        && response.controller == *controller
        && response.status.controller_active
}

struct HubResidentLease {
    connection: DashboardConnection,
    controller: ResidentControllerIdentityV1,
    holder: ResidentHolderCredentialV1,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    released: Arc<AtomicBool>,
}

impl HubResidentLease {
    fn start(
        connection: DashboardConnection,
        controller: ResidentControllerIdentityV1,
        holder: ResidentHolderCredentialV1,
        expiry: u64,
    ) -> Result<Self, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let released = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker_connection = connection.clone();
        let worker_controller = controller.clone();
        let worker_holder = holder.clone();
        let worker = thread::Builder::new()
            .name("membrane-hub-holder-renew".into())
            .spawn(move || {
                renew_loop(
                    worker_connection,
                    worker_controller,
                    worker_holder,
                    expiry,
                    worker_stop,
                )
            })
            .map_err(|_| "dashboard_holder_renew_unavailable")?;
        Ok(Self {
            connection,
            controller,
            holder,
            stop,
            worker: Some(worker),
            released,
        })
    }

    fn release_once(&mut self) {
        if self.released.swap(true, Ordering::AcqRel) {
            return;
        }
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.thread().unpark();
            let _ = worker.join();
        }
        let now = now_unix_ms();
        let request = holder_request(
            ResidentHolderOperationV1::Release,
            &self.controller,
            Some(self.holder.clone()),
            now,
            None,
        );
        let _ = self
            .connection
            .dispatch_resident_holder(&request, DEFAULT_REQUEST_TIMEOUT);
    }
}

impl Drop for HubResidentLease {
    fn drop(&mut self) {
        self.release_once();
    }
}

fn renew_loop(
    connection: DashboardConnection,
    controller: ResidentControllerIdentityV1,
    holder: ResidentHolderCredentialV1,
    mut expiry: u64,
    stop: Arc<AtomicBool>,
) {
    let initial_delay = expiry
        .saturating_sub(now_unix_ms())
        .saturating_sub(HUB_LEASE_RENEW_LEAD_MS);
    let mut next_renewal = Instant::now()
        + Duration::from_millis(initial_delay);
    loop {
        if stop.load(Ordering::Acquire) {
            return;
        }
        let now = now_unix_ms();
        if now >= expiry {
            return;
        }
        if Instant::now() < next_renewal {
            thread::park_timeout(Duration::from_millis(250));
            continue;
        }
        let now = now_unix_ms();
        if now >= expiry {
            return;
        }
        let renewed_expiry = now.saturating_add(HUB_LEASE_TTL_MS);
        if renewed_expiry <= expiry {
            thread::park_timeout(Duration::from_millis(HUB_LEASE_RETRY_MS));
            continue;
        }
        let request = holder_request(
            ResidentHolderOperationV1::Renew,
            &controller,
            Some(holder.clone()),
            now,
            Some(renewed_expiry),
        );
        let remaining_ms = expiry.saturating_sub(now).max(1);
        let timeout_ms = remaining_ms.min(HUB_LEASE_RENEW_TIMEOUT.as_millis() as u64);
        let renewed = connection
            .dispatch_resident_holder(&request, Duration::from_millis(timeout_ms))
            .is_ok_and(|response| {
                response_matches(&response, ResidentHolderOperationV1::Renew, &controller)
            });
        if renewed {
            expiry = renewed_expiry;
            next_renewal = Instant::now()
                + Duration::from_millis(HUB_LEASE_TTL_MS.saturating_sub(HUB_LEASE_RENEW_LEAD_MS));
        } else {
            thread::park_timeout(Duration::from_millis(HUB_LEASE_RETRY_MS));
        }
    }
}

fn read_http_response(stream: &mut TcpStream) -> Result<Vec<u8>, String> {
    let mut raw = Vec::new();
    let mut chunk = [0_u8; 8 * 1024];
    let mut expected_bytes = None;
    loop {
        let read = stream
            .read(&mut chunk)
            .map_err(|_| "dashboard_request_read_failed")?;
        if read == 0 {
            break;
        }
        raw.extend_from_slice(&chunk[..read]);
        if raw.len() > HTTP_MAX_RESPONSE_BYTES {
            return Err("dashboard_response_too_large".into());
        }
        if expected_bytes.is_none() {
            if let Some(split) = header_end(&raw) {
                let header =
                    std::str::from_utf8(&raw[..split]).map_err(|_| "dashboard_response_invalid")?;
                let content_length = content_length(header)?;
                let total = split
                    .checked_add(content_length)
                    .ok_or("dashboard_response_too_large")?;
                if total > HTTP_MAX_RESPONSE_BYTES {
                    return Err("dashboard_response_too_large".into());
                }
                expected_bytes = Some(total);
            }
        }
        if expected_bytes.is_some_and(|expected| raw.len() >= expected) {
            break;
        }
    }
    Ok(raw)
}

#[derive(Debug, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// Parse one complete HTTP response from resident's close-delimited reply.
///
/// The resident currently sends HTTP/1.1 with `Connection: close`; this parser
/// deliberately does not implement redirects, chunked transfer, or another
/// network protocol. The endpoint is fixed to loopback during bootstrap.
pub fn parse_http_response(raw: &[u8]) -> Result<HttpResponse, String> {
    if raw.len() > HTTP_MAX_RESPONSE_BYTES {
        return Err("dashboard_response_too_large".into());
    }
    let split = header_end(raw).ok_or("dashboard_response_invalid")?;
    let header = std::str::from_utf8(&raw[..split]).map_err(|_| "dashboard_response_invalid")?;
    let status_line = header.lines().next().ok_or("dashboard_response_invalid")?;
    let mut parts = status_line.split_whitespace();
    let version = parts.next().ok_or("dashboard_response_invalid")?;
    let status = parts
        .next()
        .ok_or("dashboard_response_invalid")?
        .parse::<u16>()
        .map_err(|_| "dashboard_response_invalid")?;
    if version != "HTTP/1.1" && version != "HTTP/1.0" {
        return Err("dashboard_response_invalid".into());
    }
    let headers = response_headers(header)?;
    let content_length = content_length(header)?;
    let body_end = split
        .checked_add(content_length)
        .ok_or("dashboard_response_too_large")?;
    let body = raw
        .get(split..body_end)
        .ok_or("dashboard_response_invalid")?;
    Ok(HttpResponse {
        status,
        headers,
        body: body.to_vec(),
    })
}

fn response_headers(header: &str) -> Result<Vec<(String, String)>, String> {
    let mut headers = Vec::new();
    for line in header.lines().skip(1) {
        if line.is_empty() { continue; }
        let (name, value) = line.split_once(':').ok_or("dashboard_response_invalid")?;
        headers.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
    }
    Ok(headers)
}

fn header_end(raw: &[u8]) -> Option<usize> {
    raw.windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
        .or_else(|| {
            raw.windows(2)
                .position(|window| window == b"\n\n")
                .map(|index| index + 2)
        })
}

fn content_length(header: &str) -> Result<usize, String> {
    let mut value = None;
    for line in header.lines().skip(1) {
        let Some((name, raw_value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            let parsed = raw_value
                .trim()
                .parse::<usize>()
                .map_err(|_| "dashboard_response_invalid")?;
            if value.replace(parsed).is_some() {
                return Err("dashboard_response_invalid".into());
            }
        }
    }
    value.ok_or_else(|| "dashboard_response_invalid".into())
}

/// Decode and validate the one-shot bootstrap frame.
pub fn parse_bootstrap_frame(frame: &[u8]) -> Result<DashboardConnection, String> {
    if frame.len() > BOOTSTRAP_MAX_FRAME_BYTES {
        return Err("dashboard_bootstrap_too_large".into());
    }
    let line = frame
        .strip_suffix(b"\n")
        .ok_or("dashboard_bootstrap_invalid")?;
    if line.is_empty() || line.last() == Some(&b'\r') || line.iter().any(|byte| *byte == b'\n') {
        return Err("dashboard_bootstrap_invalid".into());
    }
    let payload: DashboardBootstrap =
        serde_json::from_slice(line).map_err(|_| "dashboard_bootstrap_invalid")?;
    let endpoint = parse_loopback_endpoint(&payload.endpoint)?;
    validate_api_token(&payload.token)?;
    Ok(DashboardConnection {
        endpoint,
        api_token: payload.token,
    })
}

/// Read the first frame from inherited stdin. Windows console launches are
/// rejected before reading so opening the dashboard directly never blocks on
/// an interactive console; tray launches provide an anonymous pipe.
pub fn read_bootstrap_from_stdin() -> Result<DashboardConnection, String> {
    #[cfg(target_os = "windows")]
    if !stdin_is_pipe() {
        return Err("dashboard_bootstrap_unavailable".into());
    }

    let stdin = io::stdin();
    let reader = BufReader::new(stdin.lock());
    let mut frame = Vec::new();
    let read = reader
        .take((BOOTSTRAP_MAX_FRAME_BYTES + 1) as u64)
        .read_until(b'\n', &mut frame)
        .map_err(|_| "dashboard_bootstrap_unavailable")?;
    if read == 0 {
        return Err("dashboard_bootstrap_unavailable".into());
    }
    if frame.len() > BOOTSTRAP_MAX_FRAME_BYTES {
        return Err("dashboard_bootstrap_too_large".into());
    }
    parse_bootstrap_frame(&frame)
}

pub struct DashboardConnectionState {
    connection: Option<DashboardConnection>,
    bootstrap_error: Option<String>,
    lease: Option<HubResidentLease>,
}

impl DashboardConnectionState {
    pub fn from_stdin() -> Self {
        match read_bootstrap_from_stdin() {
            Ok(connection) => Self {
                connection: Some(connection),
                bootstrap_error: None,
                lease: None,
            },
            Err(error) => Self {
                connection: None,
                bootstrap_error: Some(error),
                lease: None,
            },
        }
    }

    /// Authenticate the installed controller, acquire this process's Hub
    /// holder, then retain the renewable lease in native state.
    pub fn acquire_holder(mut self) -> Result<Self, String> {
        let connection = self.connection.clone().ok_or_else(|| {
            self.bootstrap_error
                .clone()
                .unwrap_or_else(|| "dashboard_bootstrap_unavailable".into())
        })?;
        let controller = connection.controller_identity(DEFAULT_REQUEST_TIMEOUT)?;
        let holder = native_hub_holder();
        let now = now_unix_ms();
        let expiry = now.saturating_add(HUB_LEASE_TTL_MS);
        let request = holder_request(
            ResidentHolderOperationV1::Acquire,
            &controller,
            Some(holder.clone()),
            now,
            Some(expiry),
        );
        let response = connection.dispatch_resident_holder(&request, DEFAULT_REQUEST_TIMEOUT);
        let accepted = response.as_ref().is_ok_and(|response| {
            response_matches(response, ResidentHolderOperationV1::Acquire, &controller)
                && response.status.hub_holders > 0
        });
        if !accepted {
            // Acquire may have committed before a response was malformed or
            // reported an invalid holder. Roll back with one bounded release attempt.
            let release = holder_request(
                ResidentHolderOperationV1::Release,
                &controller,
                Some(holder.clone()),
                now_unix_ms(),
                None,
            );
            let _ = connection.dispatch_resident_holder(&release, DEFAULT_REQUEST_TIMEOUT);
            return Err(response
                .err()
                .unwrap_or_else(|| "dashboard_holder_not_ready".into()));
        }
        self.lease = Some(match HubResidentLease::start(
            connection.clone(),
            controller.clone(),
            holder.clone(),
            expiry,
        ) {
            Ok(lease) => lease,
            Err(error) => {
                let release = holder_request(
                    ResidentHolderOperationV1::Release,
                    &controller,
                    Some(holder),
                    now_unix_ms(),
                    None,
                );
                let _ = connection.dispatch_resident_holder(&release, DEFAULT_REQUEST_TIMEOUT);
                return Err(error);
            }
        });
        Ok(self)
    }

    /// Release the Hub holder at most once. Dashboard close/hide paths do not
    /// call this; Tauri process exit and setup rollback do.
    pub fn release_once(&mut self) {
        if let Some(lease) = self.lease.as_mut() {
            lease.release_once();
        }
    }

    pub fn connection(&self) -> Result<DashboardConnection, String> {
        self.connection.clone().ok_or_else(|| {
            self.bootstrap_error
                .clone()
                .unwrap_or_else(|| "dashboard_bootstrap_unavailable".into())
        })
    }

    #[cfg(test)]
    pub fn with_connection(connection: DashboardConnection) -> Self {
        Self {
            connection: Some(connection),
            bootstrap_error: None,
            lease: None,
        }
    }
}

fn parse_loopback_endpoint(raw: &str) -> Result<SocketAddr, String> {
    let endpoint = raw.trim();
    let authority = endpoint
        .strip_prefix("http://")
        .filter(|value| !value.is_empty())
        .ok_or("dashboard_bootstrap_endpoint_invalid")?;
    if authority
        .bytes()
        .any(|byte| byte.is_ascii_whitespace() || matches!(byte, b'/' | b'?' | b'#' | b'@'))
    {
        return Err("dashboard_bootstrap_endpoint_invalid".into());
    }
    let address = authority
        .parse::<SocketAddr>()
        .map_err(|_| "dashboard_bootstrap_endpoint_invalid")?;
    if !address.ip().is_loopback() || address.port() == 0 {
        return Err("dashboard_bootstrap_endpoint_invalid".into());
    }
    Ok(address)
}

fn validate_api_token(token: &str) -> Result<(), String> {
    if token.len() != 64
        || !token
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err("dashboard_bootstrap_token_invalid".into());
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn stdin_is_pipe() -> bool {
    use std::{ffi::c_void, os::windows::io::AsRawHandle};

    const FILE_TYPE_PIPE: u32 = 0x0003;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetFileType(file: *mut c_void) -> u32;
    }

    let stdin = io::stdin();
    let handle = stdin.as_raw_handle() as *mut c_void;
    // SAFETY: `handle` is the live process stdin handle supplied by Windows.
    unsafe { GetFileType(handle) == FILE_TYPE_PIPE }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Cursor, net::TcpListener, thread};

    fn frame(endpoint: &str, token: &str) -> Vec<u8> {
        let mut frame = format!(r#"{{"endpoint":"{endpoint}","token":"{token}"}}"#).into_bytes();
        frame.push(b'\n');
        frame
    }

    #[test]
    fn bootstrap_accepts_loopback_endpoint_and_keeps_token_native() {
        let connection =
            parse_bootstrap_frame(&frame("http://127.0.0.1:4317", &"a".repeat(64))).unwrap();
        assert_eq!(connection.endpoint(), "127.0.0.1:4317".parse().unwrap());
        // DashboardConnection intentionally has no Serialize implementation;
        // only endpoint is observable by this native test accessor.
        let state = DashboardConnectionState::with_connection(connection);
        assert!(state.connection().is_ok());
    }

    #[test]
    fn bootstrap_rejects_invalid_endpoint_token_and_unknown_fields() {
        let cases: [(&str, String, &str); 4] = [
            (
                "http://192.168.1.3:4317",
                "a".repeat(64),
                "dashboard_bootstrap_endpoint_invalid",
            ),
            (
                "https://127.0.0.1:4317",
                "a".repeat(64),
                "dashboard_bootstrap_endpoint_invalid",
            ),
            (
                "http://127.0.0.1:4317",
                "A".repeat(64),
                "dashboard_bootstrap_token_invalid",
            ),
            (
                "http://127.0.0.1:4317",
                "short".into(),
                "dashboard_bootstrap_token_invalid",
            ),
        ];
        for (endpoint, token, expected) in cases {
            assert_eq!(
                parse_bootstrap_frame(&frame(endpoint, &token))
                    .err()
                    .unwrap(),
                expected
            );
        }
        assert_eq!(
            parse_bootstrap_frame(
                br#"{"endpoint":"http://127.0.0.1:4317","token":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","extra":true}
"#
            )
            .err()
            .unwrap(),
            "dashboard_bootstrap_invalid"
        );
    }

    #[test]
    fn bootstrap_rejects_invalid_and_oversize_frames_before_decode() {
        assert_eq!(
            parse_bootstrap_frame(b"{}".as_slice()).err().unwrap(),
            "dashboard_bootstrap_invalid"
        );
        assert_eq!(
            parse_bootstrap_frame(&vec![b'x'; BOOTSTRAP_MAX_FRAME_BYTES + 1])
                .err()
                .unwrap(),
            "dashboard_bootstrap_too_large"
        );
        let mut missing_newline = frame("http://127.0.0.1:4317", &"a".repeat(64));
        missing_newline.pop();
        assert_eq!(
            parse_bootstrap_frame(&missing_newline).err().unwrap(),
            "dashboard_bootstrap_invalid"
        );
        let mut reader = Cursor::new(Vec::new());
        let read = reader
            .read_until(b'\n', &mut Vec::new())
            .expect("cursor read");
        assert_eq!(read, 0);
    }

    #[test]
    fn http_response_parser_is_bounded_and_status_preserving() {
        let response = parse_http_response(
            b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 12\r\n\r\n{\"ok\":false}",
        )
        .unwrap();
        assert_eq!(response.status, 503);
        assert_eq!(response.body, br#"{"ok":false}"#);
        assert_eq!(
            parse_http_response(&vec![b'x'; HTTP_MAX_RESPONSE_BYTES + 1]).unwrap_err(),
            "dashboard_response_too_large"
        );
    }

    #[test]
    fn health_discovery_sends_no_credential_header() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = listener.local_addr().unwrap();
        let token = "a".repeat(64);
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut chunk = [0_u8; 1024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let read = stream.read(&mut chunk).unwrap();
                assert!(read > 0, "client closed before request headers");
                request.extend_from_slice(&chunk[..read]);
            }
            let request = String::from_utf8(request).unwrap();
            assert!(!request.contains("Authorization:"));
            assert!(request.starts_with("GET /livez HTTP/1.1\r\n"));
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .unwrap();
        });
        let connection = DashboardConnection {
            endpoint,
            api_token: token,
        };
        let response = connection.get("/livez", Duration::from_secs(1)).unwrap();
        server.join().unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"ok");
    }

    #[test]
    fn authenticated_get_honors_content_length_without_socket_close() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut chunk = [0_u8; 1024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let read = stream.read(&mut chunk).unwrap();
                assert!(read > 0);
                request.extend_from_slice(&chunk[..read]);
            }
            assert!(String::from_utf8_lossy(&request).starts_with("GET /livez HTTP/1.1\r\n"));
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok").unwrap();
            thread::sleep(Duration::from_millis(250));
        });
        let connection = DashboardConnection {
            endpoint,
            api_token: "a".repeat(64),
        };
        let response = connection
            .get("/livez", Duration::from_millis(100))
            .unwrap();
        assert_eq!(response.body, b"ok");
        server.join().unwrap();
    }

    fn health_body(ok: bool, runtime_origin: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "ok": ok,
            "protocolVersion": RESIDENT_HOLDER_SCHEMA_VERSION,
            "schemaVersion": RESIDENT_HOLDER_SCHEMA_VERSION,
            "nativeOnly": true,
            "runtimeOrigin": runtime_origin,
            "installationId": "install-1",
            "cortexStoreId": "store-1",
            "releaseGeneration": "release-1",
            "startupGeneration": 1,
            "stableInstallRoot": "C:/Membrane/current"
        }))
        .unwrap()
    }

    fn authenticated_health_fixture(
        status: u16,
        body: Vec<u8>,
        token: &str,
    ) -> (DashboardConnection, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = listener.local_addr().unwrap();
        let fixture_token = token.to_owned();
        let response_body = body.clone();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut chunk = [0_u8; 1024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") { let read = stream.read(&mut chunk).unwrap(); assert!(read > 0); request.extend_from_slice(&chunk[..read]); }
            assert!(String::from_utf8_lossy(&request).starts_with("GET /livez HTTP/1.1\r\n"));
            let livez = health_body(true, "installed");
            stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", livez.len()).as_bytes()).unwrap();
            stream.write_all(&livez).unwrap();
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut chunk = [0_u8; 1024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let read = stream.read(&mut chunk).unwrap();
                assert!(read > 0);
                request.extend_from_slice(&chunk[..read]);
            }
            let request = String::from_utf8(request).unwrap();
            assert!(request.starts_with("GET /health HTTP/1.1\r\n"));
            let value = |name: &str| request.lines().find_map(|line| line.split_once(':').filter(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.trim()));
            let nonce = hex::decode(value("x-membrane-loopback-nonce").unwrap()).unwrap().try_into().unwrap();
            let expiry = value("x-membrane-loopback-expiry").unwrap().parse().unwrap();
            let identity = LoopbackIdentityFields { installation_id: "install-1".into(), cortex_store_id: "store-1".into(), release_generation: "release-1".into(), startup_generation: 1, stable_install_root: "C:/Membrane/current".into() };
            let signer = LoopbackAuthSigner::from_hex_token(&fixture_token).unwrap();
            let response_headers = build_loopback_response_headers(&signer, &identity, nonce, status, &response_body, expiry).unwrap();
            let mut header = format!("HTTP/1.1 {status} {}\r\n", if status == 200 { "OK" } else { "Service Unavailable" });
            for (name, value) in response_headers { header.push_str(&format!("{name}: {value}\r\n")); }
            header.push_str("\r\n"); stream.write_all(header.as_bytes()).unwrap();
            stream.write_all(&response_body).unwrap();
        });
        (
            DashboardConnection {
                endpoint,
                api_token: token.to_owned(),
            },
            server,
        )
    }

    fn snapshot_fixture(status: u16, body: Vec<u8>, token: &str) -> (DashboardConnection, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = listener.local_addr().unwrap();
        let fixture_token = token.to_owned();
        let response_body = body.clone();
        let server = thread::spawn(move || {
            let mut serve = |stream: &mut TcpStream| {
                let mut request = Vec::new(); let mut chunk = [0_u8; 1024];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") { let read = stream.read(&mut chunk).unwrap(); assert!(read > 0); request.extend_from_slice(&chunk[..read]); }
                request
            };
            let mut stream = listener.accept().unwrap().0;
            let _ = serve(&mut stream);
            let livez = health_body(true, "installed");
            stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", livez.len()).as_bytes()).unwrap(); stream.write_all(&livez).unwrap();
            let mut stream = listener.accept().unwrap().0;
            let request = serve(&mut stream); let request = String::from_utf8(request).unwrap();
            assert!(request.starts_with("GET /hub/snapshot HTTP/1.1\r\n"));
            let value = |name: &str| request.lines().find_map(|line| line.split_once(':').filter(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.trim()));
            let nonce = hex::decode(value("x-membrane-loopback-nonce").unwrap()).unwrap().try_into().unwrap();
            let expiry = value("x-membrane-loopback-expiry").unwrap().parse().unwrap();
            let identity = LoopbackIdentityFields { installation_id: "install-1".into(), cortex_store_id: "store-1".into(), release_generation: "release-1".into(), startup_generation: 1, stable_install_root: "C:/Membrane/current".into() };
            let signer = LoopbackAuthSigner::from_hex_token(&fixture_token).unwrap();
            let response_headers = build_loopback_response_headers(&signer, &identity, nonce, status, &response_body, expiry).unwrap();
            let mut header = format!("HTTP/1.1 {status} {}\r\n", if status == 200 { "OK" } else { "Service Unavailable" }); for (name, value) in response_headers { header.push_str(&format!("{name}: {value}\r\n")); } header.push_str("\r\n"); stream.write_all(header.as_bytes()).unwrap(); stream.write_all(&response_body).unwrap();
        });
        (DashboardConnection { endpoint, api_token: token.to_owned() }, server)
    }

    #[test]
    fn controller_identity_accepts_authenticated_catch_up_health_503() {
        let token = "b".repeat(64);
        let (connection, server) =
            authenticated_health_fixture(503, health_body(false, "installed"), &token);
        let identity = connection
            .controller_identity(Duration::from_secs(1))
            .expect("authenticated 503 still carries canonical controller identity");
        server.join().unwrap();
        assert_eq!(identity.installation_id, "install-1");
        assert_eq!(identity.startup_generation, 1);
    }

    #[test]
    fn controller_identity_rejects_invalid_authenticated_catch_up_identity() {
        let token = "c".repeat(64);
        let (connection, server) =
            authenticated_health_fixture(503, health_body(false, "development"), &token);
        let error = connection
            .controller_identity(Duration::from_secs(1))
            .unwrap_err();
        server.join().unwrap();
        assert_eq!(error, "dashboard_resident_identity_invalid");
    }

    fn holder_response(
        controller: &ResidentControllerIdentityV1,
        controller_active: bool,
        services_ready: bool,
    ) -> ResidentHolderResponseV1 {
        ResidentHolderResponseV1 {
            schema_version: RESIDENT_HOLDER_SCHEMA_VERSION,
            operation: ResidentHolderOperationV1::Acquire,
            controller: controller.clone(),
            status: membrane_protocol::ResidentHolderStatusV1 {
                controller_active,
                services_ready,
                services_unavailable_reason: None,
                hub_holders: 1,
                coderight_daemon_holders: 0,
            },
            loss: None,
        }
    }

    #[test]
    fn response_matches_keeps_active_holder_during_initial_indexing() {
        let controller = ResidentControllerIdentityV1 {
            installation_id: "install-1".into(),
            cortex_store_id: "store-1".into(),
            release_generation: "release-1".into(),
            startup_generation: 1,
            stable_current: "C:/Membrane/current".into(),
        };
        let response = holder_response(&controller, true, false);
        assert!(response_matches(
            &response,
            ResidentHolderOperationV1::Acquire,
            &controller
        ));
    }

    #[test]
    fn response_matches_rejects_inactive_or_mismatched_holder() {
        let controller = ResidentControllerIdentityV1 {
            installation_id: "install-1".into(),
            cortex_store_id: "store-1".into(),
            release_generation: "release-1".into(),
            startup_generation: 1,
            stable_current: "C:/Membrane/current".into(),
        };
        let inactive = holder_response(&controller, false, false);
        assert!(!response_matches(
            &inactive,
            ResidentHolderOperationV1::Acquire,
            &controller
        ));
        let other_controller = ResidentControllerIdentityV1 {
            startup_generation: 2,
            ..controller.clone()
        };
        let mismatched = holder_response(&controller, true, false);
        assert!(!response_matches(
            &mismatched,
            ResidentHolderOperationV1::Acquire,
            &other_controller
        ));
    }

    #[test]
    fn hub_snapshot_reads_resident_published_state_only() {
        let body =
            br#"{"schemaVersion":1,"productId":"membrane-hub","observedAtUnixMs":0,"sections":{}}"#;
        let (connection, server) = snapshot_fixture(200, body.to_vec(), &"a".repeat(64));
        let snapshot = connection
            .fetch_hub_snapshot(DEFAULT_REQUEST_TIMEOUT)
            .expect("snapshot deserializes from resident response only");
        server.join().unwrap();
        assert_eq!(snapshot.sections.len(), 0);
    }

    #[test]
    fn hub_snapshot_rejects_non_200_resident_response() {
        let (connection, server) = snapshot_fixture(503, Vec::new(), &"a".repeat(64));
        let error = connection
            .fetch_hub_snapshot(DEFAULT_REQUEST_TIMEOUT)
            .unwrap_err();
        server.join().unwrap();
        assert_eq!(error, "dashboard_hub_snapshot_unavailable");
    }
}
