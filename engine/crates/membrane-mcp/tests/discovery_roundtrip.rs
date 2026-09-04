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
        "context_envelope_invalid"
    );
}
