//! Native port of the legacy `blueprint explore` CLI command
//! (`blueprint/scripts/cli/commands.mjs`'s `"explore"` case, backed by
//! `blueprint/src/lib/explorer/index.mjs` -> `startExplorerServer` in
//! `blueprint/src/lib/http-server.mjs`).
//!
//! This is a bounded, loopback-only, read-only HTTP server: it binds to
//! 127.0.0.1 on an OS-chosen port, gates every `/api/*` route behind an
//! unguessable bearer token (constant-time compared, matching legacy
//! `session-token.mjs`'s `verifySessionToken`), and serves a small static
//! single-page UI for every other GET route. It reuses this crate's existing
//! loopback server primitive (`crate::http_server::serve`) rather than
//! standing up a second transport, and calls straight into
//! `membrane_blueprint::cli`'s existing bounded one-shot functions for
//! `status`/`search`/`impact`/`architecture_orientation`/`docs` — it opens no
//! second graph/store/planner path.
//!
//! Legacy route -> native mapping:
//! - `GET /api/status`       -> `membrane_blueprint::cli::status`
//! - `GET /api/search`       -> `membrane_blueprint::cli::search` (`q`, `limit`)
//! - `GET /api/impact`       -> `membrane_blueprint::cli::run_query(Operation::Impact, ...)` (`anchor`, `depth`, `budget`)
//! - `GET /api/architecture` -> `membrane_blueprint::cli::architecture_orientation` (`budget` forwarded as-is; legacy's `architecture` op takes no anchor)
//! - `GET /api/doc-truth`    -> `membrane_blueprint::cli::run_query(Operation::DocumentTruth, ...)` (`claimId`, `limit`)
//! - everything else (GET, no `/api/` prefix) -> static asset (`/`, `/index.html`, `/explorer.css`, `/explorer.js`), 404 otherwise
//!
//! Startup payload mirrors legacy's CLI-printed shape
//! (`{schemaVersion, state, url}`); the token remains in the URL fragment,
//! while native callers can use `RunningExplorer::token` directly.

use axum::extract::{Query, Request, State};
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::Router;
use membrane_blueprint::{cli as blueprint_cli, lib_explorer_static, lib_http_server, model::Operation, BlueprintError};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

struct ExplorerState {
    token: String,
    repo_root: String,
}

fn json_response(status: StatusCode, value: Value) -> Response {
    let body = value.to_string();
    (
        status,
        [
            ("content-type", "application/json; charset=utf-8".to_owned()),
            ("content-length", body.len().to_string()),
            ("cache-control", "no-store".to_owned()),
            ("content-security-policy", "default-src 'none'; frame-ancestors 'none'".to_owned()),
            ("x-content-type-options", "nosniff".to_owned()),
        ],
        body,
    )
        .into_response()
}

fn json_error(status: StatusCode, code: &str) -> Response {
    json_response(status, json!({"error": {"code": code}}))
}

fn json_ok(value: Value) -> Response {
    json_response(StatusCode::OK, value)
}

fn error_to_response(error: BlueprintError) -> Response {
    json_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        json!({"schemaVersion": 1, "error": {"code": error.code, "message": error.message}}),
    )
}

/// Auth gate for every `/api/*` route, delegating the actual bearer-prefix
/// strip and constant-time comparison to the already-ported pure logic in
/// `membrane_blueprint::lib_http_server` (`strip_bearer_prefix` /
/// `verify_session_token`) rather than re-implementing it here.
fn require_token(state: &ExplorerState, headers: &HeaderMap) -> Result<(), Response> {
    let raw = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    let provided = lib_http_server::strip_bearer_prefix(raw);
    if !lib_http_server::verify_session_token(&state.token, provided.as_deref()) {
        return Err(json_error(StatusCode::UNAUTHORIZED, "unauthorized"));
    }
    Ok(())
}

