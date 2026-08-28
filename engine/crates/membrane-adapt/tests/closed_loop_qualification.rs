use membrane_adapt::procedural_effectiveness::*;

#[test]
fn closed_loop_never_fabricates_missing_host_fields() {
    let out = project_effectiveness("missing", &[], &[]);
    assert_eq!(out.effectiveness_verdict.coverage, Coverage::Unavailable);
    assert_eq!(out.exposures.coverage, Coverage::Unavailable);
    assert!(out.model.value.is_none());
}
