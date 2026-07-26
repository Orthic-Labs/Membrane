//! `/federate` route against a fixture stdio worker (plan Task 4, amendments
//! A2/A3): envelope shape + transport telemetry, single-slot busy semantics,
//! and 502-on-worker-failure so the hook can fall back fast.
#![cfg(unix)]

use std::path::{Path, PathBuf};

/// A stdio worker that speaks the readiness handshake and answers every
/// request with a canned, planner-valid, empty-candidates CCS.
fn fixture_worker(dir: &Path) -> PathBuf {
    let script = dir.join("gateway.py");
    let body = r#"#!/usr/bin/env python3
import json, sys, time
print(json.dumps({"workerReady": {"gatewaySha256": "0"*64, "pid": 1}}), flush=True)
for line in iter(sys.stdin.readline, ''):
    line = line.strip()
    if not line:
        continue
    req = json.loads(line)
    if req.get("task") == "stall":
        time.sleep(30)
    ccs = {
        "schemaVersion": 1,
        "traceId": "rc-fed-fixture",
        "task": req.get("task", ""),
        "mode": "verify",
        "provider": "federated",
        "freshness": {
            "revision": "rc-fed-fixture",
            "indexedAt": "2026-07-26T00:00:00Z",
            "stale": False,
        },
        "providerCeiling": {"maxCandidates": 256, "maxEstimatedTokens": req.get("maxTokens", 4096)},
        "candidates": [],
        "omissions": [],
        "_rightcontext": {
            "providerCounts": {},
            "providerWarnings": [],
            "stageElapsedMs": {},
            "repoRoot": req.get("repo", ""),
        },
    }
    print(json.dumps(ccs), flush=True)
"#;
    std::fs::write(&script, body).unwrap();
    script
}

/// Tests share the process-global resident worker AND the script env var;
/// serialize them so cargo's parallel test threads cannot interleave.
static TEST_SLOT: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn with_fixture_env<T>(script: &Path, run: impl FnOnce() -> T) -> T {
    let _serialized = TEST_SLOT.lock().unwrap_or_else(|error| error.into_inner());
    std::env::set_var("MEMRIGHT_FEDERATION_SCRIPT", script);
    let result = run();
    std::env::remove_var("MEMRIGHT_FEDERATION_SCRIPT");
    result
}

#[test]
fn federate_route_returns_envelope_with_transport_telemetry() {
    let dir = tempfile::tempdir().unwrap();
    let script = fixture_worker(dir.path());
    let store = memright::MemoryStore::new();
    let body = serde_json::json!({
        "task": "route probe",
        "repo": dir.path(),
        "maxTokens": 512,
        "maxWaitMs": 10_000,
    })
    .to_string();

    let (status, payload) = with_fixture_env(&script, || {
        memright::serve::route_for_tests(&store, "POST", "/federate", &body)
    });

    assert_eq!(status, 200, "{payload}");
    let envelope: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(envelope["transport"], "resident");
    assert!(envelope["workerRestarts"].as_u64().unwrap() >= 1);
    assert!(envelope["packet"].is_object(), "planner packet present");
    assert!(envelope["receipts"].is_array());
    assert!(
        envelope["stageElapsedMs"]["gateway_process"].is_number(),
        "worker roundtrip time recorded"
    );
}

#[test]
fn federate_route_second_call_reuses_the_worker() {
    let dir = tempfile::tempdir().unwrap();
    let script = fixture_worker(dir.path());
    let store = memright::MemoryStore::new();
    let body = serde_json::json!({
        "task": "warm reuse",
        "repo": dir.path(),
        "maxWaitMs": 10_000,
    })
    .to_string();

    let (first, second) = with_fixture_env(&script, || {
        let first = memright::serve::route_for_tests(&store, "POST", "/federate", &body);
        let second = memright::serve::route_for_tests(&store, "POST", "/federate", &body);
        (first, second)
    });

    assert_eq!(first.0, 200, "{}", first.1);
    assert_eq!(second.0, 200, "{}", second.1);
    let restarts = |payload: &str| {
        serde_json::from_str::<serde_json::Value>(payload).unwrap()["workerRestarts"]
            .as_u64()
            .unwrap()
    };
    assert_eq!(
        restarts(&first.1),
        restarts(&second.1),
        "second request must not respawn the worker"
    );
}

#[test]
fn federate_route_missing_fields_are_400_and_dead_worker_is_502() {
    let store = memright::MemoryStore::new();
    let (status, _) = memright::serve::route_for_tests(&store, "POST", "/federate", "{}");
    assert_eq!(status, 400);

    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("gateway.py");
    std::fs::write(&script, "import sys; sys.exit(5)\n").unwrap();
    let body = serde_json::json!({
        "task": "doomed",
        "repo": dir.path(),
        "maxWaitMs": 2_000,
    })
    .to_string();
    let (status, payload) = with_fixture_env(&script, || {
        memright::serve::route_for_tests(&store, "POST", "/federate", &body)
    });
    assert_eq!(status, 502, "{payload}");
    assert!(payload.contains("resident federation failed"), "{payload}");
}
