use super::*;

const SECTIONS: [&str; 8] = [
    "deliveries",
    "providers",
    "repositories",
    "adapters",
    "devices",
    "memory",
    "sentinel",
    "alerts",
];
fn snapshot(state: &str, reason: &str) -> CachedSnapshot {
    let mut sections = serde_json::Map::new();
    for key in SECTIONS {
        sections.insert(key.into(), serde_json::json!({"state":state,"reason":reason,"items":[],"resolver":null,"evidence":null,"observedAtUnixMs":1}));
    }
    let value = serde_json::json!({"schemaVersion":1,"productId":"membrane","observedAtUnixMs":1,"sections":sections});
    CachedSnapshot {
        schema_version: 1,
        observed_at_unix_ms: 1,
        payload: value,
    }
}
#[test]
fn gate_is_passive_and_predicate_is_exact() {
    let sentinel = snapshot("unavailable", "source_not_connected");
    let gate = StartupGate::new();
    assert!(!gate.masks(None));
    assert!(gate.masks(Some(&sentinel)));
    assert!(gate.masks(Some(&sentinel)));
    let mut wrong = sentinel.clone();
    wrong.payload["sections"]["alerts"]["state"] = serde_json::json!("degraded");
    assert!(!source_not_connected_snapshot(&wrong));
    assert!(!gate.masks(Some(&wrong)));
    gate.finish();
    assert!(!gate.masks(Some(&sentinel)));
}
#[test]
fn startup_step_masks_then_caches_live_or_deadline_sentinel() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("snapshot.json");
    let mut latest = None;
    assert!(startup_step(&path, Err("down".into()), false, &mut latest).is_none());
    assert!(!path.exists());
    let sentinel = snapshot("unavailable", "source_not_connected");
    assert!(startup_step(&path, Ok(sentinel.clone()), false, &mut latest).is_none());
    assert!(!path.exists());
    let live = snapshot("available", "ready");
    assert_eq!(
        startup_step(&path, Ok(live.clone()), false, &mut latest)
            .unwrap()
            .unwrap(),
        live
    );
    let mut deadline_latest = Some(sentinel.clone());
    let deadline = startup_step(&path, Err("down".into()), true, &mut deadline_latest)
        .unwrap()
        .unwrap();
    assert_eq!(deadline, sentinel);
}
#[test]
fn tray_matrix_depends_only_on_resident_and_snapshot() {
    assert_eq!(
        tray_status(supervisor::ServiceStatus::Running, Some(true), true),
        TrayStatus::Running
    );
    assert_eq!(
        tray_status(supervisor::ServiceStatus::Running, Some(true), false),
        TrayStatus::Degraded
    );
    assert_eq!(
        tray_status(supervisor::ServiceStatus::Running, Some(false), true),
        TrayStatus::Degraded
    );
    assert_eq!(
        tray_status(supervisor::ServiceStatus::Running, None, true),
        TrayStatus::Offline
    );
    assert_eq!(
        tray_status(supervisor::ServiceStatus::Unavailable, Some(true), true),
        TrayStatus::Offline
    );
    assert_eq!(
        tray_status(supervisor::ServiceStatus::CrashLoop, Some(true), false),
        TrayStatus::Offline
    );
}
