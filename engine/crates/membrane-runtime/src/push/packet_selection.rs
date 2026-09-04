//! Materialization and measurement under one planner-owned capacity observation.
use cortex_core::planner::{BlockV1, ContextPacketV1};
use membrane_protocol::host_observation::RemainingContextCeilingV1;
use membrane_protocol::push::{PacketReductionPlanV1,PacketReductionRepresentationV1,PacketReductionSelectionReceiptV1,PACKET_REDUCTION_SELECTION_RECEIPT_SCHEMA_VERSION};
use serde_json::Value;
use std::path::Path;
use super::prep::{PushPolicy,is_code_ext,is_structured_text};
use super::selection::{PacketReductionRequestError,PacketReductionSelectionV1,RequestTimeH8Error};
use super::telemetry;

/// Count the exact serialized packet data using the named tokenizer. Unknown
/// host counters are refused; copying a host label is never a measurement.
pub fn measure_packet(value: &Value, basis: &membrane_protocol::host_observation::EstimatorBasisV1) -> Result<u64, PacketReductionRequestError> {
    if basis.id != "o200k_base" || basis.version != "1" {
        return Err(PacketReductionRequestError::H8(RequestTimeH8Error::Invalid("unsupported estimator basis; expected o200k_base/1".into())));
    }
    cortex_core::ContextTokenAccounting::count_exact(&value.to_string())
        .map(|n| n as u64).map_err(PacketReductionRequestError::ContentSerialization)
}

/// Host-proven resolver capability, scoped by the already-authorized caller.
pub struct RecoveryContext<'a> {
    pub store: &'a super::recovery::RecoveryStore,
    pub scope: &'a super::recovery::RecoveryScope,
    pub resolver_token: &'a str,
}

pub fn build_packet_reduction_plan(packet: &ContextPacketV1, basis: membrane_protocol::host_observation::EstimatorBasisV1)
    -> Result<PacketReductionPlanV1, PacketReductionRequestError> {
    build_packet_reduction_plan_with_policy(packet, basis, &PushPolicy::Control)
}
pub fn build_packet_reduction_plan_with_policy(packet: &ContextPacketV1, basis: membrane_protocol::host_observation::EstimatorBasisV1, policy: &PushPolicy)
    -> Result<PacketReductionPlanV1, PacketReductionRequestError> {
    // Pure callers cannot claim recovery for an unretained input.
    build_with_recovery(packet, basis, policy, None)
}
fn build_with_recovery(packet: &ContextPacketV1, basis: membrane_protocol::host_observation::EstimatorBasisV1,
    policy: &PushPolicy, recovery: Option<&RecoveryContext<'_>>) -> Result<PacketReductionPlanV1, PacketReductionRequestError> {
    if packet.trace_id.trim().is_empty() { return Err(PacketReductionRequestError::EmptyPacket("traceId")); }
    if packet.blocks.is_empty() { return Err(PacketReductionRequestError::EmptyPacket("blocks")); }
    if packet.blocks.len() > 4096 || packet.blocks.iter().map(|b| b.text.len()).sum::<usize>() > super::recovery::MAX_ARTIFACT_BYTES {
        return Err(PacketReductionRequestError::ContentSerialization("push_packet_limit".into()));
    }
    let mut protected = Vec::new();
    let mut all_ids = Vec::new();
    for block in &packet.blocks {
        for (field, value) in [("id", &block.id), ("resolver", &block.resolver)] {
            if value.trim().is_empty() { return Err(PacketReductionRequestError::EmptyBlockField {block:block.id.clone(), field}); }
        }
        if all_ids.contains(&block.id) { return Err(PacketReductionRequestError::ContentSerialization("duplicate block identity".into())); }
        all_ids.push(block.id.clone());
        if block.protected { protected.push(block.id.clone()); }
    }
    if protected.is_empty() { return Err(PacketReductionRequestError::NoProtectedMaterial); }
    // Refused query policy is terminal. The public opt-in cannot mint proof.
    let policy_admitted = match policy {
        PushPolicy::Control => true,
        PushPolicy::QueryAware(metadata) => metadata.authority_admitted && metadata.freshness_valid,
    };
    let recovery = recovery.filter(|r| policy_admitted && super::delivery::can_resolve(r.store, r.scope, Some(r.resolver_token)).unwrap_or(false));
    let full = measured_packet_content(packet.clone(), &basis)?;
    let full_tokens = measure_packet(&full, &basis)?;
    let mut reduced = packet.clone();
    let mut floor = packet.clone();
    if let Some(owner) = recovery {
        for (reduced_block, floor_block) in reduced.blocks.iter_mut().zip(floor.blocks.iter_mut()) {
            if reduced_block.protected { continue; }
            let original = reduced_block.text.clone();
            // Complete no-op/unsupported results stay exact; no second lossy
            // transform runs after a refusal or failed source-span proof.
            let path = Path::new(&reduced_block.source_ref);
            let (candidate, mappings) = if is_code_ext(path) {
                super::ast::render(path, &original)
            } else if is_structured_text(path, &original) {
                (original.clone(), Vec::new())
            } else {
                match super::fidelity::extract_lines(&original, original.len() / 2, &[]) {
                    Ok(result) => result, Err(_) => (original.clone(), Vec::new()),
                }
            };
            if candidate == original || candidate.len() >= original.len() { continue; }
            if super::fidelity::validate(original.as_bytes(), &super::recovery::digest(original.as_bytes()), candidate.as_bytes(), &mappings, &[]).is_err() { continue; }
            let reference = match owner.store.publish(owner.scope, original.as_bytes(), 7*24*60*60*1000, super::recovery::now_ms()) {
                Ok(reference) => reference, Err(_) => continue,
            };
            // Keep protected lines even in floor. Error/constraint information
            // is not replaced by a pointer just because the original survives.
            let protected_spans = super::fidelity::protected_lines(&original);
            let protected_size = protected_spans.iter().map(|s| s.end-s.start).sum();
            let floor_text = if is_code_ext(path) { candidate.clone() }
                else { super::fidelity::extract_lines(&original, protected_size, &[]).map(|x| x.0).unwrap_or_else(|_| original.clone()) };
            let marker = format!("\n[Push original: {}; expiresAt={}; store={}; resolve using membrane_push_resolve]", reference.handle, reference.expires_at, reference.store_id);
            reduced_block.text = format!("{candidate}{marker}");
            reduced_block.resolver = reference.handle.clone();
            reduced_block.recoverable = true;
            floor_block.text = format!("{floor_text}{marker}");
            floor_block.resolver = reference.handle;
            floor_block.recoverable = true;
        }
    }
    let mut reduced = measured_packet_content(reduced, &basis)?;
    let mut floor = measured_packet_content(floor, &basis)?;
    let mut reduced_tokens = measure_packet(&reduced, &basis)?;
    if reduced_tokens >= full_tokens { reduced = full.clone(); reduced_tokens = full_tokens; }
    let mut floor_tokens = measure_packet(&floor, &basis)?;
    if floor_tokens >= reduced_tokens { floor = reduced.clone(); floor_tokens = reduced_tokens; }
    let minimum = floor_tokens;
    let mut representations = Vec::new();
    for (id, tokens, content) in [("full",full_tokens,full),("reduced_1",reduced_tokens,reduced),("floor",floor_tokens,floor)] {
        let resolver_paths = content["blocks"].as_array().unwrap().iter()
            .filter_map(|b| b["resolver"].as_str().map(str::to_owned)).collect();
        representations.push(PacketReductionRepresentationV1 {id:id.into(), tokens,
            parent_ref:format!("packet://{}", packet.trace_id), protected:protected.clone(),
            evidence_refs:all_ids.clone(), resolver_paths, minimum_viable_tokens:minimum,
            coverage_note:"Exact serialized packet measurement; complete evidence or authorized retained original; host framing measured separately".into(), content});
    }
    telemetry::record("packet-measure", full_tokens as usize, reduced_tokens as usize, Some("unit=tokens;basis=o200k_base/1;scope=serialized_packet"), None);
    let plan = PacketReductionPlanV1 {schema_version:PacketReductionPlanV1::SCHEMA_VERSION,
        estimator_basis:basis, representations, protected, minimum_viable_tokens:minimum};
    plan.validate().map_err(PacketReductionRequestError::InvalidPlan)?;
    Ok(plan)
}
fn measured_packet_content(mut packet: ContextPacketV1, basis: &membrane_protocol::host_observation::EstimatorBasisV1)
    -> Result<Value, PacketReductionRequestError> {
    // Validate the counter before publishing any representation.
    measure_packet(&Value::Null, basis)?;
    let mut content_tokens = 0usize;
    for block in &mut packet.blocks {
        let measured = cortex_core::ContextTokenAccounting::count_exact(&block.text).map_err(PacketReductionRequestError::ContentSerialization)?;
        block.selected_tokens = Some(measured); block.rendered_tokens = Some(measured);
        content_tokens = content_tokens.saturating_add(measured);
    }
    packet.budget.admitted_tokens = content_tokens;
    serde_json::to_value(packet).map_err(|e| PacketReductionRequestError::ContentSerialization(e.to_string()))
}

