use membrane_federation::requirements::{
    compile_requirement_set, coverage_map, plan_acquisition, CandidateJourneyStateV1, CandidateJourneyV1, EvidenceDimensionV1,
    ProviderCapabilityV1, RequirementFactV1,
};
use membrane_protocol::ProviderId;

#[test]
fn generic_task_signals_are_deterministic_and_never_authority() {
    let first = compile_requirement_set("request-1", "Explain current policy failure", &[]);
    let second = compile_requirement_set("request-1", "Explain current policy failure", &[]);
    assert_eq!(first, second);
    assert_eq!(first.task_id, "request-1");
    assert!(first.facts.iter().any(|fact| fact.dimension == EvidenceDimensionV1::CurrentState));
    assert!(first.facts.iter().any(|fact| fact.dimension == EvidenceDimensionV1::Policy));
}

#[test]
fn caller_facts_only_strengthen_and_unavailable_coverage_is_unsatisfied() {
    let requirements = compile_requirement_set("task-1", "current status", &[RequirementFactV1 {
        schema_version: 1,
        dimension: EvidenceDimensionV1::Diagnostics,
        required: true,
        rule_id: String::new(),
        binding_digest: String::new(),
        exact_target: None,
        source_hash: None,
        representation_digest: None,
    }]);
    let capabilities = vec![ProviderCapabilityV1 {
        provider: ProviderId::LiveFiles,
        dimensions: vec![EvidenceDimensionV1::CurrentState],
        authoritative: true,
        fresh: true,
        ready: true,
        cost_rank: 0,
        omission: None,
    }];
    let plan = plan_acquisition(&requirements, &capabilities);
    assert_eq!(plan.providers, vec![ProviderId::LiveFiles]);
    let coverage = coverage_map(&requirements, vec![], &capabilities);
    assert!(coverage.unsatisfied.contains(&EvidenceDimensionV1::Diagnostics));
}

#[test]
fn capable_provider_without_surviving_evidence_is_unsatisfied() {
    let requirements = compile_requirement_set("task-2", "explain repository truth", &[RequirementFactV1 {
        schema_version: 1,
        dimension: EvidenceDimensionV1::RepositoryTruth,
        required: true,
        rule_id: "typed".into(),
        binding_digest: "sha256:typed".into(),
        exact_target: None,
        source_hash: None,
        representation_digest: None,
    }]);
    let capabilities = vec![ProviderCapabilityV1 {
        provider: ProviderId::Blueprint,
        dimensions: vec![EvidenceDimensionV1::RepositoryTruth],
        authoritative: true, fresh: true, ready: true, cost_rank: 0, omission: None,
    }];
    let coverage = coverage_map(&requirements, vec![], &capabilities);
    assert!(coverage.unsatisfied.contains(&EvidenceDimensionV1::RepositoryTruth));
}

#[test]
fn dropped_or_unemitted_exact_target_is_unsatisfied() {
    let requirements = compile_requirement_set("task-3", "fix foo.rs", &[]);
    assert!(requirements.facts.iter().any(|fact| fact.exact_target.as_deref() == Some("foo.rs")));
    let capabilities = vec![ProviderCapabilityV1 {
        provider: ProviderId::LiveFiles,
        dimensions: vec![EvidenceDimensionV1::CurrentState],
        authoritative: true, fresh: true, ready: true, cost_rank: 0, omission: None,
    }];
    let coverage = coverage_map(&requirements, vec![CandidateJourneyV1 {
        evidence_id: "evidence:foo".into(), requirement_binding_digest: requirements.facts.iter().find(|fact| fact.dimension == EvidenceDimensionV1::CurrentState).unwrap().binding_digest.clone(),
        dimension: EvidenceDimensionV1::CurrentState, provider: ProviderId::LiveFiles,
        source_hash: "sha256:source".into(), representation_digest: "sha256:representation".into(), target_ref: Some("foo.rs".into()),
        acquired: true, eligible: true, admitted: true, represented: true, fenced: true,
        emitted: false, retained: false, dropped: true, state: CandidateJourneyStateV1::DiscoveredBudgetDropped,
    }], &capabilities);
    assert!(coverage.unsatisfied.contains(&EvidenceDimensionV1::CurrentState));
}

