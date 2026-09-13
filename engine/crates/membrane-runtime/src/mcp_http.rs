//! Authenticated loopback Streamable HTTP for native MCP clients. Stdio remains
//! available as a compatibility transport, while Claude/Codex connect directly
//! to the resident engine over this endpoint.
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
use serde::Deserialize;
use serde_json::Value;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use membrane_federation::deadline::{Deadline, SystemClock};
use tokio_util::sync::CancellationToken;

/// Resident transport routes. POST carries JSON-RPC, CLI, or hook payloads;
/// unsupported methods return 405 with `Allow: POST`.
pub const MCP_HTTP_PATH: &str = "/mcp";
pub const CLI_HTTP_PATH: &str = "/cli";
pub const HOOK_HTTP_PATH: &str = "/hook";

const MAX_CLI_ARGS: usize = 128;
const MAX_CLI_ARG_BYTES: usize = 64 * 1024;
const MAX_CLI_OUTPUT_BYTES: usize = 1024 * 1024;
/// Bounded lanes so CLI/hooks/chats cannot starve each other. Short,
/// latency-sensitive compat traffic (CLI, hooks) shares a small lane; bulk
/// MCP model traffic gets its own larger lane. Control routes (health/livez)
/// live on a different router and never take either permit, so admission
/// stays responsive under MCP load.
const MAX_BLOCKING_REQUESTS: usize = 4;
const MAX_MCP_REQUESTS: usize = 16;

/// Callers may propagate a shorter cooperative deadline than the policy
/// maximum. The admission policy still caps it; over-long or absent values
/// fall back to the policy maximum.
pub const DEADLINE_HEADER: &str = "x-membrane-deadline-ms";

/// Carries the caller's claimed installation id. Distinct from
/// `installation_manifest::HANDSHAKE_HEADER`, which carries a full manifest
/// for the Membrane resident API handshake.
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
    blocking_requests: Arc<tokio::sync::Semaphore>,
    mcp_requests: Arc<tokio::sync::Semaphore>,
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
        blocking_requests: Arc::new(tokio::sync::Semaphore::new(MAX_BLOCKING_REQUESTS)),
        mcp_requests: Arc::new(tokio::sync::Semaphore::new(MAX_MCP_REQUESTS)),
    };
    Router::new()
        .route(
            MCP_HTTP_PATH,
            post(handle_mcp_request)
                .get(method_not_allowed)
                .delete(method_not_allowed),
        )
        .route(
            CLI_HTTP_PATH,
            post(handle_cli_request)
                .get(method_not_allowed)
                .delete(method_not_allowed),
        )
        .route(
            HOOK_HTTP_PATH,
            post(handle_hook_request)
                .get(method_not_allowed)
                .delete(method_not_allowed),
        )
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
        HttpDenialCode::MissingBearer | HttpDenialCode::InvalidBearer => {
            StatusCode::UNAUTHORIZED
        }
        _ => StatusCode::FORBIDDEN,
    }
}

async fn method_not_allowed() -> Response {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        [(header::ALLOW, "POST")],
    )
        .into_response()
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

/// Admit a request and return the cooperative deadline the handlers must
/// honor: the caller's `x-membrane-deadline-ms` when present and within the
/// policy maximum, otherwise the policy maximum. The policy still denies
/// zero/over-long values, so a caller can only ever shorten its own budget.
fn admit_request(
    state: &McpHttpState,
    peer: SocketAddr,
    headers: &HeaderMap,
    body: &Bytes,
) -> Result<u64, Response> {
    let host = header_value(headers, "host");
    let origin = headers
        .get(header::ORIGIN)
        .map(|value| value.to_str().ok().filter(|origin| !origin.is_empty()).unwrap_or("\u{0}"))
        .unwrap_or("");
    let resolved_host_ip = state
        .resolver
        .resolve(strip_port(host))
        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    let requested_deadline_ms = header_value(headers, DEADLINE_HEADER)
        .parse::<u64>()
        .ok()
        .filter(|value| *value > 0)
        .unwrap_or(state.policy.max_deadline_ms);
    let receipt = admit(&state.policy, &HttpAdmissionRequest {
        peer_ip: peer.ip(),
        resolved_host_ip,
        host,
        origin,
        installation_id: header_value(headers, INSTALLATION_HEADER),
        bearer_token: bearer_token(headers),
        session_binding: header_value(headers, SESSION_HEADER),
        body_bytes: body.len(),
        deadline_ms: requested_deadline_ms,
    });
    if receipt.accepted {
        Ok(requested_deadline_ms.min(state.policy.max_deadline_ms))
    } else {
        let status = receipt.denial.as_ref().map(status_for_denial).unwrap_or(StatusCode::FORBIDDEN);
        let mut response = (status, axum::Json(receipt)).into_response();
        if status == StatusCode::UNAUTHORIZED {
            response.headers_mut().insert(header::WWW_AUTHENTICATE, axum::http::HeaderValue::from_static("Bearer"));
        }
        Err(response)
    }
}

