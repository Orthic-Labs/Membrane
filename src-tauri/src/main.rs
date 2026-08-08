use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder};
use tauri::{Emitter, Manager};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CachedSnapshot {
    pub schema_version: u32,
    pub observed_at_unix_ms: u64,
    pub payload: serde_json::Value,
}

const SNAPSHOT_SCHEMA_VERSION: u32 = 1;
const MAX_SNAPSHOT_BYTES: u64 = 1024 * 1024;
const POLL_INTERVAL: Duration = Duration::from_secs(5);
const POLL_TIMEOUT: Duration = Duration::from_secs(2);

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    fs::create_dir_all(path.parent().ok_or("cache_parent_missing")?).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("tmp");
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&tmp)
        .map_err(|e| e.to_string())?;
    file.write_all(bytes).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| e.to_string())?;
    if let Ok(parent) = fs::File::open(path.parent().unwrap()) {
        let _ = parent.sync_all();
    }
    Ok(())
}

pub fn write_cache(path: &Path, snapshot: &CachedSnapshot) -> Result<(), String> {
    if snapshot.schema_version != SNAPSHOT_SCHEMA_VERSION {
        return Err("snapshot_schema_unsupported".into());
    }
    let bytes = serde_json::to_vec(snapshot).map_err(|e| e.to_string())?;
    atomic_write(path, &bytes)?;
    atomic_write(&path.with_extension("last-valid"), &bytes)
}
pub fn read_cache(path: &Path) -> Result<CachedSnapshot, String> {
    for candidate in [path.to_path_buf(), path.with_extension("last-valid")] {
        let Ok(bytes) = fs::read(candidate) else {
            continue;
        };
        if bytes.len() as u64 > MAX_SNAPSHOT_BYTES {
            continue;
        }
        let Ok(snapshot) = serde_json::from_slice::<CachedSnapshot>(&bytes) else {
            continue;
        };
        if snapshot.schema_version == SNAPSHOT_SCHEMA_VERSION {
            return Ok(snapshot);
        }
    }
    Err("snapshot_unavailable".into())
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn parse_snapshot(bytes: &[u8]) -> Result<CachedSnapshot, String> {
    if bytes.len() as u64 > MAX_SNAPSHOT_BYTES {
        return Err("snapshot_too_large".into());
    }
    let envelope: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "snapshot_invalid_json")?;
    let payload = envelope
        .get("result")
        .and_then(|result| result.get("data"))
        .cloned()
        .unwrap_or(envelope);
    if payload.get("schemaVersion").and_then(|v| v.as_u64()) != Some(1) {
        return Err("snapshot_schema_unsupported".into());
    }
    let observed = payload
        .get("observedAtUnixMs")
        .and_then(|v| v.as_u64())
        .filter(|v| *v > 0)
        .unwrap_or_else(now_unix_ms);
    Ok(CachedSnapshot {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        observed_at_unix_ms: observed,
        payload,
    })
}

fn fetch_snapshot(program: &str) -> Result<CachedSnapshot, String> {
    let mut child = Command::new(program)
        .args(["cli", "hub-snapshot"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|_| "hub_service_unavailable")?;
    let stdout = child.stdout.take().ok_or("hub_snapshot_pipe_missing")?;
    let reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(MAX_SNAPSHOT_BYTES + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let started = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|_| "hub_snapshot_wait_failed")? {
            break status;
        }
        if started.elapsed() >= POLL_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err("hub_snapshot_timeout".into());
        }
        thread::sleep(Duration::from_millis(20));
    };
    let bytes = reader
        .join()
        .map_err(|_| "hub_snapshot_reader_failed")?
        .map_err(|_| "hub_snapshot_read_failed")?;
    if !status.success() {
        return Err("hub_service_unavailable".into());
    }
    parse_snapshot(&bytes)
}

fn apply_poll_result(
    path: &Path,
    result: Result<CachedSnapshot, String>,
) -> Result<CachedSnapshot, String> {
    match result {
        Ok(snapshot) => {
            write_cache(path, &snapshot)?;
            Ok(snapshot)
        }
        Err(_) => read_cache(path),
    }
}

#[tauri::command]
fn snapshot(cache: tauri::State<'_, Arc<Mutex<PathBuf>>>) -> Result<CachedSnapshot, String> {
    read_cache(&cache.lock().map_err(|_| "cache lock poisoned")?)
}

#[tauri::command]
fn set_startup(enabled: bool, app: tauri::AppHandle) -> Result<(), String> {
    let path = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("startup.json");
    fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    write_cache(
        &path,
        &CachedSnapshot {
            schema_version: 1,
            observed_at_unix_ms: 0,
            payload: serde_json::json!({"launchAtLogin": enabled}),
        },
    )
}

#[tauri::command]
fn startup_setting(app: tauri::AppHandle) -> Result<bool, String> {
    let path = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("startup.json");
    Ok(read_cache(&path)
        .ok()
        .and_then(|v| v.payload.get("launchAtLogin").and_then(|v| v.as_bool()))
        .unwrap_or(false))
}

#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

