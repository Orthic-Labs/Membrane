//! Content-free cache-prefix diagnostics for finalized context packets.
//!
//! The digest includes only model-delivered prefix fields. `traceId` and
//! omissions are deliberately excluded: they are per-turn observability, not
//! reusable prompt bytes.

use membrane_protocol::{canonical_json_of, digest_str};
use serde::Serialize;
use serde_json::{json, Value};

const PREFIX_FIELDS: [&str; 6] = [
    "schemaVersion",
    "task",
    "mode",
    "budget",
    "allocations",
    "providerAccounting",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CachePrefixDiagnosticV1 {
    pub schema_version: u32,
    pub prefix_digest: String,
    pub stable_block_order: Vec<String>,
    pub block_digests: Vec<CachePrefixBlockV1>,
    pub metadata_digests: Vec<CachePrefixMetadataV1>,
    pub volatility_source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_break: Option<CacheBreakAttributionV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CachePrefixBlockV1 {
    pub id: String,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CachePrefixMetadataV1 {
    pub field: String,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheBreakAttributionV1 {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_field: Option<String>,
}

/// Deterministically describe this packet's reusable prefix, optionally
/// attributing its first difference from a prior finalized packet.
pub fn diagnose_cache_prefix(packet: &Value, previous: Option<&Value>) -> CachePrefixDiagnosticV1 {
    let blocks = packet
        .get("blocks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let stable_block_order = blocks.iter().map(block_id).collect();
    let block_digests = blocks
        .iter()
        .map(|block| CachePrefixBlockV1 {
            id: block_id(block),
            digest: digest_str(&canonical_json_of(block)),
        })
        .collect();
    let metadata_digests = PREFIX_FIELDS
        .iter()
        .map(|field| CachePrefixMetadataV1 {
            field: (*field).to_string(),
            digest: digest_str(&canonical_json_of(
                packet.get(*field).unwrap_or(&Value::Null),
            )),
        })
        .collect();
    let prefix = json!({
        "schemaVersion": packet.get("schemaVersion"), "task": packet.get("task"), "mode": packet.get("mode"),
        "budget": packet.get("budget"), "allocations": packet.get("allocations"),
        "providerAccounting": packet.get("providerAccounting"), "blocks": blocks,
    });
    let cache_break = previous.and_then(|prior| first_break(packet, prior));
    let volatility_source = match &cache_break {
        Some(breakage) if breakage.kind == "block" => {
            format!("block:{}", breakage.block_id.as_deref().unwrap_or("order"))
        }
        Some(breakage) => format!(
            "metadata:{}",
            breakage.metadata_field.as_deref().unwrap_or("unknown")
        ),
        None if previous.is_some()
            && packet.get("traceId") != previous.and_then(|prior| prior.get("traceId")) =>
        {
            "traceId_excluded".to_string()
        }
        None => "none".to_string(),
    };
    CachePrefixDiagnosticV1 {
        schema_version: 1,
        prefix_digest: digest_str(&canonical_json_of(&prefix)),
        stable_block_order,
        block_digests,
        metadata_digests,
        volatility_source,
        cache_break,
    }
}

fn block_id(block: &Value) -> String {
    block
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string()
}

fn first_break(current: &Value, prior: &Value) -> Option<CacheBreakAttributionV1> {
    let current_blocks = current
        .get("blocks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let prior_blocks = prior
        .get("blocks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for (index, block) in current_blocks.iter().enumerate() {
        let Some(prior_block) = prior_blocks.get(index) else {
            return Some(CacheBreakAttributionV1 {
                kind: "block".into(),
                block_id: Some(block_id(block)),
                metadata_field: None,
            });
        };
        if block_id(block) != block_id(prior_block)
            || canonical_json_of(block) != canonical_json_of(prior_block)
        {
            return Some(CacheBreakAttributionV1 {
                kind: "block".into(),
                block_id: Some(block_id(block)),
                metadata_field: None,
            });
        }
    }
    if current_blocks.len() != prior_blocks.len() {
        return Some(CacheBreakAttributionV1 {
            kind: "block".into(),
            block_id: Some("order".into()),
            metadata_field: None,
        });
    }
    for field in PREFIX_FIELDS {
        if canonical_json_of(current.get(field).unwrap_or(&Value::Null))
            != canonical_json_of(prior.get(field).unwrap_or(&Value::Null))
        {
            return Some(CacheBreakAttributionV1 {
                kind: "metadata".into(),
                block_id: None,
                metadata_field: Some(field.into()),
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    fn packet(block: &str, source_hash: &str) -> Value {
        json!({"schemaVersion":1,"traceId":"turn-a","task":"repair","mode":"normal","budget":{"maxTokens":10},"allocations":{},"providerAccounting":{},"blocks":[{"id":block,"sourceHash":source_hash,"text":"safe"}],"omissions":[]})
    }
    #[test]
    fn trace_identity_does_not_break_prefix() {
        let first = packet("a", "one");
        let mut next = first.clone();
        next["traceId"] = json!("turn-b");
        let diagnostic = diagnose_cache_prefix(&next, Some(&first));
        assert_eq!(diagnostic.volatility_source, "traceId_excluded");
        assert!(diagnostic.cache_break.is_none());
        assert_eq!(
            diagnostic.prefix_digest,
            diagnose_cache_prefix(&first, None).prefix_digest
        );
        assert_eq!(
            diagnostic.prefix_digest,
            diagnose_cache_prefix(&next, None).prefix_digest
        );
    }

    #[test]
    fn omissions_are_excluded_and_canonical_keys_are_stable() {
        let first = packet("a", "one");
        let mut next = first.clone();
        next["omissions"] = json!([{"reason":"private"}]);
        let mut reordered = first.clone();
        reordered["budget"] = json!({"maxTokens":10});
        assert_eq!(
            diagnose_cache_prefix(&next, None).prefix_digest,
            diagnose_cache_prefix(&reordered, None).prefix_digest
        );
        assert_eq!(
            diagnose_cache_prefix(&first, None).prefix_digest,
            diagnose_cache_prefix(&next, None).prefix_digest
        );
    }

    #[test]
    fn schema_version_break_is_attributed_exactly() {
        let first = packet("a", "one");
        let mut next = first.clone();
        next["schemaVersion"] = json!(2);
        let diagnostic = diagnose_cache_prefix(&next, Some(&first));
        assert_eq!(
            diagnostic.cache_break.unwrap().metadata_field.as_deref(),
            Some("schemaVersion")
        );
    }
    #[test]
    fn first_changed_block_is_attributed_stably() {
        let first = packet("a", "one");
        let next = packet("a", "two");
        let diagnostic = diagnose_cache_prefix(&next, Some(&first));
        assert_eq!(
            diagnostic.cache_break.unwrap().block_id.as_deref(),
            Some("a")
        );
        assert_eq!(diagnostic.stable_block_order, ["a"]);
    }
    #[test]
    fn metadata_break_names_exact_field() {
        let first = packet("a", "one");
        let mut next = first.clone();
        next["mode"] = json!("degraded");
        let diagnostic = diagnose_cache_prefix(&next, Some(&first));
        assert_eq!(
            diagnostic.cache_break.unwrap().metadata_field.as_deref(),
            Some("mode")
        );
    }
}
