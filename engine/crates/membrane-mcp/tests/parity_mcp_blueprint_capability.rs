//! Public MCP surface parity: Blueprint, Cortex, & Ledger are Pull evidence
//! providers; durable memory writes are exposed through Push.

use membrane_mcp::{discovery_response, validate_arguments};
use serde_json::json;

#[test]
fn pull_is_the_public_route_for_blueprint_cortex_and_ledger() {
    let discovery = discovery_response();
    let tool = discovery["tools"].as_array().unwrap().iter().find(|tool| tool["name"] == "pull").expect("pull is advertised");
    let description = tool["description"].as_str().unwrap();
    for provider in ["Blueprint", "Cortex", "Ledger"] { assert!(description.contains(provider)); }
    assert_eq!(tool["annotations"]["readOnlyHint"], true);
}

#[test]
fn push_is_the_public_route_for_cortex_memory_writes() {
    let discovery = discovery_response();
    let tool = discovery["tools"].as_array().unwrap().iter().find(|tool| tool["name"] == "push").expect("push is advertised");
    assert!(tool["description"].as_str().unwrap().contains("Cortex"));
    assert_eq!(tool["inputSchema"]["required"], json!(["repository", "caller", "requestId", "body"]));
    assert_eq!(tool["inputSchema"]["properties"]["body"]["maxLength"], 8_388_608);
    assert!(validate_arguments("push", &json!({
        "repository":"repo", "caller":{"root":"C:/repo","repositoryId":"repo","scopeId":"scope"},
        "requestId":"request", "body":"durable memory"
    })).is_ok());
}

#[test]
fn retired_blueprint_name_is_not_advertised() {
    let discovery = discovery_response();
    let names = discovery["tools"].as_array().unwrap().iter().filter_map(|tool| tool["name"].as_str()).collect::<Vec<_>>();
    assert_eq!(names, ["pull", "push"]);
    assert!(!names.contains(&"membrane_blueprint"));
}
