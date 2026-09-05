//! Final native MCP serialization gate. The host still owns surrounding model
//! messages; this measurement is explicitly limited to the MCP tool result.
use membrane_protocol::host_observation::RemainingContextCeilingV1;
use serde_json::{json, Value};
use super::recovery::RecoveryError;

pub fn fit_native_response(mut envelope: Value, ceiling: &RemainingContextCeilingV1) -> Result<Value, RecoveryError> {
    ceiling.validate().map_err(|_| RecoveryError::Denied)?;
    if ceiling.remaining_tokens.basis.id != "o200k_base" || ceiling.remaining_tokens.basis.version != "1" {
        return Err(RecoveryError::Denied);
    }
    let limit = ceiling.remaining_tokens.estimate.value.ok_or(RecoveryError::Denied)?;
    let data = envelope.pointer_mut("/result/data").and_then(Value::as_object_mut).ok_or(RecoveryError::Corrupt)?;
    data.insert("deliveryMeasurement".into(), json!({"schemaVersion":1,"scope":"serialized_mcp_tool_result",
        "basis":"o200k_base/1","ceilingId":ceiling.ceiling_id,"tokens":0,"bytes":0,
        "remainingTokens":limit,"providerBilledTokens":null}));
    for _ in 0..16 {
        let wire = membrane_mcp::tool_result(envelope.clone()).to_string();
        let tokens = cortex_core::ContextTokenAccounting::count_exact(&wire).map_err(|_| RecoveryError::Unavailable)? as u64;
        let bytes = wire.len();
        if envelope.pointer("/result/data/deliveryMeasurement/tokens").and_then(Value::as_u64) == Some(tokens)
            && envelope.pointer("/result/data/deliveryMeasurement/bytes").and_then(Value::as_u64) == Some(bytes as u64) {
            // Leave space for the JSON-RPC id/envelope and HTTP headers within
            // the native client's 128 KiB read cap. Never truncate the content.
            return if tokens <= limit && bytes <= 120 * 1024 { Ok(envelope) } else { Err(RecoveryError::Limit) };
        }
        envelope["result"]["data"]["deliveryMeasurement"]["tokens"] = json!(tokens);
        envelope["result"]["data"]["deliveryMeasurement"]["bytes"] = json!(bytes);
    }
    Err(RecoveryError::Corrupt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use membrane_protocol::host_observation::{EstimatorBasisV1,HostObservationProvenanceV1,ObservedFieldV1,TokenEstimateV1,REMAINING_CONTEXT_CEILING_SCHEMA_VERSION};
    fn ceiling(tokens: u64) -> RemainingContextCeilingV1 {
        RemainingContextCeilingV1 { schema_version:REMAINING_CONTEXT_CEILING_SCHEMA_VERSION,ceiling_id:"ceiling".into(),session_id:"session".into(),task_id:ObservedFieldV1::complete("task".into()),requested_at_unix_ms:1,
            remaining_tokens:TokenEstimateV1::complete(EstimatorBasisV1::new("o200k_base","1"),tokens),
            provenance_receipt:HostObservationProvenanceV1::new("receipt","fixture",1,"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa") }
    }
    #[test]
    fn counts_the_real_mcp_shape_and_refuses_wrapper_overflow() {
        let envelope = json!({"schemaVersion":1,"operation":"membrane_context","errorVersion":1,"result":{"kind":"success","data":{"packet":{"text":"payload".repeat(100)}}}});
        let fitted = fit_native_response(envelope.clone(), &ceiling(10000)).unwrap();
        let wire = membrane_mcp::tool_result(fitted.clone()).to_string();
        let count = cortex_core::ContextTokenAccounting::count_exact(&wire).unwrap();
        assert_eq!(fitted["result"]["data"]["deliveryMeasurement"]["bytes"],wire.len());
        assert_eq!(fitted["result"]["data"]["deliveryMeasurement"]["tokens"],count);
        assert!(matches!(fit_native_response(envelope,&ceiling(100)),Err(RecoveryError::Limit)));
    }
}
