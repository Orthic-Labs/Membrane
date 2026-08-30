//! Cross-language §15 authorization conformance runner.
//!
//! The cases and expected verdicts live only in the shared JSON corpus. This
//! runner supplies filesystem materialization for the Rust gate; it does not
//! duplicate authorization policy or expected outcomes.

use membrane_runtime::authorization::{authorize, AuthorizationRequest};
use serde_json::{json, Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

const REGISTRY_ENV: &str = "MEMBRANE_PROJECT_REGISTRY";
const CORPUS_RELATIVE: &str = "../../../schemas/conformance/authorization-conformance-v1.json";

fn text<'a>(value: &'a Value, key: &str, case_id: &str) -> &'a str {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("{case_id}: {key} must be a string"))
}

fn request_text<'a>(request: &'a Value, key: &str, case_id: &str) -> &'a str {
    request
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("{case_id}: request.{key} must be a string"))
}

fn root_path(workspace: &Path, name: &str, case_id: &str) -> PathBuf {
    match name {
        "caller" => workspace.join("caller"),
        "child" => workspace.join("child"),
        "sibling" => workspace.join("sibling"),
        "unknown" => workspace.join("unknown"),
        "none" => workspace.join("none"),
        other => panic!("{case_id}: runner does not know root token {other:?}"),
    }
}

fn binding_value(
    source: &Value,
    workspace: &Path,
    interval: &str,
    apply_interval: bool,
    case_id: &str,
) -> Value {
    assert!(source.is_object(), "{case_id}: binding must be an object");
    let root = text(source, "root", case_id);
    let mut binding = Map::new();
    binding.insert(
        "repository_id".into(),
        Value::String(text(source, "repository_id", case_id).into()),
    );
    binding.insert(
        "scope_id".into(),
        Value::String(text(source, "scope_id", case_id).into()),
    );
    binding.insert(
        "grant_policy".into(),
        json!({
            "level": text(source, "grant_level", case_id),
            "child_repository_ids": source.get("child_repository_ids")
                .and_then(Value::as_array)
                .unwrap_or_else(|| panic!("{case_id}: child_repository_ids must be an array"))
        }),
    );
    let generation = source
        .get("token_generation")
        .unwrap_or_else(|| panic!("{case_id}: token_generation is required"));
    if !generation.is_null() {
        let generation = generation
            .as_u64()
            .unwrap_or_else(|| panic!("{case_id}: token_generation must be an integer or null"));
        let revoked = source
            .get("revoked_token_generations")
            .and_then(Value::as_array)
            .unwrap_or_else(|| panic!("{case_id}: revoked_token_generations must be an array"));
        binding.insert(
            "token_grant".into(),
            json!({
                "generation": generation,
                "revoked_generations": revoked,
                "issued_at": "2025-01-01T00:00:00Z"
            }),
        );
    }
    if apply_interval {
        match interval {
            "absent" => {}
            "valid" => {
                binding.insert("not_before".into(), json!(0));
                binding.insert("not_after".into(), json!(9_000_000_000_000i64));
            }
            "not-yet-valid" => {
                binding.insert("not_before".into(), json!(9_000_000_000_000i64));
            }
            "expired" => {
                binding.insert("not_after".into(), json!(0));
            }
            other => panic!("{case_id}: runner does not know validity interval {other:?}"),
        }
    }
    let mut result = binding;
    result.insert(
        "__root".into(),
        Value::String(root_path(workspace, root, case_id).to_string_lossy().into()),
    );
    // Keep the intermediate root marker out of the registry object while
    // retaining a single construction path for every corpus binding.
    let registry_root = result
        .remove("__root")
        .expect("intermediate root marker");
    let mut wrapped = Map::new();
    wrapped.insert("root".into(), registry_root);
    wrapped.insert("binding".into(), Value::Object(result));
    Value::Object(wrapped)
}

fn write_registry(case: &Value, temp: &tempfile::TempDir, case_id: &str) -> (String, String) {
    let workspace = temp.path().join("workspace");
    for name in ["caller", "child", "sibling", "unknown", "none"] {
        fs::create_dir_all(root_path(&workspace, name, case_id)).expect("create conformance root");
    }
    let installation = case
        .get("installation")
        .and_then(Value::as_object)
        .unwrap_or_else(|| panic!("{case_id}: installation must be an object"));
    let state = installation
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("{case_id}: installation.state must be a string"));
    let caller_source = case
        .get("caller_binding")
        .unwrap_or_else(|| panic!("{case_id}: caller_binding is required"));
    let target_source = case
        .get("target_binding")
        .unwrap_or_else(|| panic!("{case_id}: target_binding is required"));
    let interval = text(case, "validity_interval", case_id);
    let caller_root_name = text(caller_source, "root", case_id);
    let target_root_name = text(target_source, "root", case_id);
    let same_root = caller_root_name == target_root_name;
    let caller_wrapped = binding_value(
        caller_source,
        &workspace,
        interval,
        same_root,
        case_id,
    );
    let target_wrapped = binding_value(
        target_source,
        &workspace,
        interval,
        true,
        case_id,
    );
    let mut bindings = Map::new();
    if state == "enrolled" {
        let caller_root = root_path(&workspace, caller_root_name, case_id);
            let mut caller_binding = caller_wrapped
            .get("binding")
            .cloned()
            .expect("caller binding wrapper");
        // A same-root request resolves one registry entry for both caller and
        // target. Preserve the caller authority while applying target token
        // state, which is the only target-side distinction in this corpus.
        if same_root {
            if let Some(target_token) = target_wrapped.get("binding").and_then(|value| value.get("token_grant")) {
                caller_binding["token_grant"] = target_token.clone();
            }
        }
        bindings.insert(caller_root.to_string_lossy().into(), caller_binding);
        if !same_root && text(case.get("scope_chain").expect("scope_chain"), "target", case_id) == "enrolled" {
            let target_root = root_path(&workspace, target_root_name, case_id);
            let target_binding = target_wrapped
                .get("binding")
                .cloned()
                .expect("target binding wrapper");
            bindings.insert(target_root.to_string_lossy().into(), target_binding);
        }
    } else if state != "unavailable" {
        panic!("{case_id}: runner does not know installation state {state:?}");
    }
    let registry_path = temp.path().join("project-registry.json");
    let registry = if state == "unavailable" {
        "{}".to_owned()
    } else {
        json!({"schema_version": 2, "bindings": bindings}).to_string()
    };
    fs::write(&registry_path, registry).expect("write conformance registry");
    let caller_root = root_path(
        &workspace,
        request_text(case.get("request").expect("request"), "caller_root", case_id),
        case_id,
    );
    (registry_path.to_string_lossy().into(), caller_root.to_string_lossy().into())
}

