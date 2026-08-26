use membrane_federation::blueprint_client::{
    BlueprintBounds, BlueprintCacheKey, BlueprintQuery, DEFAULT_CANDIDATE_CAP, MAX_CANDIDATE_CAP,
};

#[test]
fn bounds_are_clamped_before_request_construction() {
    let bounds = BlueprintBounds {
        max_candidates: usize::MAX,
        max_paths: 0,
        max_response_bytes: 0,
    }
    .bounded();
    assert_eq!(bounds.max_candidates, MAX_CANDIDATE_CAP);
    assert_eq!(bounds.max_paths, 1);
    assert!(bounds.max_response_bytes >= 1024);
}

#[test]
fn cache_key_retains_generation_repository_worktree_policy_and_caps() {
    let query = BlueprintQuery {
        request_id: "request".into(),
        repository_id: "repo".into(),
        repository_root: "/repo".into(),
        worktree: "worktree".into(),
        task: "find graph evidence".into(),
        anchors: vec!["src/lib.rs:Thing".into()],
        policy_digest: "policy".into(),
        expected_generation: Some("sha256:111".into()),
        symbol: None,
        bounds: BlueprintBounds::default(),
        deadline: std::time::Duration::from_secs(1),
    };
    let key = query.cache_key();
    assert_eq!(key.repository_id, "repo");
    assert_eq!(key.worktree, "worktree");
    assert_eq!(key.policy_digest, "policy");
    assert_eq!(key.expected_generation.as_deref(), Some("sha256:111"));
    assert_eq!(key.max_candidates, DEFAULT_CANDIDATE_CAP);
}

#[test]
fn query_digest_changes_with_task() {
    let mut query = BlueprintQuery {
        request_id: "request".into(),
        repository_id: "repo".into(),
        repository_root: "/repo".into(),
        worktree: "worktree".into(),
        task: "one".into(),
        anchors: vec!["src/one.rs".into()],
        policy_digest: "policy".into(),
        expected_generation: None,
        symbol: None,
        bounds: BlueprintBounds::default(),
        deadline: std::time::Duration::from_secs(1),
    };
    let first: BlueprintCacheKey = query.cache_key();
    query.task = "two".into();
    assert_ne!(first.query_digest, query.cache_key().query_digest);
}

#[test]
fn cache_key_separates_anchor_and_symbol_queries() {
    let mut query = BlueprintQuery {
        request_id: "request".into(),
        repository_id: "repo".into(),
        repository_root: "/repo".into(),
        worktree: "worktree".into(),
        task: "resolve".into(),
        anchors: vec!["src/one.rs".into()],
        policy_digest: "policy".into(),
        expected_generation: Some("sha256:111".into()),
        symbol: Some("Thing".into()),
        bounds: BlueprintBounds::default(),
        deadline: std::time::Duration::from_secs(1),
    };
    let first = query.cache_key();
    query.anchors = vec!["src/two.rs".into()];
    assert_ne!(first, query.cache_key());
    query.anchors = vec!["src/one.rs".into()];
    query.symbol = Some("Other".into());
    assert_ne!(first, query.cache_key());
}
