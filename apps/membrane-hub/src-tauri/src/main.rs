#![cfg_attr(windows, windows_subsystem = "windows")]

//! On-demand dashboard process for Architecture B.
//!
//! The resident native tray owns Membrane's daemon lifecycle. This Tauri
//! process owns presentation only: it accepts one inherited bootstrap frame,
//! keeps its bearer in native memory, and proxies read-only loopback calls.

use membrane_protocol::{HubSnapshotV1, HUB_SCHEMA_VERSION};
use serde::{Deserialize, Serialize};
use std::{
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::Manager;

mod dashboard_connection;
mod hub_telemetry;
mod update_admission;

use dashboard_connection::{DashboardConnection, DashboardConnectionState};

const SNAPSHOT_SCHEMA_VERSION: u32 = 1;
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
const MAX_SNAPSHOT_BYTES: usize = dashboard_connection::HTTP_MAX_RESPONSE_BYTES;

type ConnectionState = Arc<Mutex<DashboardConnectionState>>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CachedSnapshot {
    pub schema_version: u32,
    pub observed_at_unix_ms: u64,
    pub payload: serde_json::Value,
}

fn parse_snapshot(bytes: &[u8]) -> Result<CachedSnapshot, String> {
    if bytes.len() > MAX_SNAPSHOT_BYTES {
        return Err("snapshot_too_large".into());
    }
    let envelope: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "snapshot_invalid_json")?;
    let payload = envelope
        .get("result")
        .and_then(|result| result.get("data"))
        .cloned()
        .unwrap_or(envelope);
    if payload
        .get("schemaVersion")
        .and_then(serde_json::Value::as_u64)
        != Some(HUB_SCHEMA_VERSION as u64)
    {
        return Err("snapshot_schema_unsupported".into());
    }
    let snapshot: HubSnapshotV1 =
        serde_json::from_value(payload).map_err(|_| "snapshot_schema_invalid")?;
    if snapshot.schema_version != HUB_SCHEMA_VERSION {
        return Err("snapshot_schema_unsupported".into());
    }
    let observed = snapshot.observed_at_unix_ms;
    let payload = serde_json::to_value(snapshot).map_err(|_| "snapshot_schema_invalid")?;
    Ok(CachedSnapshot {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        observed_at_unix_ms: observed,
        payload,
    })
}

fn fetch_snapshot(
    connection: &DashboardConnection,
    timeout: std::time::Duration,
) -> Result<CachedSnapshot, String> {
    let response = connection.get("/hub/snapshot", timeout)?;
    if response.status != 200 {
        return Err("dashboard_snapshot_unavailable".into());
    }
    parse_snapshot(&response.body)
}

fn health_diagnostics(
    connection: &DashboardConnection,
    timeout: std::time::Duration,
) -> Result<hub_telemetry::HubDiagnostics, String> {
    let response = connection.get("/health", timeout)?;
    let health: serde_json::Value =
        serde_json::from_slice(&response.body).map_err(|_| "dashboard_health_invalid")?;
    let healthy = response.status == 200
        && health.get("ok").and_then(serde_json::Value::as_bool) == Some(true);
    let reason = health
        .get("error")
        .and_then(serde_json::Value::as_str)
        .or_else(|| health.get("reason").and_then(serde_json::Value::as_str))
        .map(str::to_owned)
        .or_else(|| (!healthy).then(|| "resident_health_unavailable".into()));
    Ok(hub_telemetry::HubDiagnostics {
        schema_version: 1,
        product_id: "membrane",
        owner: "dashboard",
        service_identity: "membrane-daemon-v1",
        service_state: if healthy { "running" } else { "degraded" }.into(),
        snapshot_state: if response.status == 200 {
            "available"
        } else {
            "unavailable"
        }
        .into(),
        last_reason: reason,
        observed_at_unix_ms: health
            .get("observedAtUnixMs")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_else(now_ms),
        // Dashboard has no resident log. Keep fields for command-shape
        // compatibility while avoiding a local dashboard-owned log path.
        log_path: String::new(),
        resident_log_path: String::new(),
    })
}

fn connection_from_state(state: &ConnectionState) -> Result<DashboardConnection, String> {
    state
        .lock()
        .map_err(|_| "dashboard_connection_unavailable".to_string())?
        .connection()
}

#[tauri::command]
fn snapshot(state: tauri::State<'_, ConnectionState>) -> Result<CachedSnapshot, String> {
    let connection = connection_from_state(state.inner())?;
    fetch_snapshot(&connection, REQUEST_TIMEOUT)
}

#[tauri::command]
fn diagnostics_report(
    state: tauri::State<'_, ConnectionState>,
) -> Result<hub_telemetry::HubDiagnostics, String> {
    let connection = connection_from_state(state.inner())?;
    health_diagnostics(&connection, REQUEST_TIMEOUT)
}

/// Startup registration belongs to resident native tray. Keep command names
/// so older dashboard bundles fail closed with a typed reason instead of
/// trying to register the dashboard as a second startup owner.
#[tauri::command]
fn set_startup(_enabled: bool) -> Result<(), String> {
    Err("startup_owned_by_tray".into())
}

