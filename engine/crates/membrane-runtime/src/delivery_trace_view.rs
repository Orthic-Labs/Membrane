//! Read-only, content-free projection for the Hub delivery trace explorer.
//! Digest reconciliation is explicit: missing or conflicting receipts never become success.
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SCHEMA_VERSION: u32 = 1;
const PHASES: [&str; 9] = ["task", "providers", "candidates", "admission", "render", "hostDelivery", "evidence", "outcome", "feedback"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TraceDigestReconciliation {
    pub packet_digest: Option<String>,
    pub host_digest: Option<String>,
    pub event_store_digest: Option<String>,
    pub outcome_digest: Option<String>,
    pub state: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryTracePhase { pub name: String, pub state: String, pub evidence: String }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryTraceView {
    pub schema_version: u32,
    pub trace_id: String,
    pub state: String,
    pub reason: String,
    pub phases: Vec<DeliveryTracePhase>,
    pub digests: TraceDigestReconciliation,
}

fn text(root: &Value, camel: &str, snake: &str) -> Option<String> {
    root.get(camel).or_else(|| root.get(snake)).and_then(Value::as_str).filter(|s| !s.trim().is_empty()).map(str::to_owned)
}
fn child<'a>(root: &'a Value, camel: &str, snake: &str) -> &'a Value { root.get(camel).or_else(|| root.get(snake)).unwrap_or(&Value::Null) }

/// Project one externally assembled trace. No data-plane reads or writes occur here.
pub fn project_delivery_trace(report: &Value) -> DeliveryTraceView {
    let data = report.get("result").and_then(|v| v.get("data")).unwrap_or(report);
    let trace_id = text(data, "traceId", "trace_id").unwrap_or_else(|| "unknown".into());
    let packet = child(data, "packet", "packet_receipt");
    let host = child(data, "hostDelivery", "host_delivery");
    let event = child(data, "eventStore", "event_store");
    let outcome = child(data, "outcome", "outcome_receipt");
    let packet_digest = text(packet, "digest", "packet_digest").or_else(|| text(data, "packetDigest", "packet_digest"));
    let host_digest = text(host, "digest", "host_digest").or_else(|| text(data, "hostDigest", "host_digest"));
    let event_store_digest = text(event, "digest", "event_store_digest").or_else(|| text(data, "eventStoreDigest", "event_store_digest"));
    let outcome_digest = text(outcome, "digest", "outcome_digest").or_else(|| text(data, "outcomeDigest", "outcome_digest"));
    let values = [&packet_digest, &host_digest, &event_store_digest, &outcome_digest];
    let missing = values.iter().any(Option::is_none);
    let mismatch = !missing && values.windows(2).any(|pair| pair[0] != pair[1]);
    let (state, reason) = if missing { ("unavailable", "unknown — packet, host, event-store, or outcome digest missing") }
        else if mismatch { ("degraded", "digest mismatch — trace receipts do not reconcile") }
        else { ("available", "packet, host, event-store, and outcome digests reconcile") };
    let phases = PHASES.iter().map(|name| {
        let source = child(data, name, &name.replace("Delivery", "_delivery"));
        let phase_state = text(source, "state", "status").unwrap_or_else(|| "unknown".into());
        let evidence = text(source, "evidence", "reason").unwrap_or_else(|| "unknown — no evidence".into());
        DeliveryTracePhase { name: (*name).into(), state: phase_state, evidence }
    }).collect();
    DeliveryTraceView { schema_version: SCHEMA_VERSION, trace_id, state: state.into(), reason: reason.into(), phases, digests: TraceDigestReconciliation { packet_digest, host_digest, event_store_digest, outcome_digest, state: state.into(), reason: reason.into() } }
}
