//! §15 diagnostics authorization coverage. No diagnostic state is touched until
//! the shared native AuthorizationGateV1 has accepted the verified caller and
//! target identities.

use membrane_runtime::authorization::{authorize, authorize_diagnostic, AuthorizationRequest};
use membrane_runtime::mcp_executor::RuntimeMcpExecutor;
use membrane_runtime::{AuthorizationGate, MemDb, MemoryStore};
use membrane_mcp::NativeMcpExecutor;
use serde_json::{json, Map, Value};
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

const REGISTRY_ENV: &str = "MEMBRANE_PROJECT_REGISTRY";
static SERIAL: Mutex<()> = Mutex::new(());

struct EnvGuard {
    previous: Option<std::ffi::OsString>,
    _serial: MutexGuard<'static, ()>,
}
impl EnvGuard {
    fn install(path: &str) -> Self {
        let serial = SERIAL.lock().expect("authorization test lock");
        let previous = std::env::var_os(REGISTRY_ENV);
        std::env::set_var(REGISTRY_ENV, path);
        Self { previous, _serial: serial }
    }
}
impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var(REGISTRY_ENV, value),
            None => std::env::remove_var(REGISTRY_ENV),
        }
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    _env: EnvGuard,
    registry: PathBuf,
    caller_root: String,
    child_root: String,
}
impl Fixture {
    fn new(caller_level: &str, child_grant: bool, token: Value) -> Self {
        let dir = tempfile::tempdir().expect("fixture directory");
        let workspace = dir.path().join("workspace");
        let caller_root = workspace.join("caller");
        let child_root = workspace.join("child");
        fs::create_dir_all(&caller_root).expect("caller root");
        fs::create_dir_all(&child_root).expect("child root");
        let manifest = workspace.join("tools/lib/memory/runtime.json");
        fs::create_dir_all(manifest.parent().unwrap()).expect("manifest directory");
        fs::write(&manifest, json!({"port":47851,"serviceId":"membrane-local-v1","host":"127.0.0.1"}).to_string()).expect("runtime manifest");
        let mut policy = json!({"level": caller_level});
        if child_grant { policy["child_repository_ids"] = json!(["repo-child"]); }
        let caller = json!({
            "repository_id":"repo-caller", "scope_id":"scope-caller",
            "grant_policy":policy, "token_grant":token
        });
        let child = json!({"repository_id":"repo-child","scope_id":"scope-child","grant_policy":{"level":"write-trusted"}});
        let mut bindings = Map::new();
        bindings.insert(caller_root.to_string_lossy().to_string(), caller);
        bindings.insert(child_root.to_string_lossy().to_string(), child);
        let registry = dir.path().join("project-registry.json");
        fs::write(&registry, json!({"schema_version":2,"bindings":bindings}).to_string()).expect("registry");
        let env = EnvGuard::install(registry.to_str().unwrap());
        Self { _dir: dir, _env: env, registry, caller_root: caller_root.to_string_lossy().to_string(), child_root: child_root.to_string_lossy().to_string() }
    }
    fn request<'a>(&'a self, target: &'a str, task: Option<&'a str>, action: &'a str) -> AuthorizationRequest<'a> {
        AuthorizationRequest {
            caller_root: &self.caller_root,
            caller_repository_id: "repo-caller",
            caller_scope_id: "scope-caller",
            caller_scope_descriptor: None,
            target_repository: target,
            task_grant_level: task,
            action,
        }
    }
    fn diagnostic_request<'a>(&'a self, target: &'a str, task: Option<&'a str>, action: &'a str) -> AuthorizationRequest<'a> {
        self.request(target, task, action)
    }
}

fn denied(request: &AuthorizationRequest<'_>, gate: AuthorizationGate) {
    let error = authorize(request).expect_err("authorization must deny");
    assert_eq!(error.gate(), gate);
    assert_eq!(error.code(), gate.code());
}

