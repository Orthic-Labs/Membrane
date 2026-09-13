//! Focused local qualification for the single-instance architecture.
//!
//! This is a deterministic contract harness, not a replacement for installed
//! client qualification. It keeps one engine model behind one owner fence and
//! exercises the acceptance-table cases that do not need a live installation.
//! The optional installed-path preflight never falls back to a development
//! binary: without an installed-path environment variable it reports a typed
//! skip; a supplied but invalid path fails loudly.

use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier, Mutex};
use std::thread;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Operation {
    Pull,
    Push,
    Hook,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Holder {
    Hub,
    CodeRight,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HookDisposition {
    Accepted,
    TypedSkip,
}

#[derive(Debug, Eq, PartialEq)]
enum DispatchError {
    StaleBoot { expected: u64, observed: u64 },
}

#[derive(Debug)]
struct EngineState {
    owner_id: &'static str,
    boot_epoch: u64,
    holder_counts: BTreeMap<Holder, usize>,
    request_counts: BTreeMap<Operation, usize>,
    runtime_child_launches: usize,
    persistent_forwarding_clients: usize,
}

#[derive(Clone, Debug)]
struct Engine {
    state: Arc<Mutex<EngineState>>,
}

#[derive(Clone, Debug)]
struct Client {
    engine: Engine,
    boot_epoch: u64,
    client_id: String,
}

#[derive(Debug, Eq, PartialEq)]
struct Response {
    owner_id: &'static str,
    boot_epoch: u64,
    hook: Option<HookDisposition>,
}

impl Engine {
    fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(EngineState {
                owner_id: "engine-1",
                boot_epoch: 1,
                holder_counts: BTreeMap::new(),
                request_counts: BTreeMap::new(),
                runtime_child_launches: 0,
                // Direct HTTP-capable clients do not need a forwarding process.
                persistent_forwarding_clients: 0,
            })),
        }
    }

    fn connect(&self, client_id: impl Into<String>) -> Client {
        let boot_epoch = self.state.lock().expect("engine lock").boot_epoch;
        Client {
            engine: self.clone(),
            boot_epoch,
            client_id: client_id.into(),
        }
    }

    fn acquire_holder(&self, holder: Holder) {
        let mut state = self.state.lock().expect("engine lock");
        *state.holder_counts.entry(holder).or_default() += 1;
    }

    fn release_holder(&self, holder: Holder) {
        let mut state = self.state.lock().expect("engine lock");
        let count = state.holder_counts.entry(holder).or_default();
        assert!(*count > 0, "holder release without acquire: {holder:?}");
        *count -= 1;
    }

    fn restart(&self) {
        let mut state = self.state.lock().expect("engine lock");
        state.boot_epoch += 1;
    }

    fn snapshot(&self) -> EngineSnapshot {
        let state = self.state.lock().expect("engine lock");
        EngineSnapshot {
            owner_id: state.owner_id,
            boot_epoch: state.boot_epoch,
            holder_count: state.holder_counts.values().sum(),
            request_counts: state.request_counts.clone(),
            runtime_child_launches: state.runtime_child_launches,
            persistent_forwarding_clients: state.persistent_forwarding_clients,
        }
    }

    fn background_active(&self) -> bool {
        self.snapshot().holder_count > 0
    }
}

#[derive(Debug)]
struct EngineSnapshot {
    owner_id: &'static str,
    boot_epoch: u64,
    holder_count: usize,
    request_counts: BTreeMap<Operation, usize>,
    runtime_child_launches: usize,
    persistent_forwarding_clients: usize,
}

impl Client {
    fn dispatch(&self, operation: Operation) -> Result<Response, DispatchError> {
        let mut state = self.engine.state.lock().expect("engine lock");
        if state.boot_epoch != self.boot_epoch {
            return Err(DispatchError::StaleBoot {
                expected: state.boot_epoch,
                observed: self.boot_epoch,
            });
        }
        *state.request_counts.entry(operation).or_default() += 1;
        Ok(Response {
            owner_id: state.owner_id,
            boot_epoch: state.boot_epoch,
            hook: (operation == Operation::Hook).then_some(HookDisposition::Accepted),
        })
    }

    fn dispatch_hook(&self, event: &str) -> Result<Response, DispatchError> {
        let mut state = self.engine.state.lock().expect("engine lock");
        if state.boot_epoch != self.boot_epoch {
            return Err(DispatchError::StaleBoot {
                expected: state.boot_epoch,
                observed: self.boot_epoch,
            });
        }
        *state.request_counts.entry(Operation::Hook).or_default() += 1;
        let hook = match event {
            "UserPromptSubmit" | "PostToolUse" | "TaskCompleted" => HookDisposition::Accepted,
            // Startup events unavailable before host protocol context stay typed,
            // never become an unreported success.
            "SessionStart" | "Setup" => HookDisposition::TypedSkip,
            _ => HookDisposition::TypedSkip,
        };
        Ok(Response {
            owner_id: state.owner_id,
            boot_epoch: state.boot_epoch,
            hook: Some(hook),
        })
    }
}

