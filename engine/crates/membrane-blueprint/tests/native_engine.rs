use membrane_blueprint::{native_blueprint_operation, BlueprintOperation, BlueprintRequest, Bounds, CancellationToken, NativeBlueprintOperation, Operation};
use serde_json::{json, Value};
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
fn every_findings_operation_is_dispatched_by_native_engine() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("main.rs"), "fn entry() {}\n").unwrap();
    let operation = NativeBlueprintOperation;
    execute(&operation, &request("findings-build", Operation::Build, root.path())).unwrap();

    for (id, method, input, expected_kind) in [
        ("findings-get", Operation::FindingsGet, json!({}), "findings.get"),
        ("findings-list", Operation::FindingsBaselineList, json!({}), "findings.baseline.list"),
        ("findings-sarif", Operation::FindingsSarif, json!({}), "findings.sarif"),
        ("findings-capture", Operation::FindingsBaselineCapture, json!({"name":"native"}), "findings.baseline.capture"),
    ] {
        let mut req = request(id, method, root.path());
        req.input.as_object_mut().unwrap().extend(input.as_object().unwrap().clone());
        let result = execute(&operation, &req).unwrap();
        assert_eq!(result["kind"], expected_kind, "{id}");
    }

    let mut explain = request("findings-explain", Operation::FindingsExplain, root.path());
    explain.input["fingerprint"] = Value::String("missing".into());
    let error = execute(&operation, &explain).unwrap_err();
    assert!(error.starts_with("finding_not_found:"));

    let mut pack = request("findings-pack", Operation::FindingsEvidencePack, root.path());
    pack.input["fingerprints"] = Value::Array(vec![]);
    let error = execute(&operation, &pack).unwrap_err();
    assert!(error.starts_with("finding_selection_empty:"));
}

#[test]
fn refresh_uses_incremental_delta_for_one_changed_code_file() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("main.rs"), "fn entry() {}\n").unwrap();
    let operation = NativeBlueprintOperation;
    execute(&operation, &request("build", Operation::Build, root.path())).unwrap();
    fs::write(root.path().join("main.rs"), "fn changed() {}\n").unwrap();
    let mut refresh = request("delta-refresh", Operation::Refresh, root.path());
    refresh.input["sourceClock"] = Value::from(1u64);
    refresh.input["eventKind"] = Value::String("modify".into());
    refresh.input["paths"] = Value::Array(vec![Value::String("main.rs".into())]);
    let result = execute(&operation, &refresh).unwrap();
    assert_eq!(result["refreshMode"], "incremental");
    let generation = result["generationId"].as_str().unwrap().to_owned();
    let mut query = request("delta-query", Operation::Search, root.path());
    query.generation = Some(generation);
    query.input["query"] = Value::String("changed".into());
    assert!(execute(&operation, &query).is_ok());
}

#[test]
fn refresh_falls_back_to_full_build_for_document_changes() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("main.rs"), "fn entry() {}\n").unwrap();
    fs::write(root.path().join("README.md"), "initial\n").unwrap();
    let operation = NativeBlueprintOperation;
    execute(&operation, &request("build", Operation::Build, root.path())).unwrap();
    fs::write(root.path().join("README.md"), "updated\n").unwrap();
    let mut refresh = request("document-refresh", Operation::Refresh, root.path());
    refresh.input["sourceClock"] = Value::from(1u64);
    refresh.input["eventKind"] = Value::String("modify".into());
    refresh.input["paths"] = Value::Array(vec![Value::String("README.md".into())]);
    let result = execute(&operation, &refresh).unwrap();
    assert_ne!(result["refreshMode"], "incremental");
}

