//! MBR-306: optional authenticated loopback Streamable HTTP for MCP clients
//! that cannot open a stdio pipe (e.g. some browser-hosted or sandboxed
//! agents). Stdio (`membrane_mcp::serve_stdio`, [`crate::serve::run_stdio_mcp`])
//! remains Membrane's default MCP transport: nothing in this crate's default
//! resident startup (`crate::service::run_service`, `crate::serve::run`)
//! calls anything in this module. A caller must explicitly invoke
//! [`run_mcp_streamable_http`] or [`run_mcp_streamable_http_for_resident`] to
//! open this listener.
//!
//! Every request is admitted through `membrane_mcp::http_security::admit`
//! before it reaches [`membrane_mcp::McpServer::dispatch`], the same
//! dispatcher stdio uses — this module only supplies the transport-specific
//! plumbing `http_security` deliberately does not own: pulling real request
//! context (peer address, resolved Host IP, headers, body) off the wire and
//! binding the listener to loopback only.
//!
//! No secret value (bearer token, session binding, request body) is ever
//! logged. The only values written to stderr are the bind address and port.

use axum::body::Bytes;
use axum::extract::{ConnectInfo, DefaultBodyLimit, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::Router;
use membrane_mcp::http_security::{
    admit, HttpAdmissionPolicy, HttpAdmissionRequest, HttpDenialCode, DEFAULT_MAX_BODY_BYTES,
};
use membrane_mcp::McpServer;
use serde_json::Value;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

/// The single route this transport exposes. Streamable HTTP MCP is one POST
/// endpoint carrying line-oriented JSON-RPC, mirroring `serve_stdio`'s framing
/// one transport up.
pub const MCP_HTTP_PATH: &str = "/mcp";

/// Carries the caller's claimed installation id. Distinct from
/// `installation_manifest::HANDSHAKE_HEADER`, which carries a full manifest
/// for the unrelated Crypt loopback API handshake.
pub const INSTALLATION_HEADER: &str = "x-membrane-installation-id";

/// Carries the caller's claimed per-boot session binding (see
/// `StartupClaim::service_instance_id`).
pub const SESSION_HEADER: &str = "x-membrane-session";

/// Resolves a Host header's hostname to the IP `http_security::admit` checks
/// for DNS rebinding. Abstracted so tests can supply a fixed, deterministic
/// answer instead of hitting the real system resolver.
pub trait HostResolver: Send + Sync {
    fn resolve(&self, host_only: &str) -> Option<IpAddr>;
}

/// Production resolver: an IP literal parses directly; a name is resolved
/// live (never cached) so a name that answers loopback once and something
/// else on a later request is caught at request time rather than trusted
/// from a stale answer.
pub struct SystemHostResolver;

impl HostResolver for SystemHostResolver {
    fn resolve(&self, host_only: &str) -> Option<IpAddr> {
        if host_only.is_empty() {
            return None;
        }
        if let Ok(ip) = host_only.parse::<IpAddr>() {
            return Some(ip);
        }
        use std::net::ToSocketAddrs;
        (host_only, 0)
            .to_socket_addrs()
            .ok()
            .and_then(|mut addrs| addrs.next())
            .map(|addr| addr.ip())
    }
}

#[derive(Clone)]
struct McpHttpState {
    policy: Arc<HttpAdmissionPolicy>,
    server: Arc<McpServer>,
    resolver: Arc<dyn HostResolver>,
}

/// Build the Streamable HTTP MCP router with the production (real) resolver.
/// This function does not bind a socket; see [`run_mcp_streamable_http`] for
/// the loopback listener.
pub fn build_mcp_http_router(policy: HttpAdmissionPolicy) -> Router {
    build_mcp_http_router_with_resolver(policy, Arc::new(SystemHostResolver))
}

fn build_mcp_http_router_with_resolver(
    policy: HttpAdmissionPolicy,
    resolver: Arc<dyn HostResolver>,
) -> Router {
    let state = McpHttpState {
        policy: Arc::new(policy),
        server: Arc::new(McpServer),
        resolver,
    };
    Router::new()
        .route(MCP_HTTP_PATH, post(handle_mcp_request))
        .layer(DefaultBodyLimit::max(DEFAULT_MAX_BODY_BYTES))
        .with_state(state)
}

/// Strip an optional `:port` (or bracketed IPv6 `:port`) suffix from a Host
/// header value, leaving the bare hostname/IP literal to resolve.
fn strip_port(host_header: &str) -> &str {
    if let Some(rest) = host_header.strip_prefix('[') {
        return match rest.split_once("]:") {
            Some((addr, _)) => addr,
            None => rest.trim_end_matches(']'),
        };
    }
    match host_header.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => host,
        _ => host_header,
    }
}

fn status_for_denial(denial: &HttpDenialCode) -> StatusCode {
    match denial {
        HttpDenialCode::BodyTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
        HttpDenialCode::DeadlineTooLong => StatusCode::BAD_REQUEST,
        _ => StatusCode::FORBIDDEN,
    }
}

