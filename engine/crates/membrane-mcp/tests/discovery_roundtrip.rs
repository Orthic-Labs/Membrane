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
    assert_eq!(discovery["tools"].as_array().unwrap().len(), 23);
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
fn adapt_is_optional_and_read_only() {
    let request = |groups| {
        McpServer.dispatch(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"_meta":{"membrane.toolsets.v1":groups}}})).unwrap()
    };
    let default = request(json!([]));
    assert!(!default["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["name"] == "membrane_adapt_inspect"));
    let opted = request(json!(["adapt"]));
    let tool = opted["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "membrane_adapt_inspect")
        .unwrap();
    assert_eq!(tool["annotations"]["readOnlyHint"], true);
    assert!(!tool["inputSchema"]["properties"]["operation"]["enum"]
        .as_array()
        .unwrap()
        .contains(&json!("approve")));
}

#[test]
fn push_toolset_exposes_real_schemas_and_keeps_default_narrow() {
    let response = McpServer.dispatch(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"_meta":{"membrane.toolsets.v1":["push"]}}})).unwrap();
    let tools = response["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 9);
    assert!(tools.iter().any(|v| v["name"] == "membrane_push_resolve"
        && v["inputSchema"]["properties"]["selector"]["oneOf"]
            .as_array()
            .unwrap()
            .len()
            == 4));
    assert!(tools.iter().any(|v| v["name"] == "membrane_push_prepare"));
    let default = McpServer
        .dispatch(&json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}))
        .unwrap();
    assert_eq!(default["result"]["tools"].as_array().unwrap().len(), 7);
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
        context["inputSchema"]["properties"]["consumerCapabilities"]["properties"]["resolvers"]
            ["maxItems"],
        32
    );
    assert_eq!(
        context["inputSchema"]["properties"]["consumerCapabilities"]["properties"]["resolvers"]["items"]["enum"],
        json!(["membrane_source_read", "membrane_memory_read"])
    );
}

#[test]
fn operator_review_is_opt_in_while_safe_cortex_workflow_is_default() {
    let server = McpServer;
    let default = server.dispatch(&json!({"jsonrpc":"2.0","id":4,"method":"tools/list"})).unwrap();
    let names = default["result"]["tools"].as_array().unwrap().iter()
        .map(|tool| tool["name"].as_str().unwrap()).collect::<Vec<_>>();
    assert!(names.contains(&"membrane_knowledge_propose"));
    assert!(names.contains(&"membrane_memory"));
    assert!(!names.contains(&"membrane_knowledge_review"));
    let operator = server.dispatch(&json!({"jsonrpc":"2.0","id":5,"method":"tools/list","params":{"_meta":{"membrane.toolsets.v1":["operator"]}}})).unwrap();
    assert!(operator["result"]["tools"].as_array().unwrap().iter()
        .any(|tool| tool["name"] == "membrane_knowledge_review"));
}