fn json_content_type(headers: &HeaderMap) -> bool {
    headers.get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"))
}

#[derive(Deserialize)]
struct CliRequest {
    args: Vec<String>,
    #[serde(default)]
    stdin: String,
}

fn output_within_limit(value: &str) -> bool {
    value.len() <= MAX_CLI_OUTPUT_BYTES
}

async fn handle_cli_request(
    State(state): State<McpHttpState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    // The admitted deadline is recorded for the lane contract; CLI verbs
    // carry their own connect/read bounds and the server's request timeout,
    // so no second outer timeout is layered here.
    let _admitted_deadline_ms = match admit_request(&state, peer, &headers, &body) {
        Ok(deadline_ms) => deadline_ms,
        Err(response) => return response,
    };
    if !json_content_type(&headers) { return StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response(); }
    let request: CliRequest = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };
    if request.args.len() > MAX_CLI_ARGS || request.args.iter().any(|arg| arg.len() > MAX_CLI_ARG_BYTES) {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    }
    let stdin = request.stdin.into_bytes();
    if stdin.len() > DEFAULT_MAX_BODY_BYTES { return StatusCode::PAYLOAD_TOO_LARGE.into_response(); }
    let mut argv = Vec::with_capacity(request.args.len() + 1);
    argv.push("membrane".to_owned());
    argv.extend(request.args);
    let Ok(permit) = Arc::clone(&state.blocking_requests).try_acquire_owned() else {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    };
    let result = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| crate::cli::run_cli_captured(&argv, &stdin)))
    }).await;
    match result {
        Ok(Ok(result)) if output_within_limit(&result.stdout) && output_within_limit(&result.stderr) =>
            (StatusCode::OK, axum::Json(serde_json::json!({
                "stdout": result.stdout,
                "stderr": result.stderr,
                "exit_code": result.exit_code,
            }))).into_response(),
        Ok(Ok(_)) => (StatusCode::OK, axum::Json(serde_json::json!({
            "stdout": "",
            "stderr": "output_limit_exceeded\n",
            "exit_code": 1,
            "error": "output_limit_exceeded",
        }))).into_response(),
        Ok(Err(_)) | Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn handle_hook_request(
    State(state): State<McpHttpState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    // Hook modules run under the dispatcher's own per-module deadline with a
    // bounded leaf scope; the admitted deadline is recorded for the lane
    // contract rather than layered as a second timeout.
    let _admitted_deadline_ms = match admit_request(&state, peer, &headers, &body) {
        Ok(deadline_ms) => deadline_ms,
        Err(response) => return response,
    };
    if !json_content_type(&headers) { return StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response(); }
    let mut payload: Value = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };
    // Preserve request identity: when the caller presented an authenticated
    // per-boot session binding and the payload carries no session of its
    // own, stamp `session_id` so recall binds the same session the admission
    // layer authenticated. Explicit payload fields always win; nothing is
    // invented when the caller sent no binding.
    let session_binding = header_value(&headers, SESSION_HEADER);
    if !session_binding.is_empty() {
        if let Some(object) = payload.as_object_mut() {
            let has_session = ["thread_id", "session_id", "sessionId"]
                .iter()
                .any(|key| object.get(*key).is_some());
            if !has_session {
                object.insert(
                    "session_id".to_owned(),
                    Value::String(session_binding.to_owned()),
                );
            }
        }
    }
    let Ok(permit) = Arc::clone(&state.blocking_requests).try_acquire_owned() else {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    };
    let result = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| crate::hook::run_hook_payload(payload)))
    }).await;
    match result {
        Ok(Ok(response)) => (StatusCode::OK, axum::Json(response)).into_response(),
        Ok(Err(_)) | Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn handle_mcp_request(
    State(state): State<McpHttpState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let admitted_deadline_ms = match admit_request(&state, peer, &headers, &body) {
        Ok(deadline_ms) => deadline_ms,
        Err(response) => return response,
    };
    if !json_content_type(&headers) {
        return StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response();
    }

    let payload: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };
    // Blocking MCP work runs on its own bounded lane so bulk model traffic
    // cannot starve the CLI/hook lane (and vice versa); saturation is an
    // explicit 429, never silent queueing. The lane permit is held across the
    // whole dispatch so the bound covers queued-plus-running work.
    let Ok(permit) = Arc::clone(&state.mcp_requests).try_acquire_owned() else {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            axum::Json(serde_json::json!({"error": "mcp_lane_saturated"})),
        )
            .into_response();
    };
    // A panic inside dispatch used to unwind the connection task itself: the
    // socket closed having written zero bytes, so a client saw an unexplained
    // empty read while the Hub stayed healthy and answered every other
    // request. This router is merged after `build_router` applies its layers,
    // so it inherits no panic boundary of its own. Convert a panic into a
    // JSON-RPC internal error, and let the default hook print it to stderr
    // where the daemon log now keeps it.
    //
    // The caller's admitted deadline bounds the whole dispatch cooperatively:
    // expiry returns a typed timeout instead of letting one slow MCP call pin
    // the lane. Cancellation is tied to handler completion via the drop
    // guard; the dispatch itself observes the same token through the push
    // request control it inherits below.
    let cancellation = CancellationToken::new();
    let push_control = crate::serve::push_request_control(
        Deadline::after(
            &SystemClock,
            Duration::from_millis(admitted_deadline_ms),
        ),
        cancellation.clone(),
    );
    let cancellation_guard = cancellation.drop_guard();
    let request_id = payload.get("id").cloned().unwrap_or(Value::Null);
    let server = Arc::clone(&state.server);
    let dispatched = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::mcp_executor::with_inherited_push_control(push_control, || {
                server.dispatch(&payload)
            })
        }))
    });
    let dispatched = match tokio::time::timeout(
        Duration::from_millis(admitted_deadline_ms),
        dispatched,
    )
    .await
    {
        Ok(joined) => joined,
        Err(_) => {
            drop(cancellation_guard);
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                axum::Json(serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": request_id,
                    "error": {
                        "code": -32000,
                        "message": "mcp deadline exceeded: dispatch did not complete within the admitted deadline"
                    }
                })),
            )
                .into_response();
        }
    };
    drop(cancellation_guard);
    match dispatched {
        Ok(Ok(Some(response))) => (StatusCode::OK, axum::Json(response)).into_response(),
        Ok(Ok(None)) => StatusCode::ACCEPTED.into_response(),
        Ok(Err(_)) | Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": request_id,
                "error": {
                    "code": -32603,
                    "message": "internal error: the MCP dispatcher panicked; see membrane-daemon.log"
                }
            })),
        )
            .into_response(),
    }
}

