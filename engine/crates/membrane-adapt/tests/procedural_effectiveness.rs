use membrane_adapt::procedural_effectiveness::*;

fn observation() -> ProceduralAssetObservationV1 {
    let p = Provenance {
        receipt_id: "r1".into(),
        receipt_digest: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            .into(),
        source: "host".into(),
        observed_at: "2026-08-28T00:00:00Z".into(),
    };
    fn n<T>(v: T) -> Observed<T> {
        Observed::complete(v)
    }
    ProceduralAssetObservationV1 {
        observation_id: "o1".into(),
        asset_id: "asset-1".into(),
        assessed_at: "2026-08-28T00:00:00Z".into(),
        exposures: n(4),
        selections: n(3),
        applications: n(3),
        successes: n(2),
        failures: n(1),
        corrections_after_use: n(0),
        token_cost_per_turn: n(12),
        model: n("m".into()),
        client: n("c".into()),
        evidence_refs: n(vec!["receipt:r1".into()]),
        provenance: p,
    }
}

#[test]
fn missing_joinable_evaluation_keeps_verdict_unavailable() {
    let out = project_effectiveness("asset-1", &[observation()], &[]);
    assert_eq!(out.effectiveness_verdict.coverage, Coverage::Unavailable);
    assert!(out.effectiveness_verdict.value.is_none());
}

#[test]
fn complete_h4_h6_join_emits_verdict_and_preserves_evidence() {
    fn n<T>(v: T) -> Observed<T> {
        Observed::complete(v)
    }
    let e = EvaluationObservationV1 {
        outcome_id: "e1".into(),
        asset_id: n("asset-1".into()),
        evaluator: n("eval".into()),
        dataset: n("d".into()),
        experiment: n("x".into()),
        score: n(0.9),
        evidence_refs: n(vec!["eval:e1".into()]),
        provenance: observation().provenance,
    };
    let out = project_effectiveness("asset-1", &[observation()], &[e]);
    assert_eq!(
        out.effectiveness_verdict.value,
        Some(EffectivenessVerdict::Effective)
    );
    assert_eq!(out.exposures.value, Some(4));
    assert_eq!(out.evidence_refs.value.unwrap().len(), 2);
}
