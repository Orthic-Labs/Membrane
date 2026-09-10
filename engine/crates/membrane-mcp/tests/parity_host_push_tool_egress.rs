//! Parity for legacy `mcp/host/push-tool-egress.test.mjs`.

use membrane_mcp::host_push_tool_egress::prepare_tool_egress;
use membrane_protocol::digest_str;
use serde_json::{json, Value};

fn sample() -> (Value, String) {
    let text = "ordinary repeated context\n".repeat(1000);
    let original = json!({
        "content": [{"type": "text", "text": text}],
        "structuredContent": {"data": {"text": text}, "trace": {"traceparent": "trace"}},
        "isError": false,
        "toolCallId": "call-one",
    });
    (original, text)
}

fn delivery(text: &str) -> Value {
    json!({
        "text": "compact",
        "disposition": "prepared",
        "representationKind": "protected_lines_v1",
        "recovery": {"handle": format!("mr://anchor/{}", "b".repeat(64)), "expiresAt": 2000},
        "receipt": {"sourceDigest": digest_str(text), "representationDigest": digest_str("compact")},
    })
}

#[test]
fn owned_egress_preserves_envelope_measures_actual_delivery_and_does_not_echo_original() {
    let (original, text) = sample();
    let token = "a".repeat(64);
    let mut calls = 0;
    let out = prepare_tool_egress(&original, &token, 2048, "text", |args| {
        calls += 1;
        assert_eq!(args["text"], text);
        Some(delivery(&text))
    });
    assert_eq!(out.state, "prepared");
    assert_eq!(calls, 1);
    assert_eq!(out.result["toolCallId"], "call-one");
    assert_eq!(out.result["isError"], false);
    assert_eq!(out.result["structuredContent"]["trace"], original["structuredContent"]["trace"]);
    let envelope_bytes = serde_json::to_string(&out.result).unwrap().len();
    assert_eq!(out.receipt["envelopeBytes"], envelope_bytes);
    assert!(!serde_json::to_string(&out.result).unwrap().contains(&text));
    assert_eq!(original["content"][0]["text"], text);
}

#[test]
fn exact_mixed_parts_missing_resolver_and_invalid_proof_are_not_reduced() {
    let (original, text) = sample();
    let token = "a".repeat(64);
    let should_not_call = |_: &Value| -> Option<Value> { panic!("must not call") };

    let mut exact = original.clone();
    exact["disposition"] = json!("exact");
    assert_eq!(prepare_tool_egress(&exact, &token, 2048, "text", should_not_call).result, exact);

    let mut errored = original.clone();
    errored["isError"] = json!(true);
    assert_eq!(prepare_tool_egress(&errored, &token, 2048, "text", should_not_call).result, errored);

    let mut mixed = original.clone();
    mixed["content"] = json!([{"type": "image", "data": "x"}]);
    assert_eq!(prepare_tool_egress(&mixed, &token, 2048, "text", should_not_call).result, mixed);

    let out = prepare_tool_egress(&original, &token, 2048, "text", |_| {
        let mut d = delivery(&text);
        d["receipt"] = json!({});
        Some(d)
    });
    assert_eq!(out.receipt["reason"], "delivery_identity_mismatch");
    assert_eq!(out.result, original);
}
