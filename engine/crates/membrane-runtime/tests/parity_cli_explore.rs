//! Parity test for the native `blueprint explore` loopback HTTP explorer
//! (lane V4, Task A: native port of legacy `blueprint explore` /
//! `startLocalExplorer` / `startExplorerServer`).
//!
//! Placement note: the task instructions named
//! `engine/crates/membrane-blueprint/tests/parity_cli_explore.rs` as this
//! test's location, mirroring where legacy's `explorer/index.mjs` lived
//! relative to `blueprint`. The actual server implementation could not live
//! in `membrane-blueprint` (that crate deliberately carries no HTTP/socket
//! dependency and lane rules forbid adding one -- see
//! `membrane-blueprint/src/lib_http_server.rs`'s own doc comment recording
//! this same constraint from an earlier lane). The server is therefore
//! implemented in `membrane-runtime` (`src/blueprint_explore.rs`), which
//! already depends on axum/tokio for its existing loopback HTTP
//! infrastructure (`src/http_server.rs`), and this parity test lives
//! alongside it as `membrane-runtime/tests/parity_cli_explore.rs` instead.
//!
//! Every wait in this file is bounded (a few seconds at most) -- no test may
//! hang.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// Minimal raw HTTP/1.1 client: this workspace has no async HTTP client
/// dependency in membrane-runtime and adding one is out of scope for this
/// lane ("do NOT add new dependencies"), so this issues the request over a
/// plain `TcpStream` and returns `(status_code, body)`.
fn http_request(port: u16, method: &str, path: &str, bearer: Option<&str>) -> (u16, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect to loopback explorer");
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    stream.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut request = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n");
    if let Some(token) = bearer {
        request.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes()).expect("write request");
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).expect("read response");
    let text = String::from_utf8_lossy(&raw).into_owned();
    let mut parts = text.splitn(2, "\r\n\r\n");
    let head = parts.next().unwrap_or_default();
    let body = parts.next().unwrap_or_default().to_string();
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .unwrap_or(0);
    (status, body)
}

fn http_get(port: u16, path: &str, bearer: Option<&str>) -> (u16, String) {
    http_request(port, "GET", path, bearer)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explore_server_authorizes_with_token_and_rejects_without_it() {
    let temp = tempfile::tempdir().expect("create temp repo");
    std::fs::write(temp.path().join("README.md"), b"# parity fixture\n").unwrap();

    let explorer = membrane_runtime::blueprint_explore::start(temp.path().to_string_lossy().into_owned())
        .await
        .expect("start loopback explorer");
    let port = explorer.port;
    let token = explorer.token.clone();

    // Static asset, no token required.
    let (status, body) = tokio::task::spawn_blocking(move || http_get(port, "/", None)).await.unwrap();
    assert_eq!(status, 200);
    assert!(body.contains("Blueprint Explorer"));

    // /api/status WITHOUT the token -> unauthorized.
    let (status, body) = tokio::task::spawn_blocking(move || http_get(port, "/api/status", None)).await.unwrap();
    assert_eq!(status, 401, "unauthenticated /api/status must be rejected, got body: {body}");
    assert!(body.contains("unauthorized"));

    // /api/status WITH the correct bearer token -> success.
    let token_for_status = token.clone();
    let (status, body) = tokio::task::spawn_blocking(move || http_get(port, "/api/status", Some(&token_for_status))).await.unwrap();
    assert_eq!(status, 200, "authenticated /api/status must succeed, got body: {body}");
    assert!(body.contains("\"") , "status body should be JSON: {body}");

    // A wrong token must still be rejected (not just an absent one).
    let (status, _body) = tokio::task::spawn_blocking(move || http_get(port, "/api/status", Some("not-the-real-token"))).await.unwrap();
    assert_eq!(status, 401);

    // Unknown /api/ route with a valid token -> 404, not a silent asset fallback.
    let token_for_missing = token.clone();
    let (status, _body) = tokio::task::spawn_blocking(move || http_get(port, "/api/does-not-exist", Some(&token_for_missing))).await.unwrap();
    assert_eq!(status, 404);

    // Legacy authenticates unknown API paths before its route-not-found branch.
    let (status, body) = tokio::task::spawn_blocking(move || http_get(port, "/api/does-not-exist", None)).await.unwrap();
    assert_eq!(status, 401);
    assert!(body.contains("unauthorized"));

    // Legacy rejects every non-GET before route dispatch, including static
    // paths; Axum's default 405 response must not leak through.
    let token_for_method = token.clone();
    let (status, body) = tokio::task::spawn_blocking(move || http_request(port, "POST", "/api/status", Some(&token_for_method))).await.unwrap();
    assert_eq!(status, 405);
    assert!(body.contains("method_not_allowed"));
    let (status, body) = tokio::task::spawn_blocking(move || http_request(port, "POST", "/", None)).await.unwrap();
    assert_eq!(status, 405);
    assert!(body.contains("method_not_allowed"));

    tokio::time::timeout(Duration::from_secs(5), explorer.close())
        .await
        .expect("explorer close must not hang")
        .expect("explorer close must succeed");
}
