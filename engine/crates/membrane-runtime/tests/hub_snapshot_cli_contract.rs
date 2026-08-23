//! Integration contract for the canonical Hub snapshot produced by the exact
//! production composition behind `membrane cli hub-snapshot` and the resident
//! `/hub/snapshot` route (`hub_inputs::compose_live_hub_snapshot`).
//!
//! These tests exercise the REAL chain — TCP fetch of the local resident's
//! `/health`, frozen parent mapping, Blueprint IPC seam, typed protocol
//! serialization — then assert the serialized wire payload the Hub's Tauri
//! sidecar consumes:
//!
//! - `membraneState` present and typed (Running/Degraded/Offline);
//! - exactly Pull/Push/Cortex/Blueprint/Guide/Adapt subsystems;
//! - Blueprint status obtained through the existing IPC seam (a live stub
//!   daemon yields Available; an absent endpoint stays locally Unavailable
//!   without touching the parent);
//! - Guide distinct from Cortex/sentinel evidence.
//!
//! It also regenerates/validates the golden fixtures consumed by the Hub JS
//! chain test (apps/membrane-hub/tests/hub-chain.mjs) so both languages are
//! pinned to one producer truth.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Mutex, MutexGuard};

use membrane_runtime::hub_inputs::{compose_hub_snapshot, compose_live_hub_snapshot};

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn lock_env() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Serve exactly one canned `/health` body over real HTTP on an ephemeral
/// port, so composition runs its true fetch path.
fn spawn_health_stub(body: &'static str) -> (u16, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buffer = [0u8; 1024];
            let _ = stream.read(&mut buffer);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });
    (port, handle)
}

