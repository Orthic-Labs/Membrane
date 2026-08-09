use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder};
use tauri::{Emitter, Manager, PhysicalPosition};
#[cfg(target_os = "macos")]
use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial, NSVisualEffectState};

// MBR-911: dual-signature update-admission gate. Not yet called from the
// running tray/service wiring below -- see the module doc comment in
// update_admission.rs for why (no macOS RightKit updater artifact exists
// yet) -- but present and tested so the update flow that does exist can be
// gated on it without further plumbing changes.
mod update_admission;

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
type ServiceState = Arc<Mutex<Option<Child>>>;

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

fn fetch_snapshot(program: &Path) -> Result<CachedSnapshot, String> {
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

fn workspace_root() -> Option<PathBuf> {
    std::env::var_os("MEMBRANE_WORKSPACE_ROOT")
        .or_else(|| std::env::var_os("WORKSPACE_ROOT"))
        .map(PathBuf::from)
        .filter(|path| path.is_absolute() && path.is_dir())
        .or_else(|| {
            [
                PathBuf::from("/Volumes/D/claude"),
                PathBuf::from(r"D:\Claude"),
            ]
            .into_iter()
            .find(|path| path.is_dir())
        })
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

fn start_crypt_service() -> Result<Child, String> {
    let program = std::env::var_os("MEMBRANE_CRYPT_SERVICE")
        .map(PathBuf::from)
        .unwrap_or_else(|| bundled_binary("crypt-service"));
    if !program.is_file() {
        return Err("crypt_service_missing".into());
    }
    let root = workspace_root().ok_or("workspace_root_unavailable")?;
    let mut child = Command::new(program)
        .env("MEMBRANE_OWNER_PIPE", "1")
        .env("WORKSPACE_ROOT", root)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "crypt_service_start_failed".to_string())?;
    std::thread::sleep(Duration::from_millis(120));
    if child
        .try_wait()
        .map_err(|_| "crypt_service_wait_failed")?
        .is_some()
    {
        return Err("crypt_service_start_failed".into());
    }
    Ok(child)
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

fn section_state(value: &serde_json::Value) -> Option<&str> {
    value
        .as_str()
        .or_else(|| value.get("state").and_then(serde_json::Value::as_str))
}

fn state_rank(state: &str) -> Option<u8> {
    match state {
        "unavailable" => Some(0),
        "degraded" => Some(1),
        "available" => Some(2),
        _ => None,
    }
}

/// Mirrors the popover's aggregation: worst present state wins, and a payload
/// with no explicit `overall` stays Offline rather than being promoted.
pub fn tray_status(snapshot: Option<&CachedSnapshot>) -> TrayStatus {
    let Some(payload) = snapshot.map(|s| &s.payload).and_then(|p| p.as_object()) else {
        return TrayStatus::Offline;
    };
    if payload.get("overall").and_then(section_state).is_none() {
        return TrayStatus::Offline;
    }
    match payload
        .values()
        .filter_map(section_state)
        .filter_map(state_rank)
        .min()
    {
        Some(0) => TrayStatus::Unavailable,
        Some(1) => TrayStatus::Degraded,
        Some(2) => TrayStatus::Available,
        _ => TrayStatus::Offline,
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
            if let Some(root) = workspace_root() {
                std::env::set_var("WORKSPACE_ROOT", root);
            }
            let child = start_crypt_service().map_err(std::io::Error::other)?;
            *app.state::<ServiceState>()
                .lock()
                .map_err(|_| std::io::Error::other("service_state_unavailable"))? = Some(child);
            let handle = app.handle().clone();
            let program = std::env::var_os("MEMBRANE_COMMAND")
                .map(PathBuf::from)
                .unwrap_or_else(|| bundled_binary("membrane"));
            std::thread::spawn(move || loop {
                let current = apply_poll_result(&cache, fetch_snapshot(&program));
                let status = tray_status(current.as_ref().ok());
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
        let healthy = snapshot_with(
            serde_json::json!({"overall":"available","deliveries":{"state":"available"}}),
        );
        assert_eq!(tray_status(Some(&healthy)), TrayStatus::Available);
        let mixed = snapshot_with(
            serde_json::json!({"overall":"available","deliveries":{"state":"degraded"}}),
        );
        assert_eq!(tray_status(Some(&mixed)), TrayStatus::Degraded);
        let broken = snapshot_with(
            serde_json::json!({"overall":"degraded","sources":{"state":"unavailable"}}),
        );
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
            assert!((0..height).any(|y| at(0, y) > 8), "{status:?} pads the left");
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
            br#"{"schemaVersion":1,"result":{"kind":"success","data":{"schemaVersion":1,"observedAtUnixMs":42,"deliveries":{"state":"available"}}}}"#,
        )
        .unwrap();
        assert_eq!(snapshot.observed_at_unix_ms, 42);
        assert_eq!(snapshot.payload["deliveries"]["state"], "available");
    }
}
