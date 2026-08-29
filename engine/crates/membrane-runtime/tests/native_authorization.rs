//! End-to-end §15 AuthorizationGateV1 integration tests (native path).
//!
//! Each test drives the full gate sequence through `authorize` against a
//! disposable installation (runtime manifest above the caller root + enrolled
//! registry), the same sequence `RuntimeMcpExecutor::execute` runs for every
//! repository-scoped native operation before any retrieval, scoring, or
//! admission.
//!
//! Task-authority contract under test (two deliberate defaults): the DIRECT
//! path (`authorize`, mcp/server.mjs effectiveAuthorityFor lines 192-201)
//! falls back to the caller's persisted level when no task grant travels on
//! the envelope, while the FAN-OUT primitive (`can_reach_target`,
//! mcp/authorization.mjs authorizeTarget line 52) clamps an absent grant to
//! read-only. The tests pin both defaults so neither can silently regress
//! into the other.
//!
//! The gate resolves the registry through `default_registry_path()`, which
//! honors the process-global `MEMBRANE_PROJECT_REGISTRY` env var. Rust runs a
//! test binary's tests on parallel threads, so every env-dependent test
//! installs its own registry path under the `SERIAL` lock and restores the
//! previous value before releasing it (field order of `SerialEnv` controls
//! drop order). The workspace edition is 2021, where `std::env::set_var` is
//! safe; the lock discipline remains required because concurrent readers would
//! still race the value.

use membrane_runtime::authorization::{authorize, can_reach_target, AuthorizationRequest};
use membrane_runtime::{AuthorityLevel, AuthorizationGate};
use serde_json::{json, Map, Value};
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

const REGISTRY_ENV: &str = "MEMBRANE_PROJECT_REGISTRY";

static SERIAL: Mutex<()> = Mutex::new(());

/// Restores the previous registry env value when dropped.
struct RegistryEnv {
    previous: Option<std::ffi::OsString>,
}

impl Drop for RegistryEnv {
    fn drop(&mut self) {
        match &self.previous {
            Some(previous) => std::env::set_var(REGISTRY_ENV, previous),
            None => std::env::remove_var(REGISTRY_ENV),
        }
    }
}

/// Installs a registry path under the serial lock and guarantees the restore
/// happens BEFORE the lock is released (fields drop in declaration order), so
/// the next test never observes a stale or mid-swap registry path.
struct SerialEnv {
    _registry_env: RegistryEnv,
    _serial: MutexGuard<'static, ()>,
}

impl SerialEnv {
    fn install(registry_path: &str) -> Self {
        let serial = SERIAL.lock().expect("serial test lock");
        let previous = std::env::var_os(REGISTRY_ENV);
        std::env::set_var(REGISTRY_ENV, registry_path);
        Self {
            _registry_env: RegistryEnv { previous },
            _serial: serial,
        }
    }
}

/// One disposable installation: a runtime manifest above the caller root and
/// an enrolled registry with two bindings (caller + would-be child).
struct TestInstallation {
    _dir: tempfile::TempDir,
    registry_path: PathBuf,
    caller_root: String,
    caller_repository_id: &'static str,
    caller_scope_id: &'static str,
    child_repository_id: &'static str,
    child_scope_id: &'static str,
}

impl TestInstallation {
    /// `caller_grants_child` controls whether the caller's persisted grant
    /// policy names `repo-child` in `child_repository_ids`.
    fn new(caller_grants_child: bool) -> Self {
        let dir = tempfile::tempdir().expect("temp installation dir");
        let workspace = dir.path().join("workspace");
        let caller_root = workspace.join("caller");
        let child_root = workspace.join("child");
        fs::create_dir_all(&caller_root).expect("create caller root");
        fs::create_dir_all(&child_root).expect("create child root");

        // Installation binding: valid runtime manifest above the caller root.
        let manifest = workspace.join("tools/lib/memory/runtime.json");
        fs::create_dir_all(manifest.parent().expect("manifest parent")).expect("manifest dir");
        fs::write(
            &manifest,
            json!({"port": 47851, "serviceId": "membrane-local-v1", "host": "127.0.0.1"})
                .to_string(),
        )
        .expect("write runtime manifest");

        let mut caller_policy = json!({"level": "write-trusted"});
        if caller_grants_child {
            caller_policy["child_repository_ids"] = json!(["repo-child"]);
        }
        let caller_binding = json!({
            "repository_id": "repo-caller",
            "scope_id": "D--membrane-test-caller",
            "grant_policy": caller_policy,
            "token_grant": {"generation": 1, "issued_at": "2025-01-01T00:00:00Z"}
        });
        let child_binding = json!({
            "repository_id": "repo-child",
            "scope_id": "D--membrane-test-child"
        });
        let mut bindings = Map::new();
        bindings.insert(caller_root.to_string_lossy().to_string(), caller_binding);
        bindings.insert(child_root.to_string_lossy().to_string(), child_binding);
        let registry_path = dir.path().join("project-registry.json");
        fs::write(
            &registry_path,
            json!({"schema_version": 2, "bindings": Value::Object(bindings)}).to_string(),
        )
        .expect("write registry");

        Self {
            _dir: dir,
            registry_path,
            caller_root: caller_root.to_string_lossy().to_string(),
            caller_repository_id: "repo-caller",
            caller_scope_id: "D--membrane-test-caller",
            child_repository_id: "repo-child",
            child_scope_id: "D--membrane-test-child",
        }
    }

