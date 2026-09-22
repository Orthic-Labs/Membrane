//! Materialization and measurement under one planner-owned capacity observation.
use cortex_core::planner::{BlockV1, ContextPacketV1};
use membrane_protocol::host_observation::RemainingContextCeilingV1;
use membrane_protocol::push::{PacketReductionPlanV1,PacketReductionRepresentationV1,PacketReductionSelectionError,PacketReductionSelectionReceiptV1,PACKET_REDUCTION_SELECTION_RECEIPT_SCHEMA_VERSION};
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
    // Ordinary retrieval may contain no protected blocks. The empty protected
    // set is valid; exact measurement, recovery & host capacity still apply.
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
        let mut resolver_paths = Vec::new();
        for resolver in content["blocks"].as_array().unwrap().iter()
            .filter_map(|block| block["resolver"].as_str()) {
            if !resolver_paths.iter().any(|existing| existing == resolver) {
                resolver_paths.push(resolver.to_owned());
            }
        }
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
    let encoded = serde_json::to_value(packet)
        .map_err(|error| PacketReductionRequestError::ContentSerialization(error.to_string()))?;
    let mut packet: membrane_protocol::ContextPacketV1 = serde_json::from_value(encoded)
        .map_err(|error| PacketReductionRequestError::ContentSerialization(error.to_string()))?;
    packet.provider_accounting.clear();
    for block in &mut packet.blocks {
        let (class, mode, lane, selected, rendered, chars) = if !block.text.is_empty() {
            (membrane_protocol::DeliveryClass::Rendered, Some(membrane_protocol::DeliveryMode::Inline), membrane_protocol::BudgetLaneKind::Rendered,
             block.selected_tokens.unwrap_or(0), block.rendered_tokens.unwrap_or(0),
             u32::try_from(block.text.chars().count()).map_err(|error| PacketReductionRequestError::ContentSerialization(error.to_string()))?)
        } else if !block.resolver.is_empty() {
            (membrane_protocol::DeliveryClass::ResolverBacked, Some(membrane_protocol::DeliveryMode::Reference), membrane_protocol::BudgetLaneKind::ResolverBacked, 0, 0, 0)
        } else {
            (membrane_protocol::DeliveryClass::MetadataOnly, None, membrane_protocol::BudgetLaneKind::MetadataOnly, 0, 0, 0)
        };
        block.delivery_stage = Some(membrane_protocol::DeliveryStage::Finalized);
        block.delivery_class = Some(class);
        block.delivery_mode = mode;
        block.lane = Some(lane);
        block.selected_tokens = Some(selected);
        block.rendered_tokens = Some(rendered);
        block.delivered_chars = Some(chars);
        let reason = block.drop_reason.unwrap_or(membrane_protocol::DropReason::None);
        let accounting = packet.provider_accounting.entry(block.provider.clone()).or_insert(membrane_protocol::ProviderAccountingV1 {
            delivery_stage: membrane_protocol::DeliveryStage::Finalized,
            selected_tokens: 0,
            rendered_tokens: 0,
            delivered_chars: 0,
            drop_reason: reason,
        });
        accounting.selected_tokens = accounting.selected_tokens.saturating_add(selected);
        accounting.rendered_tokens = accounting.rendered_tokens.saturating_add(rendered);
        accounting.delivered_chars = accounting.delivered_chars.saturating_add(chars);
        if accounting.drop_reason != reason { accounting.drop_reason = membrane_protocol::DropReason::Multiple; }
    }
    let reconciliation = membrane_core::reconcile::reconcile(&packet);
    packet.reconciliation = Some(serde_json::from_value(
        serde_json::to_value(reconciliation)
            .map_err(|error| PacketReductionRequestError::ContentSerialization(error.to_string()))?
    ).map_err(|error| PacketReductionRequestError::ContentSerialization(error.to_string()))?);
    serde_json::to_value(packet)
        .map_err(|error| PacketReductionRequestError::ContentSerialization(error.to_string()))
}

