//! Native Rust port of `blueprint/src/lib/http-server.mjs`.
//!
//! Lane LIB4 (r5 closure): this module has no prior native equivalent for
//! its actual semantics.
//!
//! Before porting, this lane grepped `engine/crates/membrane-runtime/src/*http*`
//! and `membrane/src/*` for an existing loopback HTTP server. The runtime
//! DOES own a loopback HTTP server
//! (`engine/crates/membrane-runtime/src/http_server.rs`, `serve()`), but it
//! is a generic axum/hyper connection-serving loop for HMAC-signed
//! request/response envelopes (`membrane_client::verify_loopback_request_headers`
//! / `build_loopback_response_headers`) used by the Hub health endpoint. It
//! implements neither this module's route table
//! (`/api/status`, `/api/search`, `/api/impact`, `/api/architecture`,
//! `/api/doc-truth`) nor its bearer-session-token auth scheme
//! (`verifySessionToken` from `session-token.mjs`), so there is no
//! behavioral overlap to reuse.
//!
//! `membrane-blueprint` has no HTTP/socket dependency (`axum`/`hyper`/
//! `tokio`) and lane rules forbid adding one ("no new deps"), so this port
//! does not stand up an actual `http.Server` equivalent. Per the lane's
//! http-server instruction, only the missing route/behavior semantics are
//! ported as pure functions: route-table dispatch, the `Bearer ` prefix
//! strip, the `/api/` auth gate decision, and the JSON error envelope
//! shape. Actual socket binding/serving stays a caller responsibility (the
//! legacy JS module's `http.createServer` + `serveAsset` callback have no
//! pure-function equivalent and are intentionally not ported).

/// The explorer server's route table. Mirrors the JS `routes` Map's key
/// set exactly (`/api/status`, `/api/search`, `/api/impact`,
/// `/api/architecture`, `/api/doc-truth`).
pub const EXPLORER_ROUTES: [&str; 5] = [
    "/api/status",
    "/api/search",
    "/api/impact",
    "/api/architecture",
    "/api/doc-truth",
];

/// Look up whether `pathname` is a known explorer route. Mirrors
/// `routes.get(url.pathname)` succeeding (`Some`) vs. missing (`None`).
pub fn resolve_route(pathname: &str) -> Option<&'static str> {
    EXPLORER_ROUTES.iter().find(|&&r| r == pathname).copied()
}

/// Strip a leading `Bearer ` (any amount of whitespace after `Bearer`) from
/// an `Authorization` header value, mirroring
/// `request.headers.authorization?.replace(/^Bearer\s+/, "")`. Returns
/// `None` when `header` itself is `None` (mirrors JS optional chaining
/// producing `undefined`, which `verifySessionToken` then treats as
/// falsy/absent).
pub fn strip_bearer_prefix(header: Option<&str>) -> Option<String> {
    let value = header?;
    if let Some(rest) = value.strip_prefix("Bearer") {
        let trimmed = rest.trim_start_matches(|c: char| c.is_whitespace());
        // Only strip when at least one whitespace char actually followed
        // "Bearer" (mirrors the JS `\s+` requiring one-or-more).
        if trimmed.len() < rest.len() {
            return Some(trimmed.to_string());
        }
    }
    Some(value.to_string())
}

/// Constant-time session-token comparison, mirroring
/// `verifySessionToken` from `blueprint/src/lib/session-token.mjs` (a
/// dependency of `http-server.mjs`, not itself one of this lane's 11
/// files, so reproduced here minimally as the exact predicate this
/// module's auth gate calls).
pub fn verify_session_token(expected: &str, provided: Option<&str>) -> bool {
    let provided = match provided {
        Some(p) if !p.is_empty() => p,
        _ => return false,
    };
    if expected.is_empty() {
        return false;
    }
    let a = expected.as_bytes();
    let b = provided.as_bytes();
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// The dispatch decision the server handler makes for one incoming
/// request, mirroring the branch structure of the JS request handler body
/// (method check -> route lookup -> `/api/` auth gate -> route dispatch ->
/// `/api/` 404 -> asset fallback).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteDecision {
    /// Non-GET method. Mirrors `405 method_not_allowed`.
    MethodNotAllowed,
    /// `/api/*` path with a missing/invalid bearer session token. Mirrors
    /// `401 unauthorized`.
    Unauthorized,
    /// A known `/api/*` route, authorized. Caller should invoke the
    /// matched route handler. Mirrors the `route(url)` call.
    Dispatch(&'static str),
    /// An `/api/*` path with no matching route. Mirrors
    /// `404 route_not_found`.
    RouteNotFound,
    /// Anything else falls through to the asset server. Mirrors
    /// `serveAsset(request, response, { sessionToken })`.
    ServeAsset,
}

/// Compute the routing decision for one request. Mirrors the legacy JS
/// handler's control flow exactly, including that the auth gate is
/// evaluated for ANY `/api/`-prefixed path (even one with no matching
/// route), before the route-not-found check.
pub fn route_decision(
    method: &str,
    pathname: &str,
    session_token: &str,
    authorization_header: Option<&str>,
) -> RouteDecision {
    if method != "GET" {
        return RouteDecision::MethodNotAllowed;
    }
    let route = resolve_route(pathname);
    let is_api = pathname.starts_with("/api/");
    if is_api {
        let provided = strip_bearer_prefix(authorization_header);
        if !verify_session_token(session_token, provided.as_deref()) {
            return RouteDecision::Unauthorized;
        }
    }
    if let Some(matched) = route {
        return RouteDecision::Dispatch(matched);
    }
    if is_api {
        return RouteDecision::RouteNotFound;
    }
    RouteDecision::ServeAsset
}

/// A JSON error envelope's fixed shape, mirroring the `json(response, ...)`
/// helper's error branches (`{ error: { code } }` for the fixed-code
/// branches, `{ schemaVersion: 1, error: { code, message } }` for the
/// generic catch-all).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorEnvelope {
    pub status: u16,
    pub code: &'static str,
}

pub const METHOD_NOT_ALLOWED: ErrorEnvelope = ErrorEnvelope { status: 405, code: "method_not_allowed" };
pub const UNAUTHORIZED: ErrorEnvelope = ErrorEnvelope { status: 401, code: "unauthorized" };
pub const ROUTE_NOT_FOUND: ErrorEnvelope = ErrorEnvelope { status: 404, code: "route_not_found" };

/// Build the generic catch-all error envelope value, mirroring the JS
/// `catch (error)` branch: `{ schemaVersion: 1, error: { code, message } }`
/// with `code` defaulting to `"internal_error"` and `status` defaulting to
/// `500`.
pub fn internal_error_envelope(status: Option<u16>, code: Option<&str>, message: &str) -> (u16, String, String) {
    (
        status.unwrap_or(500),
        code.unwrap_or("internal_error").to_string(),
        message.to_string(),
    )
}