    fn registry_path_str(&self) -> String {
        self.registry_path.to_string_lossy().to_string()
    }
}

/// Build one gate request. No test declares a `scopeDescriptor`, so
/// `caller_scope_descriptor: None` makes the gate derive the default
/// filesystem descriptor from the declared scope_id — the same shape the
/// executor forwards when the envelope omits `/caller/scopeDescriptor`.
fn request<'a>(
    installation: &'a TestInstallation,
    target: &'a str,
    task_grant: Option<&'a str>,
    action: &'a str,
) -> AuthorizationRequest<'a> {
    AuthorizationRequest {
        caller_root: &installation.caller_root,
        caller_repository_id: installation.caller_repository_id,
        caller_scope_id: installation.caller_scope_id,
        caller_scope_descriptor: None,
        target_repository: target,
        task_grant_level: task_grant,
        action,
    }
}

/// Require a denial at exactly `expected_gate`, with the typed code naming it.
fn assert_denied(request: &AuthorizationRequest<'_>, expected_gate: AuthorizationGate) {
    let denial = authorize(request).expect_err("request must be denied");
    assert_eq!(
        denial.gate(),
        expected_gate,
        "unexpected failed gate: {denial}"
    );
    assert_eq!(
        denial.code(),
        expected_gate.code(),
        "typed code must name the failed gate"
    );
}

#[test]
fn unenrolled_caller_root_is_denied_at_repository_scope_chain() {
    let installation = TestInstallation::new(true);
    let _env = SerialEnv::install(&installation.registry_path_str());
    let unenrolled = installation_registry_absent_root();
    let request = request(
        &installation,
        installation.caller_repository_id,
        None,
        "context",
    );
    let denied = AuthorizationRequest {
        caller_root: &unenrolled,
        ..request
    };
    assert_denied(&denied, AuthorizationGate::RepositoryScopeChain);
}

#[test]
fn declared_identity_mismatching_registry_binding_is_denied_at_caller_target_binding() {
    let installation = TestInstallation::new(true);
    let _env = SerialEnv::install(&installation.registry_path_str());
    // Declared scope_id does not match the persisted caller binding above.
    let request = AuthorizationRequest {
        caller_scope_id: installation.child_scope_id,
        ..request(
            &installation,
            installation.caller_repository_id,
            None,
            "context",
        )
    };
    assert_denied(&request, AuthorizationGate::CallerTargetBinding);
}

#[test]
fn cross_root_without_explicit_child_grant_is_denied_at_cross_root_denial() {
    // No child grant: the caller's persisted policy omits child_repository_ids.
    let installation = TestInstallation::new(false);
    let _env = SerialEnv::install(&installation.registry_path_str());
    // Under the §15 gate order, authority level runs BEFORE cross-root denial,
    // so the request must pass the authority gate for the reported gate to be
    // the cross-root one — a read action permits at any level (including the
    // direct path's caller-level fallback), while a mutating cross-root
    // request would report AuthorityLevel first.
    let request = request(
        &installation,
        installation.child_repository_id,
        None,
        "context",
    );
    assert_denied(&request, AuthorizationGate::CrossRootDenial);
}