pub(crate) fn finalize_receipts(fields: &mut serde_json::Map<String, Value>, selected_packet: &Value) {
    const COPIED: &[&str] = &[
        "deliveryStage",
        "deliveryClass",
        "selectedTokens",
        "allottedTokens",
        "renderedTokens",
        "deliveredChars",
        "dropReason",
    ];
    let delivered: std::collections::BTreeMap<&str, &Value> = selected_packet
        .get("blocks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|block| {
            block
                .get("id")
                .and_then(Value::as_str)
                .map(|id| (id, block))
        })
        .collect();
    let Some(receipts) = fields.get_mut("receipts").and_then(Value::as_array_mut) else {
        return;
    };
    for receipt in receipts.iter_mut().filter_map(Value::as_object_mut) {
        let Some(id) = receipt
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            continue;
        };
        if let Some(block) = delivered.get(id.as_str()) {
            for key in COPIED {
                if let Some(value) = block.get(*key) {
                    receipt.insert((*key).to_owned(), value.clone());
                }
            }
        } else {
            receipt.insert(
                "deliveryStage".to_owned(),
                Value::String("finalized".to_owned()),
            );
            receipt.insert(
                "deliveryClass".to_owned(),
                Value::String("metadata_only".to_owned()),
            );
            receipt.insert("selectedTokens".to_owned(), Value::from(0u64));
            receipt.remove("allottedTokens");
            receipt.insert("renderedTokens".to_owned(), Value::from(0u64));
            receipt.insert("deliveredChars".to_owned(), Value::from(0u64));
            let preserved = receipt
                .get("dropReason")
                .and_then(Value::as_str)
                .is_some_and(|reason| reason != "none");
            if !preserved {
                receipt.insert(
                    "dropReason".to_owned(),
                    Value::String("not_selected".to_owned()),
                );
            }
        }
    }
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

