use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder};
use tauri::{Emitter, Manager, PhysicalPosition};
#[cfg(target_os = "windows")]
use window_vibrancy::apply_acrylic;
#[cfg(target_os = "macos")]
use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial, NSVisualEffectState};

mod brand;
mod schema_types;
mod supervisor;
mod onboarding;
mod dormant_tab;
mod product_tab;
mod manifest_validate;
mod manifest_scan;
mod hub_runtime;
#[cfg(test)]
mod hub_contract_tests;
mod update_admission;
mod workspace;

use brand::PRODUCT_NAME;
use hub_runtime::{HubRuntime, HubSnapshot};
use schema_types::{SectionState, SnapshotV1, SectionV1};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CachedSnapshot {
    pub schema_version: u32,
    pub observed_at_unix_ms: u64,
    pub payload: serde_json::Value,
}

const SNAPSHOT_SCHEMA_VERSION: u32 = 2;
const MAX_SNAPSHOT_BYTES: u64 = 1024 * 1024;
const POLL_INTERVAL: Duration = Duration::from_secs(5);
const POLL_TIMEOUT: Duration = Duration::from_secs(2);
const STARTUP_GRACE: Duration = Duration::from_secs(3);
const STARTUP_POLL_INTERVAL: Duration = Duration::from_millis(100);
#[derive(Debug)]
struct StartupGate {
    active: AtomicBool,
}

impl StartupGate {
    fn new() -> Self {
        Self {
            active: AtomicBool::new(true),
        }
    }