#[tauri::command]
fn startup_setting() -> Result<bool, String> {
    Err("startup_owned_by_tray".into())
}

fn show_dashboard(app: &tauri::AppHandle) -> Result<(), String> {
    let dashboard = app
        .get_webview_window("dashboard")
        .ok_or_else(|| "dashboard_unavailable".to_string())?;
    dashboard
        .show()
        .map_err(|_| "dashboard_show_failed".to_string())?;
    dashboard
        .set_focus()
        .map_err(|_| "dashboard_focus_failed".to_string())
}

#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    // The tray/daemon are separate processes. Dashboard close or explicit
    // quit must never attempt to stop either resident owner.
    app.exit(0);
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            let _ = show_dashboard(app);
        }))
        .invoke_handler(tauri::generate_handler![
            snapshot,
            set_startup,
            startup_setting,
            diagnostics_report,
            quit_app
        ])
        .setup(|app| {
            // The native tray launches this process with one inherited pipe.
            // The parser retains endpoint/token only in native state; setup
            // never places either value in JS, an env var, or a file.
            let connection = DashboardConnectionState::from_stdin();
            app.manage(Arc::new(Mutex::new(connection)));
            // Dashboard is intentionally visible on process launch. A tray
            // action owns process creation; this setup owns no startup route.
            let _ = show_dashboard(app.handle());
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("build Membrane Hub dashboard")
        .run(|_, _| {});
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canonical_payload(overrides: &[(&str, &str)]) -> serde_json::Value {
        let mut sections = serde_json::Map::new();
        for name in [
            "deliveries",
            "providers",
            "repositories",
            "adapters",
            "devices",
            "memory",
            "sentinel",
            "alerts",
        ] {
            let state = overrides
                .iter()
                .find_map(|(key, state)| (*key == name).then_some(*state))
                .unwrap_or("available");
            sections.insert(
                name.into(),
                serde_json::json!({
                    "state": state,
                    "reason": format!("{name}_{state}"),
                    "items": [],
                    "resolver": null,
                    "evidence": null,
                    "observedAtUnixMs": 1,
                }),
            );
        }
        serde_json::json!({
            "schemaVersion": 1,
            "productId": "membrane",
            "observedAtUnixMs": 1,
            "sections": sections,
        })
    }

    #[test]
    fn bounded_parser_rejects_bad_schema_and_oversize() {
        assert_eq!(
            parse_snapshot(br#"{"schemaVersion":2}"#).unwrap_err(),
            "snapshot_schema_unsupported"
        );
        assert_eq!(
            parse_snapshot(&vec![b'x'; MAX_SNAPSHOT_BYTES + 1]).unwrap_err(),
            "snapshot_too_large"
        );
    }

    #[test]
    fn parser_accepts_hub_operation_envelope() {
        let snapshot = parse_snapshot(
            &serde_json::to_vec(&serde_json::json!({
                "schemaVersion": 1,
                "result": { "kind": "success", "data": canonical_payload(&[]) }
            }))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(snapshot.observed_at_unix_ms, 1);
        assert_eq!(
            snapshot.payload["sections"]["deliveries"]["state"],
            "available"
        );
    }

    #[test]
    fn production_setup_has_no_resident_runtime_tray_or_startup_owner() {
        let source = include_str!("main.rs");
        let production = source
            .split("#[cfg(test)]")
            .next()
            .expect("production source");
        assert!(production.contains("DashboardConnectionState::from_stdin()"));
        assert!(!production.contains("run_hub_runtime"));
        assert!(!production.contains("TrayIconBuilder"));
        assert!(!production.contains("std::thread::spawn"));
        assert!(!production.contains("supervisor.start"));
        assert!(!production.contains("set_platform_startup"));
        assert!(!production.contains("api-token"));
        assert!(!production.contains("WORKSPACE_ROOT"));
        let manifest = include_str!("../Cargo.toml");
        assert!(!manifest.contains("membrane-runtime"));
        assert!(!manifest.contains("tray-icon"));
        assert!(production.contains("startup_owned_by_tray"));
        assert!(production.contains("let _ = show_dashboard(app.handle())"));
    }

    #[test]
    fn dashboard_window_is_single_visible_on_demand_surface() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let windows = config["app"]["windows"].as_array().unwrap();
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0]["label"], "dashboard");
        assert_eq!(windows[0]["visible"], true);
        assert_eq!(windows[0]["skipTaskbar"], false);
    }

    #[test]
    fn dashboard_close_path_does_not_stop_resident_owners() {
        let source = include_str!("main.rs");
        let production = source
            .split("#[cfg(test)]")
            .next()
            .expect("production source");
        assert!(!production.contains("CloseRequested"));
        assert!(!production.contains("stop_membrane_service"));
        assert!(!production.contains("stop_blueprint_service"));
        assert!(production.contains("app.exit(0)"));
    }
}
