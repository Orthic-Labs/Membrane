//! Current remediation contract checks. Historical migration snapshots are
//! intentionally outside this test and remain byte-frozen.

use membrane_adapt::remediation::{
    InterventionTarget, RemediationEffect, RemediationProposalV1, SealedRemediationProposalV1,
    SEALED_REMEDIATION_SCHEMA,
};

#[test]
fn current_contract_carries_versioned_effect_and_target() {
    let schema: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../adapt/src/adapt/remediation-proposal.schema.json"
    )))
    .expect("current remediation schema is valid JSON");
    assert_eq!(
        schema["properties"]["schema_version"]["const"],
        SEALED_REMEDIATION_SCHEMA
    );
    let required = schema["properties"]["payload"]["required"]
        .as_array()
        .expect("payload required list");
    assert!(required.iter().any(|value| value == "effect"));
    assert!(required.iter().any(|value| value == "intervention_target"));

    let issue = format!("ii_{}", "d".repeat(64));
    let proposal = RemediationProposalV1::build_with_target(
        &issue,
        "routing_failure",
        RemediationEffect::ProcessChange,
        InterventionTarget::RoutingPolicy,
        "review routing policy",
        vec![],
    );
    let sealed = SealedRemediationProposalV1::seal(
        &proposal,
        "requires_human_review",
        "diagnostic only",
        "adapt-remediation-v1",
        "adapt-redaction-v1",
        None,
        vec![],
        "test",
        "2026-08-27T00:00:00Z",
    )
    .expect("current contract seals");
    let payload = serde_json::to_value(&sealed).expect("sealed proposal serializes");
    assert_eq!(payload["schema_version"], SEALED_REMEDIATION_SCHEMA);
    assert_eq!(payload["payload"]["effect"], "process_change");
    assert_eq!(payload["payload"]["intervention_target"], "routing_policy");
    assert_eq!(
        payload["payload"]["proposal_kind"],
        "routing_recommendation"
    );
    assert!(sealed.verify().is_ok());
}
