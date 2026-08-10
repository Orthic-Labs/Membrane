use membrane_protocol::{HubSnapshotV1, HubStateV1, HUB_SCHEMA_VERSION};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
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

// MBR-911: dual-signature update-admission gate. Not yet called from the
// running tray/service wiring below -- see the module doc comment in
// update_admission.rs for why (no macOS RightKit updater artifact exists
// yet) -- but present and tested so the update flow that does exist can be
// gated on it without further plumbing changes.
#[cfg(test)]
mod hub_contract_tests;
mod update_admission;
mod workspace;

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
const STARTUP_GRACE: Duration = Duration::from_secs(3);
const STARTUP_POLL_INTERVAL: Duration = Duration::from_millis(100);
type ServiceState = Arc<Mutex<Option<Child>>>;

/// Two-Sentinels decision: `StartupGate` is deliberately NOT backed by
/// `memory_sentinel_view`/`memory_sentinel_producer` (engine/crates/membrane-runtime).
/// It masks a *transient* condition — the Hub's local snapshot poll still
/// reporting "source not connected" during the few seconds after the
/// sidecar process starts — so the window flashes "connecting" instead of a
/// misleading "offline" state (see `source_not_connected_snapshot` and the
/// `startup_sentinel_masked` error literal below). The engine's memory
/// sentinel is an unrelated, content-free read model of memory
/// lifecycle/proposal/contradiction state exposed via `HubInputsV1::sentinel`
/// once the connection is up. Wiring this boolean start-of-day gate to that
/// steady-state view would conflate "the sidecar hasn't finished booting yet"
/// with "the memory sentinel observed a problem" — two different failure
/// domains with different remedies. They stay separate on purpose.
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

fn bundled_binary(name: &str) -> PathBuf {
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.parent()
                .map(|parent| parent.join(format!("{name}{suffix}")))
        })
        .unwrap_or_else(|| PathBuf::from(format!("{name}{suffix}")))
}

fn start_crypt_service() -> Result<Option<Child>, String> {
    let program = std::env::var_os("MEMBRANE_CRYPT_SERVICE")
        .map(PathBuf::from)
        .unwrap_or_else(|| bundled_binary("crypt-service"));
    if !program.is_file() {
        return Err("crypt_service_missing".into());
    }
    let root = workspace::resolve()
        .map_err(|_| "workspace_root_unavailable")?
        .root;
    let mut child = Command::new(program)
        .env("MEMBRANE_OWNER_PIPE", "1")
        .env("WORKSPACE_ROOT", root)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "crypt_service_start_failed".to_string())?;
    std::thread::sleep(Duration::from_millis(120));
    if child
        .try_wait()
        .map_err(|_| "crypt_service_wait_failed")?
        .is_some()
    {
        let mut stderr_output = String::new();
        if let Some(mut stderr) = child.stderr.take() {
            let _ = stderr.read_to_string(&mut stderr_output);
        }
        if stderr_output.to_lowercase().contains("already in use") {
            eprintln!("crypt-service: port already in use, adopting existing owner");
            return Ok(None);
        }
        return Err("crypt_service_start_failed".into());
    }
    Ok(Some(child))
}

