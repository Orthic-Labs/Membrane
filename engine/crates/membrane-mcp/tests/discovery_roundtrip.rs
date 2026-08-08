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
    assert_eq!(discovery["tools"].as_array().unwrap().len(), 0);
}

#[test]
fn negotiated_toolsets_never_advertise_unsupported_native_tools() {
    let server = McpServer;
    for meta in [
        json!({}),
        json!({"membrane.toolsets.v1": ["memory", "cortex"]}),
        json!({"membrane.toolsets.v1": ["unknown"]}),
    ] {
        let response = server
            .dispatch(
                &json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"_meta":meta}}),
            )
            .unwrap();
        assert_eq!(response["result"]["tools"], json!([]));
    }
}
