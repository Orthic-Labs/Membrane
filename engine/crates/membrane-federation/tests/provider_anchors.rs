use membrane_federation::providers::anchors::{classify_path, raw_candidate};

#[test]
fn raw_fallback_is_protected_user_intent_and_not_exact_evidence() {
    let candidate = raw_candidate("missing.rs");
    assert_eq!(candidate.id, "anchor:raw:missing.rs");
    assert_eq!(candidate.trust_class, "user_direct");
    assert_eq!(candidate.instruction_policy, "data_only");
    assert!(candidate.protected);
    assert!(candidate.exact);
    assert!(candidate.recoverable);
    assert_eq!(candidate.source_hash, format!("sha256:{}", "0".repeat(64)));
}

#[test]
fn traversal_and_windows_paths_are_rejected_before_resolution() {
    assert!(matches!(
        classify_path("/tmp", "../../outside.txt"),
        membrane_federation::providers::anchors::AnchorPath::Rejected
    ));
    assert!(matches!(
        classify_path("/tmp", r"C:\\outside.txt"),
        membrane_federation::providers::anchors::AnchorPath::Rejected
    ));
}
