use memright::doc_candidate_provider::{
    plan_with_doc_shadow, DocCandidateProviderRequestV1, RegisteredDocCandidateProvider,
    DOC_CANDIDATE_PROVIDER_NAME,
};
use memright::doc_shadow::{
    DocCandidateFreshnessV1, DocCandidateProviderCandidateV1, DocTaskClassV1, DocumentClass,
};
use memright_core::planner::PlannerInput;

fn planner_input() -> PlannerInput {
    serde_json::from_value(serde_json::json!({
        "candidate_set": {
            "schemaVersion": 1,
            "traceId": "doc-shadow-trace",
            "task": "find deploy runbook",
            "mode": "verify",
            "provider": "blueprint",
            "freshness": {"revision": "main@123", "indexedAt": "", "stale": false},
            "providerCeiling": {"maxCandidates": 1, "maxEstimatedTokens": 64},
            "candidates": [{
                "id": "baseline",
                "layer": 3,
                "sourceKind": "repo_code",
                "sourceRef": "src/lib.rs:1",
                "sourceHash": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "trustClass": "workspace_tracked",
                "instructionPolicy": "data_only",
                "providerScore": 0.9,
                "estimatedTokens": 32,
                "protected": false,
                "exact": false,
                "recoverable": true,
                "resolver": "blueprint resolve baseline",
                "text": "baseline context"
            }],
            "omissions": []
        },
        "max_tokens": 64
    }))
    .expect("valid planner fixture")
}

fn candidate(doc_id: &str, class: DocumentClass, hash: &str) -> DocCandidateProviderCandidateV1 {
    DocCandidateProviderCandidateV1 {
        doc_id: doc_id.into(),
        document_class: class,
        freshness: DocCandidateFreshnessV1 {
            content_hash: hash.into(),
            revision: "main@123".into(),
            index_generation: 7,
        },
    }
}

#[test]
fn shadow_doc_provider_observes_two_fresh_runbook_candidates_without_changing_planner_packet() {
    let input = planner_input();
    let current = DocCandidateFreshnessV1 {
        content_hash: "sha256:current".into(),
        revision: "main@123".into(),
        index_generation: 7,
    };
    let request = DocCandidateProviderRequestV1::new(
        DocTaskClassV1::Runbook,
        current,
        vec![
            candidate("runbook-1", DocumentClass::Runbook, "sha256:current"),
            candidate("decision-1", DocumentClass::Decision, "sha256:current"),
            candidate("stale", DocumentClass::Runbook, "sha256:old"),
        ],
    );

    let direct = memright_core::planner::plan(&input).expect("planner accepts baseline");
    let observed = plan_with_doc_shadow(&input, &RegisteredDocCandidateProvider, request)
        .expect("shadow observation leaves planner valid");

    assert_eq!(
        serde_json::to_value(&observed.planner.packet).expect("packet serializes"),
        serde_json::to_value(&direct.packet).expect("packet serializes")
    );
    assert_eq!(
        serde_json::to_value(&observed.planner.receipts).expect("receipts serialize"),
        serde_json::to_value(&direct.receipts).expect("receipts serialize")
    );
    assert_eq!(observed.doc_shadow.provider, DOC_CANDIDATE_PROVIDER_NAME);
    assert!(observed.doc_shadow.selection.shadow_only);
    assert!(!observed.doc_shadow.admitted_to_planner);
    assert_eq!(
        observed
            .doc_shadow
            .selection
            .candidates
            .iter()
            .map(|candidate| candidate.doc_id.as_str())
            .collect::<Vec<_>>(),
        vec!["runbook-1", "decision-1"]
    );
}