pub fn select_packet_for_h8(packet: &ContextPacketV1, ceiling: &RemainingContextCeilingV1) -> Result<PacketReductionSelectionV1, PacketReductionRequestError> {
    select_packet_for_h8_with_policy(packet, ceiling, &PushPolicy::Control)
}
pub fn select_packet_for_h8_with_policy(packet: &ContextPacketV1, ceiling: &RemainingContextCeilingV1, policy: &PushPolicy)
    -> Result<PacketReductionSelectionV1, PacketReductionRequestError> {
    select_packet_for_h8_with_recovery(packet, ceiling, policy, None)
}
pub fn select_packet_for_h8_with_recovery(packet: &ContextPacketV1, ceiling: &RemainingContextCeilingV1, policy: &PushPolicy, recovery: Option<&RecoveryContext<'_>>)
    -> Result<PacketReductionSelectionV1, PacketReductionRequestError> {
    let plan = build_with_recovery(packet, ceiling.remaining_tokens.basis.clone(), policy, recovery)?;
    let selected = plan.select_for_capacity(ceiling).map_err(PacketReductionRequestError::Selection)?.clone();
    let remaining_tokens = ceiling.remaining_tokens.estimate.value.ok_or_else(|| PacketReductionRequestError::H8(RequestTimeH8Error::Inexact {
        coverage:ceiling.remaining_tokens.estimate.coverage, reason:ceiling.remaining_tokens.estimate.unavailable_reason }))?;
    let receipt = PacketReductionSelectionReceiptV1 {
        schema_version:PACKET_REDUCTION_SELECTION_RECEIPT_SCHEMA_VERSION,
        plan_ref:selected.parent_ref.clone(), ceiling_id:ceiling.ceiling_id.clone(), session_id:ceiling.session_id.clone(),
        selected_representation_id:selected.id.clone(), selected_tokens:selected.tokens, remaining_tokens,
        estimator_basis:plan.estimator_basis.clone(), decision:"selected".into(),
    };
    Ok(PacketReductionSelectionV1 { plan, selected_representation:selected, selection_receipt:receipt })
}
