//! Port of legacy `mcp/host/evidence-interceptor.mjs`.
//!
//! Intercepts only Membrane evidence at an observed host egress seam.
//! Directive bytes remain Legion-owned and are deliberately discarded from
//! this result. Reduction is injected from Push; this adapter never selects
//! or truncates.

use crate::host_capability_matrix::capability_for;
use membrane_protocol::digest_str;
use serde_json::{json, Value};

pub struct EvidenceDelivery {
    pub schema: &'static str,
    pub state: &'static str,
    pub reason: String,
    pub directive: Option<Value>,
    pub evidence: Option<Value>,
    pub receipt: Value,
}

fn degraded(reason: &str, detail: Value) -> EvidenceDelivery {
    let mut receipt = json!({"verified": false, "reason": reason});
    if let (Value::Object(receipt_map), Value::Object(detail_map)) = (&mut receipt, &detail) {
        for (k, v) in detail_map {
            receipt_map.insert(k.clone(), v.clone());
        }
    }
    EvidenceDelivery {
        schema: "membrane.host-evidence-delivery.v1",
        state: "degraded",
        reason: reason.to_string(),
        directive: None,
        evidence: None,
        receipt,
    }
}

/// `reduce`: evidence -> Option<packet>. `verify_delivery`: (packet, receipt) -> bool.
/// Both mirror the async JS callbacks but are synchronous here — the native
/// MCP hot path has no reason to model these as async.
pub fn intercept_evidence(
    input: &Value,
    client: &str,
    event: &str,
    reduce: impl FnOnce(&Value) -> Option<Value>,
    verify_delivery: Option<impl FnOnce(&Value, &Value) -> bool>,
) -> EvidenceDelivery {
    let Some(evidence) = input.get("evidence").filter(|v| v.is_object()) else {
        return degraded("evidence_unavailable", json!({}));
    };

    let level = capability_for(client, event);
    if level == "unavailable" {
        return degraded("host_seam_unavailable", json!({"client": client, "event": event}));
    }

    let Some(packet) = reduce(evidence).filter(|v| v.is_object()) else {
        return degraded("push_packet_unavailable", json!({"client": client, "event": event}));
    };

    let trace_id = packet
        .get("traceId")
        .or_else(|| evidence.get("traceId"))
        .cloned()
        .unwrap_or(Value::Null);
    let packet_digest = digest_str(&serde_json::to_string(&packet).unwrap());
    let receipt = json!({
        "schema": "membrane.context-receipt.v1",
        "traceId": trace_id,
        "packetDigest": packet_digest,
        "verified": false,
        "capability": level,
    });

    let Some(verify_delivery) = verify_delivery else {
        return degraded("delivery_unverified", json!({"client": client, "event": event}));
    };
    let verified = verify_delivery(&packet, &receipt);
    if !verified {
        return degraded("delivery_unverified", json!({"client": client, "event": event}));
    }

    let mut verified_receipt = receipt;
    if let Value::Object(map) = &mut verified_receipt {
        map.insert("verified".into(), json!(true));
    }
    EvidenceDelivery {
        schema: "membrane.host-evidence-delivery.v1",
        state: "context_enforced",
        reason: "verified_delivery".to_string(),
        directive: None,
        evidence: Some(packet),
        receipt: verified_receipt,
    }
}

pub fn is_honest_delivery(result: &EvidenceDelivery) -> bool {
    let state_ok = result.state == "context_enforced" || result.state == "degraded";
    if !state_ok {
        return false;
    }
    if result.state != "context_enforced" {
        return true;
    }
    result.receipt.get("verified").and_then(Value::as_bool) == Some(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_interception_never_transports_directive_and_proves_delivery() {
        let input = json!({"directive": "change the plan", "evidence": {"traceId": "t1"}});
        let degraded = intercept_evidence(
            &input,
            "claude_code",
            "tool_result_egress",
            |evidence| Some(json!({"traceId": evidence["traceId"], "blocks": []})),
            None::<fn(&Value, &Value) -> bool>,
        );
        assert_eq!(degraded.state, "degraded");
        assert_eq!(degraded.reason, "delivery_unverified");

        let delivered = intercept_evidence(
            &input,
            "claude_code",
            "tool_result_egress",
            |evidence| Some(json!({"traceId": evidence["traceId"], "blocks": [{"id": "evidence", "text": "data"}]})),
            Some(|_packet: &Value, receipt: &Value| receipt["traceId"] == "t1"),
        );
        assert_eq!(delivered.state, "context_enforced");
        assert!(delivered.directive.is_none());
        assert_eq!(delivered.receipt["verified"], true);
        assert!(is_honest_delivery(&delivered));
    }
}
