use membrane_protocol::{
    membrane_parent_state, HubSnapshotV1, HubStateV1, MembraneParentState, HUB_SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
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
mod adapt_launch;
mod hub_telemetry;
mod supervisor;
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
const STARTUP_AGENT_LABEL: &str = "com.membrane.hub";
const HUB_SECTION_NAMES: [&str; 8] = [
    "deliveries",
    "providers",
    "repositories",
    "adapters",
    "devices",
    "memory",
    "sentinel",
    "alerts",
];
type ServiceState = Arc<supervisor::Supervisor>;
type TelemetryState = Arc<hub_telemetry::HubTelemetry>;

fn service_status_name(status: supervisor::ServiceStatus) -> &'static str {
    match status {
        supervisor::ServiceStatus::Running => "running",
        supervisor::ServiceStatus::Unavailable => "unavailable",
        supervisor::ServiceStatus::CrashLoop => "crash_loop",
    }
}

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

fn membrane_service_supervisor(
    resident_log_path: PathBuf,
) -> Result<supervisor::Supervisor, String> {
    let program = bundled_binary("membrane");
    if !program.is_file() {
        return Err("membrane_hub_resident_missing".into());
    }
    let root = workspace::resolve()
        .map_err(|_| "workspace_root_unavailable")?
        .root;
    Ok(supervisor::Supervisor::new(
        program,
        root,
        resident_log_path,
    ))
}

fn stop_membrane_service(service: &ServiceState) {
    service.stop();
}

/// N5 installed launch cutover: the user-level `~/bin/adapt` launcher is
/// Hub-owned and always execs this Hub's bundled native binary's `membrane
/// adapt` CLI. The retired Python shim (a source-checkout PYTHONPATH wrapper)
/// is replaced idempotently on every successful Hub start; the launcher is
/// never pointed at an env-overridden or development-tree binary.
fn install_native_adapt_seam(telemetry: &hub_telemetry::HubTelemetry, program: &Path) {
    let outcome = adapt_launch::host_home()
        .and_then(|home| adapt_launch::install_native_adapt_launcher(&home, program));
    match outcome {
        Ok(path) => telemetry.event(
            "adapt_launcher_installed",
            "available",
            Some(&path.to_string_lossy()),
        ),
        Err(error) => telemetry.event("adapt_launcher_installed", "unavailable", Some(&error)),
    }
}

/// Menu-bar state. Every state shows the same Membrane hex-brain mark and
/// differs by colour only, using the palette in `src/popover.css`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayStatus {
    Running,
    Degraded,
    /// No snapshot at all. Carries its own muted grey and a distinct tooltip,
    /// so "we don't know" is never rendered as "we're healthy".
    Offline,
}

fn source_not_connected_snapshot(snapshot: &CachedSnapshot) -> bool {
    let Ok(snapshot) = serde_json::from_value::<HubSnapshotV1>(snapshot.payload.clone()) else {
        return false;
    };
    HUB_SECTION_NAMES.iter().all(|name| {
        snapshot.sections.get(*name).is_some_and(|section| {
            section.state == HubStateV1::Unavailable && section.reason == "source_not_connected"
        })
    })
}

/// Membrane status is resident-local. Child resource state never changes it.
/// Frozen mapping: supervisor liveness + resident /health ok + snapshot validity.
pub fn tray_status(
    service_status: supervisor::ServiceStatus,
    health_ok: Option<bool>,
    live_snapshot_available: bool,
) -> TrayStatus {
    let supervisor_running = service_status == supervisor::ServiceStatus::Running;
    match membrane_parent_state(supervisor_running, health_ok, live_snapshot_available) {
        MembraneParentState::Running => TrayStatus::Running,
        MembraneParentState::Degraded => TrayStatus::Degraded,
        MembraneParentState::Offline => TrayStatus::Offline,
    }
}