fn header_value<'a>(headers: &'a HeaderMap, name: &'static str) -> &'a str {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
}

fn bearer_token(headers: &HeaderMap) -> &str {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or("")
}

async fn handle_mcp_request(
    State(state): State<McpHttpState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let installation_id = header_value(&headers, INSTALLATION_HEADER);
    let session_binding = header_value(&headers, SESSION_HEADER);
    let bearer_token = bearer_token(&headers);

    // A host that fails to resolve is denied the same way a host that
    // resolves off-loopback is denied: fail closed rather than admit on
    // ambiguous DNS state.
    let resolved_host_ip = state
        .resolver
        .resolve(strip_port(host))
        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));

    let admission_request = HttpAdmissionRequest {
        peer_ip: peer.ip(),
        resolved_host_ip,
        host,
        origin,
        installation_id,
        bearer_token,
        session_binding,
        body_bytes: body.len(),
        deadline_ms: state.policy.max_deadline_ms,
    };
    let receipt = admit(&state.policy, &admission_request);
    if !receipt.accepted {
        let status = receipt
            .denial
            .as_ref()
            .map(status_for_denial)
            .unwrap_or(StatusCode::FORBIDDEN);
        return (status, axum::Json(receipt)).into_response();
    }

    let payload: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };
    match state.server.dispatch(&payload) {
        Some(response) => (StatusCode::OK, axum::Json(response)).into_response(),
        None => StatusCode::ACCEPTED.into_response(),
    }
}

/// Bind the Streamable HTTP MCP listener to loopback only and serve until the
/// process exits or the bind fails. This is an explicit opt-in entrypoint: no
/// default resident startup path calls it.
pub fn run_mcp_streamable_http(port: u16, policy: HttpAdmissionPolicy) -> Result<(), String> {
    let app = build_mcp_http_router(policy);
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?
        .block_on(async move {
            let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port))
                .await
                .map_err(|error| error.to_string())?;
            eprintln!(
                "membrane mcp streamable-http on 127.0.0.1:{port} (opt-in transport; stdio remains default)"
            );
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .map_err(|error| error.to_string())
        })
}