#[test]
fn mutating_action_without_task_grant_is_permitted_on_direct_path() {
    // DIRECT-path contract (mcp/server.mjs effectiveAuthorityFor, lines
    // 192-201: "an absent grant never cold-caps legitimate same-root work
    // below the caller's persisted level"): a write-trusted caller performing
    // a mutating action with NO task grant on the envelope is PERMITTED. This
    // guards against re-introducing a read-only clamp on the direct path,
    // which would deny every native mutating operation (nothing on the native
    // envelope currently injects taskGrantLevel).
    let installation = TestInstallation::new(true);
    let _env = SerialEnv::install(&installation.registry_path_str());
    let request = request(
        &installation,
        installation.caller_repository_id,
        None,
        "checkpoint",
    );
    let decision = authorize(&request)
        .expect("direct-path mutation with caller-level task authority must pass");
    // The task slot is the caller's persisted level (no cold-cap); the
    // effective level is still bounded by the installation ceiling
    // (INSTALLATION_AUTHORITY_LEVEL = write-proposed), which permits the
    // mutating action at exactly write-proposed.
    assert_eq!(decision.effective_level, AuthorityLevel::WriteProposed);
    assert!(decision.same_root);
}

#[test]
fn can_reach_target_mutating_action_without_task_grant_is_denied() {
    // FAN-OUT clamp contract (mcp/authorization.mjs authorizeTarget, line 52:
    // `taskGrantLevel || "read-only"`, reached from mcp/server.mjs line ~817):
    // the workspace fan-out primitive denies mutating actions with no task
    // grant even when the caller's persisted level is write-trusted, and
    // returns None on any denial (the fan-out renders that target as a typed
    // omission row; it never widens scope).
    let installation = TestInstallation::new(true);
    let _env = SerialEnv::install(&installation.registry_path_str());
    let reached = can_reach_target(
        &installation.caller_root,
        installation.caller_repository_id,
        installation.caller_scope_id,
        installation.caller_repository_id,
        "checkpoint",
    );
    assert!(
        reached.is_none(),
        "fan-out mutation without task grant must be None"
    );
    // The same primitive with a READ action still succeeds at read-only: the
    // fan-out clamp clamps to read-only without over-denying reads.
    let reached_read = can_reach_target(
        &installation.caller_root,
        installation.caller_repository_id,
        installation.caller_scope_id,
        installation.caller_repository_id,
        "context",
    );
    assert_eq!(reached_read, Some(AuthorityLevel::ReadOnly));
}

#[test]
fn read_action_succeeds_for_enrolled_self_consistent_caller() {
    let installation = TestInstallation::new(true);
    let _env = SerialEnv::install(&installation.registry_path_str());
    let request = request(
        &installation,
        installation.caller_repository_id,
        None,
        "context",
    );
    let decision = authorize(&request).expect("enrolled self-consistent read must pass");
    // DIRECT-path fallback is visible in the authorized outcome
    // (mcp/server.mjs effectiveAuthorityFor, lines 192-201): no task grant on
    // the envelope means the caller's persisted write-trusted level IS the
    // task authority — the task slot does NOT cold-cap to read-only. The
    // effective level is bounded by the installation ceiling
    // (INSTALLATION_AUTHORITY_LEVEL = write-proposed).
    assert_eq!(decision.effective_level, AuthorityLevel::WriteProposed);
    assert!(decision.same_root);
    assert!(!decision.granted_child);
    assert_eq!(
        decision.caller_repository_id,
        installation.caller_repository_id
    );
    assert_eq!(
        decision.target_repository_id,
        installation.caller_repository_id
    );
}

/// A syntactically valid root that no binding in the test registry names.
fn installation_registry_absent_root() -> String {
    "D:\\not\\enrolled\\anywhere".to_owned()
}

// ---------------------------------------------------------------------------
// §16.1 executor path — pending → approved reaches Cortex admission through
// frozen interface 1, or `approved` is unreachable. No third outcome.
// ---------------------------------------------------------------------------

use membrane_mcp::NativeMcpExecutor;
use membrane_runtime::mcp_executor::RuntimeMcpExecutor;
use membrane_runtime::{ApprovedProposalAdmissionV1, MemDb, MemoryStore};
use serde_json::json as j;

/// Serial executor fixture over a file-backed store so the proposal event DB
/// (`MemDb::event_db_path`) is resolvable — the in-memory variant has none.
struct ExecutorSandbox {
    installation: TestInstallation,
    _env: SerialEnv,
    event_db: PathBuf,
}

impl ExecutorSandbox {
    fn new() -> Self {
        let installation = TestInstallation::new(true);
        let env = SerialEnv::install(&installation.registry_path_str());
        let event_db = installation
            ._dir
            .path()
            .join("tools/.cache/memory/membrane-events.sqlite3");
        fs::create_dir_all(event_db.parent().expect("event db parent")).expect("event db dir");
        Self {
            installation,
            _env: env,
            event_db,
        }
    }

