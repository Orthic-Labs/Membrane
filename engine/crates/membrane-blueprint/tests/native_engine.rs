use membrane_blueprint::{native_blueprint_operation, BlueprintOperation, BlueprintRequest, Bounds, CancellationToken, NativeBlueprintOperation, Operation};
use serde_json::Value;
use std::fs;
use tempfile::tempdir;

fn request(id: &str, method: Operation, root: &std::path::Path) -> BlueprintRequest {
    let mut request = BlueprintRequest::new(id, method, root.to_string_lossy());
    request.deadline_ms = if method.is_build() { 120_000 } else { 30_000 };
    request
}

fn execute(operation: &dyn BlueprintOperation, request: &BlueprintRequest) -> Result<Value, String> {
    let mut context = request.validate(Bounds::one_shot()).map_err(|error| error.to_string())?;
    context.cancellation = CancellationToken::new();
    operation.execute(request, &context).map_err(|error| error.to_string())
}

fn execute_with_token(operation: &dyn BlueprintOperation, request: &BlueprintRequest, cancellation: CancellationToken) -> Result<Value, String> {
    let mut context = request.validate(Bounds::one_shot()).map_err(|error| error.to_string())?;
    context.cancellation = cancellation;
    operation.execute(request, &context).map_err(|error| error.to_string())
}

#[test]
fn factory_returns_native_trait_object() {
    let operation = native_blueprint_operation();
    assert!(std::sync::Arc::strong_count(&operation) >= 1);
}

#[test]
fn hub_off_build_then_query_uses_persisted_generation() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("main.rs"), "fn entry() {}\n").unwrap();
    let operation = NativeBlueprintOperation;
    let built = execute(&operation, &request("build", Operation::Build, root.path())).unwrap();
    let generation = built["generationId"].as_str().unwrap().to_owned();
    let mut query = request("search", Operation::Search, root.path());
    query.generation = Some(generation.clone());
    query.input["query"] = Value::String("entry".into());
    let result = execute(&operation, &query).unwrap();
    assert_eq!(result["generationId"], generation);
}

#[test]
fn missing_store_status_is_typed_state() {
    let root = tempdir().unwrap();
    let operation = NativeBlueprintOperation;
    let result = execute(&operation, &request("status", Operation::Status, root.path())).unwrap();
    assert_eq!(result["state"], "missing");
    assert_eq!(result["fresh"], false);
}

#[test]
fn generation_mismatch_fails_closed() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("main.rs"), "fn entry() {}\n").unwrap();
    let operation = NativeBlueprintOperation;
    execute(&operation, &request("build", Operation::Build, root.path())).unwrap();
    let mut query = request("query", Operation::Search, root.path());
    query.generation = Some("not-served".into());
    let error = execute(&operation, &query).unwrap_err();
    assert!(error.starts_with("generation_mismatch:"));
}

#[test]
fn unsupported_operation_is_typed() {
    let root = tempdir().unwrap();
    let operation = NativeBlueprintOperation;
    let error = execute(&operation, &request("export", Operation::Export, root.path())).unwrap_err();
    assert!(error.starts_with("unsupported_operation:"));
}

#[test]
fn refresh_observation_survives_persistence_and_query() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("main.rs"), "fn entry() {}\n").unwrap();
    let operation = NativeBlueprintOperation;
    let mut refresh = request("refresh", Operation::Refresh, root.path());
    refresh.input["sourceClock"] = Value::from(42u64);
    refresh.input["eventKind"] = Value::String("changed".into());
    refresh.input["paths"] = Value::Array(vec![Value::String("main.rs".into())]);
    execute(&operation, &refresh).unwrap();
    let query = request("query", Operation::Search, root.path());
    let result = execute(&operation, &query).unwrap();
    assert_eq!(result["sourceObservation"]["sourceClock"], 42);
    assert_eq!(result["sourceObservation"]["eventKind"], "changed");
}

#[test]
fn cancelled_build_does_not_publish_a_database() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("main.rs"), "fn entry() {}\n").unwrap();
    let operation = NativeBlueprintOperation;
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let error = execute_with_token(&operation, &request("build", Operation::Build, root.path()), cancellation).unwrap_err();
    assert!(error.starts_with("request_cancelled:"));
    assert!(!root.path().join(".agent").join("graph").join("graph.db").exists());
}

#[test]
fn expired_build_does_not_publish_a_database() {
    let root = tempdir().unwrap();
    for index in 0..128 {
        fs::write(root.path().join(format!("file-{index}.rs")), "fn entry() {}\n").unwrap();
    }
    let operation = NativeBlueprintOperation;
    let mut build = request("deadline", Operation::Build, root.path());
    build.deadline_ms = 10;
    let error = execute(&operation, &build).unwrap_err();
    assert!(error.starts_with("deadline_exceeded:"));
    assert!(!root.path().join(".agent").join("graph").join("graph.db").exists());
}
