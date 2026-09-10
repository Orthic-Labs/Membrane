use membrane_federation::blueprint_client::{
    BlueprintBounds, BlueprintCacheKey, BlueprintClient, BlueprintQuery, ContextualBlueprintSource, DEFAULT_CANDIDATE_CAP,
    MAX_CANDIDATE_CAP,
};
use membrane_blueprint::{BlueprintApi, BlueprintRequest, BlueprintResponse, CancellationToken};
use membrane_provider_sdk::SourceQuery;
use std::sync::{Arc, Mutex};

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

struct NativeApi {
    request: Mutex<Option<BlueprintRequest>>,
}

impl BlueprintApi for NativeApi {
    fn dispatch(&self, request: BlueprintRequest, _cancellation: CancellationToken) -> BlueprintResponse {
        *self.request.lock().unwrap() = Some(request.clone());
        BlueprintResponse::success(
            request.request_id,
            request.generation,
            serde_json::json!({
                "generationId": "generation-1",
                "state": "ambiguous",
                "omissions": [{ "reason": "same_tier_ambiguity" }],
                "candidates": [{
                    "id": "candidate-1",
                    "layer": 1,
                    "sourceKind": "graph",
                    "sourceRef": "src/lib.rs",
                    "sourceHash": "sha256:source",
                    "trustClass": "local",
                    "instructionPolicy": "data_only",
                    "providerScore": 1.0,
                    "estimatedTokens": 3,
                    "protected": false,
                    "exact": false,
                    "recoverable": true,
                    "resolver": "blueprint",
                    "text": "native evidence",
                    "recallCircuitId": "circuit-1",
                    "evidencePathId": "path-1"
                }]
            }),
        )
    }
}

#[tokio::test]
async fn client_preserves_native_partial_disposition_and_resolve_target() {
    let api = Arc::new(NativeApi { request: Mutex::new(None) });
    let client = BlueprintClient::new(api.clone());
    let query = BlueprintQuery {
        request_id: "request-1".into(),
        repository_id: "repo-1".into(),
        repository_root: "/repo".into(),
        worktree: "/repo".into(),
        task: "find native evidence".into(),
        anchors: vec!["src/lib.rs".into()],
        policy_digest: "policy".into(),
        expected_generation: None,
        symbol: None,
        bounds: BlueprintBounds::default(),
        deadline: std::time::Duration::from_secs(1),
    };
    let source = SourceQuery {
        request_id: query.request_id.clone(), repository_id: query.repository_id.clone(),
        repository_root: query.repository_root.clone(), task: query.task.clone(), session_id: "session".into(),
        generation: None, anchors: query.anchors.clone(),
    };
    let response = client.query_with_context(
        &source, std::time::Instant::now() + std::time::Duration::from_secs(1),
        tokio_util::sync::CancellationToken::new(),
    ).await.expect("native response parses");
    let request = api.request.lock().unwrap().clone().expect("native request");
    assert_eq!(request.method.as_str(), "recall");
    assert_eq!(request.repo_id.as_deref(), Some("repo-1"));
    assert_eq!(request.input["repoRoot"], "/repo");
    assert_eq!(response.value.candidates.len(), 1);
    assert!(!response.complete, "native ambiguity must not become complete");
    assert_eq!(response.warnings[0].detail_id.as_deref(), Some("same_tier_ambiguity"));
    assert_eq!(response.value.candidates[0].provider, None);
    assert_eq!(response.value.payload.unwrap()["candidates"][0]["evidencePathId"], "path-1");

    let _ = client.resolve_symbol_with_cancellation(
        &query, "Thing", tokio_util::sync::CancellationToken::new(),
    ).expect("resolve parses");
    let request = api.request.lock().unwrap().clone().expect("resolve request");
    assert_eq!(request.method.as_str(), "resolve");
    assert_eq!(request.input["target"], "Thing");
}
