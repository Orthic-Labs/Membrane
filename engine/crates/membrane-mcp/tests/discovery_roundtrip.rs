use membrane_mcp::McpServer;
use membrane_mcp::{discovery_response, initialize_response, validate_arguments};
use serde_json::json;

#[test]
fn discovery_matches_initialize_contract_and_public_registry() {
    let discovery = discovery_response();
    assert_eq!(discovery["protocolVersion"], initialize_response()["protocolVersion"]);
    assert_eq!(discovery["serverInfo"]["name"], "membrane");
    let tools = discovery["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 2);
    assert_eq!(tools.iter().map(|tool| tool["name"].as_str().unwrap()).collect::<Vec<_>>(), ["pull", "push"]);
}

#[test]
fn negotiated_toolsets_cannot_expand_public_registry() {
    let server = McpServer;
    for meta in [
        json!({}),
        json!({"membrane.toolsets.v1": ["memory", "blueprint", "adapt", "operator"]}),
        json!({"membrane.toolsets.v1": ["unknown"]}),
    ] {
        let response = server.dispatch(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"_meta":meta}})).unwrap();
        let names = response["result"]["tools"].as_array().unwrap().iter().map(|tool| tool["name"].as_str().unwrap()).collect::<Vec<_>>();
        assert_eq!(names, ["pull", "push"]);
    }
}

#[test]
fn public_calls_fail_with_typed_envelopes() {
    for (name, code) in [("pull", "context_envelope_invalid"), ("push", "memory_envelope_invalid")] {
        let response = McpServer.dispatch(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":{}}})).unwrap();
        assert_eq!(response["result"]["isError"], true);
        assert_eq!(response["result"]["structuredContent"]["operation"], name);
        assert_eq!(response["result"]["structuredContent"]["result"]["code"], code);
    }
}

#[test]
fn pull_schema_covers_blueprint_cortex_and_ledger_evidence() {
    let discovery = discovery_response();
    let pull = discovery["tools"].as_array().unwrap().iter().find(|tool| tool["name"] == "pull").unwrap();
    let description = pull["description"].as_str().unwrap();
    for provider in ["Pull", "Blueprint", "Cortex", "Ledger"] {
        assert!(description.contains(provider), "Pull schema must name {provider} coverage");
    }
    assert_eq!(pull["inputSchema"]["required"], json!(["task", "taskId", "sessionId", "repository", "caller", "remainingContextCeiling"]));
    assert_eq!(pull["inputSchema"]["properties"]["consumerCapabilities"]["properties"]["resolvers"]["items"]["enum"], json!(["membrane_source_read", "membrane_memory_read"]));
    validate_arguments("pull", &json!({
        "task":"retrieve context", "taskId":"task", "sessionId":"session", "repository":"repo",
        "caller":{"root":"C:/repo","repositoryId":"repo","scopeId":"session"},
        "remainingContextCeiling":{"schemaVersion":1,"ceilingId":"ceiling","sessionId":"session","taskId":{},"requestedAtUnixMs":1,"remainingTokens":{},"provenanceReceipt":{}}
    })).unwrap();
}

#[test]
fn push_schema_is_cortex_memory_write() {
    let discovery = discovery_response();
    let push = discovery["tools"].as_array().unwrap().iter().find(|tool| tool["name"] == "push").unwrap();
    assert!(push["description"].as_str().unwrap().contains("Cortex"));
    assert_eq!(push["annotations"]["readOnlyHint"], false);
    assert_eq!(push["inputSchema"]["required"], json!(["repository", "caller", "requestId", "body"]));
    assert_eq!(push["inputSchema"]["properties"]["body"]["maxLength"], 8_388_608);
    assert_eq!(push["inputSchema"]["properties"]["body"]["description"], "Exact UTF-8 body stored as immutable Cortex source bytes.");
    validate_arguments("push", &json!({
        "repository":"repo", "caller":{"root":"C:/repo","repositoryId":"repo","scopeId":"scope"},
        "requestId":"request", "body":"durable memory"
    })).unwrap();
}

#[test]
fn retired_subsystem_names_are_not_discoverable() {
    let discovery = discovery_response();
    let names = discovery["tools"].as_array().unwrap().iter().filter_map(|tool| tool["name"].as_str()).collect::<Vec<_>>();
    for retired in ["membrane_context", "membrane_blueprint", "membrane_memory", "membrane_ledger", "membrane_push_prepare"] {
        assert!(!names.contains(&retired), "retired tool {retired} must not be advertised");
    }
}
