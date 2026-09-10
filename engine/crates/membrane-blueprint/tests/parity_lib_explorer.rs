// Parity tests for legacy blueprint/src/lib/explorer/layout.mjs and
// blueprint/src/lib/explorer/static.mjs, ported natively as
// engine/crates/membrane-blueprint/src/lib_explorer_layout.rs and
// lib_explorer_static.rs.
//
// Layout scenario translated from blueprint/tests/explorer-layout.test.mjs:
//   "spherical layout is deterministic and bounded"
// Static-asset scenarios translated from the routing/header portion of
// blueprint/tests/explorer.test.mjs's "explorer shell boots" test (the
// HTTP-server binding itself is out of this port's scope -- see
// lib_explorer_static.rs's module doc).

use membrane_blueprint::lib_explorer_layout::{project_orb, spherical_layout};
use membrane_blueprint::lib_explorer_static::serve_explorer_asset;
use serde_json::json;

#[test]
fn spherical_layout_is_deterministic_and_bounded() {
    let input = vec![json!({"id": "c"}), json!({"id": "a"}), json!({"id": "b"})];
    let first = spherical_layout(&input, 10.0);
    let second = spherical_layout(&input, 10.0);
    let ids_first: Vec<&str> = first.iter().map(|n| n.id.as_str()).collect();
    let ids_second: Vec<&str> = second.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids_first, ids_second);
    assert_eq!(ids_first, vec!["a", "b", "c"]);
    for node in &first {
        let radius = (node.x * node.x + node.y * node.y + node.z * node.z).sqrt();
        assert!(radius <= 10.000001, "node {} exceeded radius: {radius}", node.id);
    }
    assert_eq!(project_orb(&first, 0.0, 0.0, 720.0).len(), 3);
}

#[test]
fn root_and_index_html_serve_the_explorer_shell() {
    for path in ["/", "/index.html"] {
        let response = serve_explorer_asset(path);
        assert_eq!(response.status, 200);
        assert!(response.body.contains("Blueprint Explorer"));
        assert!(response.headers.iter().any(|(k, v)| *k == "content-type" && v.starts_with("text/html")));
        assert!(response.headers.iter().any(|(k, v)| *k == "content-security-policy" && v.contains("default-src 'self'")));
    }
}

#[test]
fn explorer_css_and_js_serve_with_correct_content_types() {
    let css = serve_explorer_asset("/explorer.css");
    assert_eq!(css.status, 200);
    assert!(css.headers.iter().any(|(k, v)| *k == "content-type" && v.starts_with("text/css")));

    let js = serve_explorer_asset("/explorer.js");
    assert_eq!(js.status, 200);
    assert!(js.headers.iter().any(|(k, v)| *k == "content-type" && v.starts_with("text/javascript")));
}

#[test]
fn unknown_path_returns_404() {
    let response = serve_explorer_asset("/api/status");
    assert_eq!(response.status, 404);
    assert_eq!(response.body, "not found");
}
