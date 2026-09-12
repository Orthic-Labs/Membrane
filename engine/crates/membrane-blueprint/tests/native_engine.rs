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
fn incremental_same_file_call_resolves_against_one_post_change_symbol() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("main.py"), "def helper():\n    return 1\ndef caller():\n    return helper()\n").unwrap();
    let operation = NativeBlueprintOperation;
    execute(&operation, &request("same-file-build", Operation::Build, root.path())).unwrap();
    fs::write(root.path().join("main.py"), "def helper():\n    return 2\ndef caller():\n    return helper()\n").unwrap();
    let mut refresh = request("same-file-refresh", Operation::Refresh, root.path());
    refresh.input["sourceClock"] = Value::from(1u64);
    refresh.input["eventKind"] = Value::String("modify".into());
    refresh.input["paths"] = Value::Array(vec![Value::String("main.py".into())]);
    execute(&operation, &refresh).unwrap();
    let path = root.path().join(".agent/graph/graph.db");
    let connection = membrane_blueprint::store::open_store_read_only(&path).unwrap();
    let generation = membrane_blueprint::store::load_generation(&connection).unwrap().unwrap();
    assert!(generation.edges.iter().any(|edge| edge["kind"] == "CALLS" && edge["target"].as_str().is_some_and(|target| target.contains("helper"))));
}

#[test]
fn document_change_updates_file_leaf_without_rebuild() {
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
    let before = fs::read_to_string(root.path().join(".agent/graph/full-constructions.jsonl")).unwrap();
    let result = execute(&operation, &refresh).unwrap();
    assert_eq!(result["refreshMode"], "incremental");
    assert!(result["invalidatedPaths"].as_array().unwrap().iter().any(|path| path == "README.md"));
    let after = fs::read_to_string(root.path().join(".agent/graph/full-constructions.jsonl")).unwrap();
    assert_eq!(before, after);
}

#[test]
fn cross_file_change_repairs_incoming_references_incrementally() {
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
    assert_eq!(incremental_result["refreshMode"], "incremental");
    let mut full_refresh = request("full-refresh", Operation::Refresh, full_root.path());
    full_refresh.input["sourceClock"] = Value::from(1u64);
    full_refresh.input["eventKind"] = Value::String("modify".into());
    full_refresh.input["paths"] = Value::Array(vec![Value::String("caller.py".into())]);
    let full_result = execute(&operation, &full_refresh).unwrap();
    assert_eq!(full_result["refreshMode"], "incremental");

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
    assert!(!full_generation.nodes.iter().any(|node| {
        node.get("id").and_then(Value::as_str).is_some_and(|id| id.contains("ConfigKey"))
            && node.get("name").and_then(Value::as_str) == Some("NEW")
    }));
}

#[test]
fn created_cross_file_facts_are_admitted_incrementally() {
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
    assert_eq!(result["refreshMode"], "incremental");

    let path = root.path().join(".agent").join("graph").join("graph.db");
    let connection = membrane_blueprint::store::open_store_read_only(&path).unwrap();
    let generation = membrane_blueprint::store::load_generation(&connection).unwrap().unwrap();
    assert!(generation.nodes.iter().any(|node| node.get("path").and_then(Value::as_str) == Some("created.py")));
}

#[test]
fn incremental_created_import_repairs_existing_unresolved_importer() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("importer.py"), "from .created import value\ndef use_value():\n    return value\n").unwrap();
    let operation = NativeBlueprintOperation;
    execute(&operation, &request("missing-import-build", Operation::Build, root.path())).unwrap();
    fs::write(root.path().join("created.py"), "value = 1\n").unwrap();
    let mut refresh = request("missing-import-create", Operation::Refresh, root.path());
    refresh.input["sourceClock"] = Value::from(1u64);
    refresh.input["eventKind"] = Value::String("create".into());
    refresh.input["paths"] = Value::Array(vec![Value::String("created.py".into())]);
    let result = execute(&operation, &refresh).unwrap();
    assert!(result["invalidatedPaths"].as_array().unwrap().iter().any(|path| path == "importer.py"));
    let path = root.path().join(".agent/graph/graph.db");
    let connection = membrane_blueprint::store::open_store_read_only(&path).unwrap();
    let generation = membrane_blueprint::store::load_generation(&connection).unwrap().unwrap();
    assert!(generation.edges.iter().any(|edge| edge["kind"] == "IMPORTS" && edge["source"] == "file:importer.py" && edge["target"] == "file:created.py" && edge["resolved"] == true));
}

