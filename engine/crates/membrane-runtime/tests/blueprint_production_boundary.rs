//! Proves: "Blueprint generation/schema mismatch fails closed in both
//! Hub-hosted and bounded one-shot modes."
//!
//! Both tests drive the *same* real Blueprint production path
//! (`NativeBlueprintOperation` via `engine::build_and_publish` /
//! `engine::ensure_generation`) against a deliberately stale generation
//! identifier, then assert the call returns the typed
//! `generation_mismatch` refusal rather than a partial or silent result.
//!
//! - Bounded one-shot mode reuses the exact construction
//!   `membrane_runtime::blueprint_one_shot::dispatch_native` wraps:
//!   `OneShotExecutor::new(native_blueprint_operation())` (Bounds::one_shot()).
//! - Hub-hosted mode uses Blueprint's real resident lifecycle,
//!   `NativeService::resident(..)` + `start()`, which only serves requests
//!   while `ServiceStatus::Running` -- the same object Hub/CodeRight host in
//!   production (see `engine/crates/membrane-blueprint/src/service.rs`).

use membrane_blueprint::{
    native_blueprint_operation, BlueprintRequest, CancellationToken, NativeBlueprintOperation,
    NativeService, OneShotExecutor, Operation,
};
use membrane_federation::blueprint_client::{BlueprintBounds, BlueprintClient, BlueprintQuery};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;
use tempfile::TempDir;

fn expected_evidence() -> Value {
    let raw = include_str!("fixtures/blueprint-production-boundary/expected-evidence.json");
    serde_json::from_str(raw).expect("expected-evidence.json parses")
}

/// Copy the tiny fixture crate into a fresh temp directory so Blueprint's
/// real graph build writes its `.agent/graph/graph.db` store under a
/// throwaway root instead of inside the checked-out repository tree.
fn fixture_root() -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let src_dir = dir.path().join("src");
    fs::create_dir_all(&src_dir).expect("create src dir");
    fs::write(
        src_dir.join("lib.rs"),
        include_str!("fixtures/blueprint-production-boundary/src/lib.rs"),
    )
    .expect("write fixture lib.rs");
    let canonical = fs::canonicalize(dir.path()).expect("canonicalize fixture root");
    (dir, canonical)
}

/// The setup build, not the behaviour under test. `BlueprintRequest::new` stamps
/// `DEFAULT_DEADLINE_MS` (2s), which a real graph build of even this tiny fixture can exceed on
/// a cold, shared CI runner -- the build then returns `deadline_exceeded` and the generation
/// assertions never run. Blueprint's own build ceiling is `MAX_BUILD_DEADLINE_MS` (120s), so
/// asking for 60s here stays well inside the product's contract while removing the timing
/// flake. The recall requests below deliberately keep the default deadline: their outcome is
/// the fails-closed behaviour this test exists to prove.
fn build_request(root: &str, id: &str) -> BlueprintRequest {
    let mut request = BlueprintRequest::new(id.to_owned(), Operation::Build, root.to_owned());
    request.input["repoRoot"] = Value::String(root.to_owned());
    request.deadline_ms = 60_000;
    request
}

fn recall_request(root: &str, id: &str, generation: Option<String>) -> BlueprintRequest {
    let mut request = BlueprintRequest::new(id.to_owned(), Operation::Recall, root.to_owned());
    request.input["repoRoot"] = Value::String(root.to_owned());
    request.input["task"] = Value::String("boundary probe".to_owned());
    request.generation = generation;
    request
}

fn assert_generation_mismatch(response: &membrane_blueprint::BlueprintResponse, expected: &Value) {
    let want = &expected["generationMismatch"];
    assert_eq!(response.ok, want["ok"].as_bool().unwrap(), "response.ok must match fails-closed contract");
    assert_eq!(response.result.is_none(), want["resultIsNull"].as_bool().unwrap(), "no result may be served on a generation mismatch");
    let error = response.error.as_ref().expect("generation mismatch must carry a typed error");
    assert_eq!(error.code, want["errorCode"].as_str().unwrap());
    assert_eq!(error.retryable, want["errorRetryable"].as_bool().unwrap());
    assert_eq!(error.message, want["errorMessage"].as_str().unwrap());
    let details = error.details.as_ref().expect("generation_mismatch must carry details");
    for key in want["errorDetailsKeys"].as_array().unwrap() {
        let key = key.as_str().unwrap();
        assert!(details.get(key).and_then(Value::as_str).is_some(), "details.{key} must be present");
    }
    assert_eq!(details.get("expected").and_then(Value::as_str), Some("generation-mismatch-probe"));
}