fn stop_crypt_service(service: &ServiceState) {
    if let Ok(mut guard) = service.lock() {
        if let Some(mut child) = guard.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Menu-bar state. Every state shows the same Membrane hex-brain mark and
/// differs by colour only, using the palette in `src/popover.css`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayStatus {
    Available,
    Degraded,
    Unavailable,
    /// No snapshot at all. Carries its own muted grey and a distinct tooltip,
    /// so "we don't know" is never rendered as "we're healthy".
    Offline,
}

fn state_rank(state: HubStateV1) -> u8 {
    match state {
        HubStateV1::Unavailable => 0,
        HubStateV1::Degraded => 1,
        HubStateV1::Available => 2,
    }
}

fn presentation_rank(section: &membrane_protocol::HubSectionV1) -> u8 {
    match section.state {
        HubStateV1::Unavailable if section.reason == "not_instrumented" => 1,
        state => state_rank(state),
    }
}

fn source_not_connected_snapshot(snapshot: &CachedSnapshot) -> bool {
    let Ok(snapshot) = serde_json::from_value::<HubSnapshotV1>(snapshot.payload.clone()) else {
        return false;
    };
    [
        &snapshot.deliveries,
        &snapshot.providers,
        &snapshot.repositories,
        &snapshot.adapters,
        &snapshot.devices,
        &snapshot.memory,
        &snapshot.sentinel,
        &snapshot.alerts,
    ]
    .iter()
    .all(|section| {
        section.state == HubStateV1::Unavailable && section.reason == "source_not_connected"
    })
}

/// Mirrors popover aggregation: all eight canonical sections must parse, then
/// their worst state controls the tray.
pub fn tray_status(snapshot: Option<&CachedSnapshot>) -> TrayStatus {
    let Some(snapshot) = snapshot.and_then(|snapshot| {
        serde_json::from_value::<HubSnapshotV1>(snapshot.payload.clone()).ok()
    }) else {
        return TrayStatus::Offline;
    };
    let worst = [
        &snapshot.deliveries,
        &snapshot.providers,
        &snapshot.repositories,
        &snapshot.adapters,
        &snapshot.devices,
        &snapshot.memory,
        &snapshot.sentinel,
        &snapshot.alerts,
    ]
    .into_iter()
    .map(presentation_rank)
    .min()
    .expect("canonical HubSnapshotV1 has eight sections");
    match worst {
        0 => TrayStatus::Unavailable,
        1 => TrayStatus::Degraded,
        2 => TrayStatus::Available,
        _ => unreachable!("HubStateV1 has three variants"),
    }
}

pub fn tray_tooltip(status: TrayStatus) -> &'static str {
    match status {
        TrayStatus::Available => "Membrane — available",
        TrayStatus::Degraded => "Membrane — degraded",
        TrayStatus::Unavailable => "Membrane — unavailable",
        TrayStatus::Offline => "Membrane — offline, no cached snapshot",
    }
}

// macOS forces menu-bar images to 18pt tall, so 36px is exactly 2x there;
// Windows tray slots take 32px. Both are cropped to the glyph's own bounds by
// scripts/tray-icons.mjs, so the mark fills its slot instead of sitting inside
// transparent padding that the 18pt scale would otherwise spend.
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