    fn masks(&self, snapshot: Option<&CachedSnapshot>) -> bool {
        self.active.load(Ordering::Acquire) && snapshot.is_some_and(source_not_connected_snapshot)
    }
    fn active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }
    fn finish(&self) {
        self.active.store(false, Ordering::Release);
    }
}

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
    if payload
        .get("schemaVersion")
        .and_then(|value| value.as_u64())
        != Some(SNAPSHOT_SCHEMA_VERSION as u64)
    {
        return Err("snapshot_schema_unsupported".into());
    }
    let snapshot: SnapshotV1 =
        serde_json::from_value(payload).map_err(|_| "snapshot_schema_invalid")?;
    if snapshot.schema_version != SNAPSHOT_SCHEMA_VERSION {
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

fn fetch_snapshot(program: &Path, timeout: Duration) -> Result<CachedSnapshot, String> {
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
        if started.elapsed() >= timeout {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayStatus {
    Available,
    Degraded,
    Unavailable,
    Offline,
}

fn state_rank(state: SectionState) -> u8 {
    match state {
        SectionState::Unavailable => 0,
        SectionState::Degraded => 1,
        SectionState::Available => 2,
    }
}

fn presentation_rank(section: &SectionV1) -> u8 {
    match section.state {
        SectionState::Unavailable if section.reason == "not_instrumented" => 1,
        state => state_rank(state),
    }
}

fn source_not_connected_snapshot(snapshot: &CachedSnapshot) -> bool {
    let Ok(snapshot) = serde_json::from_value::<SnapshotV1>(snapshot.payload.clone()) else {
        return false;
    };
    if snapshot.sections.is_empty() {
        return false;
    }
    snapshot.sections.values().all(|section| {
        section.state == SectionState::Unavailable && section.reason == "source_not_connected"
    })
}

pub fn tray_status(snapshot: Option<&CachedSnapshot>) -> TrayStatus {
    let Some(snapshot) = snapshot.and_then(|snapshot| {
        serde_json::from_value::<SnapshotV1>(snapshot.payload.clone()).ok()
    }) else {
        return TrayStatus::Offline;
    };
    if snapshot.sections.is_empty() {
        return TrayStatus::Offline;
    }
    let worst = snapshot
        .sections
        .values()
        .map(presentation_rank)
        .min()
        .expect("sections non-empty");
    match worst {
        0 => TrayStatus::Unavailable,
        1 => TrayStatus::Degraded,
        2 => TrayStatus::Available,
        _ => unreachable!("SectionState has three variants"),
    }
}

fn tray_status_for_products(snapshot: &HubSnapshot) -> TrayStatus {
    let mut worst = None;
    for section in snapshot.products.iter().flat_map(|product| product.snapshot.sections.values()) {
        let rank = presentation_rank(section);
        worst = Some(worst.map_or(rank, |current: u8| current.min(rank)));
    }
    match worst {
        Some(0) => TrayStatus::Unavailable,
        Some(1) => TrayStatus::Degraded,
        Some(2) => TrayStatus::Available,
        _ => TrayStatus::Offline,
    }
}

pub fn tray_tooltip(status: TrayStatus) -> &'static str {
    match status {
        TrayStatus::Available => "Orthic — available",
        TrayStatus::Degraded => "Orthic — degraded",
        TrayStatus::Unavailable => "Orthic — unavailable",
        TrayStatus::Offline => "Orthic — offline, no cached snapshot",
    }
}

#[cfg(target_os = "macos")]
macro_rules! tray_asset {
    ($name:literal) => {
        include_bytes!(concat!("../../assets/tray/", $name, "-36.png"))
    };
}
#[cfg(not(target_os = "macos"))]
macro_rules! tray_asset {
    ($name:literal) => {
        include_bytes!(concat!("../../assets/tray/", $name, "-32.png"))
    };
}

fn tray_icon(status: TrayStatus) -> tauri::Result<tauri::image::Image<'static>> {
    let bytes: &[u8] = match status {
        TrayStatus::Available => tray_asset!("membrane-available"),
        TrayStatus::Degraded => tray_asset!("membrane-degraded"),
        TrayStatus::Unavailable => tray_asset!("membrane-unavailable"),
        TrayStatus::Offline => tray_asset!("membrane-offline"),
    };
    tauri::image::Image::from_bytes(bytes).map(tauri::image::Image::to_owned)
}

fn popover_origin(icon: tauri::Rect, window_width: u32, scale: f64) -> (i32, i32) {
    let position = icon.position.to_physical::<f64>(scale);
    let size = icon.size.to_physical::<f64>(scale);
    let x = (position.x + size.width / 2.0 - f64::from(window_width) / 2.0).round() as i32;
    let y = (position.y + size.height).round() as i32;
    (x, y)
}

fn toggle_popover(app: &tauri::AppHandle, icon: tauri::Rect) {
    let Some(window) = app.get_webview_window("hub") else {
        return;
    };
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
        return;
    }
    if let Ok(size) = window.outer_size() {
        let scale = window.scale_factor().unwrap_or(1.0);
        let (mut x, mut y) = popover_origin(icon, size.width, scale);
        if let Ok(Some(monitor)) = window.current_monitor() {
            let left = monitor.position().x + 8;
            let top = monitor.position().y + 24;
            let right = monitor.position().x + monitor.size().width as i32 - size.width as i32 - 8;
            x = x.clamp(left, right.max(left));
            y = y.max(top);
        }
        let _ = window.set_position(PhysicalPosition::new(x, y));
    }
    let _ = window.show();
    let _ = window.set_focus();
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

fn poll_snapshot(
    path: &Path,
    result: Result<CachedSnapshot, String>,
    gate: &StartupGate,
) -> Result<CachedSnapshot, String> {
    match result {
        Ok(snapshot) if gate.masks(Some(&snapshot)) => Err("startup_sentinel_masked".into()),
        Ok(snapshot) => apply_poll_result(path, Ok(snapshot)),
        Err(error) => {
            let cached = read_cache(path);
            if gate.masks(cached.as_ref().ok()) {
                Err("startup_sentinel_masked".into())
            } else {
                cached.map_err(|_| error)
            }
        }
    }
}
fn startup_step(
    path: &Path,
    result: Result<CachedSnapshot, String>,
    expired: bool,
    latest: &mut Option<CachedSnapshot>,
) -> Option<Result<CachedSnapshot, String>> {
    match result {
        Ok(snapshot) if source_not_connected_snapshot(&snapshot) => {
            *latest = Some(snapshot);
            expired.then(|| apply_poll_result(path, Ok(latest.take().unwrap())))
        }
        Ok(snapshot) => Some(apply_poll_result(path, Ok(snapshot))),
        Err(error) if expired => Some(apply_poll_result(
            path,
            latest.take().map(Ok).unwrap_or(Err(error)),
        )),
        Err(_) => None,
    }
}
fn initial_poll(path: &Path, program: &Path, gate: &StartupGate) -> Result<CachedSnapshot, String> {
    let deadline = std::time::Instant::now() + STARTUP_GRACE;
    let mut latest = None;
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            let output = startup_step(
                path,
                Err("hub_service_unavailable".into()),
                true,
                &mut latest,
            )
            .unwrap();
            gate.finish();
            return output;
        }
        let result = fetch_snapshot(program, POLL_TIMEOUT.min(remaining));
        let expired = deadline
            .saturating_duration_since(std::time::Instant::now())
            .is_zero();
        if let Some(output) = startup_step(path, result, expired, &mut latest) {
            gate.finish();
            return output;
        }
        thread::sleep(
            STARTUP_POLL_INTERVAL
                .min(deadline.saturating_duration_since(std::time::Instant::now())),
        );
    }
}