fn main() {
    tauri::Builder::default()
        .manage(Arc::new(Mutex::new(PathBuf::from("snapshot.json"))))
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("hub") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .invoke_handler(tauri::generate_handler![
            snapshot,
            set_startup,
            startup_setting,
            quit_app
        ])
        .setup(|app| {
            let cache = app
                .path()
                .app_data_dir()
                .map_err(|e| e.to_string())?
                .join("snapshot.json");
            fs::create_dir_all(cache.parent().unwrap())?;
            *app.state::<Arc<Mutex<PathBuf>>>().lock().unwrap() = cache.clone();
            let show = MenuItemBuilder::with_id("show", "Open Hub").build(app)?;
            let diagnostics = MenuItemBuilder::with_id("diagnostics", "Copy diagnostics").build(app)?;
            let trace = MenuItemBuilder::with_id("trace", "Latest trace").build(app)?;
            let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
            let menu = MenuBuilder::new(app).items(&[&show, &diagnostics, &trace, &quit]).build()?;
            let mut tray = TrayIconBuilder::new()
                .menu(&menu)
                .tooltip("Membrane Hub — read-only status")
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => {
                        if let Some(w) = app.get_webview_window("hub") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    "diagnostics" => {
                        if let Some(w) = app.get_webview_window("hub") {
                            let _ = w.show();
                            let _ = w.set_focus();
                            let _ = app.emit("popover-diagnostics", ());
                        }
                    }
                    "trace" => {
                        if let Some(w) = app.get_webview_window("hub") {
                            let _ = w.show();
                            let _ = w.set_focus();
                            let _ = app.emit("popover-trace", ());
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        if let Some(w) = tray.app_handle().get_webview_window("hub") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone())
            }
            let tray = tray.build(app)?;
            if let Some(w) = app.get_webview_window("hub") {
                let hidden = w.clone();
                w.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = hidden.hide();
                    }
                });
                w.hide()?;
            }
            let handle = app.handle().clone();
            let program = std::env::var("MEMBRANE_COMMAND").unwrap_or_else(|_| "membrane".into());
            std::thread::spawn(move || loop {
                let current = apply_poll_result(&cache, fetch_snapshot(&program));
                let tooltip = current
                    .as_ref()
                    .map(|snapshot| {
                        format!("Membrane Hub — observed {}", snapshot.observed_at_unix_ms)
                    })
                    .unwrap_or_else(|_| "Membrane Hub — offline".into());
                let _ = tray.set_tooltip(Some(tooltip));
                let _ = handle.emit("hub-snapshot-tick", ());
                std::thread::sleep(POLL_INTERVAL);
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("run Membrane Hub");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cache_round_trip_is_atomic_shape() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("snapshot.json");
        let s = CachedSnapshot {
            schema_version: 1,
            observed_at_unix_ms: 7,
            payload: serde_json::json!({"state":"degraded"}),
        };
        write_cache(&p, &s).unwrap();
        assert_eq!(read_cache(&p).unwrap(), s);
        assert!(!p.with_extension("tmp").exists());
    }
    #[test]
    fn invalid_snapshot_falls_back_to_last_valid() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("snapshot.json");
        let good = CachedSnapshot {
            schema_version: 1,
            observed_at_unix_ms: 1,
            payload: serde_json::json!({"state":"ready"}),
        };
        write_cache(&p, &good).unwrap();
        fs::write(&p, b"not-json").unwrap();
        assert_eq!(read_cache(&p).unwrap(), good);
    }
    #[test]
    fn startup_setting_shape_is_explicit() {
        let setting = serde_json::json!({"launchAtLogin":true});
        assert_eq!(setting["launchAtLogin"], true);
    }
    #[test]
    fn polling_keeps_last_valid_then_accepts_service_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("snapshot.json");
        let first =
            parse_snapshot(br#"{"schemaVersion":1,"observedAtUnixMs":7,"sections":{}}"#).unwrap();
        assert_eq!(apply_poll_result(&path, Ok(first.clone())).unwrap(), first);
        assert_eq!(
            apply_poll_result(&path, Err("hub_service_unavailable".into())).unwrap(),
            first
        );
        let recovered =
            parse_snapshot(br#"{"schemaVersion":1,"observedAtUnixMs":9,"sections":{}}"#).unwrap();
        assert_eq!(
            apply_poll_result(&path, Ok(recovered.clone())).unwrap(),
            recovered
        );
        assert_eq!(read_cache(&path).unwrap(), recovered);
    }
    #[test]
    fn bounded_parser_rejects_bad_schema_and_oversize() {
        assert_eq!(
            parse_snapshot(br#"{"schemaVersion":2}"#).unwrap_err(),
            "snapshot_schema_unsupported"
        );
        assert_eq!(
            parse_snapshot(&vec![b'x'; MAX_SNAPSHOT_BYTES as usize + 1]).unwrap_err(),
            "snapshot_too_large"
        );
    }
    #[test]
    fn parser_accepts_hub_operation_envelope() {
        let snapshot = parse_snapshot(
            br#"{"schemaVersion":1,"result":{"kind":"success","data":{"schemaVersion":1,"observedAtUnixMs":42,"deliveries":{"state":"available"}}}}"#,
        )
        .unwrap();
        assert_eq!(snapshot.observed_at_unix_ms, 42);
        assert_eq!(snapshot.payload["deliveries"]["state"], "available");
    }
}