fn run_case(case: &Value) {
    let case_id = text(case, "case_id", "<unknown>");
    let mode = text(case, "mode", case_id);
    let expected = case
        .get("expected")
        .and_then(Value::as_object)
        .unwrap_or_else(|| panic!("{case_id}: expected must be an object"));
    let expected_allowed = expected
        .get("allowed")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| panic!("{case_id}: expected.allowed must be boolean"));
    if mode == "ungated" {
        if !expected_allowed {
            panic!("{case_id}: ungated cases must be allowed");
        }
        let operation = case
            .get("diagnostic_operation")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("{case_id}: ungated operation is required"));
        if !matches!(
            operation,
            "membrane_diagnostic_fence"
                | "membrane_diagnostic_capabilities"
                | "membrane_diagnostic_provider:list"
                | "membrane_diagnostic_provider:status"
        ) {
            panic!("{case_id}: runner does not know ungated operation {operation:?}");
        }
        return;
    }
    if mode != "gated" {
        panic!("{case_id}: runner does not know mode {mode:?}");
    }
    if case.get("diagnostic_operation").is_some_and(|value| !value.is_null()) {
        panic!("{case_id}: gated case unexpectedly declares a diagnostic operation");
    }
    let cross_root = text(case, "cross_root_reach", case_id);
    if !matches!(cross_root, "same_root" | "explicit_child_grant" | "neither") {
        panic!("{case_id}: runner does not know cross-root state {cross_root:?}");
    }
    let validity = text(case, "validity_interval", case_id);
    if !matches!(validity, "absent" | "not-yet-valid" | "valid" | "expired") {
        panic!("{case_id}: runner does not know validity state {validity:?}");
    }
    let revocation = text(case, "revocation_state", case_id);
    if !matches!(revocation, "live" | "revoked token generation") {
        panic!("{case_id}: runner does not know revocation state {revocation:?}");
    }
    let temp = tempfile::tempdir().expect("conformance temp directory");
    let (registry, caller_root) = write_registry(case, &temp, case_id);
    let previous = std::env::var_os(REGISTRY_ENV);
    std::env::set_var(REGISTRY_ENV, &registry);
    let request_value = case.get("request").expect("request");
    let caller_repository_id = request_text(request_value, "caller_repository_id", case_id);
    let caller_scope_id = request_text(request_value, "caller_scope_id", case_id);
    let target_repository = request_text(request_value, "target_repository", case_id);
    let action = request_text(request_value, "action", case_id);
    let task_grant_level = match request_value.get("task_grant_level") {
        Some(Value::Null) | None => None,
        Some(Value::String(value)) => Some(value.as_str()),
        Some(_) => panic!("{case_id}: request.task_grant_level must be string or null"),
    };
    let request = AuthorizationRequest {
        caller_root: &caller_root,
        caller_repository_id,
        caller_scope_id,
        caller_scope_descriptor: None,
        target_repository,
        task_grant_level,
        action,
    };
    let result = authorize(&request);
    match (expected_allowed, result) {
        (true, Ok(_)) => {}
        (true, Err(error)) => panic!("{case_id}: expected allow, got {error}"),
        (false, Ok(_)) => panic!("{case_id}: expected denial, got allow"),
        (false, Err(error)) => {
            let failed_gate = expected
                .get("failed_gate")
                .and_then(Value::as_str)
                .unwrap_or_else(|| panic!("{case_id}: denied case needs expected.failed_gate"));
            assert_eq!(
                error.code(),
                failed_gate,
                "{case_id}: Rust failed gate differs from corpus"
            );
            assert_eq!(error.gate().code(), failed_gate);
        }
    }
    match previous {
        Some(value) => std::env::set_var(REGISTRY_ENV, value),
        None => std::env::remove_var(REGISTRY_ENV),
    }
}

#[test]
fn authorization_corpus_matches_rust_gate_in_file_order() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(CORPUS_RELATIVE);
    let raw = fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    let corpus: Value = serde_json::from_str(&raw).expect("authorization corpus is valid JSON");
    assert_eq!(
        corpus.get("schema_version").and_then(Value::as_str),
        Some("membrane.authorization-conformance.v1")
    );
    let cases = corpus
        .get("cases")
        .and_then(Value::as_array)
        .expect("authorization corpus cases array");
    assert_eq!(cases.len(), 18, "authorization corpus case count");
    let mut executed = 0usize;
    for case in cases {
        run_case(case);
        executed += 1;
    }
    assert_eq!(executed, cases.len(), "every corpus case must execute");
}
