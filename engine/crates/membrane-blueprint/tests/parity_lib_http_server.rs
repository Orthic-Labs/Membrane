//! Parity tests for `lib_http_server` (native port of the pure
//! route/auth-gate semantics in `blueprint/src/lib/http-server.mjs`),
//! lane LIB4 (r5 closure). See that module's doc comment for why this is a
//! pure-function-only port (no runtime HTTP/socket equivalent exists, and
//! `membrane-blueprint` has no HTTP dependency to add one).

use membrane_blueprint::lib_http_server::{
    resolve_route, route_decision, strip_bearer_prefix, verify_session_token, RouteDecision,
    EXPLORER_ROUTES,
};

#[test]
fn explorer_routes_match_legacy_route_table() {
    assert_eq!(
        EXPLORER_ROUTES,
        ["/api/status", "/api/search", "/api/impact", "/api/architecture", "/api/doc-truth"]
    );
}

#[test]
fn resolve_route_matches_known_paths_only() {
    assert_eq!(resolve_route("/api/status"), Some("/api/status"));
    assert_eq!(resolve_route("/api/unknown"), None);
    assert_eq!(resolve_route("/"), None);
}

#[test]
fn strip_bearer_prefix_removes_bearer_and_whitespace() {
    assert_eq!(strip_bearer_prefix(Some("Bearer abc123")), Some("abc123".to_string()));
    assert_eq!(strip_bearer_prefix(Some("Bearer   abc123")), Some("abc123".to_string()));
    // No whitespace after "Bearer" -> \s+ does not match -> unchanged.
    assert_eq!(strip_bearer_prefix(Some("Bearerabc123")), Some("Bearerabc123".to_string()));
    // Not anchored at start -> unchanged.
    assert_eq!(strip_bearer_prefix(Some("xBearer abc")), Some("xBearer abc".to_string()));
    assert_eq!(strip_bearer_prefix(None), None);
}

#[test]
fn verify_session_token_constant_time_equality() {
    assert!(verify_session_token("secret-token", Some("secret-token")));
    assert!(!verify_session_token("secret-token", Some("wrong-token")));
    assert!(!verify_session_token("secret-token", Some("short")));
    assert!(!verify_session_token("secret-token", None));
    assert!(!verify_session_token("", Some("x")));
    assert!(!verify_session_token("secret-token", Some("")));
}

#[test]
fn route_decision_rejects_non_get() {
    let decision = route_decision("POST", "/api/status", "tok", Some("Bearer tok"));
    assert_eq!(decision, RouteDecision::MethodNotAllowed);
}

#[test]
fn route_decision_requires_auth_for_any_api_path_even_unknown_route() {
    // Auth gate runs for ANY /api/ prefixed path, before the route-not-found
    // check -- mirrors the legacy handler's control flow exactly.
    let decision = route_decision("GET", "/api/does-not-exist", "tok", None);
    assert_eq!(decision, RouteDecision::Unauthorized);
}

#[test]
fn route_decision_dispatches_known_authorized_route() {
    let decision = route_decision("GET", "/api/search", "tok", Some("Bearer tok"));
    assert_eq!(decision, RouteDecision::Dispatch("/api/search"));
}

#[test]
fn route_decision_route_not_found_for_authorized_unknown_api_path() {
    let decision = route_decision("GET", "/api/nope", "tok", Some("Bearer tok"));
    assert_eq!(decision, RouteDecision::RouteNotFound);
}

#[test]
fn route_decision_falls_through_to_asset_for_non_api_paths() {
    let decision = route_decision("GET", "/index.html", "tok", None);
    assert_eq!(decision, RouteDecision::ServeAsset);
}

#[test]
fn route_decision_unauthorized_with_wrong_bearer_token() {
    let decision = route_decision("GET", "/api/status", "tok", Some("Bearer wrong"));
    assert_eq!(decision, RouteDecision::Unauthorized);
}