#[test]
fn journey_binding_prevents_cross_dimension_coverage() {
    let requirements = compile_requirement_set("task-4", "policy diagnostics", &[]);
    let capabilities = vec![
        ProviderCapabilityV1 { provider: ProviderId::Rules, dimensions: vec![EvidenceDimensionV1::Policy], authoritative: true, fresh: true, ready: true, cost_rank: 0, omission: None },
        ProviderCapabilityV1 { provider: ProviderId::Audit, dimensions: vec![EvidenceDimensionV1::Diagnostics], authoritative: true, fresh: true, ready: true, cost_rank: 1, omission: None },
        ProviderCapabilityV1 { provider: ProviderId::LiveFiles, dimensions: vec![EvidenceDimensionV1::CurrentState], authoritative: true, fresh: true, ready: true, cost_rank: 2, omission: None },
    ];
    let current = requirements.facts.iter().find(|fact| fact.dimension == EvidenceDimensionV1::CurrentState).unwrap();
    let coverage = coverage_map(&requirements, vec![CandidateJourneyV1 {
        evidence_id: "evidence:current".into(), requirement_binding_digest: current.binding_digest.clone(),
        dimension: EvidenceDimensionV1::CurrentState, provider: ProviderId::LiveFiles,
        source_hash: "sha256:source".into(), representation_digest: "sha256:representation".into(), target_ref: None,
        acquired: true, eligible: true, admitted: true, represented: true, fenced: true,
        emitted: true, retained: false, dropped: false, state: CandidateJourneyStateV1::DiscoveredAccepted,
    }], &capabilities);
    assert!(coverage.unsatisfied.contains(&EvidenceDimensionV1::Policy));
    assert!(coverage.unsatisfied.contains(&EvidenceDimensionV1::Diagnostics));
}

#[test]
fn source_and_representation_constraints_require_exact_match() {
    let requirements = compile_requirement_set("task-5", "current", &[RequirementFactV1 {
        schema_version: 1, dimension: EvidenceDimensionV1::CurrentState, required: true,
        rule_id: "typed".into(), binding_digest: "binding-5".into(), exact_target: None,
        source_hash: Some("sha256:right-source".into()), representation_digest: Some("sha256:right-representation".into()),
    }]);
    let capabilities = vec![ProviderCapabilityV1 { provider: ProviderId::LiveFiles, dimensions: vec![EvidenceDimensionV1::CurrentState], authoritative: true, fresh: true, ready: true, cost_rank: 0, omission: None }];
    let wrong = CandidateJourneyV1 {
        evidence_id: "evidence:1".into(), requirement_binding_digest: "binding-5".into(),
        dimension: EvidenceDimensionV1::CurrentState, provider: ProviderId::LiveFiles,
        source_hash: "sha256:wrong-source".into(), representation_digest: "sha256:wrong-representation".into(), target_ref: None,
        acquired: true, eligible: true, admitted: true, represented: true, fenced: true,
        emitted: true, retained: false, dropped: false, state: CandidateJourneyStateV1::DiscoveredAccepted,
    };
    assert!(coverage_map(&requirements, vec![wrong.clone()], &capabilities).unsatisfied.contains(&EvidenceDimensionV1::CurrentState));
    let right = CandidateJourneyV1 { source_hash: "sha256:right-source".into(), representation_digest: "sha256:right-representation".into(), ..wrong };
    assert!(!coverage_map(&requirements, vec![right], &capabilities).unsatisfied.contains(&EvidenceDimensionV1::CurrentState));
}

#[test]
fn exact_target_and_provider_capability_cannot_cross_bind() {
    let requirements = compile_requirement_set("task-6", "fix foo.rs", &[]);
    let fact = requirements.facts.iter().find(|fact| fact.dimension == EvidenceDimensionV1::CurrentState).unwrap();
    let capabilities = vec![ProviderCapabilityV1 {
        provider: ProviderId::LiveFiles, dimensions: vec![EvidenceDimensionV1::CurrentState],
        authoritative: true, fresh: true, ready: true, cost_rank: 0, omission: None,
    }];
    let wrong_target = CandidateJourneyV1 {
        evidence_id: "evidence:other".into(), requirement_binding_digest: fact.binding_digest.clone(),
        dimension: EvidenceDimensionV1::CurrentState, provider: ProviderId::LiveFiles,
        source_hash: "sha256:source".into(), representation_digest: "sha256:representation".into(),
        target_ref: Some("other.rs".into()), acquired: true, eligible: true, admitted: true,
        represented: true, fenced: true, emitted: true, retained: false, dropped: false, state: CandidateJourneyStateV1::Stale,
    };
    assert!(coverage_map(&requirements, vec![wrong_target.clone()], &capabilities).unsatisfied.contains(&EvidenceDimensionV1::CurrentState));
    let wrong_provider = CandidateJourneyV1 { provider: ProviderId::Rules, target_ref: Some("foo.rs".into()), ..wrong_target };
    assert!(coverage_map(&requirements, vec![wrong_provider], &capabilities).unsatisfied.contains(&EvidenceDimensionV1::CurrentState));
}

#[test]
fn journey_states_are_typed_without_claiming_host_use() {
    let state = CandidateJourneyStateV1::AdapterDropped;
    assert_eq!(serde_json::to_value(state).unwrap(), "ADAPTER_DROPPED");
    let parsed: CandidateJourneyStateV1 = serde_json::from_value(serde_json::json!("EXECUTION_FAILURE")).unwrap();
    assert_eq!(parsed, CandidateJourneyStateV1::ExecutionFailure);
}
