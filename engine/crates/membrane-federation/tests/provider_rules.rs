use membrane_federation::providers::rules::{
    normalize_rule_path, stable_rule_candidate_id, DeliveryMode, MAX_RULE_BYTES,
};

#[test]
fn rule_paths_are_normalized_and_confined() {
    assert_eq!(
        normalize_rule_path(".claude/rules/base.md").unwrap(),
        ".claude/rules/base.md"
    );
    assert!(normalize_rule_path("/AGENTS.md").is_err());
    assert!(normalize_rule_path("../AGENTS.md").is_err());
    assert!(normalize_rule_path("nested\\AGENTS.md").is_err());
    assert!(normalize_rule_path(".env").is_err());
    assert!(normalize_rule_path("keys/id_ed25519").is_err());
    assert!(normalize_rule_path("keys/service.key").is_err());
}

#[test]
fn candidate_identity_includes_all_stable_inputs() {
    let first = stable_rule_candidate_id("repo-a", "AGENTS.md", "sha256:a", "root");
    assert_eq!(
        first,
        stable_rule_candidate_id("repo-a", "AGENTS.md", "sha256:a", "root")
    );
    assert_ne!(
        first,
        stable_rule_candidate_id("repo-b", "AGENTS.md", "sha256:a", "root")
    );
    assert_ne!(
        first,
        stable_rule_candidate_id("repo-a", "AGENTS.md", "sha256:b", "root")
    );
    assert_ne!(
        first,
        stable_rule_candidate_id("repo-a", "AGENTS.md", "sha256:a", "nested")
    );
}

#[test]
fn delivery_modes_are_closed_and_rule_cap_is_explicit() {
    assert_eq!(
        serde_json::to_string(&DeliveryMode::Native).unwrap(),
        "\"native\""
    );
    assert_eq!(
        serde_json::to_string(&DeliveryMode::Inline).unwrap(),
        "\"inline\""
    );
    assert_eq!(
        serde_json::to_string(&DeliveryMode::Reference).unwrap(),
        "\"reference\""
    );
    assert_eq!(MAX_RULE_BYTES, 1_500_000);
}