/// Anchors the popover under the tray icon itself rather than under the cursor,
/// so the arrow lines up wherever the click landed inside the icon.
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
fn snapshot(
    cache: tauri::State<'_, Arc<Mutex<PathBuf>>>,
    gate: tauri::State<'_, Arc<StartupGate>>,
) -> Result<CachedSnapshot, String> {
    let snapshot = read_cache(&cache.lock().map_err(|_| "cache lock poisoned")?)?;
    (!gate.masks(Some(&snapshot)))
        .then_some(snapshot)
        .ok_or_else(|| "snapshot_unavailable".into())
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
fn quit_app(app: tauri::AppHandle, service: tauri::State<'_, ServiceState>) {
    stop_crypt_service(&service);
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
    tauri::Builder::default()
        .manage(Arc::new(Mutex::new(PathBuf::from("snapshot.json"))))
        .manage(Arc::new(Mutex::new(None::<Child>)))
        .manage(Arc::new(StartupGate::new()))
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
            let cache = app
                .path()
                .app_data_dir()
                .map_err(|e| e.to_string())?
                .join("snapshot.json");
            fs::create_dir_all(cache.parent().unwrap())?;
            *app.state::<Arc<Mutex<PathBuf>>>().lock().unwrap() = cache.clone();
            let diagnostics =
                MenuItemBuilder::with_id("diagnostics", "Copy diagnostics").build(app)?;
            let trace = MenuItemBuilder::with_id("trace", "Latest trace").build(app)?;
            let quit = MenuItemBuilder::with_id("quit", "Quit Membrane").build(app)?;
            let menu = MenuBuilder::new(app)
                .items(&[&diagnostics, &trace, &quit])
                .build()?;
            let tray = TrayIconBuilder::new()
                .menu(&menu)
                .show_menu_on_left_click(false)
                .icon(tray_icon(TrayStatus::Offline)?)
                // Not a template: macOS ignores RGB in a template image and
                // tints the alpha itself, which would discard the status colour.
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
                    "quit" => app.exit(0),
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
            let child = start_crypt_service().map_err(std::io::Error::other)?;
            *app.state::<ServiceState>()
                .lock()
                .map_err(|_| std::io::Error::other("service_state_unavailable"))? = child;
            let handle = app.handle().clone();
            let gate = app.state::<Arc<StartupGate>>().inner().clone();
            let program = std::env::var_os("MEMBRANE_COMMAND")
                .map(PathBuf::from)
                .unwrap_or_else(|| bundled_binary("membrane"));
            std::thread::spawn(move || loop {
                let current = if gate.active() {
                    initial_poll(&cache, &program, &gate)
                } else {
                    poll_snapshot(&cache, fetch_snapshot(&program, POLL_TIMEOUT), &gate)
                };
                let status = tray_status(current.as_ref().ok());
                if let Ok(icon) = tray_icon(status) {
                    let _ = tray.set_icon(Some(icon));
                    let _ = tray.set_icon_as_template(false);
                }
                let _ = tray.set_tooltip(Some(tray_tooltip(status)));
                if current.is_ok() {
                    let _ = handle.emit("hub-snapshot-tick", ());
                }
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
    fn canonical_payload(overrides: &[(&str, &str)]) -> serde_json::Value {
        let mut payload = serde_json::json!({
            "schemaVersion": 1,
            "observedAtUnixMs": 1,
        });
        let sections = payload.as_object_mut().unwrap();
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
                    "source": null,
                    "evidence": null,
                    "observedAtUnixMs": 1,
                    "cacheAgeMs": 0,
                }),
            );
        }
        payload
    }
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
            parse_snapshot(br#"{"schemaVersion":2}"#).unwrap_err(),
            "snapshot_schema_unsupported"
        );
        assert_eq!(
            parse_snapshot(&vec![b'x'; MAX_SNAPSHOT_BYTES as usize + 1]).unwrap_err(),
            "snapshot_too_large"
        );
    }
    fn snapshot_with(payload: serde_json::Value) -> CachedSnapshot {
        CachedSnapshot {
            schema_version: 1,
            observed_at_unix_ms: 1,
            payload,
        }
    }

    #[test]
    fn tray_status_never_promotes_missing_data_to_healthy() {
        assert_eq!(tray_status(None), TrayStatus::Offline);
        // No explicit overall: unknown, not available.
        let implicit = snapshot_with(serde_json::json!({"deliveries":{"state":"available"}}));
        assert_eq!(tray_status(Some(&implicit)), TrayStatus::Offline);
        let empty = snapshot_with(serde_json::json!({}));
        assert_eq!(tray_status(Some(&empty)), TrayStatus::Offline);
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

    /// Locks the three properties the tray art has to keep: it is always the
    /// same Membrane mark, states are told apart by colour alone, and the mark
    /// is cropped tight so it fills the menu bar's fixed 18pt slot.
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

            // Same artwork every time: compare the alpha channel, which is the
            // shape. A silhouette drift means someone drew a new glyph.
            let alpha: Vec<u8> = rgba.chunks_exact(4).map(|px| px[3]).collect();
            match &silhouette {
                None => silhouette = Some(alpha),
                Some(first) => assert_eq!(
                    first, &alpha,
                    "{status:?} uses a different silhouette; states differ by colour only"
                ),
            }

            // One opaque colour, and not black -- a black glyph would mean the
            // tint was lost and the icon fell back to template-style artwork.
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

            // Cropped tight: every edge carries ink, so none of the 18pt slot
            // is spent on transparent padding.
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

            assert!(tray_tooltip(status).starts_with("Membrane — "));
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
        // Icon spans 100..124, centre 112; a 300-wide popover starts at -38.
        assert_eq!(popover_origin(icon, 300, 1.0), (-38, 24));
        // Scale factor applies to the icon rect, not the already-physical window.
        assert_eq!(popover_origin(icon, 300, 2.0), (74, 48));
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
        assert_eq!(snapshot.payload["deliveries"]["state"], "available");
    }
}
