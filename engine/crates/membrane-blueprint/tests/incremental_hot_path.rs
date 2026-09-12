use membrane_blueprint::watch::{EventKind, NativeWatcher, SnapshotConfig};
use membrane_blueprint::{
    BlueprintOperation, BlueprintRequest, Bounds, CancellationToken, NativeBlueprintOperation,
    Operation,
};
use serde_json::Value;
use std::fs;
use std::path::Path;
use tempfile::{tempdir, TempDir};

const TARGET_INITIAL: &str = "export function target() { return 1; }\n";
const TARGET_UPDATED: &str =
    "export function target() { return 2; }\nexport const updated = target();\n";
const CALLER: &str =
    "import { target } from './target';\nexport function caller() { return target(); }\n";

fn fixture() -> TempDir {
    let root = tempdir().unwrap();
    fs::write(root.path().join("target.ts"), TARGET_INITIAL).unwrap();
    fs::write(root.path().join("caller.ts"), CALLER).unwrap();
    root
}

fn request(id: &str, method: Operation, root: &Path) -> BlueprintRequest {
    let mut request = BlueprintRequest::new(id, method, root.to_string_lossy());
    request.deadline_ms = if method.is_build() { 120_000 } else { 30_000 };
    request
}

fn execute(
    operation: &dyn BlueprintOperation,
    request: &BlueprintRequest,
) -> Result<Value, String> {
    let mut context = request
        .validate(Bounds::one_shot())
        .map_err(|error| error.to_string())?;
    context.cancellation = CancellationToken::new();
    operation
        .execute(request, &context)
        .map_err(|error| error.to_string())
}

fn load_generation(root: &Path) -> membrane_blueprint::store::Generation {
    let db = root.join(".agent/graph/graph.db");
    let connection = membrane_blueprint::store::open_store_read_only(&db).unwrap();
    membrane_blueprint::store::load_generation(&connection)
        .unwrap()
        .unwrap()
}

