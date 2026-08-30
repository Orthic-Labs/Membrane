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
            "ordinary context evidence ".repeat(tokens)
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
    let h8 = ceiling(TokenEstimateV1::complete(
        EstimatorBasisV1::new("fixture-estimator", "v1"),
        120,
    ));
    let selected = select_packet_for_h8(&packet(), &h8).expect("exact H8 should select");

    assert_eq!(selected.selected_representation.id, "reduced_1");
    assert_eq!(selected.selected_representation.tokens, 112);
    assert_eq!(selected.selection_receipt.ceiling_id, h8.ceiling_id);
    assert_eq!(selected.selection_receipt.session_id, h8.session_id);
    assert_eq!(selected.selection_receipt.plan_ref, "packet://trace-h8");
    assert_eq!(
        selected.selection_receipt.estimator_basis,
        EstimatorBasisV1::new("fixture-estimator", "v1")
    );
}

#[test]
fn selected_content_is_complete_for_full_reduced_and_floor() {
    let basis = EstimatorBasisV1::new("fixture-estimator", "v1");
    let full = select_packet_for_h8(
        &packet(),
        &ceiling(TokenEstimateV1::complete(basis.clone(), 200)),
    )
    .expect("full representation should fit");
    let reduced = select_packet_for_h8(
        &packet(),
        &ceiling(TokenEstimateV1::complete(basis.clone(), 120)),
    )
    .expect("reduced representation should fit");
    let floor = select_packet_for_h8(&packet(), &ceiling(TokenEstimateV1::complete(basis, 32)))
        .expect("protected floor should fit");

    assert_eq!(full.selected_representation.id, "full");
    assert_eq!(reduced.selected_representation.id, "reduced_1");
    assert_eq!(floor.selected_representation.id, "floor");

    let full_bytes = serde_json::to_vec(&full.selected_representation.content).unwrap();
    let reduced_bytes = serde_json::to_vec(&reduced.selected_representation.content).unwrap();
    let floor_bytes = serde_json::to_vec(&floor.selected_representation.content).unwrap();
    assert_ne!(full_bytes, reduced_bytes);
    assert_ne!(reduced_bytes, floor_bytes);
    assert_ne!(full_bytes, floor_bytes);

    assert_eq!(
        full.selected_representation.content["blocks"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        reduced.selected_representation.content["blocks"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        floor.selected_representation.content["blocks"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_ne!(
        full.selected_representation.content["blocks"][1]["text"],
        reduced.selected_representation.content["blocks"][1]["text"]
    );
    assert_eq!(
        floor.selected_representation.content["blocks"][0]["id"],
        "protected"
    );
}

#[test]
fn rejects_missing_or_inexact_request_time_capacity_without_fallback() {
    let body = serde_json::json!({"task": "task-h8"});
    assert!(matches!(
        parse_request_time_h8(&body, "session-h8", "task-h8"),
        Err(RequestTimeH8Error::Missing)
    ));

    let h8 = ceiling(TokenEstimateV1::unavailable(
        EstimatorBasisV1::new("fixture-estimator", "v1"),
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
        EstimatorBasisV1::new("fixture-estimator", "v1"),
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
        EstimatorBasisV1::new("fixture-estimator", "v1"),
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
        EstimatorBasisV1::new("packet-estimator", "v1"),
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
        EstimatorBasisV1::new("fixture-estimator", "v1"),
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
        EstimatorBasisV1::new("fixture-estimator", "v1"),
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
