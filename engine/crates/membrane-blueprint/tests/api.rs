use membrane_blueprint::api::{BlueprintApi, BlueprintError, BlueprintOperation, BlueprintRequest, BlueprintResponse, Bounds, CancellationToken};
use membrane_blueprint::model::{Operation, PROTOCOL_VERSION, MAX_BUILD_DEADLINE_MS, MAX_PATH_LENGTH};
use serde_json::json;

struct Echo;
impl BlueprintOperation for Echo {
    fn execute(&self, request: &BlueprintRequest, context: &membrane_blueprint::api::RequestContext) -> Result<serde_json::Value, BlueprintError> {
        context.check()?;
        Ok(json!({"method": request.method.as_str(), "repoRoot": context.scope.repo_root, "generation": request.generation}))
    }
}

#[test]
fn request_serialization_is_protocol_v1_and_deterministic() {
    let request = BlueprintRequest::new("request-1", Operation::DocumentTruth, "D:/repo");
    let value = serde_json::to_value(&request).unwrap();
    assert_eq!(value["protocolVersion"], PROTOCOL_VERSION);
    assert_eq!(value["method"], "documentTruth");
    assert_eq!(value["input"]["repoRoot"], "D:/repo");
    assert_eq!(serde_json::to_vec(&request).unwrap(), serde_json::to_vec(&request).unwrap());
}

#[test]
fn operation_parser_preserves_manual_surface() {
    for (wire, expected) in [("build", Operation::Build), ("status", Operation::Status), ("documentTruth", Operation::DocumentTruth), ("phase2_plan", Operation::Phase2Plan), ("db_status", Operation::DbStatus), ("freshness_barrier", Operation::FreshnessBarrier), ("findings.get", Operation::FindingsGet)] {
        assert_eq!(Operation::parse(wire), Some(expected));
        assert_eq!(expected.as_str(), wire);
    }
    assert_eq!(Operation::parse("pretend_success"), None);
}

#[test]
fn validation_rejects_bad_protocol_deadline_root_and_bounds() {
    let mut request = BlueprintRequest::new("request-1", Operation::Status, "D:/repo");
    request.protocol_version = 9;
    assert_eq!(request.validate(Bounds::default()).unwrap_err().code, "protocol_version_mismatch");
    request.protocol_version = PROTOCOL_VERSION;
    request.deadline_ms = 9;
    assert_eq!(request.validate(Bounds::default()).unwrap_err().code, "deadline_invalid");
    request.deadline_ms = 100;
    request.input = json!({});
    assert_eq!(request.validate(Bounds::default()).unwrap_err().code, "required_field_missing");
    assert!(Bounds { max_response_bytes: usize::MAX, ..Bounds::default() }.validate().is_err());
    assert_eq!(Bounds::one_shot().max_frame_bytes, 65_536);
    assert_eq!(Bounds::daemon().max_frame_bytes, 16_384);
}

#[test]
fn only_build_accepts_extended_deadline() {
    for method in [Operation::Refresh, Operation::DbMigrate, Operation::Phase2Seal] {
        let mut request = BlueprintRequest::new("request-1", method, "D:/repo");
        request.deadline_ms = MAX_BUILD_DEADLINE_MS;
        assert_eq!(request.validate(Bounds::default()).unwrap_err().code, "deadline_invalid");
    }
    let mut build = BlueprintRequest::new("request-1", Operation::Build, "D:/repo");
    build.deadline_ms = MAX_BUILD_DEADLINE_MS;
    assert!(build.validate(Bounds::default()).is_ok());
}

#[test]
fn request_paths_fail_closed_at_count_length_and_type_bounds() {
    let mut request = BlueprintRequest::new("request-1", Operation::Status, "D:/repo");
    request.input = json!({ "repoRoot": "D:/repo", "paths": ["a", "b"] });
    assert_eq!(request.validate(Bounds { max_paths: 1, ..Bounds::default() }).unwrap_err().code, "blueprint_oversized");

    request.input = json!({ "repoRoot": "D:/repo", "paths": ["x".repeat(MAX_PATH_LENGTH + 1)] });
    assert_eq!(request.validate(Bounds::default()).unwrap_err().code, "blueprint_oversized");

    request.input = json!({ "repoRoot": "D:/repo", "paths": [17] });
    assert_eq!(request.validate(Bounds::default()).unwrap_err().code, "invalid_request");
}