#[test]
fn valid_graph_ordinary_build_records_zero_additional_full_constructions() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("main.rs"), "fn entry() {}\n").unwrap();
    let operation = NativeBlueprintOperation;
    execute(&operation, &request("initial", Operation::Build, root.path())).unwrap();
    let receipt = root.path().join(".agent/graph/full-constructions.jsonl");
    let before = fs::read_to_string(&receipt).unwrap();
    let result = execute(&operation, &request("ordinary", Operation::Build, root.path())).unwrap();
    assert_eq!(result["refreshMode"], "incremental_noop");
    assert_eq!(before, fs::read_to_string(receipt).unwrap());
}

#[test]
fn empty_refresh_discovers_source_changes_instead_of_reporting_fresh() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("main.rs"), "fn entry() {}\n").unwrap();
    let operation = NativeBlueprintOperation;
    execute(&operation, &request("initial", Operation::Build, root.path())).unwrap();
    fs::write(root.path().join("main.rs"), "fn changed() {}\n").unwrap();
    let error = execute(&operation, &request("discover", Operation::Refresh, root.path())).unwrap_err();
    assert!(error.starts_with("blueprint_incremental_unsupported:"));
}

#[test]
fn status_reads_source_fingerprint_without_full_construction_receipt() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("main.rs"), "fn entry() {}\n").unwrap();
    let operation = NativeBlueprintOperation;
    execute(&operation, &request("initial", Operation::Build, root.path())).unwrap();
    let receipt = root.path().join(".agent/graph/full-constructions.jsonl");
    let before = fs::read_to_string(&receipt).unwrap();
    let before_generation = result_generation_identity(root.path());
    fs::write(root.path().join("main.rs"), "fn changed() {}\n").unwrap();
    let result = execute(&operation, &request("status", Operation::Status, root.path())).unwrap();
    assert_eq!(result["state"], "stale");
    assert_eq!(before_generation, result_generation_identity(root.path()));
    assert_eq!(before, fs::read_to_string(receipt).unwrap());
}

#[test]
fn readable_incompatible_generation_is_preserved_and_rejected_typed() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("main.rs"), "fn entry() {}\n").unwrap();
    let operation = NativeBlueprintOperation;
    execute(&operation, &request("initial", Operation::Build, root.path())).unwrap();
    let path = root.path().join(".agent/graph/graph.db");
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.execute("UPDATE generation SET value=json_set(value, '$.version', 'foreign-provider') WHERE key='provider'", []).unwrap();
    drop(connection);
    let error = execute(&operation, &request("incompatible", Operation::Refresh, root.path())).unwrap_err();
    assert!(error.starts_with("blueprint_generation_incompatible:"));
    assert!(path.is_file(), "readable incompatible store must remain available for migration/recovery");
}

#[test]
fn incremental_repair_updates_incoming_reference_edges_and_reuses_unchanged_files() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("helper.py"), "def helper():\n    return 1\n").unwrap();
    fs::write(root.path().join("caller.py"), "from .helper import helper\ndef caller():\n    return helper()\n").unwrap();
    fs::write(root.path().join("untouched.rs"), "fn stable() {}\n").unwrap();
    let operation = NativeBlueprintOperation;
    execute(&operation, &request("initial", Operation::Build, root.path())).unwrap();
    fs::write(root.path().join("helper.py"), "def helper():\n    return 2\n").unwrap();
    let mut refresh = request("repair", Operation::Refresh, root.path());
    refresh.input["sourceClock"] = Value::from(1u64);
    refresh.input["eventKind"] = Value::String("modify".into());
    refresh.input["paths"] = Value::Array(vec![Value::String("helper.py".into())]);
    let result = execute(&operation, &refresh).unwrap();
    let invalidated = result["invalidatedPaths"].as_array().unwrap();
    assert!(invalidated.iter().any(|path| path == "helper.py"));
    assert!(invalidated.iter().any(|path| path == "caller.py"), "incoming dependent must be repaired");
    assert!(result["reusedFiles"].as_u64().unwrap() >= 1, "unchanged file facts must be content-addressed reuse");
    let connection = membrane_blueprint::store::open_store_read_only(&root.path().join(".agent/graph/graph.db")).unwrap();
    let generation = membrane_blueprint::store::load_generation(&connection).unwrap().unwrap();
    assert!(generation.edges.iter().any(|edge| edge["kind"] == "IMPORTS" && edge["target"] == "file:helper.py"));
}