/// Speak the existing Blueprint status IPC framing (newline-delimited JSON,
/// protocolVersion 1) so the production client exercises its full request +
/// response path against the public seam.
#[cfg(unix)]
fn spawn_blueprint_stub(reply: &'static str) -> std::path::PathBuf {
    use std::os::unix::net::UnixListener;
    let dir = tempfile::tempdir().expect("tempdir");
    let endpoint = dir.path().join("blueprint.sock");
    let listener = UnixListener::bind(&endpoint).expect("bind blueprint stub socket");
    std::fs::create_dir_all(dir.path()).ok();
    // Keep the socket alive for the duration of the process by parking the
    // accept loop on a detached thread; tempdir cleanup happens at test end.
    let endpoint_for_cleanup = endpoint.clone();
    std::mem::forget(dir);
    std::thread::spawn(move || {
        let _ = &endpoint_for_cleanup;
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            let mut buffer = Vec::new();
            let mut byte = [0u8; 1];
            loop {
                match stream.read(&mut byte) {
                    Ok(0) => break,
                    Ok(_) => {
                        if byte[0] == b'\n' {
                            break;
                        }
                        buffer.push(byte[0]);
                        if buffer.len() >= 16 * 1024 {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            let _ = buffer;
            let response = format!("{reply}\n");
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });
    endpoint
}

const HEALTH_OK: &str = r#"{"ok":true,"catalog":{"status":"ok"},"database":{"status":"ok","memoryCount":7},"dailyAnalysis":{"status":"fresh","alert":false}}"#;
const HEALTH_UNHEALTHY: &str =
    r#"{"ok":false,"catalog":{"status":"error"},"database":{"status":"ok"},"dailyAnalysis":{"status":"fresh","alert":false}}"#;

fn set_missing_blueprint_endpoint() {
    unsafe {
        std::env::set_var(
            "BLUEPRINT_DAEMON_ENDPOINT",
            "/nonexistent/hub_cli_contract/blueprint.sock",
        );
    }
}

fn unset_blueprint_endpoint() {
    unsafe {
        std::env::remove_var("BLUEPRINT_DAEMON_ENDPOINT");
    }
}

fn with_missing_db<R>(f: impl FnOnce() -> R) -> R {
    let prior = std::env::var_os("MEMBRANE_DB_PATH");
    unsafe {
        std::env::set_var(
            "MEMBRANE_DB_PATH",
            "/nonexistent/hub_cli_contract/cortex-engine.db",
        );
    }
    let result = f();
    unsafe {
        match prior {
            Some(value) => std::env::set_var("MEMBRANE_DB_PATH", value),
            None => std::env::remove_var("MEMBRANE_DB_PATH"),
        }
    }
    result
}

fn live_snapshot_with_health(body: &'static str) -> membrane_protocol::HubSnapshotV1 {
    live_snapshot_with_health_and_blueprint(body, None)
}

fn live_snapshot_with_health_and_blueprint(
    health_body: &'static str,
    blueprint_endpoint: Option<std::path::PathBuf>,
) -> membrane_protocol::HubSnapshotV1 {
    let _guard = lock_env();
    let (port, server) = spawn_health_stub(health_body);
    unsafe {
        std::env::set_var("MEMBRANE_PORT", port.to_string());
    }
    match blueprint_endpoint {
        Some(endpoint) => unsafe {
            std::env::set_var("BLUEPRINT_DAEMON_ENDPOINT", endpoint);
        },
        None => set_missing_blueprint_endpoint(),
    }
    // Deterministic observation stamp via compose_hub_snapshot.
    let snapshot = with_missing_db(|| {
        let parts = membrane_runtime::hub_inputs::live_snapshot_parts_from_local_service()
            .expect("stub /health must be reachable");
        compose_hub_snapshot(parts, 42)
    });
    unsafe {
        std::env::remove_var("MEMBRANE_PORT");
    }
    unset_blueprint_endpoint();
    let _ = server.join();
    snapshot
}

fn subsystems_of(
    snapshot: &membrane_protocol::HubSnapshotV1,
) -> &membrane_protocol::HubSubsystemsV1 {
    snapshot
        .subsystems
        .as_ref()
        .expect("canonical snapshots always carry the six subsystems")
}

fn subsystem<'a>(
    subsystems: &'a membrane_protocol::HubSubsystemsV1,
    name: &str,
) -> &'a membrane_protocol::HubSubsystemV1 {
    match name {
        "pull" => &subsystems.pull,
        "push" => &subsystems.push,
        "cortex" => &subsystems.cortex,
        "blueprint" => &subsystems.blueprint,
        "guide" => &subsystems.guide,
        "adapt" => &subsystems.adapt,
        _ => panic!("unknown subsystem {name}"),
    }
}

fn assert_canonical_shape(snapshot: &membrane_protocol::HubSnapshotV1, expected_state: &str) {
    let encoded = serde_json::to_value(snapshot).expect("snapshot serializes");

    // Typed parent state on the wire — never a free-form string value.
    assert_eq!(
        encoded["membraneState"].as_str(),
        Some(expected_state),
        "serialized membraneState must match the frozen producer mapping"
    );

    // Exactly the six semantic subsystems, by name.
    let subsystems = encoded["subsystems"]
        .as_object()
        .expect("typed subsystems object");
    let mut names: Vec<&str> = subsystems.keys().map(String::as_str).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        ["adapt", "blueprint", "cortex", "guide", "pull", "push"],
        "exactly Pull/Push/Cortex/Blueprint/Guide/Adapt must be present"
    );

    // The eight operational resources stay separate from subsystems.
    let sections = encoded["sections"].as_object().unwrap();
    assert_eq!(sections.len(), 8, "eight operational resources expected");
    assert!(sections.get("blueprint").is_none());
    assert!(subsystems.get("memory").is_none());

    // Schema stays v1.
    assert_eq!(encoded["schemaVersion"], 1);
    assert_eq!(encoded["productId"], "membrane");
}

#[test]
fn cli_composition_healthy_resident_is_running_with_absent_blueprint_ipc() {
    let snapshot = live_snapshot_with_health(HEALTH_OK);
    assert_canonical_shape(&snapshot, "running");

    // Blueprint through the existing IPC seam: absent daemon endpoint =>
    // locally Unavailable while the parent stays Running.
    assert_eq!(
        subsystems_of(&snapshot).blueprint.state,
        membrane_protocol::SubsystemStateV1::Unavailable
    );
    assert_eq!(subsystems_of(&snapshot).blueprint.reason, "blueprint_unavailable");

    // Not-configured is a first-class typed state, not degraded/unavailable.
    let subsystems = subsystems_of(&snapshot);
    for name in ["pull", "push", "guide", "adapt"] {
        let section = subsystem(subsystems, name);
        assert_eq!(section.state, membrane_protocol::SubsystemStateV1::NotConfigured);
        assert_eq!(section.reason, "not_instrumented");
    }

    // Guide is distinct from Cortex/sentinel evidence.
    assert_ne!(subsystems_of(&snapshot).guide.reason, subsystems_of(&snapshot).cortex.reason);

    // Child failure never promotes into parent state.
    assert_eq!(
        snapshot.membrane_state,
        Some(membrane_protocol::MembraneParentState::Running)
    );
}

#[test]
fn cli_composition_unhealthy_resident_is_degraded_through_real_chain() {
    let snapshot = live_snapshot_with_health(HEALTH_UNHEALTHY);
    assert_canonical_shape(&snapshot, "degraded");
    assert_eq!(
        snapshot.membrane_state,
        Some(membrane_protocol::MembraneParentState::Degraded)
    );
}

#[cfg(unix)]
#[test]
fn cli_composition_reads_blueprint_through_live_ipc_seam() {
    let reply = r#"{"protocolVersion":1,"ok":true,"result":{"state":"ready","artifacts":{"graphState":"fresh","generationId":"gen-contract"},"manifest":{"complete":true},"repository":{"revision":"abc123"},"runtime":{"daemonRunning":true,"watcherRunning":true,"enrolledRepoCount":1}}}"#;
    let endpoint = spawn_blueprint_stub(reply);
    let snapshot = live_snapshot_with_health_and_blueprint(HEALTH_OK, Some(endpoint));

    // The existing typed public IPC boundary delivered fresh+complete status.
    assert_eq!(
        subsystems_of(&snapshot).blueprint.state,
        membrane_protocol::SubsystemStateV1::Available
    );
    let items = subsystems_of(&snapshot).blueprint.items.as_ref().expect("evidence items");
    assert_eq!(items[0]["graphState"], "fresh");
    assert_eq!(items[0]["generationId"], "gen-contract");
    // Parent state remains untouched by child availability.
    assert_eq!(
        snapshot.membrane_state,
        Some(membrane_protocol::MembraneParentState::Running)
    );
}

#[test]
fn offline_fallback_is_offline_without_inventing_child_health() {
    let parts = membrane_runtime::hub_inputs::offline_snapshot_parts();
    let snapshot = compose_hub_snapshot(parts, 42);
    assert_canonical_shape(&snapshot, "offline");
    let subsystems = subsystems_of(&snapshot);
    for name in membrane_protocol::SUBSYSTEM_NAMES {
        let section = subsystem(subsystems, name);
        assert_eq!(section.state, membrane_protocol::SubsystemStateV1::Unavailable);
        assert_eq!(section.reason, "source_not_connected");
    }
}

#[test]
fn unreachable_resident_via_live_path_is_offline() {
    let _guard = lock_env();
    set_missing_blueprint_endpoint();
    unsafe {
        // Port 1 is reserved and refuses connections deterministically.
        std::env::set_var("MEMBRANE_PORT", "1");
    }
    let encoded = with_missing_db(|| serde_json::to_value(compose_live_hub_snapshot()).unwrap());
    unsafe {
        std::env::remove_var("MEMBRANE_PORT");
    }
    unset_blueprint_endpoint();
    assert_eq!(encoded["membraneState"], "offline");
}

/// Keep the committed JS-chain fixtures byte-faithful to this producer. To
/// regenerate after an intentional contract change, create the sentinel file
/// `crates/membrane-runtime/tests/fixture-write.sentinel` (or set
/// HUB_SNAPSHOT_FIXTURE_WRITE=1 outside sandboxed runners) and re-run this
/// test; it writes both fixtures and removes the sentinel.
#[test]
fn js_chain_fixtures_match_producer_serialization() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    // Repo root is three levels above this crate directory.
    let fixture_dir = manifest_dir.join("../../../apps/membrane-hub/tests/fixtures");
    let sentinel = manifest_dir.join("tests/fixture-write.sentinel");
    let cases = [
        ("hub-snapshot-running.json", HEALTH_OK),
        ("hub-snapshot-degraded.json", HEALTH_UNHEALTHY),
    ];
    let expected_states = ["running", "degraded"];
    let mut generated = Vec::new();
    for (_, health) in &cases {
        let mut snapshot = live_snapshot_with_health(health);
        // Freeze volatile stamps so the golden bytes are deterministic.
        snapshot.observed_at_unix_ms = 42;
        for section in snapshot.sections.values_mut() {
            section.observed_at_unix_ms = Some(42);
        }
        let subsystems = snapshot.subsystems.as_mut().unwrap();
        for field in [
            &mut subsystems.pull,
            &mut subsystems.push,
            &mut subsystems.cortex,
            &mut subsystems.blueprint,
            &mut subsystems.guide,
            &mut subsystems.adapt,
        ] {
            field.observed_at_unix_ms = Some(42);
        }
        generated.push(serde_json::to_string_pretty(&snapshot).unwrap() + "\n");
    }

    let write_requested =
        std::env::var_os("HUB_SNAPSHOT_FIXTURE_WRITE").is_some() || sentinel.exists();
    for (((name, _), body), expected_state) in
        cases.iter().zip(generated.iter()).zip(expected_states.iter())
    {
        let path = fixture_dir.join(name);
        if write_requested {
            std::fs::create_dir_all(&fixture_dir).unwrap();
            std::fs::write(&path, body).unwrap();
            continue;
        }
        let committed = std::fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!(
                "missing fixture {path:?} ({error}); create tests/fixture-write.sentinel and rerun"
            )
        });
        assert_eq!(
            committed, *body,
            "fixture {name} drifted from producer serialization"
        );
        let parsed: serde_json::Value = serde_json::from_str(&committed).unwrap();
        assert_eq!(parsed["membraneState"], *expected_state);
        assert_eq!(parsed["observedAtUnixMs"], 42);
        assert_eq!(
            parsed["subsystems"].as_object().unwrap().len(),
            6,
            "fixtures must carry all six subsystems"
        );
    }
    if write_requested {
        std::fs::remove_file(&sentinel).ok();
    }
}