/// MBR-306: resident convenience entrypoint. Sources installation id, host
/// binding, and bearer token from the same resident identity and credential
/// path the loopback Crypt API already uses
/// (`crate::service::runtime_from_exe`, `crate::serve::configured_api_token`)
/// rather than minting a parallel credential, and binds the per-boot session
/// binding to `StartupClaim::service_instance_id`.
pub fn run_mcp_streamable_http_for_resident(port: u16) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|error| format!("resolve binary: {error}"))?;
    let runtime = crate::service::runtime_from_exe(&exe)?;
    let (identity, claim) = crate::service::prepare_runtime_identity(&runtime)?;
    let bind_port = if port >= 1024 { port } else { runtime.port };
    let bearer_token = crate::serve::configured_api_token(&runtime.db)?;
    let host = format!("127.0.0.1:{bind_port}");
    let origin = format!("http://{host}");
    let policy = HttpAdmissionPolicy::local(
        identity.installation_id,
        host,
        origin,
        bearer_token,
        claim.service_instance_id,
    );
    run_mcp_streamable_http(bind_port, policy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use tower::ServiceExt;

    const MAX_BODY: usize = 1_048_576;

    struct FixedResolver(Option<IpAddr>);

    impl HostResolver for FixedResolver {
        fn resolve(&self, _host_only: &str) -> Option<IpAddr> {
            self.0
        }
    }

    fn loopback_peer() -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], 51234))
    }

    fn remote_peer() -> SocketAddr {
        SocketAddr::from(([203, 0, 113, 5], 51234))
    }

    fn policy() -> HttpAdmissionPolicy {
        HttpAdmissionPolicy::local(
            "installation-1",
            "127.0.0.1:9",
            "http://127.0.0.1:9",
            "correct-token",
            "session-abc",
        )
    }

    fn router_with(resolver: Arc<dyn HostResolver>) -> Router {
        build_mcp_http_router_with_resolver(policy(), resolver)
    }

    fn ping_payload() -> Value {
        serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "ping"})
    }

    fn full_request(peer: SocketAddr, host: &str, origin: &str, bearer: &str) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(MCP_HTTP_PATH)
            .header(header::HOST, host)
            .header(header::ORIGIN, origin)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
            .header(INSTALLATION_HEADER, "installation-1")
            .header(SESSION_HEADER, "session-abc")
            .extension(ConnectInfo(peer))
            .body(Body::from(ping_payload().to_string()))
            .unwrap()
    }

    async fn denial_of(response: Response) -> String {
        let bytes = to_bytes(response.into_body(), MAX_BODY).await.unwrap();
        let receipt: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(receipt["accepted"], false);
        assert_eq!(receipt["transport"], "streamable_http");
        receipt["denial"].as_str().unwrap().to_string()
    }

    #[tokio::test]
    async fn accepted_request_dispatches_the_same_result_as_stdio_json_rpc() {
        let app = router_with(Arc::new(FixedResolver(Some(Ipv4Addr::LOCALHOST.into()))));
        let request = full_request(
            loopback_peer(),
            "127.0.0.1:9",
            "http://127.0.0.1:9",
            "correct-token",
        );
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), MAX_BODY).await.unwrap();
        let http_result: Value = serde_json::from_slice(&bytes).unwrap();

        // Same dispatcher stdio uses: this transport is an alternate
        // admission-gated entrypoint onto the one MCP server, not a second
        // implementation of it.
        let stdio_result = McpServer.dispatch(&ping_payload()).unwrap();
        assert_eq!(http_result, stdio_result);
    }

    #[tokio::test]
    async fn non_loopback_peer_fails_closed() {
        let app = router_with(Arc::new(FixedResolver(Some(Ipv4Addr::LOCALHOST.into()))));
        let request = full_request(
            remote_peer(),
            "127.0.0.1:9",
            "http://127.0.0.1:9",
            "correct-token",
        );
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(denial_of(response).await, "non_loopback_peer");
    }

    #[tokio::test]
    async fn dns_rebinding_resolved_host_fails_closed() {
        // The Host header names an allowed literal, but the resolver answers
        // with a public IP for it right now — the live rebinding case.
        let public_ip: IpAddr = IpAddr::from([203, 0, 113, 5]);
        let app = router_with(Arc::new(FixedResolver(Some(public_ip))));
        let request = full_request(
            loopback_peer(),
            "127.0.0.1:9",
            "http://127.0.0.1:9",
            "correct-token",
        );
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(denial_of(response).await, "dns_rebinding");
    }

    #[tokio::test]
    async fn unresolvable_host_fails_closed_as_dns_rebinding() {
        let app = router_with(Arc::new(FixedResolver(None)));
        let request = full_request(
            loopback_peer(),
            "127.0.0.1:9",
            "http://127.0.0.1:9",
            "correct-token",
        );
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(denial_of(response).await, "dns_rebinding");
    }

    #[tokio::test]
    async fn remote_origin_fails_closed() {
        let app = router_with(Arc::new(FixedResolver(Some(Ipv4Addr::LOCALHOST.into()))));
        let request = full_request(
            loopback_peer(),
            "127.0.0.1:9",
            "https://attacker.example",
            "correct-token",
        );
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(denial_of(response).await, "origin_not_allowed");
    }

    #[tokio::test]
    async fn missing_bearer_fails_closed() {
        let app = router_with(Arc::new(FixedResolver(Some(Ipv4Addr::LOCALHOST.into()))));
        let request = Request::builder()
            .method("POST")
            .uri(MCP_HTTP_PATH)
            .header(header::HOST, "127.0.0.1:9")
            .header(header::ORIGIN, "http://127.0.0.1:9")
            .header(header::CONTENT_TYPE, "application/json")
            .header(INSTALLATION_HEADER, "installation-1")
            .header(SESSION_HEADER, "session-abc")
            .extension(ConnectInfo(loopback_peer()))
            .body(Body::from(ping_payload().to_string()))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(denial_of(response).await, "missing_bearer");
    }

    #[tokio::test]
    async fn wrong_bearer_fails_closed_without_leaking_the_expected_token() {
        let app = router_with(Arc::new(FixedResolver(Some(Ipv4Addr::LOCALHOST.into()))));
        let request = full_request(
            loopback_peer(),
            "127.0.0.1:9",
            "http://127.0.0.1:9",
            "wrong-token",
        );
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let bytes = to_bytes(response.into_body(), MAX_BODY).await.unwrap();
        let receipt: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(receipt["denial"], "invalid_bearer");
        let serialized = serde_json::to_string(&receipt).unwrap();
        assert!(!serialized.contains("wrong-token"));
        assert!(!serialized.contains("correct-token"));
    }

    #[test]
    fn strip_port_handles_ipv4_ipv6_and_bare_names() {
        assert_eq!(strip_port("127.0.0.1:9"), "127.0.0.1");
        assert_eq!(strip_port("localhost"), "localhost");
        assert_eq!(strip_port("localhost:9"), "localhost");
        assert_eq!(strip_port("[::1]:9"), "::1");
        assert_eq!(strip_port("[::1]"), "::1");
    }

    #[test]
    fn denial_response_never_carries_bearer_or_session_values() {
        let active_policy = policy();
        let request = HttpAdmissionRequest {
            peer_ip: Ipv4Addr::LOCALHOST.into(),
            resolved_host_ip: Ipv4Addr::LOCALHOST.into(),
            host: "127.0.0.1:9",
            origin: "http://127.0.0.1:9",
            installation_id: "installation-1",
            bearer_token: "definitely-wrong",
            session_binding: "session-abc",
            body_bytes: 0,
            deadline_ms: active_policy.max_deadline_ms,
        };
        let receipt = admit(&active_policy, &request);
        assert!(!receipt.accepted);
        let serialized = serde_json::to_string(&receipt).unwrap();
        assert!(!serialized.contains("definitely-wrong"));
        assert!(!serialized.contains("session-abc"));
        assert!(!serialized.contains("correct-token"));
    }
}