#[test]
fn incremental_repair_invalidates_outgoing_dependents_without_full_rebuild() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("helper.py"), "def helper():\n    return 1\n").unwrap();
    fs::write(root.path().join("caller.py"), "from .helper import helper\ndef caller():\n    return helper()\n").unwrap();
    let operation = NativeBlueprintOperation;
    execute(&operation, &request("initial", Operation::Build, root.path())).unwrap();
    fs::write(root.path().join("caller.py"), "from .helper import helper\ndef caller():\n    return helper() + 1\n").unwrap();
    let mut refresh = request("outgoing-repair", Operation::Refresh, root.path());
    refresh.input["sourceClock"] = Value::from(1u64);
    refresh.input["eventKind"] = Value::String("modify".into());
    refresh.input["paths"] = Value::Array(vec![Value::String("caller.py".into())]);
    let result = execute(&operation, &refresh).unwrap();
    let invalidated = result["invalidatedPaths"].as_array().unwrap();
    assert!(invalidated.iter().any(|path| path == "caller.py"));
    assert!(invalidated.iter().any(|path| path == "helper.py"), "outgoing dependency must be repaired");
}

#[test]
fn incremental_rename_admits_old_and_new_paths_without_full_rebuild() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("old.py"), "def old():\n    return 1\n").unwrap();
    let operation = NativeBlueprintOperation;
    execute(&operation, &request("initial", Operation::Build, root.path())).unwrap();
    fs::rename(root.path().join("old.py"), root.path().join("new.py")).unwrap();
    let mut refresh = request("rename", Operation::Refresh, root.path());
    refresh.input["sourceClock"] = Value::from(1u64);
    refresh.input["eventKind"] = Value::String("rename".into());
    refresh.input["paths"] = Value::Array(vec![Value::String("old.py".into()), Value::String("new.py".into())]);
    let result = execute(&operation, &refresh).unwrap();
    assert_eq!(result["refreshMode"], "incremental");
    let connection = membrane_blueprint::store::open_store_read_only(&root.path().join(".agent/graph/graph.db")).unwrap();
    let generation = membrane_blueprint::store::load_generation(&connection).unwrap().unwrap();
    assert!(generation.nodes.iter().any(|node| node["path"] == "new.py"));
    assert!(!generation.nodes.iter().any(|node| node["path"] == "old.py"));
}

#[test]
fn incremental_config_change_seals_equivalent_config_digest() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("tsconfig.json"), "{\"compilerOptions\":{\"strict\":true}}\n").unwrap();
    fs::write(root.path().join("main.ts"), "export const entry = 1;\n").unwrap();
    let operation = NativeBlueprintOperation;
    execute(&operation, &request("initial", Operation::Build, root.path())).unwrap();
    let db_path = root.path().join(".agent/graph/graph.db");
    let before_connection = membrane_blueprint::store::open_store_read_only(&db_path).unwrap();
    let before_generation = membrane_blueprint::store::load_generation(&before_connection).unwrap().unwrap();
    let before_digest = before_generation.manifest.as_ref().unwrap()["configDigest"].as_str().unwrap().to_owned();
    fs::write(root.path().join("tsconfig.json"), "{\"compilerOptions\":{\"strict\":false}}\n").unwrap();
    let mut refresh = request("config-repair", Operation::Refresh, root.path());
    refresh.input["sourceClock"] = Value::from(1u64);
    refresh.input["eventKind"] = Value::String("modify".into());
    refresh.input["paths"] = Value::Array(vec![Value::String("tsconfig.json".into())]);
    let result = execute(&operation, &refresh).unwrap();
    assert_eq!(result["refreshMode"], "incremental");
    let connection = membrane_blueprint::store::open_store_read_only(&db_path).unwrap();
    let generation = membrane_blueprint::store::load_generation(&connection).unwrap().unwrap();
    let digest = generation.manifest.unwrap()["configDigest"].as_str().unwrap().to_owned();
    assert!(digest.starts_with("sha256:"));
    assert_ne!(digest, before_digest, "config content change must reseal equivalent config identity");
}