fn require_get(method: &Method) -> Result<(), Response> {
    if method == Method::GET {
        Ok(())
    } else {
        Err(json_error(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed"))
    }
}

#[derive(Deserialize, Default)]
struct StatusQuery {}

async fn route_status(
    State(state): State<Arc<ExplorerState>>,
    method: Method,
    headers: HeaderMap,
    Query(_query): Query<StatusQuery>,
) -> Response {
    if let Err(response) = require_get(&method) {
        return response;
    }
    if let Err(response) = require_token(&state, &headers) {
        return response;
    }
    match blueprint_cli::status(state.repo_root.clone(), None) {
        Ok(value) => json_ok(value),
        Err(error) => error_to_response(error),
    }
}

#[derive(Deserialize, Default)]
struct SearchQuery {
    q: Option<String>,
    limit: Option<usize>,
}

async fn route_search(
    State(state): State<Arc<ExplorerState>>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<SearchQuery>,
) -> Response {
    if let Err(response) = require_get(&method) {
        return response;
    }
    if let Err(response) = require_token(&state, &headers) {
        return response;
    }
    let text = query.q.unwrap_or_default();
    let limit = query.limit.unwrap_or(20);
    match blueprint_cli::search(state.repo_root.clone(), text, Some(limit), None) {
        Ok(value) => json_ok(value),
        Err(error) => error_to_response(error),
    }
}

#[derive(Deserialize, Default)]
struct ImpactQuery {
    anchor: Option<String>,
    depth: Option<u64>,
    budget: Option<u64>,
}

async fn route_impact(
    State(state): State<Arc<ExplorerState>>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<ImpactQuery>,
) -> Response {
    if let Err(response) = require_get(&method) {
        return response;
    }
    if let Err(response) = require_token(&state, &headers) {
        return response;
    }
    let anchor = query.anchor.unwrap_or_default();
    let mut input = json!({
        "seed": anchor.clone(),
        "target": anchor,
        "maxDepth": query.depth.unwrap_or(3),
        "maxBytes": query.budget.unwrap_or(2000),
    });
    match blueprint_cli::run_query("explore-impact", Operation::Impact, state.repo_root.clone(), input, None) {
        Ok(value) => json_ok(value),
        Err(error) => error_to_response(error),
    }
}

#[derive(Deserialize, Default)]
struct ArchitectureQuery {
    budget: Option<u64>,
}

async fn route_architecture(
    State(state): State<Arc<ExplorerState>>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<ArchitectureQuery>,
) -> Response {
    if let Err(response) = require_get(&method) {
        return response;
    }
    if let Err(response) = require_token(&state, &headers) {
        return response;
    }
    let input = json!({"task": "", "maxBytes": query.budget.unwrap_or(2000)});
    match blueprint_cli::run_query(
        "explore-architecture",
        Operation::Architecture,
        state.repo_root.clone(),
        input,
        None,
    ) {
        Ok(value) => json_ok(value),
        Err(error) => error_to_response(error),
    }
}

#[derive(Deserialize, Default)]
struct DocTruthQuery {
    #[serde(rename = "claimId")]
    claim_id: Option<String>,
    limit: Option<u64>,
}

async fn route_doc_truth(
    State(state): State<Arc<ExplorerState>>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<DocTruthQuery>,
) -> Response {
    if let Err(response) = require_get(&method) {
        return response;
    }
    if let Err(response) = require_token(&state, &headers) {
        return response;
    }
    let mut input = json!({});
    if let Some(claim_id) = query.claim_id {
        input["claimId"] = Value::from(claim_id);
    }
    input["limit"] = Value::from(query.limit.unwrap_or(200));
    match blueprint_cli::run_query("explore-docs", Operation::DocumentTruth, state.repo_root.clone(), input, None) {
        Ok(value) => json_ok(value),
        Err(error) => error_to_response(error),
    }
}

/// Serves the four static explorer assets via the already-ported
/// `membrane_blueprint::lib_explorer_static::serve_explorer_asset`, which
/// carries the byte-identical HTML/CSS/JS payloads and legacy response
/// headers (content-type, cache-control, CSP, X-Content-Type-Options).
async fn route_asset(uri: Uri) -> Response {
    let resolved = lib_explorer_static::serve_explorer_asset(uri.path());
    let mut response = Response::builder().status(resolved.status);
    for (name, value) in &resolved.headers {
        response = response.header(*name, value);
    }
    response.body(axum::body::Body::from(resolved.body)).unwrap().into_response()
}

/// Axum only invokes a fallback after route matching. Keep the legacy
/// request-order semantics here so unknown `/api/*` paths still authenticate
/// before returning `route_not_found`, and every non-GET gets the same JSON
/// 405 envelope as known routes.
async fn route_fallback(
    State(state): State<Arc<ExplorerState>>,
    method: Method,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    match lib_http_server::route_decision(
        method.as_str(),
        uri.path(),
        &state.token,
        headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok()),
    ) {
        lib_http_server::RouteDecision::MethodNotAllowed => {
            json_error(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed")
        }
        lib_http_server::RouteDecision::Unauthorized => {
            json_error(StatusCode::UNAUTHORIZED, "unauthorized")
        }
        lib_http_server::RouteDecision::RouteNotFound => {
            json_error(StatusCode::NOT_FOUND, "route_not_found")
        }
        lib_http_server::RouteDecision::ServeAsset => route_asset(uri).await,
        // A matched route is handled by its explicit Axum route. This branch
        // is defensive, preserving a typed response if routing changes.
        lib_http_server::RouteDecision::Dispatch(_) => {
            json_error(StatusCode::INTERNAL_SERVER_ERROR, "route_dispatch_error")
        }
    }
}

async fn get_only(request: Request, next: Next) -> Response {
    if request.method() != Method::GET {
        return json_error(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed");
    }
    next.run(request).await
}

fn build_router(state: Arc<ExplorerState>) -> Router {
    Router::new()
        .route("/api/status", any(route_status))
        .route("/api/search", any(route_search))
        .route("/api/impact", any(route_impact))
        .route("/api/architecture", any(route_architecture))
        .route("/api/doc-truth", any(route_doc_truth))
        .fallback(route_fallback)
        .layer(middleware::from_fn(get_only))
        .with_state(state)
}

/// Generate an unguessable session token: 32 random bytes, base64url encoded
/// (matches legacy `randomBytes(32).toString("base64url")`).
fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("OS entropy source for explorer session token");
    base64_url_encode(&bytes)
}

fn base64_url_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity((bytes.len() * 4 + 2) / 3);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[((n >> 18) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[((n >> 6) & 0x3f) as usize] as char);
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[(n & 0x3f) as usize] as char);
        }
    }
    out
}