fn assert_matched_generation(response: &membrane_blueprint::BlueprintResponse, expected: &Value) {
    let want = &expected["matchedGeneration"];
    assert_eq!(response.ok, want["ok"].as_bool().unwrap());
    assert_eq!(response.error.is_none(), want["errorIsNull"].as_bool().unwrap());
}

/// Bounded one-shot mode: no resident holder anywhere. This is the exact
/// executor construction the native CLI/MCP path
/// (`membrane_runtime::blueprint_one_shot::dispatch_native`) uses for every
/// explicit, holder-independent Blueprint request.
#[test]
fn bounded_one_shot_generation_mismatch_fails_closed() {
    let evidence = expected_evidence();
    let (_guard, root) = fixture_root();
    let root_str = root.to_string_lossy().into_owned();

    let executor = OneShotExecutor::new(native_blueprint_operation());
    let built = executor.execute(
        build_request(&root_str, "one-shot-build"),
        CancellationToken::new(),
    );
    assert!(built.ok, "initial build must succeed: {:?}", built.error);
    let real_generation = built
        .result
        .as_ref()
        .and_then(|value| value.get("generationId"))
        .and_then(Value::as_str)
        .expect("build result carries generationId")
        .to_owned();

    // Deliberately stale generation identifier: never matches what is on disk.
    let stale = executor.execute(
        recall_request(&root_str, "one-shot-recall-stale", Some("generation-mismatch-probe".to_owned())),
        CancellationToken::new(),
    );
    assert_generation_mismatch(&stale, &evidence);

    // Contrast: the correct, freshly-observed generation id still serves.
    let matched = executor.execute(
        recall_request(&root_str, "one-shot-recall-matched", Some(real_generation)),
        CancellationToken::new(),
    );
    assert_matched_generation(&matched, &evidence);
}

/// Hub-hosted mode: a resident holder is present (Blueprint's real
/// `NativeService` lifecycle, started and `Running`, mirroring how Hub or
/// the CodeRight daemon host Blueprint in production). The same generation
/// mismatch condition must still fail closed rather than being served by
/// the resident's cache/state.
#[test]
fn hub_hosted_generation_mismatch_fails_closed() {
    let evidence = expected_evidence();
    let (_guard, root) = fixture_root();
    let root_str = root.to_string_lossy().into_owned();

    let service = NativeService::resident(NativeBlueprintOperation, root.clone());
    service.start().expect("resident Blueprint service starts");
    assert_eq!(service.status(), membrane_blueprint::ServiceStatus::Running);

    let built = service.dispatch(build_request(&root_str, "hub-build"), CancellationToken::new());
    assert!(built.ok, "initial resident build must succeed: {:?}", built.error);
    let real_generation = built
        .result
        .as_ref()
        .and_then(|value| value.get("generationId"))
        .and_then(Value::as_str)
        .expect("build result carries generationId")
        .to_owned();

    let stale = service.dispatch(
        recall_request(&root_str, "hub-recall-stale", Some("generation-mismatch-probe".to_owned())),
        CancellationToken::new(),
    );
    assert_generation_mismatch(&stale, &evidence);

    let matched = service.dispatch(
        recall_request(&root_str, "hub-recall-matched", Some(real_generation)),
        CancellationToken::new(),
    );
    assert_matched_generation(&matched, &evidence);

    service.stop().expect("resident Blueprint service stops");
}

#[test]
fn native_fixture_recall_crosses_real_blueprint_client_boundary() {
    let (_guard, root) = fixture_root();
    let root_str = root.to_string_lossy().into_owned();
    let operation = native_blueprint_operation();
    let executor = OneShotExecutor::new(operation.clone());
    let built = executor.execute(build_request(&root_str, "bm01-build"), CancellationToken::new());
    assert!(built.ok, "native fixture build must succeed: {:?}", built.error);
    let generation = built.result.as_ref().and_then(|value| value.get("generationId")).and_then(Value::as_str).expect("generation").to_owned();
    let query = BlueprintQuery { request_id: "bm01-real-client".into(), repository_id: "bm01-fixture".into(), repository_root: root_str.clone(), worktree: root_str, task: "describe_probe".into(), anchors: Vec::new(), policy_digest: String::new(), expected_generation: Some(generation), symbol: None, bounds: BlueprintBounds::default(), deadline: Duration::from_secs(30) };
    let response = BlueprintClient::from_operation(operation).query(&query).expect("native producer must satisfy real BlueprintClient contract");
    assert!(!response.candidates.is_empty());
    assert!(response.candidates.iter().all(|candidate| !candidate.id.is_empty() && !candidate.source_ref.is_empty()));
}
