//! NCL-02 parity test for the legacy `mcp/blueprint-capability.test.mjs` suite.
//!
//! The legacy test exercised the membrane_blueprint tool surface served by
//! the retired MCP server (operation enum, required fields, per-operation argument
//! validation). This asserts the same contract against the native
//! membrane-mcp crate: the discovery schema and the dispatch-time validation
//! that backs it.

use membrane_mcp::{discovery_response, validate_arguments, McpServer};
use serde_json::json;

fn blueprint_caller() -> serde_json::Value {
    json!({"root":"C:/repo","repositoryId":"repo","scopeId":"scope"})
}

#[test]
fn membrane_blueprint_tool_is_discoverable_with_required_fields_and_operations() {
    let discovery = discovery_response();
    let tools = discovery["tools"].as_array().unwrap();
    let tool = tools
        .iter()
        .find(|t| t["name"] == "membrane_blueprint")
        .expect("membrane_blueprint is advertised");
    assert_eq!(
        tool["inputSchema"]["required"],
        json!(["repository", "caller", "operation"])
    );
    let ops = tool["inputSchema"]["properties"]["operation"]["enum"]
        .as_array()
        .unwrap();
    for expected in [
        "search", "recall", "expand", "build", "refresh", "status", "impact", "federate",
        "findings.get", "findings.explain", "findings.evidence_pack",
        "findings.baseline.capture", "findings.baseline.list", "findings.sarif",
    ] {
        assert!(
            ops.iter().any(|op| op == expected),
            "membrane_blueprint operation enum must include {expected}"
        );
    }
}

#[test]
fn findings_operations_advertise_and_validate_native_inputs() {
    let base = json!({
        "repository": "repo", "caller": blueprint_caller(),
    });
    for (operation, extra) in [
        ("findings.get", json!({})),
        ("findings.explain", json!({"fingerprint": "fp"})),
        ("findings.evidence_pack", json!({"fingerprints": ["fp"]})),
        ("findings.baseline.capture", json!({"name": "ci"})),
        ("findings.baseline.list", json!({})),
        ("findings.sarif", json!({"toolVersion": "ci"})),
    ] {
        let mut value = base.clone();
        value["operation"] = json!(operation);
        if let (Some(dst), Some(src)) = (value.as_object_mut(), extra.as_object()) {
            dst.extend(src.clone());
        }
        assert!(validate_arguments("membrane_blueprint", &value).is_ok(), "{operation}");
    }
    let mut missing = base;
    missing["operation"] = json!("findings.explain");
    assert!(validate_arguments("membrane_blueprint", &missing).is_err());
}

#[test]
fn federate_is_admitted_to_runtime_and_scope_fields_are_closed() {
    let valid = json!({
        "repository": "repo", "caller": blueprint_caller(), "operation": "federate",
        "federateOperation": "search", "repositories": [{"repositoryId": "repo"}],
        "allowedRepoIds": ["repo"], "query": "native"
    });
    assert!(validate_arguments("membrane_blueprint", &valid).is_ok());
    let response = McpServer.dispatch(&json!({
        "jsonrpc": "2.0", "id": 7, "method": "tools/call",
        "params": {"name": "membrane_blueprint", "arguments": valid}
    })).unwrap();
    assert_ne!(response["result"]["structuredContent"]["result"]["code"], "blueprint_envelope_invalid");

    let invalid = json!({
        "repository": "repo", "caller": blueprint_caller(), "operation": "federate",
        "repositories": [], "unexpectedScopeField": true
    });
    assert!(validate_arguments("membrane_blueprint", &invalid).is_err());
}

#[test]
fn search_and_recall_require_a_non_empty_query() {
    let missing = json!({
        "repository": "repo", "caller": blueprint_caller(), "operation": "search"
    });
    assert!(validate_arguments("membrane_blueprint", &missing)
        .unwrap_err()
        .contains("query"));

    let blank = json!({
        "repository": "repo", "caller": blueprint_caller(), "operation": "recall", "query": "   "
    });
    assert!(validate_arguments("membrane_blueprint", &blank).is_err());

    let ok = json!({
        "repository": "repo", "caller": blueprint_caller(), "operation": "search", "query": "hub_runtime"
    });
    assert!(validate_arguments("membrane_blueprint", &ok).is_ok());
}

#[test]
fn expand_requires_a_non_empty_node() {
    let missing = json!({
        "repository": "repo", "caller": blueprint_caller(), "operation": "expand"
    });
    assert!(validate_arguments("membrane_blueprint", &missing)
        .unwrap_err()
        .contains("node"));

    let ok = json!({
        "repository": "repo", "caller": blueprint_caller(), "operation": "expand", "node": "src/lib.rs"
    });
    assert!(validate_arguments("membrane_blueprint", &ok).is_ok());
}

#[test]
fn calling_membrane_blueprint_without_operation_returns_a_typed_error() {
    let response = McpServer
        .dispatch(&json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": "membrane_blueprint", "arguments": {}}
        }))
        .unwrap();
    assert_eq!(response["result"]["isError"], true);
    assert_eq!(
        response["result"]["structuredContent"]["operation"],
        "membrane_blueprint"
    );
    assert_eq!(
        response["result"]["structuredContent"]["result"]["code"],
        "blueprint_envelope_invalid"
    );
}