    fn caller_envelope(&self) -> Value {
        j!({
            "root": self.installation.caller_root,
            "repositoryId": self.installation.caller_repository_id,
            "scopeId": self.installation.caller_scope_id
        })
    }

    fn store(&self) -> MemoryStore {
        MemoryStore::open(MemDb::open(&self.event_db).expect("file-backed memdb opens"))
    }

    /// Durable DB probe through the store's own event connection — the test
    /// crate has no direct rusqlite dependency.
    fn proposal_state(&self, store: &MemoryStore, proposal_id: &str) -> Option<String> {
        store
            .db()
            .lock_events()
            .query_row(
                "SELECT state FROM membrane_knowledge_proposal WHERE proposal_id = ?1",
                [proposal_id],
                |row| row.get::<_, String>(0),
            )
            .ok()
    }

    fn proposal_row_count(&self, store: &MemoryStore) -> i64 {
        store
            .db()
            .lock_events()
            .query_row(
                "SELECT COUNT(*) FROM membrane_knowledge_proposal",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("proposal count")
    }
}

fn execute(executor: &RuntimeMcpExecutor, name: &str, arguments: &Value) -> Value {
    executor.execute(name, arguments)
}

fn result_code(response: &Value) -> &str {
    response
        .pointer("/result/code")
        .and_then(Value::as_str)
        .unwrap_or("")
}

#[test]
fn approved_proposal_reaches_cortex_admission_via_the_executor_review_path() {
    let sandbox = ExecutorSandbox::new();
    let store = sandbox.store();
    let executor = RuntimeMcpExecutor::for_hub(store.clone()).expect("executor constructs");

    // 1. Submit: lands pending, quarantine-only.
    let propose = execute(
        &executor,
        "membrane_knowledge_propose",
        &j!({
            "repository": sandbox.installation.caller_repository_id,
            "caller": sandbox.caller_envelope(),
            "emission": {"text": "Approved emission about deterministic ledger reindexing."}
        }),
    );
    assert_eq!(
        propose.pointer("/result/data/reviewState"),
        Some(&j!("pending")),
        "submit response: {propose}"
    );
    let proposal_id = propose
        .pointer("/result/data/proposalId")
        .and_then(Value::as_str)
        .expect("proposalId")
        .to_owned();
    assert!(
        !store
            .entries(100)
            .iter()
            .any(|entry| entry.content.contains("deterministic ledger reindexing")),
        "a pending proposal must never become durable truth"
    );

    // 2. Review approve: the pending row transitions and the frozen interface
    // consumes it into Cortex admission. The named review loads the row's
    // stored emission; no second carrier row is created.
    let review = execute(
        &executor,
        "membrane_knowledge_propose",
        &j!({
            "repository": sandbox.installation.caller_repository_id,
            "caller": sandbox.caller_envelope(),
            "review": {"proposalId": proposal_id, "decision": "approve", "reviewer": "human-reviewer"}
        }),
    );
    assert_eq!(
        review.pointer("/result/data/reviewState"),
        Some(&j!("approved")),
        "review response: {review}"
    );
    let outcome = review
        .pointer("/result/data/admission/outcome")
        .and_then(Value::as_str)
        .expect("admission outcome");
    assert_eq!(outcome, "approved", "approved reaches admission: {review}");
    let memory_id = review
        .pointer("/result/data/admission/provenance/memoryId")
        .and_then(Value::as_str)
        .expect("memoryId")
        .to_owned();
    let admitted = store
        .entries(200)
        .into_iter()
        .find(|entry| entry.id == memory_id)
        .unwrap_or_else(|| panic!("admitted memory {memory_id} is durable"));
    assert!(
        admitted.content.contains("deterministic ledger reindexing"),
        "the admitted payload is the proposal's own stored emission"
    );
    // DB-state proof goes through the store's own event connection (the test
    // crate has no direct rusqlite dependency).
    assert_eq!(
        sandbox.proposal_state(&store, &proposal_id).as_deref(),
        Some("approved"),
        "the reviewed proposal row is durably approved"
    );
}

#[test]
fn rejected_and_pending_proposals_never_become_durable_truth() {
    let sandbox = ExecutorSandbox::new();
    let store = sandbox.store();
    let executor = RuntimeMcpExecutor::for_hub(store.clone()).expect("executor constructs");

    let propose = execute(
        &executor,
        "membrane_knowledge_propose",
        &j!({
            "repository": sandbox.installation.caller_repository_id,
            "caller": sandbox.caller_envelope(),
            "emission": {"text": "A rejected emission for quarantine"}
        }),
    );
    let proposal_id = propose
        .pointer("/result/data/proposalId")
        .and_then(Value::as_str)
        .expect("proposalId")
        .to_owned();
    let review = execute(
        &executor,
        "membrane_knowledge_propose",
        &j!({
            "repository": sandbox.installation.caller_repository_id,
            "caller": sandbox.caller_envelope(),
            "review": {"proposalId": proposal_id, "decision": "reject", "reviewer": "human-reviewer"}
        }),
    );
    assert_eq!(
        review.pointer("/result/data/reviewState"),
        Some(&j!("rejected")),
        "review response: {review}"
    );
    assert!(
        !store
            .entries(200)
            .iter()
            .any(|entry| entry.content.contains("A rejected emission")),
        "a rejected proposal must never become durable truth"
    );
}

#[test]
fn an_already_decided_proposal_cannot_be_re_decided() {
    let sandbox = ExecutorSandbox::new();
    let store = sandbox.store();
    let executor = RuntimeMcpExecutor::for_hub(store.clone()).expect("executor constructs");
    let propose = execute(
        &executor,
        "membrane_knowledge_propose",
        &j!({
            "repository": sandbox.installation.caller_repository_id,
            "caller": sandbox.caller_envelope(),
            "emission": {"text": "Decided-once emission"}
        }),
    );
    let proposal_id = propose
        .pointer("/result/data/proposalId")
        .and_then(Value::as_str)
        .expect("proposalId")
        .to_owned();
    let first = execute(
        &executor,
        "membrane_knowledge_propose",
        &j!({
            "repository": sandbox.installation.caller_repository_id,
            "caller": sandbox.caller_envelope(),
            "review": {"proposalId": proposal_id, "decision": "approve", "reviewer": "human-reviewer"}
        }),
    );
    assert_eq!(
        first.pointer("/result/data/reviewState"),
        Some(&j!("approved")),
        "first review: {first}"
    );
    let second = execute(
        &executor,
        "membrane_knowledge_propose",
        &j!({
            "repository": sandbox.installation.caller_repository_id,
            "caller": sandbox.caller_envelope(),
            "review": {"proposalId": proposal_id, "decision": "reject", "reviewer": "human-reviewer"}
        }),
    );
    assert_eq!(result_code(&second), "proposal_already_decided", "{second}");
}

#[test]
fn review_of_an_unknown_proposal_id_is_typed_and_creates_nothing() {
    let sandbox = ExecutorSandbox::new();
    let store = sandbox.store();
    let executor = RuntimeMcpExecutor::for_hub(store.clone()).expect("executor constructs");
    let response = execute(
        &executor,
        "membrane_knowledge_propose",
        &j!({
            "repository": sandbox.installation.caller_repository_id,
            "caller": sandbox.caller_envelope(),
            "review": {"proposalId": "proposal-does-not-exist", "decision": "approve", "reviewer": "human-reviewer"}
        }),
    );
    assert_eq!(
        result_code(&response),
        "proposal_review_unknown",
        "{response}"
    );
    assert_eq!(
        sandbox.proposal_row_count(&store),
        0,
        "a failed review must not create any proposal row"
    );
}

#[test]
fn frozen_interface_duplicate_disposition_is_typed_not_a_second_record() {
    // Frozen interface 1's store-side contract: a near-duplicate approved
    // proposal resolves to `duplicate`/`conflict`, never a second record.
    let sandbox = ExecutorSandbox::new();
    let store = sandbox.store();
    let payload = r#"{"text": "Approved emission about deterministic ledger reindexing."}"#;
    let first = store
        .admit_approved_proposal("prop-1", payload)
        .expect("novel proposal admits");
    assert!(
        matches!(first, ApprovedProposalAdmissionV1::Admitted { .. }),
        "{first:?}"
    );
    let second = store
        .admit_approved_proposal("prop-2", payload)
        .expect("the call succeeds and returns a typed outcome");
    match second {
        ApprovedProposalAdmissionV1::Duplicate { existing_id } => {
            let admitted = match first {
                ApprovedProposalAdmissionV1::Admitted { memory_id } => memory_id,
                other => panic!("first was admitted, got {other:?}"),
            };
            assert_eq!(existing_id, admitted, "duplicate names the existing record");
        }
        ApprovedProposalAdmissionV1::Conflict { existing_id } => {
            assert!(!existing_id.is_empty());
        }
        ApprovedProposalAdmissionV1::Admitted { memory_id } => {
            panic!("a duplicate proposal must never admit a second record: {memory_id}");
        }
    }
}
