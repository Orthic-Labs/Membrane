//! One disposable installed binding, real native dispatch and shared storage.
//! This is transport/owner qualification, not an installed third-party host test.
use membrane_runtime::push::recovery::{self, RecoveryScope, RecoveryStore, Selector};
use serde_json::{json, Value};
use std::sync::Arc;

struct Environment(Vec<(&'static str, Option<std::ffi::OsString>)>);
impl Environment {
    fn set(values: &[(&'static str, std::path::PathBuf)]) -> Self {
        let mut previous = Vec::new();
        for (key, value) in values {
            previous.push((*key, std::env::var_os(key)));
            std::env::set_var(key, value);
        }
        Self(previous)
    }
}
impl Drop for Environment {
    fn drop(&mut self) {
        for (key, previous) in self.0.drain(..).rev() {
            match previous { Some(value) => std::env::set_var(key, value), None => std::env::remove_var(key) }
        }
    }
}
fn call(server: &membrane_mcp::McpServer, name: &str, arguments: Value) -> Value {
    server.dispatch(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":arguments}})).unwrap()
}
fn data(response: &Value) -> &Value {
    assert_eq!(response.pointer("/result/isError"), Some(&json!(false)), "{response}");
    response.pointer("/result/structuredContent/result/data").unwrap()
}
#[test]
fn native_http_cli_restore_share_scope_integrity_lifetime_and_store() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("workspace");
    std::fs::create_dir_all(root.join("tools/lib/memory")).unwrap();
    let root = root.canonicalize().unwrap();
    std::fs::write(root.join("tools/lib/memory/runtime.json"), json!({"port":47851,"serviceId":"membrane-local-v1","host":"127.0.0.1"}).to_string()).unwrap();
    let registry = temp.path().join("registry.json");
    let binding = json!({"repository_id":"repo-push","scope_id":"scope-push","grant_policy":{"level":"read-only"}});
    std::fs::write(&registry, json!({"schema_version":2,"bindings":{root.to_string_lossy().as_ref():binding}}).to_string()).unwrap();
    let store_dir = root.join("tools/.cache/runc");
    let _environment = Environment::set(&[
        ("MEMBRANE_PROJECT_REGISTRY", registry.clone()),
        ("WORKSPACE_ROOT", root.clone()),
        ("MEMBRANE_ANCHOR_DIR", store_dir.clone()),
        ("MEMBRANE_PUSH_SESSION", "scope-push".into()),
    ]);
    let memory = membrane_runtime::MemoryStore::new();
    let executor = membrane_runtime::mcp_executor::RuntimeMcpExecutor::for_hub(memory.clone()).unwrap();
    assert!(membrane_mcp::install_executor(Arc::new(executor)).is_ok());
    let server = membrane_mcp::McpServer::default();
    let listed = server.dispatch(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"_meta":{"membrane.toolsets.v1":["push"]}}})).unwrap();
    for name in ["membrane_push_prepare", "membrane_push_resolve"] {
        assert!(listed["result"]["tools"].as_array().unwrap().iter().any(|tool| tool["name"] == name));
    }
    let caller = json!({"root":root,"repositoryId":"repo-push","scopeId":"scope-push"});
    let probe = call(&server, "membrane_push_resolve", json!({"repository":"repo-push","caller":caller,"operation":"probe"}));
    let token = data(&probe)["resolverToken"].as_str().unwrap();
    let original = "same exact event\r\n".repeat(500);
    let prepared = call(&server, "membrane_push_prepare", json!({"repository":"repo-push","caller":caller,
        "request":{"text":original,"kind":"log","maxBytes":2500,"resolverToken":token,"optimize":true}}));
    let prepared = data(&prepared);
    assert_eq!(prepared["disposition"], "prepared");
    assert!(prepared["receipt"]["savedBytes"].as_u64().unwrap() > 0);
    let reference = prepared["recovery"].clone();
    let handle = reference["handle"].as_str().unwrap();
    let request = json!({"repository":"repo-push","caller":caller,"operation":"resolve","handle":handle,"maxBytes":20000});
    let restored = call(&server, "membrane_push_resolve", request.clone());
    assert_eq!(data(&restored)["content"], original);
    assert_eq!(data(&restored)["disposition"], "exact");
    let (status, body) = membrane_runtime::serve::route_for_tests(&memory, "POST", "/expand", &request.to_string());
    assert_eq!(status, 200, "{body}");
    let http: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(http["result"]["data"]["content"], original);
    // CLI uses the same default directory and session namespace, not cwd/.cache.
    membrane_runtime::cli::run_cli_from(&["membrane", "push", "restore", handle, "--max-bytes", "20000"]).unwrap();
    let reopened = RecoveryStore::at(&store_dir);
    let scope = RecoveryScope::new(&root, "scope-push").unwrap();
    assert_eq!(reopened.resolve(&scope, handle, &Selector::Lines {start:2,end:2}, 100, recovery::now_ms()).unwrap().bytes().unwrap(), b"same exact event\r\n");
    // A guessed handle never grants another caller access.
    let denied = call(&server, "membrane_push_resolve", json!({"repository":"repo-push","caller":{"root":root,"repositoryId":"repo-push","scopeId":"other-session"},"operation":"resolve","handle":handle}));
    assert_eq!(denied.pointer("/result/isError"), Some(&json!(true)));
    // Binary exact bytes remain recoverable through the same API, not lossy UTF-8.
    let binary = reopened.publish(&scope, &[0,255,128,10], 1000, recovery::now_ms()).unwrap();
    let raw = call(&server, "membrane_push_resolve", json!({"repository":"repo-push","caller":caller,"operation":"resolve","handle":binary.handle}));
    assert_eq!(data(&raw)["contentEncoding"], "hex");
    assert_eq!(data(&raw)["content"], "00ff800a");
    // Tamper and expiry are tested at the live consumer, not only a helper.
    let db = rusqlite::Connection::open(store_dir.join("push-artifacts.sqlite")).unwrap();
    db.execute("UPDATE push_originals SET content=x'0000',size=2 WHERE digest=?1", [&handle[12..]]).unwrap();
    let tampered = call(&server, "membrane_push_resolve", request.clone());
    assert_eq!(tampered.pointer("/result/structuredContent/result/code"), Some(&json!("push_artifact_corrupt")));
    db.execute("UPDATE push_originals SET expires=1 WHERE digest=?1", [&handle[12..]]).unwrap();
    let (status, _) = membrane_runtime::serve::route_for_tests(&memory, "POST", "/push/resolve", &request.to_string());
    assert_eq!(status, 410);
    assert!(membrane_runtime::cli::run_cli_from(&["membrane", "push", "restore", handle]).unwrap_err().contains("expired"));
    // Registry revocation is re-observed on every operation.
    std::fs::write(&registry, json!({"schema_version":2,"bindings":{}}).to_string()).unwrap();
    let revoked = call(&server, "membrane_push_resolve", request);
    assert_eq!(revoked.pointer("/result/isError"), Some(&json!(true)));
}