/// Bounded handle for a running explorer server: callers await `wait` up to
/// `timeout`, or drop/cancel to close early. Mirrors legacy's
/// `explorer.close()` semantics (graceful, bounded drain).
pub struct RunningExplorer {
    pub url: String,
    pub token: String,
    pub port: u16,
    join: Option<tokio::task::JoinHandle<Result<(), String>>>,
    cancel: tokio_util::sync::CancellationToken,
}

impl RunningExplorer {
    pub async fn close(mut self) -> Result<(), String> {
        self.cancel.cancel();
        let join = self
            .join
            .take()
            .ok_or_else(|| "explorer already closed".to_string())?;
        let mut join = join;
        match tokio::time::timeout(Duration::from_secs(1), &mut join).await {
            Ok(Ok(result)) => result,
            Ok(Err(error)) => Err(error.to_string()),
            Err(_) => {
                join.abort();
                Err("explorer shutdown timed out".to_string())
            }
        }
    }
}

impl Drop for RunningExplorer {
    fn drop(&mut self) {
        // A dropped handle must not detach a loopback listener indefinitely.
        // `close` still gets the graceful drain path; Drop is the bounded
        // best-effort fallback for callers that discard the handle.
        self.cancel.cancel();
        if let Some(join) = &self.join {
            join.abort();
        }
    }
}