fn path_rows(generation: &membrane_blueprint::store::Generation, path: &str) -> Value {
    let nodes = generation
        .nodes
        .iter()
        .filter(|node| node["path"] == path)
        .cloned()
        .collect::<Vec<_>>();
    let edges = generation
        .edges
        .iter()
        .filter(|edge| {
            edge["evidence"]
                .as_array()
                .is_some_and(|evidence| evidence.iter().any(|item| item["path"] == path))
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut rows = serde_json::json!({"nodes": nodes, "edges": edges});
    strip_generation_ids(&mut rows);
    rows
}

fn strip_generation_ids(value: &mut Value) {
    match value {
        Value::Object(object) => {
            object.remove("generationId");
            for child in object.values_mut() {
                strip_generation_ids(child);
            }
        }
        Value::Array(values) => {
            for child in values {
                strip_generation_ids(child);
            }
        }
        _ => {}
    }
}

fn set_refresh(
    request_id: &str,
    root: &Path,
    source_clock: u64,
    event_kind: &str,
    paths: &[&str],
) -> BlueprintRequest {
    let mut request = request(request_id, Operation::Refresh, root);
    request.input["sourceClock"] = Value::from(source_clock);
    request.input["eventKind"] = Value::String(event_kind.into());
    request.input["paths"] = Value::Array(
        paths
            .iter()
            .map(|path| Value::String((*path).into()))
            .collect(),
    );
    request
}

fn assert_updated_generation(
    operation: &NativeBlueprintOperation,
    root: &Path,
    result: &Value,
    before_caller_rows: &Value,
) {
    assert_eq!(result["refreshMode"], "incremental");
    assert_eq!(
        result["invalidatedPaths"],
        serde_json::json!(["caller.ts", "target.ts"])
    );
    assert_eq!(
        result["reusedFiles"], 0,
        "dependency repair reparses caller as affected closure; only target source changed on disk"
    );

    let generation = load_generation(root);
    assert_eq!(
        path_rows(&generation, "caller.ts"),
        *before_caller_rows,
        "unrelated caller rows must retain their semantic identity"
    );
    assert!(
        generation
            .nodes
            .iter()
            .any(|node| node["id"] == "symbol:target.ts::updated"),
        "updated symbol must be admitted"
    );
    assert!(
        generation.edges.iter().any(|edge| edge["kind"] == "IMPORTS"
            && edge["source"] == "file:caller.ts"
            && edge["target"] == "file:target.ts"
            && edge["resolved"] == true),
        "caller import edge must remain queryable"
    );

    let generation_id = result["generationId"].as_str().unwrap();
    let mut search = request("search-updated", Operation::Search, root);
    search.generation = Some(generation_id.into());
    search.input["query"] = Value::String("updated".into());
    let search_result = execute(operation, &search).unwrap();
    assert!(search_result["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .any(|candidate| candidate["id"] == "symbol:target.ts::updated"));

    let mut path = request("path-caller-target", Operation::Path, root);
    path.generation = Some(generation_id.into());
    path.input["from"] = Value::String("file:caller.ts".into());
    path.input["to"] = Value::String("file:target.ts".into());
    let path_result = execute(operation, &path).unwrap();
    assert_eq!(path_result["found"], true);
    assert!(path_result["edges"]
        .as_array()
        .unwrap()
        .iter()
        .any(|edge| edge["kind"] == "IMPORTS"));
}

#[test]
fn manual_one_file_refresh_updates_typescript_graph_without_full_construction() {
    let root = fixture();
    let operation = NativeBlueprintOperation;
    execute(
        &operation,
        &request("initial-build", Operation::Build, root.path()),
    )
    .unwrap();
    let before = load_generation(root.path());
    let before_caller_rows = path_rows(&before, "caller.ts");
    let construction_receipt = root.path().join(".agent/graph/full-constructions.jsonl");
    let before_constructions = fs::read_to_string(&construction_receipt).unwrap();

    fs::write(root.path().join("target.ts"), TARGET_UPDATED).unwrap();
    let refresh = set_refresh("manual-refresh", root.path(), 1, "modify", &["target.ts"]);
    let result = execute(&operation, &refresh).unwrap();
    assert_updated_generation(&operation, root.path(), &result, &before_caller_rows);
    assert_eq!(
        before_constructions,
        fs::read_to_string(construction_receipt).unwrap(),
        "healthy graph refresh must record zero full constructions"
    );
}

#[test]
fn watcher_one_file_refresh_updates_typescript_graph_without_full_construction() {
    let root = fixture();
    let operation = NativeBlueprintOperation;
    execute(
        &operation,
        &request("initial-build", Operation::Build, root.path()),
    )
    .unwrap();
    let before = load_generation(root.path());
    let before_caller_rows = path_rows(&before, "caller.ts");
    let construction_receipt = root.path().join(".agent/graph/full-constructions.jsonl");
    let before_constructions = fs::read_to_string(&construction_receipt).unwrap();
    let mut watcher = NativeWatcher::start(SnapshotConfig::new(root.path())).unwrap();

    fs::write(root.path().join("target.ts"), TARGET_UPDATED).unwrap();
    let mut refresh_result = None;
    let events = watcher
        .poll(|event| {
            assert_eq!(event.kind, EventKind::Modify);
            assert_eq!(event.path, "target.ts");
            let refresh = set_refresh(
                "watcher-refresh",
                root.path(),
                event.source_clock,
                "modify",
                &[event.path.as_str()],
            );
            refresh_result = Some(execute(&operation, &refresh).unwrap());
            Ok(())
        })
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_updated_generation(
        &operation,
        root.path(),
        refresh_result.as_ref().unwrap(),
        &before_caller_rows,
    );
    assert_eq!(
        before_constructions,
        fs::read_to_string(construction_receipt).unwrap(),
        "healthy watcher refresh must record zero full constructions"
    );
    assert_eq!(watcher.source_clock(), watcher.applied_clock());
}

#[test]
fn same_byte_refresh_is_zero_parse_zero_publication_and_generation_stable() {
    let root = fixture();
    let operation = NativeBlueprintOperation;
    execute(
        &operation,
        &request("initial-build", Operation::Build, root.path()),
    )
    .unwrap();
    let before = load_generation(root.path());
    let before_generation = before.generation_id.clone();
    let before_source_hash = before.source_hash.clone();
    let construction_receipt = root.path().join(".agent/graph/full-constructions.jsonl");
    let before_constructions = fs::read_to_string(&construction_receipt).unwrap();

    fs::write(root.path().join("target.ts"), TARGET_INITIAL).unwrap();
    let refresh = set_refresh("same-byte-refresh", root.path(), 1, "modify", &["target.ts"]);
    let result = execute(&operation, &refresh).unwrap();
    assert_eq!(
        result["refreshMode"], "incremental_noop",
        "same-byte content must avoid parsing and publication"
    );
    assert_eq!(
        result["reusedFiles"], 2,
        "same-byte no-op must reuse both fixture files"
    );
    assert_eq!(result["generationId"], before_generation);
    assert_eq!(result["sourceHash"], before_source_hash);
    assert_eq!(
        before_constructions,
        fs::read_to_string(&construction_receipt).unwrap(),
        "same-byte no-op must publish no full construction"
    );
    let after = load_generation(root.path());
    assert_eq!(after.generation_id, before_generation);
    assert_eq!(after.source_hash, before_source_hash);
}
