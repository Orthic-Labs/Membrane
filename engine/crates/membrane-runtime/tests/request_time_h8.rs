use cortex_core::planner::{BlockV1, BudgetV1, ContextPacketV1};
use membrane_protocol::host_observation::{
    EstimatorBasisV1, HostObservationProvenanceV1, ObservationUnavailableReasonV1, ObservedFieldV1,
    RemainingContextCeilingV1, TokenEstimateV1, REMAINING_CONTEXT_CEILING_SCHEMA_VERSION,
};
use membrane_runtime::push::selection::{
    build_packet_reduction_plan, parse_request_time_h8, select_packet_for_h8,
    PacketReductionRequestError, RequestTimeH8Error,
};

fn block(id: &str, protected: bool, tokens: usize) -> BlockV1 {
    BlockV1 {
        id: id.to_owned(),
        layer: 1,
        provider: "fixture".to_owned(),
        source_kind: "file".to_owned(),
        source_ref: format!("source://{id}"),
        source_hash: format!("sha256:{:0<64}", id),
        trust_class: "trusted".to_owned(),
        instruction_policy: "data".to_owned(),
        base_commit: None,
        overlay_digest: None,
        freshness_class: None,
        snapshot_id: None,
        priority: 1,
        estimated_tokens: tokens,
        delivery_stage: None,
        delivery_class: None,
        selected_tokens: Some(tokens),
        allotted_tokens: Some(tokens.saturating_sub(8)),
        rendered_tokens: None,
        delivered_chars: None,
        drop_reason: None,
        protected,
        recoverable: true,
        resolver: format!("resolver://{id}"),
        text: if protected {
            "protected task evidence ".repeat(tokens)
        } else {
            "ordinary context evidence more prose for useful review\n".repeat(tokens)
        },
    }
}

fn packet() -> ContextPacketV1 {
    ContextPacketV1 {
        schema_version: 1,
        trace_id: "trace-h8".to_owned(),
        task: "task-h8".to_owned(),
        mode: "test".to_owned(),
        budget: BudgetV1 {
            max_tokens: 128,
            admitted_tokens: 128,
            packet_char_budget_default: None,
            packet_char_budget_override: None,
            packet_char_budget_model: None,
            configured_packet_char_budget: None,
            effective_packet_char_budget: None,
        },
        allocations: Default::default(),
        provider_accounting: Default::default(),
        blocks: vec![block("protected", true, 32), block("ordinary", false, 96)],
        omissions: Vec::new(),
    }
}

fn ceiling(remaining: TokenEstimateV1) -> RemainingContextCeilingV1 {
    RemainingContextCeilingV1 {
        schema_version: REMAINING_CONTEXT_CEILING_SCHEMA_VERSION,
        ceiling_id: "ceiling-h8".to_owned(),
        session_id: "session-h8".to_owned(),
        task_id: ObservedFieldV1::complete("task-h8".to_owned()),
        requested_at_unix_ms: 1_700_000_000_000,
        remaining_tokens: remaining,
        provenance_receipt: HostObservationProvenanceV1::new(
            "receipt-h8",
            "fixture-host",
            1_700_000_000_000,
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ),
    }
}

#[test]
fn selects_largest_membrane_representation_for_same_request() {
    let h8 = ceiling(TokenEstimateV1::complete(EstimatorBasisV1::new("o200k_base", "1"), 100_000));
    let selected = select_packet_for_h8(&packet(), &h8).expect("exact H8 should select");
    assert_eq!(selected.selected_representation.id, "full");
    assert_eq!(selected.selected_representation.tokens,
        membrane_runtime::push::selection::measure_packet(&selected.selected_representation.content, &h8.remaining_tokens.basis).unwrap());
    assert!(selected.selected_representation.tokens > 128, "metadata allocations must not masquerade as measured size");
    assert_eq!(selected.selection_receipt.ceiling_id, h8.ceiling_id);
    assert_eq!(selected.selection_receipt.session_id, h8.session_id);
    assert_eq!(selected.selection_receipt.plan_ref, "packet://trace-h8");
    assert_eq!(selected.selection_receipt.estimator_basis, h8.remaining_tokens.basis);
}

#[test]
fn selected_content_is_complete_for_full_reduced_and_floor() {
    use membrane_runtime::push::{delivery, recovery, selection};
    let temp = tempfile::tempdir().unwrap();
    let store = recovery::RecoveryStore::at(temp.path());
    let scope = recovery::RecoveryScope::new(temp.path(), "session-h8").unwrap();
    let proof = delivery::resolver_probe(&store, &scope).unwrap();
    let owner = selection::RecoveryContext {store:&store,scope:&scope,resolver_token:proof["resolverToken"].as_str().unwrap()};
    let basis = EstimatorBasisV1::new("o200k_base", "1");
    let result = selection::select_packet_for_h8_with_recovery(&packet(),
        &ceiling(TokenEstimateV1::complete(basis.clone(),100_000)),
        &membrane_runtime::push::prep::PushPolicy::Control, Some(&owner)).unwrap();
    let representations = &result.plan.representations;
    assert!(representations[0].tokens > representations[1].tokens);
    assert!(representations[1].tokens > representations[2].tokens);
    for representation in representations {
        let h8 = ceiling(TokenEstimateV1::complete(basis.clone(), representation.tokens));
        let chosen = result.plan.select_for_capacity(&h8).unwrap();
        assert_eq!(chosen.id, representation.id);
        assert_eq!(chosen.tokens, selection::measure_packet(&chosen.content,&basis).unwrap());
        assert_eq!(chosen.content["blocks"][0]["text"], packet().blocks[0].text);
        // Omitted bodies retain explicit evidence identity and an authorized
        // original, rather than silently disappearing from the floor packet.
        assert_eq!(chosen.content["blocks"].as_array().unwrap().len(),2);
        if chosen.id != "full" {
            let handle = chosen.content["blocks"][1]["resolver"].as_str().unwrap();
            let original = store.resolve(&scope, handle, &recovery::Selector::Whole, recovery::MAX_RESTORE_BYTES, recovery::now_ms()).unwrap();
            assert_eq!(original.content, packet().blocks[1].text);
        }
    }
}