/// Bind the Streamable HTTP MCP listener to loopback only and serve until the
/// process exits or the bind fails. This is an explicit opt-in entrypoint: no
/// default resident startup path calls it.
pub fn run_mcp_streamable_http(port: u16, policy: HttpAdmissionPolicy) -> Result<(), String> {
    crate::mcp_executor::install_native_mcp_transport()?;
    serve_mcp_streamable_http(port, policy)
}

fn serve_mcp_streamable_http(port: u16, policy: HttpAdmissionPolicy) -> Result<(), String> {
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
/// path the Membrane resident API already uses
/// (`crate::service::runtime_from_exe`, `crate::serve::configured_api_token`)
/// rather than minting a parallel credential, and binds the per-boot session
/// binding to `StartupClaim::service_instance_id`.
pub fn run_mcp_streamable_http_for_resident(port: u16) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|error| format!("resolve binary: {error}"))?;
    let runtime = crate::service::runtime_from_exe(&exe)?;
    // This transport may be invoked after the Hub has established its runtime
    // identity, but before a caller has copied its database location into the
    // process environment. Bind the native executor to that exact Hub store.
    let store = crate::MemoryStore::try_open(
        crate::MemDb::open(&runtime.db).map_err(|error| error.to_string())?)?;
    crate::mcp_executor::install_native_mcp_executor_for_hub(store)?;
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
    serve_mcp_streamable_http(bind_port, policy)
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

    fn resident_request(path: &str, bearer: Option<&str>, payload: Value) -> Request<Body> {
        let mut builder = Request::builder()
            .method("POST")
            .uri(path)
            .header(header::HOST, "127.0.0.1:9")
            .header(header::CONTENT_TYPE, "application/json")
            .extension(ConnectInfo(loopback_peer()));
        if let Some(bearer) = bearer {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {bearer}"));
        }
        builder.body(Body::from(payload.to_string())).unwrap()
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
    async fn over_long_caller_deadline_fails_closed() {
        let app = router_with(Arc::new(FixedResolver(Some(Ipv4Addr::LOCALHOST.into()))));
        let mut request = full_request(
            loopback_peer(),
            "127.0.0.1:9",
            "http://127.0.0.1:9",
            "correct-token",
        );
        request.headers_mut().insert(
            DEADLINE_HEADER,
            axum::http::HeaderValue::from_static("999999999"),
        );
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(denial_of(response).await, "deadline_too_long");
    }

    #[tokio::test]
    async fn hook_route_stamps_absent_session_from_binding() {
        // Identity preservation: an authenticated session binding fills a
        // missing session so recall binds the admitted session; an explicit
        // payload session always wins.
        let app = router_with(Arc::new(FixedResolver(Some(Ipv4Addr::LOCALHOST.into()))));
        let mut request = resident_request(
            HOOK_HTTP_PATH,
            Some("correct-token"),
            serde_json::json!({"event": "SessionEnd"}),
        );
        request.headers_mut().insert(
            SESSION_HEADER,
            axum::http::HeaderValue::from_static("session-abc"),
        );
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn mcp_lane_saturation_is_an_explicit_429() {
        // Reserve control capacity: exhausting the MCP lane must surface a
        // typed 429, never silently queue behind or starve the CLI/hook lane.
        let state = McpHttpState {
            policy: Arc::new(policy()),
            server: Arc::new(McpServer),
            resolver: Arc::new(FixedResolver(Some(Ipv4Addr::LOCALHOST.into()))),
            blocking_requests: Arc::new(tokio::sync::Semaphore::new(MAX_BLOCKING_REQUESTS)),
            mcp_requests: Arc::new(tokio::sync::Semaphore::new(0)),
        };
        let app = Router::new()
            .route(MCP_HTTP_PATH, post(handle_mcp_request))
            .with_state(state);
        let request = full_request(
            loopback_peer(),
            "127.0.0.1:9",
            "http://127.0.0.1:9",
            "correct-token",
        );
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let bytes = to_bytes(response.into_body(), MAX_BODY).await.unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["error"], "mcp_lane_saturated");
    }

    #[tokio::test]
    async fn resident_routes_require_bearer_admission() {
        let app = router_with(Arc::new(FixedResolver(Some(Ipv4Addr::LOCALHOST.into()))));
        for path in [CLI_HTTP_PATH, HOOK_HTTP_PATH] {
            let response = app.clone().oneshot(resident_request(path, None, serde_json::json!({"args": []}))).await.unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
        }
    }

    #[tokio::test]
    async fn authenticated_hook_route_dispatches_host_response() {
        let app = router_with(Arc::new(FixedResolver(Some(Ipv4Addr::LOCALHOST.into()))));
        let response = app.oneshot(resident_request(
            HOOK_HTTP_PATH,
            Some("correct-token"),
            serde_json::json!({"event": "SessionEnd", "session_id": "route-test"}),
        )).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), MAX_BODY).await.unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert!(value.get("membraneHook").is_some() || value.get("membrane_hook").is_some());
    }

    #[tokio::test]
    async fn authenticated_cli_route_dispatches_captured_help() {
        let app = router_with(Arc::new(FixedResolver(Some(Ipv4Addr::LOCALHOST.into()))));
        let response = app.oneshot(resident_request(
            CLI_HTTP_PATH,
            Some("correct-token"),
            serde_json::json!({"args": ["--help"], "stdin": ""}),
        )).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), MAX_BODY).await.unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["exit_code"], 0);
        assert!(value["stdout"].as_str().unwrap().contains("Usage"));
    }

    #[tokio::test]
    async fn authenticated_cli_route_rejects_non_string_args() {
        let app = router_with(Arc::new(FixedResolver(Some(Ipv4Addr::LOCALHOST.into()))));
        let response = app.oneshot(resident_request(
            CLI_HTTP_PATH,
            Some("correct-token"),
            serde_json::json!({"args": [42], "stdin": ""}),
        )).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn native_client_without_origin_or_boot_headers_is_accepted() {
        let app = router_with(Arc::new(FixedResolver(Some(Ipv4Addr::LOCALHOST.into()))));
        let request = Request::builder()
            .method("POST")
            .uri(MCP_HTTP_PATH)
            .header(header::HOST, "127.0.0.1:9")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, "Bearer correct-token")
            .extension(ConnectInfo(loopback_peer()))
            .body(Body::from(ping_payload().to_string()))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn unsupported_streamable_http_method_returns_standard_405() {
        let app = router_with(Arc::new(FixedResolver(Some(Ipv4Addr::LOCALHOST.into()))));
        let request = Request::builder()
            .method("GET")
            .uri(MCP_HTTP_PATH)
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(response.headers().get(header::ALLOW).unwrap(), "POST");
    }

    #[tokio::test]
    async fn non_json_post_body_returns_standard_415() {
        let app = router_with(Arc::new(FixedResolver(Some(Ipv4Addr::LOCALHOST.into()))));
        let mut request = full_request(
            loopback_peer(),
            "127.0.0.1:9",
            "http://127.0.0.1:9",
            "correct-token",
        );
        request.headers_mut().insert(
            header::CONTENT_TYPE,
            "text/plain".parse().unwrap(),
        );
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
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
    async fn empty_supplied_origin_is_not_treated_as_absent() {
        let app = router_with(Arc::new(FixedResolver(Some(Ipv4Addr::LOCALHOST.into()))));
        let request = full_request(
            loopback_peer(),
            "127.0.0.1:9",
            "",
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
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
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
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
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