#[test]
fn incremental_refresh_regenerates_fastapi_and_express_provider_facts() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("api.py"), "from fastapi import FastAPI\napp = FastAPI()\n@app.get('/items')\ndef items():\n    return {}\n").unwrap();
    fs::write(root.path().join("server.js"), "const express = require('express');\nconst app = express();\napp.get('/items', items);\nfunction items() {}\n").unwrap();
    let operation = NativeBlueprintOperation;
    execute(&operation, &request("provider-build", Operation::Build, root.path())).unwrap();

    fs::write(root.path().join("api.py"), "from fastapi import FastAPI\napp = FastAPI()\n@app.get('/users')\ndef items():\n    return {}\n").unwrap();
    fs::write(root.path().join("server.js"), "const express = require('express');\nconst app = express();\napp.post('/users', items);\nfunction items() {}\n").unwrap();
    let mut refresh = request("provider-refresh", Operation::Refresh, root.path());
    refresh.input["sourceClock"] = Value::from(1u64);
    refresh.input["eventKind"] = Value::String("modify".into());
    refresh.input["paths"] = Value::Array(vec![Value::String("api.py".into()), Value::String("server.js".into())]);
    execute(&operation, &refresh).unwrap();

    let connection = membrane_blueprint::store::open_store_read_only(&root.path().join(".agent/graph/graph.db")).unwrap();
    let generation = membrane_blueprint::store::load_generation(&connection).unwrap().unwrap();
    let provider_routes = generation.nodes.iter().filter(|node| {
        node.get("evidence").and_then(Value::as_array).and_then(|evidence| evidence.first())
            .and_then(|evidence| evidence.get("provider")).and_then(Value::as_str) == Some("blueprint-frameworks")
    }).collect::<Vec<_>>();
    let route_paths = provider_routes.iter().filter_map(|node| {
        node.get("evidence")
            .and_then(Value::as_array)
            .and_then(|evidence| evidence.first())
            .and_then(|evidence| evidence.get("routePath"))
            .and_then(Value::as_str)
    }).collect::<Vec<_>>();
    assert!(route_paths.contains(&"/users"));
    assert!(!route_paths.contains(&"/items"));
    assert!(provider_routes.iter().any(|node| node.get("path").and_then(Value::as_str) == Some("server.js")));
}

#[test]
fn incremental_refresh_repairs_callers_when_missing_symbol_is_added() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("helper.py"), "# helper added later\n").unwrap();
    fs::write(root.path().join("caller.py"), "def caller():\n    return helper()\n").unwrap();
    let operation = NativeBlueprintOperation;
    execute(&operation, &request("unresolved-build", Operation::Build, root.path())).unwrap();
    fs::write(root.path().join("helper.py"), "def helper():\n    return 1\n").unwrap();
    let mut refresh = request("unresolved-refresh", Operation::Refresh, root.path());
    refresh.input["sourceClock"] = Value::from(1u64);
    refresh.input["eventKind"] = Value::String("modify".into());
    refresh.input["paths"] = Value::Array(vec![Value::String("helper.py".into())]);
    let result = execute(&operation, &refresh).unwrap();
    assert!(result["invalidatedPaths"].as_array().unwrap().iter().any(|path| path == "caller.py"));

    let connection = membrane_blueprint::store::open_store_read_only(&root.path().join(".agent/graph/graph.db")).unwrap();
    let generation = membrane_blueprint::store::load_generation(&connection).unwrap().unwrap();
    assert!(generation.edges.iter().any(|edge| {
        edge.get("kind") == Some(&Value::String("CALLS".into()))
            && edge.get("source").and_then(Value::as_str).is_some_and(|source| source.contains("caller.py"))
            && edge.get("target").and_then(Value::as_str).is_some_and(|target| target.contains("helper"))
            && edge.get("resolved") == Some(&Value::Bool(true))
    }), "caller CALLS edge must resolve against newly added helper symbol");
}

