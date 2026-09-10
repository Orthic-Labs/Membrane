//! Port of legacy `mcp/host/delivery-receipt.cjs`.
//!
//! Host-issued delivery receipt — proves host loaded content natively for a
//! session. Related but distinct from
//! `membrane_runtime::delivery_trace_view::project_delivery_trace` (which
//! projects a whole multi-phase trace, not a single receipt); no exact native
//! equivalent of this schema existed, so it is ported as a new module.

use membrane_protocol::digest_str;
use serde_json::{json, Value};

pub const DELIVERY_RECEIPT_SCHEMA: &str = "membrane.delivery-receipt.v1";

fn is_valid_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    })
}

pub struct BuildReceipt<'a> {
    pub trace_id: &'a str,
    pub digest: &'a str,
    pub at: Option<String>,
    pub mechanism: Option<String>,
}

/// Mirrors `buildHostDeliveryReceipt`. Returns `Err` in place of `throw`.
pub fn build_host_delivery_receipt(params: BuildReceipt<'_>) -> Result<Value, String> {
    let trace_id = params.trace_id.trim();
    if trace_id.is_empty() {
        return Err("traceId required".to_string());
    }
    let digest = params.digest.trim();
    if !is_valid_digest(digest) {
        return Err("digest must be sha256:hex".to_string());
    }
    Ok(json!({
        "schema": DELIVERY_RECEIPT_SCHEMA,
        "traceId": trace_id,
        "digest": digest,
        "at": params.at.unwrap_or_else(|| "1970-01-01T00:00:00.000Z".to_string()),
        "mechanism": params.mechanism.unwrap_or_else(|| "host_delivery".to_string()),
    }))
}

/// Mirrors `validateHostDeliveryReceipt`. Returns `None` on any structural
/// failure, never panics on malformed input.
pub fn validate_host_delivery_receipt(receipt: &Value) -> Option<Value> {
    if !receipt.is_object() {
        return None;
    }
    if receipt.get("schema").and_then(Value::as_str) != Some(DELIVERY_RECEIPT_SCHEMA) {
        return None;
    }
    let trace_id = receipt.get("traceId").and_then(Value::as_str)?.trim();
    let digest = receipt.get("digest").and_then(Value::as_str)?.trim();
    if trace_id.is_empty() || !is_valid_digest(digest) {
        return None;
    }
    Some(json!({
        "schema": DELIVERY_RECEIPT_SCHEMA,
        "traceId": trace_id,
        "digest": digest,
        "at": receipt.get("at").and_then(Value::as_str).unwrap_or_default(),
        "mechanism": receipt.get("mechanism").and_then(Value::as_str).unwrap_or("host_delivery"),
    }))
}

pub fn digest_of(value: &Value) -> String {
    let text = value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| serde_json::to_string(value).unwrap());
    digest_str(&text)
}

pub fn receipt_digest(receipt: &Value) -> Result<String, String> {
    let normalized = validate_host_delivery_receipt(receipt).ok_or("invalid receipt")?;
    Ok(digest_of(&normalized))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_is_stable() {
        assert_eq!(DELIVERY_RECEIPT_SCHEMA, "membrane.delivery-receipt.v1");
    }

    #[test]
    fn build_and_validate_round_trip() {
        let receipt = build_host_delivery_receipt(BuildReceipt {
            trace_id: "t-1",
            digest: &format!("sha256:{}", "a".repeat(64)),
            at: None,
            mechanism: None,
        })
        .unwrap();
        assert_eq!(
            validate_host_delivery_receipt(&receipt).unwrap()["traceId"],
            "t-1"
        );
        assert!(validate_host_delivery_receipt(
            &json!({"schema": "wrong", "traceId": "t", "digest": receipt["digest"]})
        )
        .is_none());
    }

    #[test]
    fn invalid_digest_is_rejected() {
        assert!(build_host_delivery_receipt(BuildReceipt {
            trace_id: "t",
            digest: "bad",
            at: None,
            mechanism: None,
        })
        .is_err());
        assert!(validate_host_delivery_receipt(
            &json!({"schema": DELIVERY_RECEIPT_SCHEMA, "traceId": "t", "digest": "bad"})
        )
        .is_none());
    }
}
