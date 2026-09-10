//! Parity for legacy `mcp/host/host-delivery-receipt.test.mjs`.

use membrane_mcp::host_delivery_receipt::{
    build_host_delivery_receipt, validate_host_delivery_receipt, BuildReceipt, DELIVERY_RECEIPT_SCHEMA,
};
use serde_json::json;

#[test]
fn host_delivery_receipt_schema_is_stable() {
    assert_eq!(DELIVERY_RECEIPT_SCHEMA, "membrane.delivery-receipt.v1");
}

#[test]
fn build_and_validate_round_trip() {
    let digest = format!("sha256:{}", "a".repeat(64));
    let receipt = build_host_delivery_receipt(BuildReceipt { trace_id: "t-1", digest: &digest, at: None, mechanism: None }).unwrap();
    assert_eq!(validate_host_delivery_receipt(&receipt).unwrap()["traceId"], "t-1");
    assert!(validate_host_delivery_receipt(&json!({"schema": "wrong", "traceId": "t", "digest": receipt["digest"]})).is_none());
}

#[test]
fn invalid_digest_is_rejected() {
    assert!(build_host_delivery_receipt(BuildReceipt { trace_id: "t", digest: "bad", at: None, mechanism: None }).is_err());
    assert!(validate_host_delivery_receipt(&json!({"schema": DELIVERY_RECEIPT_SCHEMA, "traceId": "t", "digest": "bad"})).is_none());
}