#[test]
fn structured_passthrough_cannot_fit_by_claiming_a_smaller_allocation() {
    let mut packet = packet();
    packet.blocks[1].source_ref = "data.json".into();
    packet.blocks[1].text = format!("[{}]", vec!["{\"a\":123456,\"b\":\"long\"}";100].join(","));
    packet.blocks[1].selected_tokens = Some(1000);
    packet.blocks[1].allotted_tokens = Some(2);
    packet.budget.admitted_tokens = 1100;
    let basis = EstimatorBasisV1::new("o200k_base", "1");
    let plan = build_packet_reduction_plan(&packet,basis.clone()).unwrap();
    let measured = plan.representations[0].tokens;
    assert!(measured > 1100);
    assert!(select_packet_for_h8(&packet,&ceiling(TokenEstimateV1::complete(basis,350))).is_err());
}

#[test]
fn unknown_counter_is_not_relabelled_as_a_host_tokenizer() {
    assert!(build_packet_reduction_plan(&packet(),EstimatorBasisV1::new("unregistered-provider","1")).is_err());
}

#[test]
fn rejects_missing_or_inexact_request_time_capacity_without_fallback() {
    let body = serde_json::json!({"task": "task-h8"});
    assert!(matches!(
        parse_request_time_h8(&body, "session-h8", "task-h8"),
        Err(RequestTimeH8Error::Missing)
    ));

    let h8 = ceiling(TokenEstimateV1::unavailable(
        EstimatorBasisV1::new("o200k_base", "1"),
        ObservationUnavailableReasonV1::HostUnsupported,
    ));
    let body = serde_json::json!({
        "task": "task-h8",
        "remainingContextCeiling": h8,
    });
    assert!(matches!(
        parse_request_time_h8(&body, "session-h8", "task-h8"),
        Err(RequestTimeH8Error::Inexact { .. })
    ));
}

#[test]
fn refuses_identity_mismatch_as_typed_request_error() {
    let h8 = ceiling(TokenEstimateV1::complete(
        EstimatorBasisV1::new("o200k_base", "1"),
        120,
    ));
    let body = serde_json::json!({"remainingContextCeiling": h8});
    let error = parse_request_time_h8(&body, "different-session", "task-h8")
        .expect_err("session mismatch must refuse");
    assert!(matches!(
        PacketReductionRequestError::H8(error),
        PacketReductionRequestError::H8(RequestTimeH8Error::IdentityMismatch {
            field: "sessionId",
            ..
        })
    ));
}

#[test]
fn refuses_unbound_task_identity_as_inexact_request_time_h8() {
    let mut h8 = ceiling(TokenEstimateV1::complete(
        EstimatorBasisV1::new("o200k_base", "1"),
        120,
    ));
    h8.task_id = ObservedFieldV1::unavailable(ObservationUnavailableReasonV1::HostUnsupported);
    let body = serde_json::json!({"remainingContextCeiling": h8});
    assert!(matches!(
        parse_request_time_h8(&body, "session-h8", "task-h8"),
        Err(RequestTimeH8Error::Inexact { .. })
    ));
}

#[test]
fn refuses_mismatched_estimator_basis_in_request_selection() {
    let plan = build_packet_reduction_plan(
        &packet(),
        EstimatorBasisV1::new("o200k_base", "1"),
    )
    .expect("packet plan should be valid");
    let h8 = ceiling(TokenEstimateV1::complete(
        EstimatorBasisV1::new("different-estimator", "v2"),
        120,
    ));
    let error = plan
        .select_for_capacity(&h8)
        .expect_err("a different H8 estimator basis must not be compared");
    assert!(matches!(
        error,
        membrane_protocol::push::PacketReductionSelectionError::EstimatorBasisMismatch(_)
    ));
}

#[test]
fn refuses_cached_or_next_request_ceiling_without_direct_request_time_field() {
    let h8 = ceiling(TokenEstimateV1::complete(
        EstimatorBasisV1::new("o200k_base", "1"),
        120,
    ));
    let cached = serde_json::json!({
        "task": "task-h8",
        "cachedRemainingContextCeiling": h8,
    });
    assert!(matches!(
        parse_request_time_h8(&cached, "session-h8", "task-h8"),
        Err(RequestTimeH8Error::Missing)
    ));
}

#[test]
fn refuses_request_when_no_viable_floor_fits() {
    let h8 = ceiling(TokenEstimateV1::complete(
        EstimatorBasisV1::new("o200k_base", "1"),
        31,
    ));
    let error = select_packet_for_h8(&packet(), &h8)
        .expect_err("capacity below the protected floor must fail typed");
    assert!(matches!(
        error,
        PacketReductionRequestError::Selection(
            membrane_protocol::push::PacketReductionSelectionError::NoRepresentationFits {
                remaining_tokens: 31,
                ..
            }
        )
    ));
}
