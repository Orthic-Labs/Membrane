//! One preparation owner shared by MCP, HTTP and host adapters. This API has a
//! byte budget, explicitly not a fabricated model-token/H8 observation.
use super::fidelity::{self, Span};
use super::recovery::{self, RecoveryError, RecoveryReference, RecoveryScope, RecoveryStore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentKind { #[default] Text, Code, Json, Log }
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrepareRequest {
    pub text: String,
    #[serde(default)] pub kind: ContentKind,
    #[serde(default)] pub source_path: Option<String>,
    pub max_bytes: usize,
    #[serde(default)] pub resolver_token: Option<String>,
    /// Exact is monotone: it is never overridden by optimize or a small budget.
    #[serde(default)] pub exact: bool,
    #[serde(default)] pub optimize: bool,
    #[serde(default)] pub protected_spans: Vec<Span>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryReceipt {
    pub source_digest: String,
    pub representation_digest: String,
    pub input_bytes: usize,
    pub serialized_delivery_bytes: usize,
    pub baseline_delivery_bytes: usize,
    pub saved_bytes: usize,
    pub measurement_basis: &'static str,
    pub transform: &'static str,
    pub decision: &'static str,
    pub segment_count: usize,
    pub task_outcome: &'static str,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedDelivery {
    pub schema_version: u32,
    pub text: String,
    pub representation_kind: &'static str,
    pub inline_fidelity: &'static str,
    pub disposition: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery: Option<RecoveryReference>,
    pub receipt: DeliveryReceipt,
}
struct ConsumerProof { scope: String, store_id: String, expires: u64 }
const MAX_CONSUMER_PROOFS: usize = 512;
const CONSUMER_PROOF_TTL_MS: u64 = 300_000;
const CONSUMER_PROOF_REUSE_FLOOR_MS: u64 = 30_000;
static CONSUMERS: OnceLock<Mutex<HashMap<String, ConsumerProof>>> = OnceLock::new();
fn consumers() -> &'static Mutex<HashMap<String, ConsumerProof>> { CONSUMERS.get_or_init(|| Mutex::new(HashMap::new())) }
fn probe_payload(token: String, store_id: String, expires: u64) -> Value {
    json!({"schemaVersion":1,"resolver":"membrane_push_resolve","resolverToken":token,
        "storeId":store_id,"expiresAt":expires,"selectors":["whole","bytes","lines","json"],
        "maxRestoreBytes":recovery::MAX_RESTORE_BYTES,"disposition":"exact","telemetry":super::telemetry::status()})
}

/// Called through the authorized resolver operation, not self-declared by a
/// prepare request. Restart invalidates proofs, but never stored originals.
pub fn resolver_probe(store: &RecoveryStore, scope: &RecoveryScope) -> Result<Value, RecoveryError> {
    let now = recovery::now_ms();
    let store_id = store.identity()?;
    let mut proofs = consumers().lock().map_err(|_| RecoveryError::Unavailable)?;
    proofs.retain(|_, proof| proof.expires > now);
    if let Some((token, proof)) = proofs.iter().find(|(_, proof)|
        proof.scope == scope.binding() && proof.store_id == store_id && proof.expires.saturating_sub(now) > CONSUMER_PROOF_REUSE_FLOOR_MS)
    {
        return Ok(probe_payload(token.clone(), store_id, proof.expires));
    }
    proofs.retain(|_, proof| !(proof.scope == scope.binding() && proof.store_id == store_id));
    if proofs.len() >= MAX_CONSUMER_PROOFS { return Err(RecoveryError::Limit); }
    let mut nonce = [0u8; 32];
    getrandom::fill(&mut nonce).map_err(|_| RecoveryError::Unavailable)?;
    let token = hex::encode(nonce);
    let expires = now + CONSUMER_PROOF_TTL_MS;
    proofs.insert(token.clone(), ConsumerProof { scope: scope.binding().into(), store_id: store_id.clone(), expires });
    Ok(probe_payload(token, store_id, expires))
}
pub(crate) fn can_resolve(store: &RecoveryStore, scope: &RecoveryScope, token: Option<&str>) -> Result<bool, RecoveryError> {
    let Some(token) = token.filter(|t| t.len() == 64) else { return Ok(false); };
    let id = store.identity()?;
    let now = recovery::now_ms();
    let mut proofs = consumers().lock().map_err(|_| RecoveryError::Unavailable)?;
    proofs.retain(|_, proof| proof.expires > now);
    Ok(proofs.get(token).is_some_and(|p| p.scope == scope.binding() && p.store_id == id))
}
fn exact_delivery(text: &str) -> PreparedDelivery {
    let hash = format!("sha256:{}", recovery::digest(text.as_bytes()));
    PreparedDelivery { schema_version:1, text:text.into(), representation_kind:"original",
        inline_fidelity:"exact_bytes", disposition:"exact", recovery:None,
        receipt:DeliveryReceipt { source_digest:hash.clone(), representation_digest:hash,
            input_bytes:text.len(), serialized_delivery_bytes:0, baseline_delivery_bytes:0,
            saved_bytes:0, measurement_basis:"utf8_serialized_push_delivery_v1", transform:"none",
            decision:"passthrough", segment_count:0, task_outcome:"unknown" } }
}
/// Fixed-point accounting includes the count fields themselves. No output is
/// classified as fitting on the basis of its pre-transform allocation.
fn measure(delivery: &mut PreparedDelivery, baseline: Option<usize>) -> Result<usize, RecoveryError> {
    for _ in 0..16 {
        let length = serde_json::to_vec(delivery).map_err(|_| RecoveryError::Corrupt)?.len();
        let baseline = baseline.unwrap_or(length);
        let saved = baseline.saturating_sub(length);
        if delivery.receipt.serialized_delivery_bytes == length && delivery.receipt.baseline_delivery_bytes == baseline && delivery.receipt.saved_bytes == saved { return Ok(length); }
        delivery.receipt.serialized_delivery_bytes = length;
        delivery.receipt.baseline_delivery_bytes = baseline;
        delivery.receipt.saved_bytes = saved;
    }
    Err(RecoveryError::Corrupt)
}
fn fold_log(source: &str) -> Result<String, RecoveryError> {
    let mut runs: Vec<(String, usize)> = Vec::new();
    for line in source.split_inclusive('\n') {
        if let Some((last, count)) = runs.last_mut().filter(|(last, _)| last.as_str() == line) { let _ = last; *count += 1; }
        else { runs.push((line.into(), 1)); }
        if runs.len() > 100_000 { return Err(RecoveryError::Limit); }
    }
    let wire = serde_json::to_string(&json!({"encoding":"membrane.rle-lines.v1","runs":runs})).map_err(|_| RecoveryError::Corrupt)?;
    let decoded: Value = serde_json::from_str(&wire).map_err(|_| RecoveryError::Corrupt)?;
    let mut restored = String::new();
    for run in decoded["runs"].as_array().ok_or(RecoveryError::Corrupt)? {
        let text = run[0].as_str().ok_or(RecoveryError::Corrupt)?;
        let count = run[1].as_u64().ok_or(RecoveryError::Corrupt)? as usize;
        if text.len().checked_mul(count).and_then(|n| restored.len().checked_add(n)).is_none_or(|n| n > recovery::MAX_ARTIFACT_BYTES) { return Err(RecoveryError::Limit); }
        for _ in 0..count { restored.push_str(text); }
    }
    if restored != source { return Err(RecoveryError::Corrupt); }
    Ok(wire)
}

pub fn prepare(store: &RecoveryStore, scope: &RecoveryScope, request: PrepareRequest) -> Result<PreparedDelivery, RecoveryError> {
    if request.text.len() > recovery::MAX_ARTIFACT_BYTES || request.max_bytes == 0
        || request.max_bytes > recovery::MAX_ARTIFACT_BYTES || request.protected_spans.len() > 4096 {
        return Err(RecoveryError::Limit);
    }
    if request.protected_spans.iter().any(|s| s.start >= s.end || s.end > request.text.len()) { return Err(RecoveryError::InvalidSelector); }
    let mut exact = exact_delivery(&request.text);
    let original_bytes = measure(&mut exact, None)?;
    if request.exact || (!request.optimize && original_bytes <= request.max_bytes) {
        return if original_bytes <= request.max_bytes { Ok(exact) } else { Err(RecoveryError::Limit) };
    }
    if !can_resolve(store, scope, request.resolver_token.as_deref())? {
        if original_bytes <= request.max_bytes {
            exact.receipt.decision = "resolver_unavailable_passthrough";
            if measure(&mut exact, None)? <= request.max_bytes { return Ok(exact); }
        }
        return Err(RecoveryError::Denied);
    }
    let (text, transform, fidelity, segments) = match request.kind {
        ContentKind::Json => (recovery::minify_json(&request.text).ok_or(RecoveryError::InvalidSelector)?, "json_whitespace_v1", "json_syntax_equivalent", 0),
        ContentKind::Log => (fold_log(&request.text)?, "rle_lines_v1", "reversible_codec", 0),
        ContentKind::Code => {
            let path = request.source_path.as_deref().map(Path::new).ok_or(RecoveryError::InvalidSelector)?;
            let (text, mappings) = super::skel::skeletonize_with_spans(path, &request.text);
            fidelity::validate(request.text.as_bytes(), &recovery::digest(request.text.as_bytes()), text.as_bytes(), &mappings, &request.protected_spans)?;
            let count = mappings.len();
            (text, "ast_function_bodies_v2", "interface_projection", count)
        }
        ContentKind::Text => {
            let budget = request.max_bytes.saturating_sub(2048);
            let (text, mappings) = fidelity::extract_lines(&request.text, budget, &request.protected_spans)?;
            let count = mappings.len();
            (text, "protected_lines_v1", "source_projection", count)
        }
    };
    if text == request.text || text.len() >= request.text.len() {
        return if original_bytes <= request.max_bytes { Ok(exact) } else { Err(RecoveryError::Limit) };
    }
    // Commit verified original before a reduced result can escape this owner.
    let reference = store.publish(scope, request.text.as_bytes(), 7*24*60*60*1000, recovery::now_ms())?;
    let mut reduced = exact.clone();
    reduced.text = text;
    reduced.representation_kind = transform;
    reduced.inline_fidelity = fidelity;
    reduced.disposition = "prepared";
    reduced.recovery = Some(reference);
    reduced.receipt.transform = transform;
    reduced.receipt.decision = "reduced";
    reduced.receipt.segment_count = segments;
    reduced.receipt.representation_digest = format!("sha256:{}", recovery::digest(reduced.text.as_bytes()));
    let measured = measure(&mut reduced, Some(original_bytes))?;
    if measured < original_bytes && measured <= request.max_bytes {
        super::telemetry::record("prepare", original_bytes, measured, Some("status=reduced;scope=serialized_delivery"), Some(&reduced.receipt.source_digest));
        return Ok(reduced);
    }
    if original_bytes <= request.max_bytes { Ok(exact) } else { Err(RecoveryError::Limit) }
}
use std::path::Path;

#[cfg(test)]
mod tests {
    use super::*;
    fn request(text: String, kind: ContentKind, token: Option<String>, max_bytes: usize) -> PrepareRequest {
        PrepareRequest { text, kind, source_path:None, max_bytes, resolver_token:token, exact:false, optimize:true, protected_spans:vec![] }
    }
    #[test]
    fn real_prepare_resolve_loop_includes_final_wire_cost() {
        let temp = tempfile::tempdir().unwrap();
        let store = RecoveryStore::at(temp.path());
        let scope = RecoveryScope::new(temp.path(), "session").unwrap();
        let token = resolver_probe(&store, &scope).unwrap()["resolverToken"].as_str().unwrap().to_string();
        let text = "repeat this exact event\r\n".repeat(200);
        let delivery = prepare(&store, &scope, request(text.clone(), ContentKind::Log, Some(token), 2500)).unwrap();
        assert!(delivery.receipt.saved_bytes > 0);
        assert_eq!(delivery.receipt.serialized_delivery_bytes, serde_json::to_vec(&delivery).unwrap().len());
        let reference = delivery.recovery.unwrap();
        let recovered = store.resolve(&scope, &reference.handle, &recovery::Selector::Whole, recovery::MAX_RESTORE_BYTES, recovery::now_ms()).unwrap();
        assert_eq!(recovered.bytes().unwrap(), text.as_bytes());
        let mut reentry = request(recovered.content, ContentKind::Log, None, 10_000);
        reentry.exact = true;
        assert_eq!(prepare(&store, &scope, reentry).unwrap().text, text);
    }
    #[test]
    fn repeated_probe_reuses_live_binding_instead_of_exhausting_registry() {
        let temp = tempfile::tempdir().unwrap();
        let store = RecoveryStore::at(temp.path());
        let scope = RecoveryScope::new(temp.path(), "probe-reuse").unwrap();
        let first = resolver_probe(&store, &scope).unwrap()["resolverToken"].as_str().unwrap().to_string();
        for _ in 0..600 {
            assert_eq!(resolver_probe(&store, &scope).unwrap()["resolverToken"].as_str(), Some(first.as_str()));
        }
    }

    #[test]
    fn consumer_proof_is_scope_bound_and_exact_never_gets_reduced() {
        let temp = tempfile::tempdir().unwrap();
        let store = RecoveryStore::at(temp.path());
        let a = RecoveryScope::new(temp.path(), "a").unwrap();
        let b = RecoveryScope::new(temp.path(), "b").unwrap();
        let token = resolver_probe(&store, &a).unwrap()["resolverToken"].as_str().unwrap().to_string();
        assert!(prepare(&store, &b, request("text\n".repeat(1000), ContentKind::Log, Some(token), 2000)).is_err());
        let mut r = request("text\n".repeat(1000), ContentKind::Log, None, 2000);
        r.exact = true;
        assert!(matches!(prepare(&store, &a, r), Err(RecoveryError::Limit)));
    }
}