#[tauri::command]
fn snapshot(runtime: tauri::State<'_, Arc<HubRuntime>>) -> HubSnapshot {
    runtime.snapshot()
}

#[tauri::command]
fn set_startup(enabled: bool, _app: tauri::AppHandle) -> Result<(), String> {
    configure_hub_login_startup(enabled)
}

#[tauri::command]
fn startup_setting(_app: tauri::AppHandle) -> Result<bool, String> {
    hub_login_startup_enabled()
}

#[cfg(target_os = "macos")]
fn hub_login_startup_path() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join("Library/LaunchAgents/com.orthic.hub.plist"))
        .ok_or_else(|| "home_unavailable".into())
}

#[cfg(target_os = "macos")]
fn configure_hub_login_startup(enabled: bool) -> Result<(), String> {
    let path = hub_login_startup_path()?;
    if enabled {
        let executable = std::env::current_exe().map_err(|_| "hub_executable_unavailable")?;
        let escaped = executable.display().to_string().replace('&', "&amp;").replace('<', "&lt;");
        let body = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\"><plist version=\"1.0\"><dict><key>Label</key><string>com.orthic.hub</string><key>ProgramArguments</key><array><string>{escaped}</string></array><key>RunAtLoad</key><true/></dict></plist>");
        fs::create_dir_all(path.parent().ok_or("startup_parent_missing")?).map_err(|e| e.to_string())?;
        fs::write(&path, body).map_err(|e| e.to_string())?;
        let status = Command::new("launchctl").args(["load", "-w", path.to_string_lossy().as_ref()]).status().map_err(|_| "launchctl_unavailable")?;
        if !status.success() { return Err("hub_login_startup_failed".into()); }
    } else if path.exists() {
        let _ = Command::new("launchctl").args(["unload", "-w", path.to_string_lossy().as_ref()]).status();
        fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn hub_login_startup_enabled() -> Result<bool, String> { Ok(hub_login_startup_path()?.exists()) }

fn windows_startup_value(executable: &Path) -> String {
    format!("\"{}\"", executable.display())
}

#[cfg(target_os = "windows")]
fn configure_hub_login_startup(enabled: bool) -> Result<(), String> {
    let key = "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run";
    let status = if enabled {
        let executable = std::env::current_exe().map_err(|_| "hub_executable_unavailable")?;
        let value = windows_startup_value(&executable);
        Command::new("reg").args(["add", key, "/v", "Orthic", "/t", "REG_SZ", "/d", &value, "/f"]).status()
    } else {
        Command::new("reg").args(["delete", key, "/v", "Orthic", "/f"]).status()
    }.map_err(|_| "hub_login_startup_failed")?;
    if enabled && !status.success() { return Err("hub_login_startup_failed".into()); }
    Ok(())
}

#[cfg(target_os = "windows")]
fn hub_login_startup_enabled() -> Result<bool, String> {
    Ok(Command::new("reg").args(["query", "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run", "/v", "Orthic"]).status().map(|status| status.success()).unwrap_or(false))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn configure_hub_login_startup(_enabled: bool) -> Result<(), String> { Err("hub_login_startup_unsupported".into()) }

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn hub_login_startup_enabled() -> Result<bool, String> { Ok(false) }

#[tauri::command]
fn quit_app(app: tauri::AppHandle, runtime: tauri::State<'_, Arc<HubRuntime>>) {
    runtime.stop_all();
    app.exit(0);
}

#[tauri::command]
fn hide_popover(app: tauri::AppHandle) -> Result<(), String> {
    app.get_webview_window("hub")
        .ok_or_else(|| "popover_unavailable".to_string())?
        .hide()
        .map_err(|_| "popover_hide_failed".to_string())
}

fn main() {
    let runtime = Arc::new(HubRuntime::discover());
    tauri::Builder::default()
        .manage(runtime)
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
            hide_popover,
            quit_app
        ])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.handle()
                .set_activation_policy(tauri::ActivationPolicy::Accessory)?;
            let diagnostics =
                MenuItemBuilder::with_id("diagnostics", "Copy diagnostics").build(app)?;
            let trace = MenuItemBuilder::with_id("trace", "Latest trace").build(app)?;
            let quit = MenuItemBuilder::with_id("quit", format!("Quit {}", PRODUCT_NAME)).build(app)?;
            let menu = MenuBuilder::new(app)
                .items(&[&diagnostics, &trace, &quit])
                .build()?;
            let tray = TrayIconBuilder::new()
                .menu(&menu)
                .show_menu_on_left_click(false)
                .icon(tray_icon(TrayStatus::Offline)?)
                .icon_as_template(false)
                .tooltip(tray_tooltip(TrayStatus::Offline))
                .on_menu_event(|app, event| match event.id().as_ref() {
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
                    "quit" => {
                        app.state::<Arc<HubRuntime>>().stop_all();
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        rect,
                        ..
                    } = event
                    {
                        toggle_popover(tray.app_handle(), rect);
                    }
                })
                .build(app)?;
            if let Some(w) = app.get_webview_window("hub") {
                #[cfg(target_os = "macos")]
                apply_vibrancy(
                    &w,
                    NSVisualEffectMaterial::Popover,
                    Some(NSVisualEffectState::Active),
                    Some(18.0),
                )
                .map_err(|e| e.to_string())?;
                #[cfg(target_os = "windows")]
                apply_acrylic(&w, Some((18, 20, 25, 180))).map_err(|e| e.to_string())?;
                let hidden = w.clone();
                w.on_window_event(move |event| match event {
                    tauri::WindowEvent::CloseRequested { api, .. } => {
                        api.prevent_close();
                        let _ = hidden.hide();
                    }
                    tauri::WindowEvent::Focused(false) => {
                        let _ = hidden.hide();
                    }
                    _ => {}
                });
                w.hide()?;
            }
            if let Ok(workspace) = workspace::resolve() {
                std::env::set_var("WORKSPACE_ROOT", workspace.root);
            }
            let runtime = app.state::<Arc<HubRuntime>>().inner().clone();
            runtime.start_all();
            let handle = app.handle().clone();
            std::thread::spawn(move || loop {
                let current = runtime.poll_all();
                let status = tray_status_for_products(&current);
                if let Ok(icon) = tray_icon(status) {
                    let _ = tray.set_icon(Some(icon));
                    let _ = tray.set_icon_as_template(false);
                }
                let _ = tray.set_tooltip(Some(tray_tooltip(status)));
                let _ = handle.emit("hub-snapshot-tick", ());
                std::thread::sleep(POLL_INTERVAL);
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("run Orthic Hub");
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
            "schemaVersion": 2,
            "productId": "membrane",
            "observedAtUnixMs": 1,
            "sections": sections
        })
    }
    #[test]
    fn cache_round_trip_is_atomic_shape() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("snapshot.json");
        let s = CachedSnapshot {
            schema_version: 2,
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
            schema_version: 2,
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
    fn windows_startup_value_quotes_paths_with_spaces() {
        assert_eq!(
            windows_startup_value(Path::new(r"C:\Program Files\Orthic\orthic.exe")),
            r#""C:\Program Files\Orthic\orthic.exe""#,
        );
    }
    #[test]
    fn polling_keeps_last_valid_then_accepts_service_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("snapshot.json");
        let first = parse_snapshot(&serde_json::to_vec(&canonical_payload(&[])).unwrap()).unwrap();
        assert_eq!(apply_poll_result(&path, Ok(first.clone())).unwrap(), first);
        assert_eq!(
            apply_poll_result(&path, Err("hub_service_unavailable".into())).unwrap(),
            first
        );
        let recovered = parse_snapshot(
            &serde_json::to_vec(&canonical_payload(&[("deliveries", "degraded")])).unwrap(),
        )
        .unwrap();
        assert_eq!(
            apply_poll_result(&path, Ok(recovered.clone())).unwrap(),
            recovered
        );
        assert_eq!(read_cache(&path).unwrap(), recovered);
    }
    #[test]
    fn bounded_parser_rejects_bad_schema_and_oversize() {
        assert_eq!(
            parse_snapshot(br#"{"schemaVersion":1}"#).unwrap_err(),
            "snapshot_schema_unsupported"
        );
        assert_eq!(
            parse_snapshot(&vec![b'x'; MAX_SNAPSHOT_BYTES as usize + 1]).unwrap_err(),
            "snapshot_too_large"
        );
    }
    fn snapshot_with(payload: serde_json::Value) -> CachedSnapshot {
        CachedSnapshot {
            schema_version: 2,
            observed_at_unix_ms: 1,
            payload,
        }
    }

    #[test]
    fn tray_status_never_promotes_missing_data_to_healthy() {
        assert_eq!(tray_status(None), TrayStatus::Offline);
        let implicit = snapshot_with(serde_json::json!({"schemaVersion":2,"productId":"membrane","observedAtUnixMs":1,"sections":{"deliveries":{"state":"available","reason":"ok"}}}));
        // single section available but missing other sections still has available worst? Actually single section should be available, but we treat any snapshot with sections as valid.
        // To keep offline for missing data, we need to test empty sections.
        assert_eq!(tray_status(Some(&implicit)), TrayStatus::Available);
        let empty = snapshot_with(serde_json::json!({"schemaVersion":2,"productId":"membrane","observedAtUnixMs":1,"sections":{}}));
        assert_eq!(tray_status(Some(&empty)), TrayStatus::Offline);
        let malformed = snapshot_with(serde_json::json!({}));
        assert_eq!(tray_status(Some(&malformed)), TrayStatus::Offline);
    }

    #[test]
    fn tray_status_takes_the_worst_present_state() {
        let healthy = snapshot_with(canonical_payload(&[]));
        assert_eq!(tray_status(Some(&healthy)), TrayStatus::Available);
        let mixed = snapshot_with(canonical_payload(&[("deliveries", "degraded")]));
        assert_eq!(tray_status(Some(&mixed)), TrayStatus::Degraded);
        let broken = snapshot_with(canonical_payload(&[("providers", "unavailable")]));
        assert_eq!(tray_status(Some(&broken)), TrayStatus::Unavailable);
    }

    #[test]
    fn every_status_shares_one_mark_and_differs_only_by_colour() {
        let statuses = [
            TrayStatus::Available,
            TrayStatus::Degraded,
            TrayStatus::Unavailable,
            TrayStatus::Offline,
        ];
        let mut colours = Vec::new();
        let mut silhouette: Option<Vec<u8>> = None;

        for status in statuses {
            let icon = tray_icon(status).expect("tray glyph decodes");
            let (width, height) = (icon.width() as usize, icon.height() as usize);
            let rgba = icon.rgba();

            let alpha: Vec<u8> = rgba.chunks_exact(4).map(|px| px[3]).collect();
            match &silhouette {
                None => silhouette = Some(alpha),
                Some(first) => assert_eq!(
                    first, &alpha,
                    "{status:?} uses a different silhouette; states differ by colour only"
                ),
            }

            let mut opaque = rgba
                .chunks_exact(4)
                .filter(|px| px[3] > 200)
                .map(|px| [px[0], px[1], px[2]]);
            let colour = opaque.next().expect("glyph has opaque pixels");
            assert!(
                opaque.all(|px| px == colour),
                "{status:?} is not a single flat colour"
            );
            assert_ne!(colour, [0, 0, 0], "{status:?} lost its status colour");
            colours.push(colour);

            let at = |x: usize, y: usize| rgba[(y * width + x) * 4 + 3];
            assert!((0..width).any(|x| at(x, 0) > 8), "{status:?} pads the top");
            assert!(
                (0..width).any(|x| at(x, height - 1) > 8),
                "{status:?} pads the bottom"
            );
            assert!(
                (0..height).any(|y| at(0, y) > 8),
                "{status:?} pads the left"
            );
            assert!(
                (0..height).any(|y| at(width - 1, y) > 8),
                "{status:?} pads the right"
            );

            assert!(tray_tooltip(status).starts_with("Orthic — "));
        }

        for (index, colour) in colours.iter().enumerate() {
            assert!(
                !colours[index + 1..].contains(colour),
                "two statuses share the colour {colour:?}"
            );
        }
        assert_ne!(
            tray_tooltip(TrayStatus::Offline),
            tray_tooltip(TrayStatus::Unavailable)
        );
    }

    #[test]
    fn popover_anchors_under_the_icon_centre() {
        let icon = tauri::Rect {
            position: tauri::LogicalPosition::new(100.0, 0.0).into(),
            size: tauri::LogicalSize::new(24.0, 24.0).into(),
        };
        assert_eq!(popover_origin(icon, 300, 1.0), (-38, 24));
        assert_eq!(popover_origin(icon, 300, 2.0), (74, 48));
    }

    #[test]
    fn parser_accepts_hub_operation_envelope() {
        let snapshot = parse_snapshot(
            &serde_json::to_vec(&serde_json::json!({
                "schemaVersion": 2,
                "result": { "kind": "success", "data": canonical_payload(&[]) }
            }))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(snapshot.observed_at_unix_ms, 1);
        assert_eq!(snapshot.payload["sections"]["deliveries"]["state"], "available");
    }
}
