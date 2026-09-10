//! Authenticated installed-resident health probe.
use std::{
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpStream},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use membrane_client::{
    build_loopback_request_headers, verify_loopback_request_headers,
    verify_loopback_response_headers, LoopbackAuthSigner, LoopbackIdentityFields,
};

const MAX_HEADERS: usize = 16 * 1024;
const MAX_BODY: usize = 2 * 1024 * 1024;

pub struct HealthResponse {
    pub status: u16,
    pub body: Vec<u8>,
    pub identity: LoopbackIdentityFields,
}

struct HttpResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

/// Discovery supplies hints only. The subsequent signed response proves them.
pub fn probe(port: u16, token: &str, timeout: Duration) -> Result<HealthResponse, &'static str> {
    probe_at_path(port, token, timeout, None)
}

/// Installed callers pass the already resolved canonical runtime token path.
pub fn probe_installed(
    port: u16, token: &str, timeout: Duration, token_path: &std::path::Path,
) -> Result<HealthResponse, &'static str> {
    probe_at_path(port, token, timeout, Some(token_path))
}

fn probe_at_path(
    port: u16, token: &str, timeout: Duration, token_path: Option<&std::path::Path>,
) -> Result<HealthResponse, &'static str> {
    let deadline = Instant::now() + timeout;
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let livez = exchange(address,
        b"GET /livez HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
        deadline)?;
    if livez.status != 200 { return Err("health_livez_unavailable"); }
    let hints: serde_json::Value = serde_json::from_slice(&livez.body)
        .map_err(|_| "health_livez_invalid")?;
    let field = |name: &str| hints.get(name).and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty()).map(str::to_owned)
        .ok_or("health_identity_invalid");
    let identity = LoopbackIdentityFields {
        installation_id: field("installationId")?,
        cortex_store_id: field("cortexStoreId")?,
        release_generation: field("releaseGeneration")?,
        startup_generation: hints.get("startupGeneration").and_then(serde_json::Value::as_u64)
            .filter(|value| *value != 0).ok_or("health_identity_invalid")?,
        stable_install_root: field("stableInstallRoot")?,
    };
    probe_at_deadline(address, token, deadline, identity, token_path)
}

pub fn probe_with_identity(
    address: SocketAddr, token: &str, timeout: Duration, identity: LoopbackIdentityFields,
) -> Result<HealthResponse, &'static str> {
    probe_at_deadline(address, token, Instant::now() + timeout, identity, None)
}

fn probe_at_deadline(
    address: SocketAddr, token: &str, deadline: Instant, identity: LoopbackIdentityFields,
    token_path: Option<&std::path::Path>,
) -> Result<HealthResponse, &'static str> {
    let signer = LoopbackAuthSigner::from_hex_token(token).map_err(|_| "health_auth_invalid")?;
    let nonce = LoopbackAuthSigner::generate_nonce().map_err(|_| "health_auth_unavailable")?;
    if let Some(path) = token_path { fence_token(path, token)?; }
    let now = now();
    let expiry = LoopbackAuthSigner::bounded_expiry(now, 10);
    let headers = build_loopback_request_headers(
        &signer, &identity, "GET", "/health", "127.0.0.1", "", &[], nonce, expiry,
    ).map_err(|_| "health_auth_invalid")?;
    let fields = verify_loopback_request_headers(
        &signer, &headers, "GET", "/health", "127.0.0.1", "", &[], &identity, now,
    ).map_err(|_| "health_auth_invalid")?;
    let mut request = b"GET /health HTTP/1.1\r\n".to_vec();
    for (name, value) in headers {
        request.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
    }
    request.extend_from_slice(b"\r\n");
    let response = exchange(address, &request, deadline)?;
    if verify_loopback_response_headers(
        &signer, &response.headers, &fields, response.status, &response.body,
        &identity, self::now(),
    ).is_err() {
        if let Some(path) = token_path { fence_token(path, token)?; }
        return Err("health_response_auth_invalid");
    }
    if !matches!(response.status, 200 | 503) { return Err("health_status_unavailable"); }
    if let Some(path) = token_path { fence_token(path, token)?; }
    Ok(HealthResponse { status: response.status, body: response.body, identity })
}

fn fence_token(path: &std::path::Path, captured: &str) -> Result<(), &'static str> {
    let raw = std::fs::read(path).map_err(|_| "credential_migration_required")?;
    if raw.len() != 64 || !raw.iter().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte)) {
        return Err("credential_migration_required");
    }
    if raw != captured.as_bytes() { return Err("credential_rotated"); }
    Ok(())
}

fn remaining(deadline: Instant) -> Result<Duration, &'static str> {
    deadline.checked_duration_since(Instant::now())
        .filter(|duration| !duration.is_zero()).ok_or("health_timeout")
}

