//! Native dashboard bootstrap & authenticated loopback transport.
//!
//! The resident tray launches this process with one inherited stdin pipe. The
//! pipe carries exactly one newline-delimited endpoint/token object. The token
//! is retained only in this native state; no command returns it to the
//! webview, and no environment or filesystem fallback is accepted.

use serde::Deserialize;
use std::{
    io::{self, BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpStream},
    time::Duration,
};

pub const BOOTSTRAP_MAX_FRAME_BYTES: usize = 16 * 1024;
pub const HTTP_MAX_RESPONSE_BYTES: usize = 1024 * 1024 + 4096;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DashboardBootstrap {
    endpoint: String,
    token: String,
}

/// Authenticated resident connection held by the native backend.
///
/// This type deliberately has no `Serialize` implementation. Its bearer is
/// never part of a Tauri command result or any webview-facing state.
#[derive(Clone)]
pub struct DashboardConnection {
    endpoint: SocketAddr,
    bearer_token: String,
}

impl DashboardConnection {
    #[cfg(test)]
    pub fn endpoint(&self) -> SocketAddr {
        self.endpoint
    }

    /// Issue one bounded authenticated GET against resident loopback HTTP.
    pub fn get(&self, path: &str, timeout: Duration) -> Result<HttpResponse, String> {
        if !path.starts_with('/') || path.bytes().any(|byte| matches!(byte, b'\r' | b'\n')) {
            return Err("dashboard_request_invalid".into());
        }
        let mut stream = TcpStream::connect_timeout(&self.endpoint, timeout)
            .map_err(|_| "dashboard_resident_unavailable")?;
        stream
            .set_read_timeout(Some(timeout))
            .map_err(|_| "dashboard_request_timeout")?;
        stream
            .set_write_timeout(Some(timeout))
            .map_err(|_| "dashboard_request_timeout")?;
        let request = format!(
            "GET {path} HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nConnection: close\r\n\r\n",
            self.endpoint, self.bearer_token
        );
        stream
            .write_all(request.as_bytes())
            .map_err(|_| "dashboard_request_write_failed")?;
        let mut raw = Vec::new();
        stream
            .take((HTTP_MAX_RESPONSE_BYTES + 1) as u64)
            .read_to_end(&mut raw)
            .map_err(|_| "dashboard_request_read_failed")?;
        if raw.len() > HTTP_MAX_RESPONSE_BYTES {
            return Err("dashboard_response_too_large".into());
        }
        parse_http_response(&raw)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
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
    let split = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
        .or_else(|| {
            raw.windows(2)
                .position(|window| window == b"\n\n")
                .map(|index| index + 2)
        })
        .ok_or("dashboard_response_invalid")?;
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
    Ok(HttpResponse {
        status,
        body: raw[split..].to_vec(),
    })
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
    validate_bearer_token(&payload.token)?;
    Ok(DashboardConnection {
        endpoint,
        bearer_token: payload.token,
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
}

impl DashboardConnectionState {
    pub fn from_stdin() -> Self {
        match read_bootstrap_from_stdin() {
            Ok(connection) => Self {
                connection: Some(connection),
                bootstrap_error: None,
            },
            Err(error) => Self {
                connection: None,
                bootstrap_error: Some(error),
            },
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

fn validate_bearer_token(token: &str) -> Result<(), String> {
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
        let response =
            parse_http_response(b"HTTP/1.1 503 Service Unavailable\r\n\r\n{\"ok\":false}").unwrap();
        assert_eq!(response.status, 503);
        assert_eq!(response.body, br#"{"ok":false}"#);
        assert_eq!(
            parse_http_response(&vec![b'x'; HTTP_MAX_RESPONSE_BYTES + 1]).unwrap_err(),
            "dashboard_response_too_large"
        );
    }

    #[test]
    fn authenticated_get_sends_bearer_to_loopback_only() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = listener.local_addr().unwrap();
        let token = "a".repeat(64);
        let expected = format!("Authorization: Bearer {token}");
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
            assert!(request.contains(&expected));
            assert!(request.starts_with("GET /health HTTP/1.1\r\n"));
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .unwrap();
        });
        let connection = DashboardConnection {
            endpoint,
            bearer_token: token,
        };
        let response = connection.get("/health", Duration::from_secs(1)).unwrap();
        server.join().unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"ok");
    }
}