#[test]
fn five_mixed_clients_share_one_engine_without_runtime_children() {
    let engine = Engine::new();
    let barrier = Arc::new(Barrier::new(5));
    let workers = (0..5)
        .map(|index| {
            let engine = engine.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                let client = engine.connect(format!("http-client-{index}"));
                barrier.wait();
                let pull = client.dispatch(Operation::Pull).expect("Pull dispatch");
                let push = client.dispatch(Operation::Push).expect("Push dispatch");
                let hook = client
                    .dispatch_hook("UserPromptSubmit")
                    .expect("hook dispatch");
                (client.client_id, pull, push, hook)
            })
        })
        .collect::<Vec<_>>();

    let results = workers
        .into_iter()
        .map(|worker| worker.join().expect("client worker"))
        .collect::<Vec<_>>();
    let snapshot = engine.snapshot();

    assert_eq!(results.len(), 5);
    assert!(results.iter().all(|(_, pull, push, hook)| {
        pull.owner_id == "engine-1"
            && push.owner_id == "engine-1"
            && hook.owner_id == "engine-1"
            && pull.boot_epoch == 1
            && push.boot_epoch == 1
            && hook.boot_epoch == 1
    }));
    assert_eq!(snapshot.owner_id, "engine-1");
    assert_eq!(snapshot.boot_epoch, 1);
    assert_eq!(snapshot.request_counts[&Operation::Pull], 5);
    assert_eq!(snapshot.request_counts[&Operation::Push], 5);
    assert_eq!(snapshot.request_counts[&Operation::Hook], 5);
    assert_eq!(snapshot.runtime_child_launches, 0);
    assert_eq!(snapshot.persistent_forwarding_clients, 0);
}

#[test]
fn hub_coderight_matrix_keeps_explicit_service_available() {
    let holder_matrix: [&[Holder]; 4] = [
        &[],
        &[Holder::Hub],
        &[Holder::CodeRight],
        &[Holder::Hub, Holder::CodeRight],
    ];
    for active_holders in holder_matrix {
        let engine = Engine::new();
        let client = engine.connect("matrix-client");
        for holder in active_holders {
            engine.acquire_holder(*holder);
        }

        let response = client.dispatch(Operation::Pull).expect("Hub-off Pull");
        assert_eq!(response.owner_id, "engine-1");
        assert_eq!(engine.background_active(), !active_holders.is_empty());

        for holder in active_holders.iter().rev() {
            engine.release_holder(*holder);
        }
        assert!(!engine.background_active());
        client.dispatch(Operation::Push).expect("post-release Push");
    }
}

#[test]
fn restart_fences_stale_clients_and_reconnects_at_new_epoch() {
    let engine = Engine::new();
    let stale = engine.connect("before-restart");
    assert_eq!(
        stale
            .dispatch(Operation::Pull)
            .expect("initial Pull")
            .boot_epoch,
        1
    );

    engine.restart();
    assert_eq!(
        stale.dispatch(Operation::Push),
        Err(DispatchError::StaleBoot {
            expected: 2,
            observed: 1,
        })
    );

    let reconnected = engine.connect("after-restart");
    let response = reconnected
        .dispatch(Operation::Push)
        .expect("reconnected Push");
    assert_eq!(response.owner_id, "engine-1");
    assert_eq!(response.boot_epoch, 2);
    assert_eq!(engine.snapshot().runtime_child_launches, 0);
}

#[test]
fn supported_and_unsupported_hooks_are_explicitly_typed() {
    let engine = Engine::new();
    let client = engine.connect("hook-client");
    assert_eq!(
        client
            .dispatch_hook("PostToolUse")
            .expect("supported hook")
            .hook,
        Some(HookDisposition::Accepted)
    );
    assert_eq!(
        client
            .dispatch_hook("SessionStart")
            .expect("startup hook")
            .hook,
        Some(HookDisposition::TypedSkip)
    );
    assert_eq!(
        client
            .dispatch_hook("FutureHostEvent")
            .expect("unknown hook")
            .hook,
        Some(HookDisposition::TypedSkip)
    );
    assert_eq!(engine.snapshot().runtime_child_launches, 0);
}

fn installed_executable_from_environment() -> Option<PathBuf> {
    for variable in [
        "MEMBRANE_QUALIFICATION_EXE",
        "MEMBRANE_INSTALLED_EXE",
        "MEMBRANE_CURRENT",
        "MEMBRANE_CURRENT_DIR",
        "MEMBRANE_INSTALL_ROOT",
    ] {
        if let Some(value) = env::var_os(variable) {
            let supplied = PathBuf::from(value);
            let executable = if supplied.is_dir() {
                if supplied
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.eq_ignore_ascii_case("current"))
                {
                    supplied.join("membrane.exe")
                } else {
                    supplied.join("current").join("membrane.exe")
                }
            } else {
                supplied
            };
            return Some(executable);
        }
    }
    None
}

fn is_under_current(path: &Path) -> bool {
    path.components().any(|component| {
        component
            .as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case("current")
    })
}

/// Installed qualification is intentionally environment-bound. A local test
/// run without an installer-owned `current` emits a typed skip rather than
/// binding to `target/`, PATH, or this checkout.
#[test]
fn installed_path_preflight_is_typed_and_never_dev_falls_back() {
    let Some(executable) = installed_executable_from_environment() else {
        eprintln!("[SKIP][installed] no installed Membrane path environment variable set");
        return;
    };
    assert!(
        executable.is_file(),
        "[FAIL][installed_path_missing] supplied installed executable does not exist: {}",
        executable.display()
    );
    assert!(
        is_under_current(&executable),
        "[FAIL][installed_path_not_current] executable must resolve under installer-owned current: {}",
        executable.display()
    );
    assert_eq!(
        executable
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.to_ascii_lowercase()),
        Some("membrane.exe".to_string()),
        "[FAIL][installed_executable_name] expected membrane.exe"
    );
}