fn exchange(address: SocketAddr, request: &[u8], deadline: Instant) -> Result<HttpResponse, &'static str> {
    if !address.ip().is_loopback() { return Err("health_endpoint_invalid"); }
    let mut stream = TcpStream::connect_timeout(&address, remaining(deadline)?)
        .map_err(|_| "health_unavailable")?;
    let mut written = 0;
    while written < request.len() {
        stream.set_write_timeout(Some(remaining(deadline)?)).map_err(|_| "health_timeout")?;
        let count = stream.write(&request[written..]).map_err(|_| "health_unavailable")?;
        if count == 0 { return Err("health_unavailable"); }
        written += count;
    }
    let mut raw = Vec::new();
    let split = loop {
        if let Some(index) = raw.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
            if index + 4 > MAX_HEADERS { return Err("health_headers_too_large"); }
            break index + 4;
        }
        if raw.len() >= MAX_HEADERS { return Err("health_headers_too_large"); }
        stream.set_read_timeout(Some(remaining(deadline)?)).map_err(|_| "health_timeout")?;
        let mut chunk = [0u8; 4096];
        let count = stream.read(&mut chunk).map_err(|_| "health_unavailable")?;
        if count == 0 { return Err("health_invalid_http"); }
        raw.extend_from_slice(&chunk[..count]);
    };
    let header = std::str::from_utf8(&raw[..split - 4]).map_err(|_| "health_invalid_http")?;
    let mut lines = header.split("\r\n");
    let mut status_line = lines.next().ok_or("health_invalid_http")?.split_whitespace();
    if status_line.next() != Some("HTTP/1.1") { return Err("health_invalid_http"); }
    let status = status_line.next().ok_or("health_invalid_http")?.parse::<u16>()
        .map_err(|_| "health_invalid_http")?;
    let mut headers = Vec::new();
    let mut length = None;
    for line in lines {
        let (name, value) = line.split_once(':').ok_or("health_invalid_http")?;
        let name = name.to_ascii_lowercase();
        let value = value.trim().to_string();
        if name == "transfer-encoding" { return Err("health_chunked_unsupported"); }
        if name == "content-length" {
            if length.is_some() { return Err("health_duplicate_content_length"); }
            length = Some(value.parse::<usize>().map_err(|_| "health_invalid_http")?);
        }
        headers.push((name, value));
    }
    let length = length.ok_or("health_invalid_http")?;
    if length > MAX_BODY { return Err("health_body_too_large"); }
    if raw.len() - split > length { return Err("health_extra_bytes"); }
    while raw.len() - split < length {
        stream.set_read_timeout(Some(remaining(deadline)?)).map_err(|_| "health_timeout")?;
        let mut chunk = [0u8; 8192];
        let wanted = chunk.len().min(length - (raw.len() - split));
        let count = stream.read(&mut chunk[..wanted]).map_err(|_| "health_unavailable")?;
        if count == 0 { return Err("health_invalid_http"); }
        raw.extend_from_slice(&chunk[..count]);
    }
    // Canonical responses promise Connection: close. Require EOF after the
    // declared body so delayed trailing bytes cannot escape framing checks.
    stream.set_read_timeout(Some(remaining(deadline)?)).map_err(|_| "health_timeout")?;
    let mut trailing = [0u8; 1];
    match stream.read(&mut trailing) {
        Ok(0) => {}
        Ok(_) => return Err("health_extra_bytes"),
        Err(_) => return Err("health_unavailable"),
    }
    Ok(HttpResponse { status, headers, body: raw[split..].to_vec() })
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    fn identity() -> LoopbackIdentityFields {
        LoopbackIdentityFields { installation_id: "install-1".into(), cortex_store_id: "store-1".into(), release_generation: "release-1".into(), startup_generation: 7, stable_install_root: r"C:\Membrane\current".into() }
    }

    fn mock_signed(listener: std::net::TcpListener, token: String, response_body: Vec<u8>, status: u16, tamper: bool) {
        let (mut stream, _) = listener.accept().unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let mut raw = Vec::new();
        while !raw.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
            let mut chunk = [0u8; 2048];
            let count = stream.read(&mut chunk).unwrap();
            assert!(count > 0 && raw.len() + count <= MAX_HEADERS);
            raw.extend_from_slice(&chunk[..count]);
        }
        let text = String::from_utf8(raw).unwrap();
        let request_headers = text.split("\r\n").skip(1).filter(|line| !line.is_empty())
            .map(|line| { let (name, value) = line.split_once(':').unwrap(); (name.to_string(), value.trim().to_string()) })
            .collect::<Vec<_>>();
        let signer = LoopbackAuthSigner::from_hex_token(&token).unwrap();
        let id = identity();
        let fields = verify_loopback_request_headers(&signer, &request_headers, "GET", "/health", "127.0.0.1", "", &[], &id, now()).unwrap();
        let mut headers = membrane_client::build_loopback_response_headers(&signer, &id, fields.nonce, status, &response_body, fields.expiry_unix_secs).unwrap();
        if tamper { headers.iter_mut().find(|(name, _)| name == membrane_client::LOOPBACK_HEADER_PROOF).unwrap().1 = "00".repeat(32); }
        let mut response = format!("HTTP/1.1 {status} X\r\n");
        for (name, value) in headers { response.push_str(&format!("{name}: {value}\r\n")); }
        response.push_str("\r\n");
        stream.write_all(response.as_bytes()).unwrap();
        stream.write_all(&response_body).unwrap();
    }

    #[test]
    fn canonical_token_fence_reports_rotation_or_migration() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("api-token");
        let token = "11".repeat(32);
        std::fs::write(&path, token.as_bytes()).unwrap();
        assert!(fence_token(&path, &token).is_ok());
        std::fs::write(&path, format!("{token}\n")).unwrap();
        assert_eq!(fence_token(&path, &token), Err("credential_migration_required"));
        std::fs::write(&path, "22".repeat(32)).unwrap();
        assert_eq!(fence_token(&path, &token), Err("credential_rotated"));
        std::fs::write(&path, "A".repeat(64)).unwrap();
        assert_eq!(fence_token(&path, &token), Err("credential_migration_required"));
    }

    #[test]
    fn tcp_probe_accepts_signed_200_response() {
        let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let token = "11".repeat(32);
        let id = identity();
        let body = br#"{"serviceId":"membrane-hub"}"#.to_vec();
        let worker = thread::spawn({ let token = token.clone(); let body = body.clone(); move || mock_signed(listener, token, body, 200, false) });
        let result = probe_with_identity(address, &token, Duration::from_secs(2), id).unwrap();
        worker.join().unwrap();
        assert_eq!(result.status, 200);
    }

    #[test]
    fn tcp_probe_reports_rotation_after_verified_response() {
        let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let token = "11".repeat(32);
        let rotated = "22".repeat(32);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("api-token");
        std::fs::write(&path, token.as_bytes()).unwrap();
        let id = identity();
        let worker = thread::spawn({
            let token = token.clone(); let rotated = rotated.clone(); let path = path.clone(); let id = id.clone();
            move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut raw = Vec::new();
                while !raw.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    let mut chunk = [0u8; 2048]; let count = stream.read(&mut chunk).unwrap();
                    assert!(count > 0); raw.extend_from_slice(&chunk[..count]);
                }
                let text = String::from_utf8(raw).unwrap();
                let request_headers = text.split("\r\n").skip(1).filter(|line| !line.is_empty())
                    .map(|line| { let (name, value) = line.split_once(':').unwrap(); (name.to_string(), value.trim().to_string()) }).collect::<Vec<_>>();
                let signer = LoopbackAuthSigner::from_hex_token(&token).unwrap();
                let fields = verify_loopback_request_headers(&signer, &request_headers, "GET", "/health", "127.0.0.1", "", &[], &id, now()).unwrap();
                std::fs::write(&path, rotated.as_bytes()).unwrap();
                let body = b"{}";
                let headers = membrane_client::build_loopback_response_headers(&signer, &id, fields.nonce, 200, body, fields.expiry_unix_secs).unwrap();
                let mut response = String::from("HTTP/1.1 200 OK\r\n");
                for (name, value) in headers { response.push_str(&format!("{name}: {value}\r\n")); }
                response.push_str("\r\n{}"); stream.write_all(response.as_bytes()).unwrap();
            }
        });
        assert!(matches!(probe_at_deadline(address, &token, Instant::now() + Duration::from_secs(2), id, Some(&path)), Err("credential_rotated")));
        worker.join().unwrap();
    }

    #[test]
    fn tcp_probe_rejects_tampered_body_proof() {
        let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let token = "22".repeat(32);
        let id = identity();
        let worker = thread::spawn({ let token = token.clone(); move || mock_signed(listener, token, b"{}".to_vec(), 200, true) });
        assert!(matches!(probe_with_identity(address, &token, Duration::from_secs(2), id), Err("health_response_auth_invalid")));
        worker.join().unwrap();
    }

    #[test]
    fn exchange_rejects_duplicate_content_length() {
        let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let worker = thread::spawn(move || { let (mut stream, _) = listener.accept().unwrap(); let mut request = [0u8; 1024]; let _ = stream.read(&mut request); stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap(); });
        assert!(matches!(exchange(address, b"GET /livez HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n", Instant::now() + Duration::from_secs(2)), Err("health_duplicate_content_length")));
        worker.join().unwrap();
    }
}