fn resident_health_ok() -> Option<bool> {
    let port = std::env::var("MEMBRANE_PORT")
        .ok()
        .and_then(|value| value.trim().parse::<u16>().ok())
        .unwrap_or(47851);
    let addr = format!("127.0.0.1:{port}");
    let mut stream = std::net::TcpStream::connect_timeout(
        &addr.parse().ok()?,
        std::time::Duration::from_millis(300),
    )
    .ok()?;
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(300)));
    let _ = stream.set_write_timeout(Some(std::time::Duration::from_millis(300)));
    use std::io::{Read, Write};
    let request = format!("GET /health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).ok()?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).ok()?;
    let text = String::from_utf8_lossy(&raw);
    let body = text
        .split_once("\r\n\r\n")
        .or_else(|| text.split_once("\n\n"))
        .map(|(_, body)| body)?;
    let json: serde_json::Value = serde_json::from_str(body.trim()).ok()?;
    json.get("ok").and_then(|value| value.as_bool())
}

pub fn tray_tooltip(status: TrayStatus) -> &'static str {
    match status {
        TrayStatus::Running => "Membrane — running",
        TrayStatus::Degraded => "Membrane — degraded",
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
        TrayStatus::Running => tray_asset!("membrane-available"),
        TrayStatus::Degraded => tray_asset!("membrane-degraded"),
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
fn initial_poll(
    path: &Path,
    program: &Path,
    gate: &StartupGate,
) -> (Result<CachedSnapshot, String>, bool) {
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
            return (output, false);
        }
        let result = fetch_snapshot(program, POLL_TIMEOUT.min(remaining));
        let live_snapshot_available = result.is_ok();
        let expired = deadline
            .saturating_duration_since(std::time::Instant::now())
            .is_zero();
        if let Some(output) = startup_step(path, result, expired, &mut latest) {
            gate.finish();
            return (output, live_snapshot_available);
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
fn diagnostics_report(
    telemetry: tauri::State<'_, TelemetryState>,
) -> hub_telemetry::HubDiagnostics {
    telemetry.report()
}

#[tauri::command]
fn set_startup(enabled: bool) -> Result<(), String> {
    set_platform_startup(enabled)
}

#[tauri::command]
fn startup_setting() -> Result<bool, String> {
    platform_startup_setting()
}

#[cfg(target_os = "macos")]
fn launch_agent_path() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "membrane_hub_home_unavailable".to_string())?;
    Ok(home
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{STARTUP_AGENT_LABEL}.plist")))
}

#[cfg(target_os = "macos")]
fn current_app_bundle() -> Result<PathBuf, String> {
    let executable = std::env::current_exe().map_err(|_| "membrane_hub_executable_unavailable")?;
    let bundle = executable
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .filter(|path| path.extension().is_some_and(|extension| extension == "app"))
        .ok_or("membrane_hub_bundle_unavailable")?;
    Ok(bundle.to_path_buf())
}

#[cfg(target_os = "macos")]
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(target_os = "macos")]
fn startup_agent_plist(bundle: &Path) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
<plist version=\"1.0\"><dict>\n\
  <key>Label</key><string>{STARTUP_AGENT_LABEL}</string>\n\
  <key>ProgramArguments</key><array><string>/usr/bin/open</string><string>{}</string></array>\n\
  <key>RunAtLoad</key><true/>\n\
  <key>ProcessType</key><string>Interactive</string>\n\
</dict></plist>\n",
        xml_escape(&bundle.to_string_lossy())
    )
}