#[test]
fn incremental_symbol_rename_re_resolves_or_clears_existing_callers() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("helper.py"), "def old_helper():\n    return 1\n").unwrap();
    fs::write(root.path().join("caller.py"), "def caller():\n    return old_helper()\n").unwrap();
    let operation = NativeBlueprintOperation;
    execute(&operation, &request("rename-build", Operation::Build, root.path())).unwrap();

    fs::write(root.path().join("helper.py"), "def new_helper():\n    return 2\n").unwrap();
    let mut remove_refresh = request("rename-remove-refresh", Operation::Refresh, root.path());
    remove_refresh.input["sourceClock"] = Value::from(1u64);
    remove_refresh.input["eventKind"] = Value::String("modify".into());
    remove_refresh.input["paths"] = Value::Array(vec![Value::String("helper.py".into())]);
    execute(&operation, &remove_refresh).unwrap();
    let load = || {
        let connection = membrane_blueprint::store::open_store_read_only(&root.path().join(".agent/graph/graph.db")).unwrap();
        membrane_blueprint::store::load_generation(&connection).unwrap().unwrap()
    };
    let generation = load();
    assert!(generation.edges.iter().any(|edge| {
        edge.get("kind") == Some(&Value::String("CALLS".into()))
            && edge.get("source").and_then(Value::as_str).is_some_and(|source| source.contains("caller.py"))
            && edge.get("target").is_none_or(Value::is_null)
    }), "removing target symbol must clear caller binding");

    fs::write(root.path().join("caller.py"), "def caller():\n    return new_helper()\n").unwrap();
    let mut resolve_refresh = request("rename-resolve-refresh", Operation::Refresh, root.path());
    resolve_refresh.input["sourceClock"] = Value::from(2u64);
    resolve_refresh.input["eventKind"] = Value::String("modify".into());
    resolve_refresh.input["paths"] = Value::Array(vec![Value::String("caller.py".into())]);
    execute(&operation, &resolve_refresh).unwrap();
    let generation = load();
    assert!(generation.edges.iter().any(|edge| {
        edge.get("kind") == Some(&Value::String("CALLS".into()))
            && edge.get("source").and_then(Value::as_str).is_some_and(|source| source.contains("caller.py"))
            && edge.get("target").and_then(Value::as_str).is_some_and(|target| target.contains("new_helper"))
            && edge.get("resolved") == Some(&Value::Bool(true))
    }), "renamed target symbol must bind updated caller");
}

#[test]
fn cancelled_multi_path_refresh_preserves_prior_generation_identity() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("one.rs"), "fn one() {}\n").unwrap();
    fs::write(root.path().join("two.rs"), "fn two() {}\n").unwrap();
    let operation = NativeBlueprintOperation;
    execute(&operation, &request("atomic-build", Operation::Build, root.path())).unwrap();
    let before = result_generation_identity(root.path());
    fs::write(root.path().join("one.rs"), "fn changed_one() {}\n").unwrap();
    fs::write(root.path().join("two.rs"), "fn changed_two() {}\n").unwrap();
    let mut refresh = request("atomic-cancel", Operation::Refresh, root.path());
    refresh.input["sourceClock"] = Value::from(1u64);
    refresh.input["eventKind"] = Value::String("modify".into());
    refresh.input["paths"] = Value::Array(vec![Value::String("one.rs".into()), Value::String("two.rs".into())]);
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    assert!(execute_with_token(&operation, &refresh, cancellation).unwrap_err().starts_with("request_cancelled:"));
    assert_eq!(before, result_generation_identity(root.path()));
}

fn result_generation_identity(root: &std::path::Path) -> (String, String) {
    let path = root.join(".agent/graph/graph.db");
    let connection = membrane_blueprint::store::open_store_read_only(&path).unwrap();
    let generation = membrane_blueprint::store::load_generation(&connection).unwrap().unwrap();
    let manifest = generation.manifest.unwrap();
    (
        manifest["generationId"].as_str().unwrap().to_owned(),
        manifest["sourceHash"].as_str().unwrap().to_owned(),
    )
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
