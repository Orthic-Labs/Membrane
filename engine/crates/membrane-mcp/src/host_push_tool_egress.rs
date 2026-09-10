//! Port of legacy `mcp/host/push-tool-egress.mjs`.
//!
//! This is an owned result boundary, not a claim to intercept unrelated
//! tools. Call only after execution. No command, retries, or provider
//! ranking live here. No native equivalent of this MCP-tool-result wrapping
//! logic exists (`membrane-runtime::push::delivery` implements the
//! `membrane_push_prepare` operation itself, which this module calls into
//! via the injected `request` closure — mirroring the JS `pushRequest`
//! seam).

use membrane_protocol::digest_str;
use serde_json::{json, Value};

fn size(value: &Value) -> usize {
    serde_json::to_string(value).unwrap().len()
}
fn digest(text: &str) -> String {
    digest_str(text)
}

pub struct EgressOutcome {
    pub result: Value,
    pub state: &'static str,
    pub receipt: Value,
}

fn kept(result: &Value, reason: &'static str, max_bytes: Option<usize>) -> EgressOutcome {
    let refused = max_bytes.is_some_and(|max| max >= 2048 && size(result) > max);
    EgressOutcome {
        result: result.clone(),
        state: if refused { "refused" } else { "passthrough" },
        receipt: json!({"reason": reason, "observed": true, "savingsBytes": 0}),
    }
}

/// Mirrors `prepareToolEgress(result, binding, options)`.
///
/// `request` mirrors the injected `membrane_push_prepare` call
/// (`options.request`); it returns `None` on refusal/cancellation (mirroring
/// the JS `catch`), matching `kept("prepare_refused")`.
pub fn prepare_tool_egress(
    result: &Value,
    resolver_token: &str,
    max_bytes: i64,
    kind: &str,
    mut request: impl FnMut(&Value) -> Option<Value>,
) -> EgressOutcome {
    let max_bytes_opt = if max_bytes >= 0 { Some(max_bytes as usize) } else { None };

    let is_exact = result.get("isError").and_then(Value::as_bool) == Some(true)
        || result.get("disposition").and_then(Value::as_str) == Some("exact")
        || result["structuredContent"]["data"]["disposition"].as_str() == Some("exact")
        || result["structuredContent"]["result"]["data"]["disposition"].as_str() == Some("exact")
        || !result["structuredContent"]["data"]["pushRepresentation"].is_null();
    if is_exact {
        return kept(result, "exact_or_error", max_bytes_opt);
    }

    let content = result.get("content").and_then(Value::as_array);
    let single_text = content.filter(|c| c.len() == 1).and_then(|c| c[0].get("text")).and_then(Value::as_str);
    let is_text_part = content.map(|c| c.len() == 1 && c[0].get("type").and_then(Value::as_str) == Some("text")).unwrap_or(false);
    if !is_text_part || single_text.is_none() {
        return kept(result, "unsupported_parts", max_bytes_opt);
    }
    let text = single_text.unwrap();

    if !(2048..=49152).contains(&max_bytes) {
        return kept(result, "invalid_budget", max_bytes_opt);
    }
    let max_bytes = max_bytes as usize;

    if resolver_token.len() != 64 || !resolver_token.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()) {
        return kept(result, "resolver_unavailable", Some(max_bytes));
    }

    let original_bytes = size(result);
    if original_bytes <= max_bytes {
        return kept(result, "within_budget", Some(max_bytes));
    }

    let mut inner_budget = max_bytes.saturating_sub(1024).max(1);
    for _attempt in 0..3 {
        let request_payload = json!({
            "text": text,
            "kind": kind,
            "maxBytes": inner_budget,
            "resolverToken": resolver_token,
            "exact": false,
            "optimize": true,
        });
        let Some(delivery) = request(&request_payload) else {
            return kept(result, "prepare_refused", Some(max_bytes));
        };
        let source_digest_ok = delivery["receipt"]["sourceDigest"].as_str() == Some(&digest(text));
        let rep_text = delivery.get("text").and_then(Value::as_str).unwrap_or("");
        let rep_digest_ok = delivery["receipt"]["representationDigest"].as_str() == Some(&digest(rep_text));
        if !source_digest_ok || !rep_digest_ok {
            return kept(result, "delivery_identity_mismatch", Some(max_bytes));
        }
        let handle = delivery["recovery"]["handle"].as_str();
        if delivery.get("disposition").and_then(Value::as_str) != Some("prepared") || handle.is_none() {
            return kept(result, "not_reduced", Some(max_bytes));
        }
        let marker = format!(
            "\n[Push original: {}; expiresAt={}; resolve with membrane_push_resolve]",
            handle.unwrap(),
            delivery["recovery"]["expiresAt"]
        );
        let mut candidate = result.clone();
        candidate["content"][0]["text"] = json!(format!("{rep_text}{marker}"));
        if !candidate.get("structuredContent").is_none() && !candidate["structuredContent"].is_null() {
            candidate["structuredContent"]["data"] = json!({
                "pushRepresentation": delivery.get("representationKind").cloned().unwrap_or(Value::Null),
                "recovery": delivery.get("recovery").cloned().unwrap_or(Value::Null),
                "receipt": delivery.get("receipt").cloned().unwrap_or(Value::Null),
            });
        }
        let measured = size(&candidate);
        if measured <= max_bytes && measured < original_bytes {
            let mut receipt = delivery.get("receipt").cloned().unwrap_or(json!({}));
            if let Value::Object(map) = &mut receipt {
                map.insert("envelopeBytes".into(), json!(measured));
                map.insert("envelopeBasis".into(), json!("utf8_mcp_tool_result_v1"));
                map.insert("savingsBytes".into(), json!(original_bytes.saturating_sub(measured)));
            }
            return EgressOutcome { result: candidate, state: "prepared", receipt };
        }
        inner_budget = inner_budget.saturating_sub((measured.saturating_sub(max_bytes)).max(256)).max(1);
    }
    kept(result, "final_envelope_does_not_fit", Some(max_bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(text: &str) -> String {
        digest_str(text)
    }

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
            "receipt": {"sourceDigest": hash(text), "representationDigest": hash("compact")},
        })
    }

    #[test]
    fn owned_egress_preserves_envelope_and_does_not_echo_original() {
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
        let serialized = serde_json::to_string(&out.result).unwrap();
        assert!(!serialized.contains(&text));
        assert_eq!(original["content"][0]["text"], text);
    }

    #[test]
    fn exact_mixed_parts_missing_resolver_and_invalid_proof_are_not_reduced() {
        let (original, text) = sample();
        let token = "a".repeat(64);
        let should_not_call = |_: &Value| -> Option<Value> { panic!("must not call") };

        let mut exact = original.clone();
        exact["disposition"] = json!("exact");
        let out = prepare_tool_egress(&exact, &token, 2048, "text", should_not_call);
        assert_eq!(out.result, exact);

        let mut errored = original.clone();
        errored["isError"] = json!(true);
        let out = prepare_tool_egress(&errored, &token, 2048, "text", should_not_call);
        assert_eq!(out.result, errored);

        let mut mixed = original.clone();
        mixed["content"] = json!([{"type": "image", "data": "x"}]);
        let out = prepare_tool_egress(&mixed, &token, 2048, "text", should_not_call);
        assert_eq!(out.result, mixed);

        let out = prepare_tool_egress(&original, &token, 2048, "text", |_| {
            let mut d = delivery(&text);
            d["receipt"] = json!({});
            Some(d)
        });
        assert_eq!(out.receipt["reason"], "delivery_identity_mismatch");
        assert_eq!(out.result, original);
    }
}