/// Start the loopback explorer server bound to an OS-chosen port on
/// 127.0.0.1. Returns once the listener is bound and accepting.
pub async fn start(repo_root: impl Into<String>) -> Result<RunningExplorer, String> {
    let repo_root = repo_root.into();
    let token = generate_token();
    let state = Arc::new(ExplorerState { token: token.clone(), repo_root });
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|error| error.to_string())?;
    let port = listener.local_addr().map_err(|error| error.to_string())?.port();
    let cancel = tokio_util::sync::CancellationToken::new();
    let shutdown = cancel.clone();
    let join = tokio::spawn(async move { crate::http_server::serve(listener, app, shutdown.cancelled_owned()).await });
    let url = format!("http://127.0.0.1:{port}/#token={token}");
    Ok(RunningExplorer { url, token, port, join: Some(join), cancel })
}

/// Run the native `blueprint explore` CLI verb: start the loopback server,
/// print the legacy-shaped startup payload, then block for
/// `--duration-ms` (if positive) or until SIGINT/SIGTERM (matching legacy's
/// `explore` case in `commands.mjs`), then close cleanly.
pub(crate) fn run_cli(args: &[String]) -> Result<(), String> {
    let mut repo_root = std::env::current_dir().map_err(|error| format!("resolve repository root: {error}"))?;
    let mut duration_ms: u64 = 0;
    let mut json_output = false;
    let mut index = 1; // args[0] == "explore"
    while index < args.len() {
        match args[index].as_str() {
            // `blueprint`'s facade calls this option `--root`; retain the
            // one-shot native spelling too so both shells dispatch parity.
            "--repo-root" | "--root" => {
                index += 1;
                repo_root = std::path::PathBuf::from(args.get(index).ok_or("--repo-root requires a path")?);
            }
            value if value.starts_with("--repo-root=") || value.starts_with("--root=") => {
                let (_, path) = value.split_once('=').expect("prefix includes equals");
                if path.is_empty() {
                    return Err("--repo-root requires a path".to_string());
                }
                repo_root = std::path::PathBuf::from(path);
            }
            "--duration-ms" => {
                index += 1;
                duration_ms = args
                    .get(index)
                    .ok_or("--duration-ms requires a value")?
                    .parse()
                    .map_err(|_| "--duration-ms requires an unsigned integer".to_string())?;
            }
            value if value.starts_with("--duration-ms=") => {
                duration_ms = value["--duration-ms=".len()..]
                    .parse()
                    .map_err(|_| "--duration-ms requires an unsigned integer".to_string())?;
            }
            "--json" | "--no-open" => json_output = true,
            _ => {}
        }
        index += 1;
    }
    let repo_root = repo_root
        .canonicalize()
        .map_err(|error| format!("canonicalize repository root: {error}"))?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    runtime.block_on(async move {
        let explorer = start(repo_root.to_string_lossy().into_owned()).await?;
        let payload = json!({
            "schemaVersion": 1,
            "state": "listening",
            "url": explorer.url.clone(),
        });
        if json_output {
            println!("{payload}");
        } else {
            println!("Open Blueprint Explorer: {}", explorer.url);
        }
        if duration_ms > 0 {
            tokio::time::sleep(Duration::from_millis(duration_ms)).await;
        } else {
            // Native parity note: legacy blocks on SIGINT/SIGTERM
            // indefinitely. This crate's `tokio` dependency does not enable
            // the `signal` feature (no other native caller needs it), and
            // this lane may not add dependency features to a file outside
            // its whitelist. Absent an explicit `--duration-ms`, block on a
            // conservative upper bound instead of hanging forever with no
            // interrupt path.
            tokio::time::sleep(Duration::from_secs(24 * 60 * 60)).await;
        }
        explorer.close().await
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_verification_matches_only_equal_nonempty_values() {
        assert!(lib_http_server::verify_session_token("abc", Some("abc")));
        assert!(!lib_http_server::verify_session_token("abc", Some("abd")));
        assert!(!lib_http_server::verify_session_token("abc", Some("ab")));
        assert!(!lib_http_server::verify_session_token("", Some("")));
        assert!(!lib_http_server::verify_session_token("abc", None));
    }

    #[test]
    fn base64_url_encode_has_no_padding_or_unsafe_chars() {
        let token = generate_token();
        assert_eq!(token.len(), 43);
        assert!(token.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }
}