#[test]
fn refresh_falls_back_when_cross_file_resolution_or_framework_facts_are_in_scope() {
    let incremental_root = tempdir().unwrap();
    let full_root = tempdir().unwrap();
    for root in [incremental_root.path(), full_root.path()] {
        fs::write(root.join("helper.py"), "def helper():\n    return 1\n").unwrap();
        fs::write(
            root.join("caller.py"),
            "from .helper import helper\nimport os\ndef caller():\n    return helper() + os.getenv(\"OLD\")\n",
        )
        .unwrap();
    }
    let operation = NativeBlueprintOperation;
    for (id, root) in [("incremental-build", incremental_root.path()), ("full-build", full_root.path())] {
        execute(&operation, &request(id, Operation::Build, root)).unwrap();
    }
    for root in [incremental_root.path(), full_root.path()] {
        fs::write(
            root.join("caller.py"),
            "from .helper import helper\nimport os\ndef caller():\n    return helper() + os.getenv(\"NEW\")\n",
        )
        .unwrap();
    }

    let mut incremental = request("cross-file-refresh", Operation::Refresh, incremental_root.path());
    incremental.input["sourceClock"] = Value::from(1u64);
    incremental.input["eventKind"] = Value::String("modify".into());
    incremental.input["paths"] = Value::Array(vec![Value::String("caller.py".into())]);
    let incremental_result = execute(&operation, &incremental).unwrap();

    let full_result = execute(&operation, &request("full-refresh", Operation::Refresh, full_root.path())).unwrap();
    assert_ne!(incremental_result["refreshMode"], "incremental");
    assert_ne!(full_result["refreshMode"], "incremental");

    let load = |root: &std::path::Path| {
        let path = root.join(".agent").join("graph").join("graph.db");
        let connection = membrane_blueprint::store::open_store_read_only(&path).unwrap();
        membrane_blueprint::store::load_generation(&connection).unwrap().unwrap()
    };
    let incremental_generation = load(incremental_root.path());
    let full_generation = load(full_root.path());
    let node_ids = |generation: &membrane_blueprint::store::Generation| {
        generation
            .nodes
            .iter()
            .filter_map(|node| node.get("id").and_then(Value::as_str).map(str::to_owned))
            .collect::<std::collections::BTreeSet<_>>()
    };
    let edge_ids = |generation: &membrane_blueprint::store::Generation| {
        generation
            .edges
            .iter()
            .filter_map(|edge| edge.get("id").and_then(Value::as_str).map(str::to_owned))
            .collect::<std::collections::BTreeSet<_>>()
    };
    assert_eq!(node_ids(&incremental_generation), node_ids(&full_generation));
    assert_eq!(edge_ids(&incremental_generation), edge_ids(&full_generation));
    assert!(
        full_generation.edges.iter().any(|edge| {
            edge.get("kind") == Some(&Value::String("IMPORTS".into()))
                && edge.get("target").and_then(Value::as_str) == Some("file:helper.py")
        }),
        "full rebuild must retain resolved cross-file import edge"
    );
    assert!(
        full_generation.nodes.iter().any(|node| {
            node.get("id").and_then(Value::as_str).is_some_and(|id| id.contains("ConfigKey"))
                && node.get("name").and_then(Value::as_str) == Some("NEW")
        }),
        "full rebuild must retain framework config fact for edited file"
    );
}

#[test]
fn refresh_falls_back_for_created_import_and_framework_facts() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("helper.py"), "def helper():\n    return 1\n").unwrap();
    fs::write(root.path().join("main.rs"), "fn entry() {}\n").unwrap();
    let operation = NativeBlueprintOperation;
    execute(&operation, &request("create-build", Operation::Build, root.path())).unwrap();
    fs::write(
        root.path().join("created.py"),
        "from .helper import helper\nimport os\ndef created():\n    return helper() + os.getenv(\"NEW\")\n",
    )
    .unwrap();

    let mut refresh = request("create-refresh", Operation::Refresh, root.path());
    refresh.input["sourceClock"] = Value::from(1u64);
    refresh.input["eventKind"] = Value::String("create".into());
    refresh.input["paths"] = Value::Array(vec![Value::String("created.py".into())]);
    let result = execute(&operation, &refresh).unwrap();
    assert_ne!(result["refreshMode"], "incremental");

    let path = root.path().join(".agent").join("graph").join("graph.db");
    let connection = membrane_blueprint::store::open_store_read_only(&path).unwrap();
    let generation = membrane_blueprint::store::load_generation(&connection).unwrap().unwrap();
    assert!(generation.edges.iter().any(|edge| {
        edge.get("kind") == Some(&Value::String("IMPORTS".into()))
            && edge.get("target").and_then(Value::as_str) == Some("file:helper.py")
    }));
    assert!(generation.nodes.iter().any(|node| {
        node.get("id").and_then(Value::as_str).is_some_and(|id| id.contains("ConfigKey"))
            && node.get("name").and_then(Value::as_str) == Some("NEW")
    }));
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
