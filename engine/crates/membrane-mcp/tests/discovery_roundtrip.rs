use membrane_mcp::McpServer;
use membrane_mcp::{discovery_response, initialize_response};
use serde_json::json;

#[test]
fn discovery_matches_initialize_contract() {
    let discovery = discovery_response();
    assert_eq!(
        discovery["protocolVersion"],
        initialize_response()["protocolVersion"]
    );
    assert_eq!(discovery["serverInfo"]["name"], "membrane");
    assert_eq!(discovery["tools"].as_array().unwrap().len(), 19);
}

#[test]
fn negotiated_toolsets_advertise_only_registered_native_tools() {
    let server = McpServer;
    for meta in [
        json!({}),
        json!({"membrane.toolsets.v1": ["memory", "blueprint"]}),
        json!({"membrane.toolsets.v1": ["unknown"]}),
    ] {
        let response = server
            .dispatch(
                &json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"_meta":meta}}),
            )
            .unwrap();
        let tools = response["result"]["tools"].as_array().unwrap();
        assert!(!tools.is_empty());
        assert!(tools
            .iter()
            .all(|tool| tool["name"].as_str().unwrap().starts_with("membrane_")));
    }
}

#[test]
fn tool_calls_are_typed_and_never_use_legacy_fallback() {
    let response = McpServer.dispatch(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"membrane_context","arguments":{}}})).unwrap();
    assert_eq!(response["result"]["isError"], true);
    assert_eq!(
        response["result"]["structuredContent"]["operation"],
        "membrane_context"
    );
    assert_eq!(
        response["result"]["structuredContent"]["result"]["code"],
        "context_unavailable"
    );
}

#[test]
fn push_toolset_exposes_real_schemas_and_keeps_default_narrow() {
    let response = McpServer.dispatch(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"_meta":{"membrane.toolsets.v1":["push"]}}})).unwrap();
    let tools = response["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(),3);
    assert!(tools.iter().any(|v| v["name"] == "membrane_push_resolve" && v["inputSchema"]["properties"]["selector"]["oneOf"].as_array().unwrap().len() == 4));
    assert!(tools.iter().any(|v| v["name"] == "membrane_push_prepare"));
    let default = McpServer.dispatch(&json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})).unwrap();
    assert_eq!(default["result"]["tools"].as_array().unwrap().len(),1);
}

#[test]
fn context_schema_advertises_workspace_targets_and_resolver_negotiation() {
    let default = McpServer
        .dispatch(&json!({"jsonrpc":"2.0","id":3,"method":"tools/list"}))
        .unwrap();
    let context = default["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "membrane_context")
        .unwrap();
    assert_eq!(
        context["inputSchema"]["properties"]["scope"]["enum"],
        json!(["repo", "workspace"])
    );
    assert_eq!(
        context["inputSchema"]["properties"]["workspaceTargets"]["maxItems"],
        32
    );
    assert_eq!(
        context["inputSchema"]["properties"]["consumerCapabilities"]
            ["properties"]["resolvers"]["maxItems"],
        32
    );
}