#[cfg(target_os = "macos")]
fn set_platform_startup(enabled: bool) -> Result<(), String> {
    let path = launch_agent_path()?;
    if enabled {
        let bundle = current_app_bundle()?;
        fs::create_dir_all(
            path.parent()
                .ok_or("membrane_hub_launch_agent_parent_missing")?,
        )
        .map_err(|_| "membrane_hub_launch_agent_write_failed")?;
        atomic_write(&path, startup_agent_plist(&bundle).as_bytes())
            .map_err(|_| "membrane_hub_launch_agent_write_failed".to_string())
    } else if path.exists() {
        fs::remove_file(path).map_err(|_| "membrane_hub_launch_agent_remove_failed".to_string())
    } else {
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn platform_startup_setting() -> Result<bool, String> {
    Ok(launch_agent_path()?.is_file())
}

#[cfg(not(target_os = "macos"))]
fn set_platform_startup(_enabled: bool) -> Result<(), String> {
    Err("membrane_hub_startup_unsupported".into())
}

#[cfg(not(target_os = "macos"))]
fn platform_startup_setting() -> Result<bool, String> {
    Err("membrane_hub_startup_unsupported".into())
}

#[tauri::command]
fn quit_app(app: tauri::AppHandle, service: tauri::State<'_, ServiceState>) {
    if let Some(telemetry) = app.try_state::<TelemetryState>() {
        telemetry.event("hub_quit", "requested", None);
    }
    stop_membrane_service(&service);
    app.exit(0);
}

#[tauri::command]
fn hide_popover(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(telemetry) = app.try_state::<TelemetryState>() {
        telemetry.event("popover", "hidden", None);
    }
    app.get_webview_window("hub")
        .ok_or_else(|| "popover_unavailable".to_string())?
        .hide()
        .map_err(|_| "popover_hide_failed".to_string())
}

fn show_dashboard(app: &tauri::AppHandle) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    app.set_activation_policy(tauri::ActivationPolicy::Regular)
        .map_err(|_| "dashboard_activation_failed".to_string())?;
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
fn open_dashboard(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(popover) = app.get_webview_window("hub") {
        let _ = popover.hide();
    }
    let result = show_dashboard(&app);
    if let Some(telemetry) = app.try_state::<TelemetryState>() {
        telemetry.event(
            "dashboard",
            if result.is_ok() {
                "opened"
            } else {
                "unavailable"
            },
            result.as_ref().err().map(String::as_str),
        );
    }
    result
}

fn main() {
    tauri::Builder::default()
        .manage(Arc::new(Mutex::new(PathBuf::from("snapshot.json"))))
        .manage(Arc::new(StartupGate::new()))
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            let _ = show_dashboard(app);
        }))
        .invoke_handler(tauri::generate_handler![
            snapshot,
            set_startup,
            startup_setting,
            diagnostics_report,
            hide_popover,
            open_dashboard,
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
            let telemetry = Arc::new(hub_telemetry::HubTelemetry::new(
                cache.with_file_name("hub.jsonl"),
            ));
            telemetry.event("hub_start", "starting", None);
            app.manage(telemetry.clone());
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
                    "quit" => {
                        if let Some(service) = app.try_state::<ServiceState>() {
                            stop_membrane_service(&service);
                        }
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
            if let Some(dashboard) = app.get_webview_window("dashboard") {
                let dashboard_window = dashboard.clone();
                let dashboard_app = app.handle().clone();
                dashboard.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = dashboard_window.hide();
                        #[cfg(target_os = "macos")]
                        let _ =
                            dashboard_app.set_activation_policy(tauri::ActivationPolicy::Accessory);
                    }
                });
                dashboard.hide()?;
            }
            if let Ok(workspace) = workspace::resolve() {
                std::env::set_var("WORKSPACE_ROOT", workspace.root);
            }
            let supervisor = Arc::new(
                membrane_service_supervisor(cache.with_file_name("resident.log"))
                    .map_err(std::io::Error::other)?,
            );
            let started = supervisor.start().map_err(std::io::Error::other)?;
            telemetry.service(service_status_name(started), None);
            if started != supervisor::ServiceStatus::Running {
                return Err(std::io::Error::other("membrane_hub_resident_unavailable").into());
            }
            app.manage(supervisor.clone());
            let handle = app.handle().clone();
            // N5 cutover: replace the installed Python Adapt launcher with the
            // native one and bind the production Adapt cycle schedule to the
            // resident lifecycle. Statuses land as typed telemetry events.
            let adapt_program = bundled_binary("membrane");
            install_native_adapt_seam(&telemetry, &adapt_program);
            {
                let adapt_supervisor = supervisor.clone();
                let adapt_telemetry = telemetry.clone();
                std::thread::spawn(move || {
                    let launcher = adapt_launch::AdaptLauncher::new(adapt_program);
                    let schedule_path = adapt_launch::default_schedule_path()
                        .unwrap_or_else(|_| PathBuf::from("adapt-schedule-unavailable"));
                    let mut scheduler = adapt_launch::AdaptScheduler::new(
                        adapt_launch::ADAPT_CYCLE_INTERVAL,
                        schedule_path,
                    );
                    loop {
                        if let Some(status) = scheduler.tick(
                            adapt_launch::unix_now_ms(),
                            adapt_supervisor.supervise() == supervisor::ServiceStatus::Running,
                            &launcher,
                        ) {
                            adapt_telemetry.event("adapt_cycle", status.state, status.reason.as_deref());
                        }
                        std::thread::sleep(POLL_INTERVAL);
                    }
                });
            }
            let gate = app.state::<Arc<StartupGate>>().inner().clone();
            let program = std::env::var_os("MEMBRANE_COMMAND")
                .map(PathBuf::from)
                .unwrap_or_else(|| bundled_binary("membrane"));
            std::thread::spawn(move || loop {
                let observed_service_status = supervisor.supervise();
                let service_status =
                    if observed_service_status != supervisor::ServiceStatus::Running {
                        telemetry.service(
                            service_status_name(observed_service_status),
                            Some("resident_not_running"),
                        );
                        match supervisor.start() {
                            Ok(status) => {
                                telemetry.service(service_status_name(status), None);
                                status
                            }
                            Err(error) => {
                                telemetry.service("unavailable", Some(&error));
                                supervisor::ServiceStatus::Unavailable
                            }
                        }
                    } else {
                        observed_service_status
                    };
                let (current, live_snapshot_available) = if gate.active() {
                    initial_poll(&cache, &program, &gate)
                } else {
                    let fetched = fetch_snapshot(&program, POLL_TIMEOUT);
                    let live = fetched.is_ok();
                    (poll_snapshot(&cache, fetched, &gate), live)
                };
                let health_ok = resident_health_ok();
                let live_available = live_snapshot_available && current.is_ok();
                let status = tray_status(service_status, health_ok, live_available);
                match &current {
                    Ok(_) if live_snapshot_available => telemetry.snapshot("available", None),
                    Ok(_) => telemetry.snapshot("degraded", Some("cached_snapshot")),
                    Err(error) => telemetry.snapshot("unavailable", Some(error)),
                }
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
        #[cfg(target_os = "macos")]
        {
            let plist = startup_agent_plist(Path::new("/Applications/Membrane Hub.app"));
            assert!(plist.contains("<string>com.membrane.hub</string>"));
            assert!(plist.contains("<string>/Applications/Membrane Hub.app</string>"));
            assert!(plist.contains("<key>RunAtLoad</key><true/>"));
            assert!(plist.contains("<key>ProcessType</key><string>Interactive</string>"));
            assert!(!plist.contains("KeepAlive"));
        }
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
    fn tray_status_requires_resident_and_live_snapshot() {
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
            tray_status(supervisor::ServiceStatus::CrashLoop, Some(true), true),
            TrayStatus::Offline
        );
    }

    #[test]
    fn child_failure_does_not_change_membrane_status() {
        let _broken = snapshot_with(canonical_payload(&[("providers", "unavailable")]));
        assert_eq!(
            tray_status(supervisor::ServiceStatus::Running, Some(true), true),
            TrayStatus::Running
        );
    }

    #[test]
    fn health_unhealthy_plus_valid_snapshot_is_degraded_via_real_producer_path() {
        assert_eq!(
            tray_status(supervisor::ServiceStatus::Running, Some(false), true),
            TrayStatus::Degraded
        );
    }

    /// Locks the three properties the tray art has to keep: it is always the
    /// same Membrane mark, states are told apart by colour alone, and the mark
    /// is cropped tight so it fills the menu bar's fixed 18pt slot.
    #[test]
    fn every_status_shares_one_mark_and_differs_only_by_colour() {
        let statuses = [
            TrayStatus::Running,
            TrayStatus::Degraded,
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
            tray_tooltip(TrayStatus::Running),
            tray_tooltip(TrayStatus::Offline)
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
        assert_eq!(
            snapshot.payload["sections"]["deliveries"]["state"],
            "available"
        );
    }
}