/// Bounded-response selection (implementation-contract §"Pull budget
/// contract", PUL-050/051): the same measured full/reduced/floor ladder and
/// protected-item invariants as `select_packet_for_h8_with_recovery`, but the
/// capacity bound is the caller's declared response budget in
/// `o200k_base/1` tokens rather than a host observation. No host identity is
/// invented — the receipt names the declared budget instead of a ceiling
/// identity. A budget below the minimum viable representation is the typed
/// `NoRepresentationFits` refusal: never a partial, truncated, or
/// protected-evidence-dropping emission.
pub fn select_packet_for_token_budget(packet: &ContextPacketV1, budget_tokens: u64,
    policy: &PushPolicy, recovery: Option<&RecoveryContext<'_>>)
    -> Result<PacketReductionSelectionV1, PacketReductionRequestError> {
    if budget_tokens == 0 {
        return Err(PacketReductionRequestError::Selection(
            PacketReductionSelectionError::NoRepresentationFits {
                remaining_tokens: 0, minimum_viable_tokens: u64::MAX }));
    }
    let basis = membrane_protocol::host_observation::EstimatorBasisV1::new("o200k_base", "1");
    let plan = build_with_recovery(packet, basis, policy, recovery)?;
    let mut selected: Option<&PacketReductionRepresentationV1> = None;
    for representation in &plan.representations {
        if representation.tokens <= budget_tokens
            && selected.as_ref().map_or(true, |current| representation.tokens > current.tokens)
        {
            selected = Some(representation);
        }
    }
    let selected = selected
        .ok_or(PacketReductionSelectionError::NoRepresentationFits {
            remaining_tokens: budget_tokens,
            minimum_viable_tokens: plan.minimum_viable_tokens })
        .map_err(PacketReductionRequestError::Selection)?
        .clone();
    let receipt = PacketReductionSelectionReceiptV1 {
        schema_version:PACKET_REDUCTION_SELECTION_RECEIPT_SCHEMA_VERSION,
        plan_ref:selected.parent_ref.clone(), ceiling_id:format!("budget://declared/{budget_tokens}"),
        session_id:packet.trace_id.clone(),
        selected_representation_id:selected.id.clone(), selected_tokens:selected.tokens,
        remaining_tokens:budget_tokens.saturating_sub(selected.tokens),
        estimator_basis:plan.estimator_basis.clone(), decision:"selected".into(),
    };
    Ok(PacketReductionSelectionV1 { plan, selected_representation:selected, selection_receipt:receipt })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn block_json(id: &str, provider: &str, text: &str, resolver: &str) -> Value {
        json!({
            "id": id,
            "layer": 0,
            "provider": provider,
            "sourceKind": "doc",
            "sourceRef": format!("src/{id}"),
            "sourceHash": format!("sha256:{}", "0".repeat(64)),
            "trustClass": "trusted",
            "instructionPolicy": "data_only",
            "priority": 0,
            "estimatedTokens": 1,
            "protected": false,
            "recoverable": true,
            "resolver": resolver,
            "text": text,
        })
    }

    fn packet_json(blocks: Vec<Value>) -> Value {
        json!({
            "schemaVersion": 1,
            "traceId": "t1",
            "task": "task",
            "mode": "mode",
            "budget": {"maxTokens": 100000, "admittedTokens": 0},
            "allocations": {},
            "blocks": blocks,
            "omissions": [],
        })
    }

    #[test]
    fn measured_packet_content_finalizes_rendered_accounting() {
        let packet: ContextPacketV1 = serde_json::from_value(packet_json(vec![
            block_json("b1", "alpha", "alpha rendered text", "resolve-a"),
            block_json("b2", "beta", "beta rendered text body", "resolve-b"),
        ]))
        .unwrap();
        let basis = membrane_protocol::host_observation::EstimatorBasisV1::new("o200k_base", "1");
        let plan = build_packet_reduction_plan(&packet, basis.clone()).unwrap();
        let selected = plan
            .representations
            .iter()
            .find(|representation| representation.id == "full")
            .unwrap();
        let finalized = &selected.content;
        let blocks = finalized["blocks"].as_array().unwrap();
        assert_eq!(blocks.len(), 2);
        for (block, text) in blocks
            .iter()
            .zip(["alpha rendered text", "beta rendered text body"])
        {
            let measured = cortex_core::ContextTokenAccounting::count_exact(text).unwrap() as u64;
            assert_eq!(block["deliveryStage"], json!("finalized"));
            assert_eq!(block["deliveryClass"], json!("rendered"));
            assert_eq!(block["deliveryMode"], json!("inline"));
            assert_eq!(block["lane"], json!("rendered"));
            assert_eq!(
                block["deliveredChars"],
                json!(u64::try_from(text.chars().count()).unwrap())
            );
            assert_eq!(block["selectedTokens"], json!(measured));
            assert_eq!(block["renderedTokens"], json!(measured));
        }
        let accounting = finalized["providerAccounting"].as_object().unwrap();
        assert_eq!(accounting.len(), 2);
        for (provider, text) in
            [("alpha", "alpha rendered text"), ("beta", "beta rendered text body")]
        {
            let measured = cortex_core::ContextTokenAccounting::count_exact(text).unwrap() as u64;
            let entry = &accounting[provider];
            assert_eq!(entry["deliveryStage"], json!("finalized"));
            assert_eq!(entry["selectedTokens"], json!(measured));
            assert_eq!(entry["renderedTokens"], json!(measured));
            assert_eq!(
                entry["deliveredChars"],
                json!(u64::try_from(text.chars().count()).unwrap())
            );
            assert_eq!(entry["dropReason"], json!("none"));
        }
        assert!(finalized["reconciliation"].is_object());
        let protocol: membrane_protocol::ContextPacketV1 =
            serde_json::from_value(finalized.clone()).unwrap();
        let cortex: ContextPacketV1 = serde_json::from_value(finalized.clone()).unwrap();
        assert_eq!(protocol.blocks.len(), 2);
        assert_eq!(cortex.blocks.len(), 2);
        assert_eq!(measure_packet(finalized, &basis).unwrap(), selected.tokens);
    }

    #[test]
    fn measured_packet_content_preserves_nonrendered_semantics() {
        let mut metadata = block_json("m1", "beta", "", "");
        metadata["dropReason"] = json!("missing_resolver");
        let packet: ContextPacketV1 = serde_json::from_value(packet_json(vec![
            block_json("r1", "alpha", "", "resolve-handle"),
            metadata,
        ]))
        .unwrap();
        let basis = membrane_protocol::host_observation::EstimatorBasisV1::new("o200k_base", "1");
        let finalized = measured_packet_content(packet, &basis).unwrap();
        let blocks = finalized["blocks"].as_array().unwrap();
        let resolver_backed = &blocks[0];
        assert_eq!(resolver_backed["deliveryStage"], json!("finalized"));
        assert_eq!(resolver_backed["deliveryClass"], json!("resolver_backed"));
        assert_eq!(resolver_backed["deliveryMode"], json!("reference"));
        assert_eq!(resolver_backed["lane"], json!("resolver_backed"));
        assert_eq!(resolver_backed["selectedTokens"], json!(0));
        assert_eq!(resolver_backed["renderedTokens"], json!(0));
        assert_eq!(resolver_backed["deliveredChars"], json!(0));
        let metadata_only = &blocks[1];
        assert_eq!(metadata_only["deliveryStage"], json!("finalized"));
        assert_eq!(metadata_only["deliveryClass"], json!("metadata_only"));
        assert!(metadata_only.get("deliveryMode").is_none());
        assert_eq!(metadata_only["lane"], json!("metadata_only"));
        assert_eq!(metadata_only["selectedTokens"], json!(0));
        assert_eq!(metadata_only["renderedTokens"], json!(0));
        assert_eq!(metadata_only["deliveredChars"], json!(0));
        assert_eq!(metadata_only["dropReason"], json!("missing_resolver"));
        let accounting = finalized["providerAccounting"].as_object().unwrap();
        assert_eq!(accounting["alpha"]["dropReason"], json!("none"));
        assert_eq!(accounting["beta"]["dropReason"], json!("missing_resolver"));
    }

    #[test]
    fn finalize_receipts_reconciles_selected_and_omitted_ids() {
        let selected_packet = json!({
            "blocks": [{
                "id": "b1",
                "deliveryStage": "finalized",
                "deliveryClass": "rendered",
                "selectedTokens": 10,
                "allottedTokens": 10,
                "renderedTokens": 10,
                "deliveredChars": 40,
                "dropReason": "none",
                "lane": "rendered",
                "deliveryMode": "inline",
            }],
        });
        let mut fields = serde_json::Map::new();
        fields.insert(
            "receipts".to_owned(),
            json!([
                {"id": "b1", "decision": "admitted", "deliveryStage": "planned", "allottedTokens": 7},
                {"id": "absent-admitted", "decision": "admitted", "allottedTokens": 5, "dropReason": "none"},
                {"id": "absent-rejected", "decision": "rejected", "allottedTokens": 3, "dropReason": "missing_resolver"},
            ]),
        );
        finalize_receipts(&mut fields, &selected_packet);
        let receipts = fields["receipts"].as_array().unwrap();
        let selected = receipts[0].as_object().unwrap();
        assert_eq!(selected["deliveryStage"], json!("finalized"));
        assert_eq!(selected["deliveryClass"], json!("rendered"));
        assert_eq!(selected["selectedTokens"], json!(10));
        assert_eq!(selected["allottedTokens"], json!(10));
        assert_eq!(selected["renderedTokens"], json!(10));
        assert_eq!(selected["deliveredChars"], json!(40));
        assert_eq!(selected["dropReason"], json!("none"));
        assert!(!selected.contains_key("lane"));
        assert!(!selected.contains_key("deliveryMode"));
        let omitted = receipts[1].as_object().unwrap();
        assert_eq!(omitted["deliveryStage"], json!("finalized"));
        assert_eq!(omitted["deliveryClass"], json!("metadata_only"));
        assert_eq!(omitted["selectedTokens"], json!(0));
        assert!(!omitted.contains_key("allottedTokens"));
        assert_eq!(omitted["renderedTokens"], json!(0));
        assert_eq!(omitted["deliveredChars"], json!(0));
        assert_eq!(omitted["dropReason"], json!("not_selected"));
        let rejected = receipts[2].as_object().unwrap();
        assert_eq!(rejected["deliveryStage"], json!("finalized"));
        assert_eq!(rejected["deliveryClass"], json!("metadata_only"));
        assert_eq!(rejected["selectedTokens"], json!(0));
        assert!(!rejected.contains_key("allottedTokens"));
        assert_eq!(rejected["renderedTokens"], json!(0));
        assert_eq!(rejected["deliveredChars"], json!(0));
        assert_eq!(rejected["dropReason"], json!("missing_resolver"));
    }
}
