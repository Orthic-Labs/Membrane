//! MBR-308 compatibility tests.
//!
//! The Rust client is validated against the exact same canonical v1 golden
//! envelopes (`operations/operations/*.golden.json`) and the exact same
//! `operations-index.v1.golden.json` error taxonomy that the TypeScript
//! (`tests/sdk/cross-language-compat.test.mjs`) and Python
//! (`tests/sdk/python_client_test.py`) clients are tested against. All three
//! SDKs read the identical fixture files rather than independently authored
//! equivalents, so a client that silently drifts from the shared schema
//! fails here mechanically instead of being caught only by review.
//!
//! This crate owns no transport; every test below supplies its own injected
//! closure, matching the "thin, transport-injected client" contract in
//! `docs/sdk/README.md`.

use membrane_client::{MembraneClient, ProtocolResult, CONTEXT_OPERATION, SOURCE_READ_OPERATION};
use serde_json::{json, Map, Value};
use std::fs;
use std::path::PathBuf;

/// `engine/crates/membrane-client` -> repo root is three levels up.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn golden(name: &str) -> Value {
    let path = repo_root().join("operations/operations").join(name);
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("parse {}: {error}", path.display()))
}

fn error_codes_for(operation: &str) -> Vec<String> {
    let index = golden("operations-index.v1.golden.json");
    index["operations"]
        .as_array()
        .expect("operations-index.operations is an array")
        .iter()
        .find(|entry| entry["name"] == operation)
        .unwrap_or_else(|| panic!("{operation} present in operations-index.v1.golden.json"))
        ["errorCodes"]
        .as_array()
        .expect("errorCodes is an array")
        .iter()
        .map(|code| code.as_str().expect("error code is a string").to_string())
        .collect()
}

fn client_returning(response: Value) -> MembraneClient {
    MembraneClient::new(Box::new(
        move |_operation: &str, _request: &Map<String, Value>| -> ProtocolResult<Value> {
            Ok(response.clone())
        },
    ))
}

#[test]
fn context_golden_success_matches_the_shared_fixture() {
    let response = golden("membrane-context.v1.golden.json");
    let expected = response["result"]["data"].clone();
    let client = client_returning(response);
    let data = client
        .context(&Map::new())
        .expect("golden success envelope must be accepted");
    assert_eq!(Value::Object(data), expected);
}

#[test]
fn context_golden_error_is_typed_and_retryable() {
    let response = golden("membrane-context.v1.error.golden.json");
    let client = client_returning(response);
    let error = client
        .context(&Map::new())
        .expect_err("golden error envelope must be typed");
    assert_eq!(error.code, "context_deadline_exceeded");
    assert!(error.retryable);
}

#[test]
fn source_read_golden_success_matches_the_shared_fixture() {
    let response = golden("membrane-source-read.v1.golden.json");
    let expected = response["result"]["data"].clone();
    let client = client_returning(response);
    let data = client
        .source_read(&Map::new())
        .expect("golden success envelope must be accepted");
    assert_eq!(Value::Object(data), expected);
}

#[test]
fn source_read_golden_error_is_typed_and_not_retryable() {
    let response = golden("membrane-source-read.v1.error.golden.json");
    let client = client_returning(response);
    let error = client
        .source_read(&Map::new())
        .expect_err("golden error envelope must be typed");
    assert_eq!(error.code, "source_read_hash_mismatch");
    assert!(!error.retryable);
}

#[test]
fn every_indexed_error_code_round_trips_for_each_operation() {
    for operation in [CONTEXT_OPERATION, SOURCE_READ_OPERATION] {
        for code in error_codes_for(operation) {
            let response = json!({
                "schemaVersion": 1,
                "operation": operation,
                "errorVersion": 1,
                "result": { "kind": "error", "code": code.clone(), "message": "typed", "retryable": false }
            });
            let client = client_returning(response);
            let error = client
                .call(operation, &Map::new())
                .expect_err("every operations-index error code must be a typed client error");
            assert_eq!(error.code, code);
        }
    }
}

#[test]
fn error_code_outside_the_index_fails_closed() {
    let response = json!({
        "schemaVersion": 1,
        "operation": CONTEXT_OPERATION,
        "errorVersion": 1,
        "result": { "kind": "error", "code": "not_in_the_index", "message": "x", "retryable": false }
    });
    let client = client_returning(response);
    let error = client
        .context(&Map::new())
        .expect_err("an error code outside the closed index must fail closed");
    assert_eq!(error.code, "protocol_envelope_invalid");
}

#[test]
fn schema_version_drift_fails_closed() {
    let response = json!({
        "schemaVersion": 2,
        "operation": CONTEXT_OPERATION,
        "errorVersion": 1,
        "result": { "kind": "success", "data": {} }
    });
    let client = client_returning(response);
    let error = client
        .context(&Map::new())
        .expect_err("unsupported schemaVersion must fail closed");
    assert_eq!(error.code, "protocol_version_unsupported");
}

#[test]
fn mismatched_response_operation_fails_closed() {
    let response = json!({
        "schemaVersion": 1,
        "operation": SOURCE_READ_OPERATION,
        "errorVersion": 1,
        "result": { "kind": "success", "data": {} }
    });
    let client = client_returning(response);
    let error = client
        .context(&Map::new())
        .expect_err("a response for a different operation must fail closed");
    assert_eq!(error.code, "protocol_envelope_invalid");
}

#[test]
fn unknown_top_level_field_closes_the_envelope() {
    let mut response = golden("membrane-context.v1.golden.json");
    response
        .as_object_mut()
        .expect("golden envelope is an object")
        .insert("raw".to_string(), json!("leak"));
    let client = client_returning(response);
    let error = client
        .context(&Map::new())
        .expect_err("an unknown top-level field must fail closed");
    assert_eq!(error.code, "protocol_envelope_invalid");
}

#[test]
fn unsupported_operation_is_rejected_before_any_transport_call() {
    let client = client_returning(json!({
        "schemaVersion": 1,
        "operation": "membrane_unknown",
        "errorVersion": 1,
        "result": { "kind": "success", "data": {} }
    }));
    let error = client
        .call("membrane_unknown", &Map::new())
        .expect_err("an unsupported operation must be rejected");
    assert_eq!(error.code, "operation_unsupported");
}