#[test]
fn response_candidates_fail_closed_at_count_length_and_path_count_bounds() {
    let over_count = BlueprintResponse::success(
        "request-1",
        Some("generation-1".into()),
        json!({ "candidates": [{ "sourceRef": "a" }, { "sourceRef": "b" }] }),
    );
    assert_eq!(over_count.validate(Bounds { max_candidates: 1, ..Bounds::default() }).unwrap_err().code, "blueprint_oversized");

    let over_length = BlueprintResponse::success(
        "request-1",
        Some("generation-1".into()),
        json!({ "candidates": [{ "sourceRef": "x".repeat(MAX_PATH_LENGTH + 1) }] }),
    );
    assert_eq!(over_length.validate(Bounds::default()).unwrap_err().code, "blueprint_oversized");

    let over_paths = BlueprintResponse::success(
        "request-1",
        Some("generation-1".into()),
        json!({ "candidates": [{ "sourceRef": "a" }, { "sourceRef": "b" }] }),
    );
    assert_eq!(over_paths.validate(Bounds { max_paths: 1, ..Bounds::default() }).unwrap_err().code, "blueprint_oversized");
}

#[test]
fn response_paths_fail_closed_at_count_type_shape_and_length_bounds() {
    let over_count = BlueprintResponse::success(
        "request-1",
        Some("generation-1".into()),
        json!({ "paths": [{ "nodes": [], "edges": [] }, { "nodes": [], "edges": [] }] }),
    );
    assert_eq!(over_count.validate(Bounds { max_paths: 1, ..Bounds::default() }).unwrap_err().code, "blueprint_oversized");

    let wrong_type = BlueprintResponse::success(
        "request-1",
        Some("generation-1".into()),
        json!({ "paths": ["src/main.rs"] }),
    );
    assert_eq!(wrong_type.validate(Bounds::default()).unwrap_err().code, "blueprint_malformed");

    let wrong_shape = BlueprintResponse::success(
        "request-1",
        Some("generation-1".into()),
        json!({ "paths": [{ "nodes": "not-an-array", "edges": [] }] }),
    );
    assert_eq!(wrong_shape.validate(Bounds::default()).unwrap_err().code, "blueprint_malformed");

    let over_length = BlueprintResponse::success(
        "request-1",
        Some("generation-1".into()),
        json!({ "paths": [{ "nodes": [{ "path": "x".repeat(MAX_PATH_LENGTH + 1) }], "edges": [] }] }),
    );
    assert_eq!(over_length.validate(Bounds::default()).unwrap_err().code, "blueprint_oversized");
}

#[test]
fn typed_cancelled_response_never_reports_success() {
    let token = CancellationToken::new();
    token.cancel();
    let response = Echo.dispatch(BlueprintRequest::new("request-1", Operation::Status, "D:/repo"), token);
    assert!(!response.ok);
    assert_eq!(response.error.unwrap().code, "request_cancelled");
    assert!(response.result.is_none());
}

#[test]
fn successful_response_has_generation_and_bounded_shape() {
    let mut request = BlueprintRequest::new("request-1", Operation::Status, "D:/repo");
    request.generation = Some("generation-1".into());
    let response = Echo.dispatch(request, CancellationToken::new());
    response.validate(Bounds::default()).unwrap();
    assert!(response.ok);
    assert_eq!(response.generation.as_deref(), Some("generation-1"));
}

#[test]
fn response_validation_rejects_fake_success() {
    let response = BlueprintResponse { protocol_version: PROTOCOL_VERSION, request_id: Some("request-1".into()), ok: true, generation: None, result: None, error: None };
    assert_eq!(response.validate(Bounds::default()).unwrap_err().code, "blueprint_malformed");
}