#[test]
fn malformed_installation_grant_is_denied_at_gate_one() {
    let fixture = Fixture::new("write-trusted", false, json!({"generation":1}));
    fs::write(&fixture.registry, "{}").expect("corrupt registry");
    denied(&fixture.request("repo-caller", None, "checkpoint"), AuthorizationGate::InstallationGrant);
}

#[test]
fn unenrolled_installation_is_denied_before_diagnostic_state() {
    let fixture = Fixture::new("write-trusted", false, json!({"generation":1}));
    let missing = fixture._dir.path().join("not-enrolled").to_string_lossy().to_string();
    let request = AuthorizationRequest { caller_root: &missing, ..fixture.request("repo-caller", None, "checkpoint") };
    denied(&request, AuthorizationGate::RepositoryScopeChain);
}

#[test]
fn broken_target_scope_chain_is_denied() {
    let fixture = Fixture::new("write-trusted", false, json!({"generation":1}));
    denied(&fixture.request("repo-absent", None, "checkpoint"), AuthorizationGate::RepositoryScopeChain);
}

#[test]
fn mismatched_caller_identity_is_denied_at_binding_gate() {
    let fixture = Fixture::new("write-trusted", false, json!({"generation":1}));
    let request = AuthorizationRequest { caller_scope_id: "scope-forged", ..fixture.request("repo-caller", None, "checkpoint") };
    denied(&request, AuthorizationGate::CallerTargetBinding);
}

#[test]
fn insufficient_monotone_authority_is_denied() {
    let fixture = Fixture::new("read-only", false, json!({"generation":1}));
    denied(&fixture.request("repo-caller", Some("admin"), "checkpoint"), AuthorizationGate::AuthorityLevel);
}

#[test]
fn cross_root_reach_without_child_grant_is_denied() {
    let fixture = Fixture::new("write-trusted", false, json!({"generation":1}));
    denied(&fixture.request("repo-child", Some("admin"), "context"), AuthorizationGate::CrossRootDenial);
}

#[test]
fn expired_validity_interval_is_denied_at_gate_six() {
    let fixture = Fixture::new("write-trusted", false, json!({"generation":1,"not_after":0}));
    denied(&fixture.request("repo-caller", Some("write-proposed"), "checkpoint"), AuthorizationGate::ValidityRevocation);
}

#[test]
fn revoked_grant_is_denied_at_the_same_gate_six_identity() {
    let fixture = Fixture::new("write-trusted", false, json!({"generation":7,"revoked_generations":[7]}));
    denied(&fixture.request("repo-caller", Some("write-proposed"), "checkpoint"), AuthorizationGate::ValidityRevocation);
}

#[test]
fn authorized_self_diagnostic_identity_succeeds() {
    let fixture = Fixture::new("write-trusted", false, json!({"generation":1}));
    let decision = authorize_diagnostic(&fixture.diagnostic_request("repo-caller", Some("write-proposed"), "checkpoint"), &fixture.caller_root)
        .expect("verified enrolled self diagnostic may proceed");
    assert!(decision.same_root);
    assert_eq!(decision.target_repository_id, "repo-caller");
}

#[test]
fn native_executor_calls_shared_gate_before_diagnostic_dispatch() {
    // A forged diagnostic repository claim is not allowed to reach the
    // service. This fails closed if the executor ever reintroduces the old
    // repoId-only diagnostic branch instead of calling authorization.rs.
    let dir = tempfile::tempdir().expect("event directory");
    let db = MemoryStore::open(MemDb::open(dir.path().join("events.sqlite3")).expect("event db"));
    let executor = RuntimeMcpExecutor::for_hub(db).expect("runtime executor");
    let response = executor.execute("membrane_diagnostic_workspace", &json!({
        "operation":"status", "repoId":"repo-forged", "worktreeId":"worktree"
    }));
    assert_eq!(response.pointer("/result/code").and_then(Value::as_str), Some("authorization_denied"));
}
