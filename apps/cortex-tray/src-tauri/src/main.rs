#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde_json::{json, Value};
use std::{
    env, fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{mpsc, Arc, Mutex},
    thread,
    time::Duration,
};
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder};
use tauri::{Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};

struct ExplorerProcess {
    child: Child,
    url: String,
}

type ExplorerState = Arc<Mutex<Option<ExplorerProcess>>>;

fn cortex_command() -> String {
    env::var("CORTEX_COMMAND").unwrap_or_else(|_| "cortex".into())
}

fn repo_root() -> Result<PathBuf, String> {
    if let Some(path) = env::var_os("CORTEX_REPO_ROOT")
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
    {
        return Ok(path);
    }
    if let Ok(status) = cortex_json(&["service", "status", "--json"]) {
        if let Some(path) = status
            .get("enrolledRepos")
            .and_then(Value::as_array)
            .and_then(|repos| repos.first())
            .and_then(|repo| repo.get("root"))
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .filter(|path| path.is_dir())
        {
            return Ok(path);
        }
    }
    env::current_dir()
        .ok()
        .filter(|path| path.join(".git").exists())
        .ok_or_else(|| "repository_root_unavailable".into())
}

fn read_endpoint(stdout: impl Read + Send + 'static) -> Result<String, String> {
    let (tx, rx) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut json_text = String::new();
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            json_text.push_str(&line);
            if let Ok(value) = serde_json::from_str::<Value>(&json_text) {
                let _ = tx.send(value.get("url").and_then(Value::as_str).map(str::to_owned));
                return;
            }
        }
        let _ = tx.send(None);
    });
    rx.recv_timeout(Duration::from_secs(8))
        .map_err(|_| "explorer_start_timeout".to_string())?
        .ok_or_else(|| "explorer_endpoint_missing".into())
}

fn start_explorer() -> Result<ExplorerProcess, String> {
    let root = repo_root()?;
    let mut child = Command::new(cortex_command())
        .args(["explore", "--no-open", "--json", "--root"])
        .arg(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "explorer_start_failed".to_string())?;
    let stdout = child.stdout.take().ok_or("explorer_stdout_missing")?;
    match read_endpoint(stdout) {
        Ok(url) => Ok(ExplorerProcess { child, url }),
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            Err(error)
        }
    }
}

fn endpoint(state: &ExplorerState) -> Result<String, String> {
    let mut guard = state.lock().map_err(|_| "explorer_state_unavailable")?;
    if guard
        .as_mut()
        .is_some_and(|item| item.child.try_wait().ok().flatten().is_some())
    {
        guard.take();
    }
    if guard.is_none() {
        *guard = Some(start_explorer()?);
    }
    Ok(guard.as_ref().unwrap().url.clone())
}

fn split_endpoint(url: &str) -> Result<(String, String), String> {
    let remainder = url
        .strip_prefix("http://")
        .ok_or("explorer_endpoint_invalid")?;
    let (host, fragment) = remainder
        .split_once("/#token=")
        .ok_or("explorer_endpoint_invalid")?;
    if !host.starts_with("127.0.0.1:") || fragment.is_empty() {
        return Err("explorer_endpoint_invalid".into());
    }
    Ok((host.into(), fragment.into()))
}

fn api_get(url: &str, path: &str) -> Result<Value, String> {
    let (host, token) = split_endpoint(url)?;
    let mut stream = TcpStream::connect(&host).map_err(|_| "service_offline")?;
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .map_err(|_| "service_timeout")?;
    write!(stream, "GET {path} HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n")
        .map_err(|_| "service_request_failed")?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|_| "service_response_failed")?;
    let boundary = response
        .windows(4)
        .position(|bytes| bytes == b"\r\n\r\n")
        .ok_or("service_response_invalid")?
        + 4;
    let head = String::from_utf8_lossy(&response[..boundary]);
    if !head.starts_with("HTTP/1.1 200") {
        return Err("service_response_rejected".into());
    }
    serde_json::from_slice(&response[boundary..]).map_err(|_| "service_response_invalid".into())
}

fn cortex_json(args: &[&str]) -> Result<Value, String> {
    let output = Command::new(cortex_command())
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|_| "cortex_command_failed".to_string())?;
    if !output.status.success() {
        return Err("cortex_command_failed".into());
    }
    serde_json::from_slice(&output.stdout).map_err(|_| "cortex_response_invalid".into())
}

fn icon_for(state: &str) -> tauri::image::Image<'static> {
    let color = match state {
        "fresh" => [63, 205, 136, 255],
        "stale" | "degraded" => [238, 176, 76, 255],
        _ => [219, 91, 109, 255],
    };
    let mut rgba = vec![0_u8; 32 * 32 * 4];
    for y in 0..32_i32 {
        for x in 0..32_i32 {
            if (x - 16).pow(2) + (y - 16).pow(2) <= 132 {
                let offset = ((y * 32 + x) * 4) as usize;
                rgba[offset..offset + 4].copy_from_slice(&color);
            }
        }
    }
    tauri::image::Image::new_owned(rgba, 32, 32)
}

#[tauri::command]
fn snapshot(
    app: tauri::AppHandle,
    state: tauri::State<'_, ExplorerState>,
) -> Result<Value, String> {
    let service = cortex_json(&["service", "status", "--json"])
        .unwrap_or_else(|_| json!({ "registered": false, "running": false, "enrolledRepos": [] }));
    let mut value = endpoint(&state)
        .and_then(|url| api_get(&url, "/api/status"))
        .unwrap_or_else(|error| json!({ "state": "offline", "reason": error }));
    if let Some(object) = value.as_object_mut() {
        object.insert("service".into(), service.clone());
        object.insert(
            "enrolledRepos".into(),
            service.get("enrolledRepos").cloned().unwrap_or_else(|| json!([])),
        );
        object.insert(
            "watcher".into(),
            json!({ "state": if service.get("running").and_then(Value::as_bool).unwrap_or(false) { "running" } else { "stopped" } }),
        );
    }
    let status = value
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or("offline");
    if let Some(tray) = app.tray_by_id("cortex") {
        let _ = tray.set_icon(Some(icon_for(status)));
        let _ = tray.set_tooltip(Some(format!("Cortex — {status}")));
    }
    Ok(value)
}

#[tauri::command]
fn open_explorer(
    app: tauri::AppHandle,
    state: tauri::State<'_, ExplorerState>,
) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("explorer") {
        window.show().map_err(|_| "explorer_window_unavailable")?;
        window
            .set_focus()
            .map_err(|_| "explorer_window_unavailable")?;
        return Ok(());
    }
    let url = endpoint(&state)?
        .parse()
        .map_err(|_| "explorer_endpoint_invalid")?;
    WebviewWindowBuilder::new(&app, "explorer", WebviewUrl::External(url))
        .title("Cortex Explorer")
        .inner_size(1180.0, 760.0)
        .build()
        .map_err(|_| "explorer_window_unavailable")?;
    Ok(())
}

#[tauri::command]
fn service_action(action: &str) -> Result<Value, String> {
    if !matches!(action, "start" | "stop" | "restart") {
        return Err("service_action_invalid".into());
    }
    if action == "start" {
        let status = cortex_json(&["service", "status", "--json"])?;
        if !status.get("registered").and_then(Value::as_bool).unwrap_or(false) {
            cortex_json(&["service", "install", "--json"])?;
        }
    }
    cortex_json(&["service", action, "--json"]).map_err(|_| "service_control_failed".into())
}

fn startup_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("startup.json"))
        .map_err(|_| "startup_path_unavailable".into())
}

fn configure_startup(enabled: bool) -> Result<(), String> {
    let executable = env::current_exe().map_err(|_| "startup_executable_unavailable")?;
    #[cfg(target_os = "windows")]
    {
        let mut command = Command::new("schtasks");
        if enabled {
            command
                .args([
                    "/Create",
                    "/F",
                    "/SC",
                    "ONLOGON",
                    "/TN",
                    "OrthicCortexTray",
                    "/TR",
                ])
                .arg(executable);
        } else {
            command.args(["/Delete", "/F", "/TN", "OrthicCortexTray"]);
        }
        return command
            .status()
            .map_err(|_| "startup_control_failed")?
            .success()
            .then_some(())
            .ok_or_else(|| "startup_control_failed".into());
    }
    #[cfg(target_os = "macos")]
    {
        let home = env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or("startup_path_unavailable")?;
        let target = home.join("Library/LaunchAgents/io.orthic.cortex-tray.plist");
        if enabled {
            fs::create_dir_all(target.parent().unwrap()).map_err(|_| "startup_write_failed")?;
            let program = executable
                .to_string_lossy()
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            let body = format!(
                r#"<?xml version="1.0" encoding="UTF-8"?><!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd"><plist version="1.0"><dict><key>Label</key><string>io.orthic.cortex-tray</string><key>ProgramArguments</key><array><string>{program}</string></array><key>RunAtLoad</key><true/></dict></plist>"#
            );
            fs::write(&target, body).map_err(|_| "startup_write_failed")?;
            Command::new("launchctl")
                .args(["load"])
                .arg(&target)
                .status()
                .map_err(|_| "startup_control_failed")?
                .success()
                .then_some(())
                .ok_or_else(|| "startup_control_failed".into())
        } else {
            let _ = Command::new("launchctl")
                .args(["unload"])
                .arg(&target)
                .status();
            fs::remove_file(target)
                .or_else(|error| {
                    if error.kind() == std::io::ErrorKind::NotFound {
                        Ok(())
                    } else {
                        Err(error)
                    }
                })
                .map_err(|_| "startup_control_failed")
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let home = env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or("startup_path_unavailable")?;
        let target = home.join(".config/autostart/cortex-tray.desktop");
        if enabled {
            fs::create_dir_all(target.parent().unwrap()).map_err(|_| "startup_write_failed")?;
            fs::write(
                target,
                format!(
                    "[Desktop Entry]\nType=Application\nName=Cortex\nExec=\"{}\"\n",
                    executable.display()
                ),
            )
            .map_err(|_| "startup_write_failed")
        } else {
            fs::remove_file(target)
                .or_else(|error| {
                    if error.kind() == std::io::ErrorKind::NotFound {
                        Ok(())
                    } else {
                        Err(error)
                    }
                })
                .map_err(|_| "startup_control_failed")
        }
    }
}

#[tauri::command]
fn set_startup(enabled: bool, app: tauri::AppHandle) -> Result<(), String> {
    configure_startup(enabled)?;
    let path = startup_path(&app)?;
    fs::create_dir_all(path.parent().ok_or("startup_path_unavailable")?)
        .map_err(|_| "startup_write_failed")?;
    fs::write(
        path,
        serde_json::to_vec(&json!({ "enabled": enabled })).unwrap(),
    )
    .map_err(|_| "startup_write_failed".to_string())
}

#[tauri::command]
fn startup_setting(app: tauri::AppHandle) -> Result<bool, String> {
    let bytes = fs::read(startup_path(&app)?).unwrap_or_default();
    Ok(serde_json::from_slice::<Value>(&bytes)
        .ok()
        .and_then(|value| value.get("enabled").and_then(Value::as_bool))
        .unwrap_or(false))
}

#[tauri::command]
fn hide_tray(app: tauri::AppHandle) -> Result<(), String> {
    app.get_webview_window("tray")
        .ok_or("tray_unavailable")?
        .hide()
        .map_err(|_| "tray_unavailable".into())
}

fn stop_owned_explorer(state: &ExplorerState) {
    if let Ok(mut guard) = state.lock() {
        if let Some(mut explorer) = guard.take() {
            let _ = explorer.child.kill();
            let _ = explorer.child.wait();
        }
    }
}

#[tauri::command]
fn quit_app(app: tauri::AppHandle, state: tauri::State<'_, ExplorerState>) {
    stop_owned_explorer(&state);
    app.exit(0);
}

fn toggle_tray(app: &tauri::AppHandle, rect: tauri::Rect) {
    let Some(window) = app.get_webview_window("tray") else {
        return;
    };
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
        return;
    }
    if let Ok(size) = window.outer_size() {
        let scale = window.scale_factor().unwrap_or(1.0);
        let position = rect.position.to_physical::<f64>(scale);
        let icon = rect.size.to_physical::<f64>(scale);
        let x = (position.x + icon.width / 2.0 - f64::from(size.width) / 2.0).round() as i32;
        let y = (position.y + icon.height).round() as i32;
        let _ = window.set_position(PhysicalPosition::new(x, y));
    }
    let _ = window.show();
    let _ = window.set_focus();
}

fn main() {
    tauri::Builder::default()
        .manage(Arc::new(Mutex::new(None::<ExplorerProcess>)))
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("tray") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .invoke_handler(tauri::generate_handler![
            snapshot,
            open_explorer,
            service_action,
            set_startup,
            startup_setting,
            hide_tray,
            quit_app
        ])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.handle()
                .set_activation_policy(tauri::ActivationPolicy::Accessory)?;
            let open = MenuItemBuilder::with_id("open", "Open Cortex").build(app)?;
            let explorer = MenuItemBuilder::with_id("explorer", "Open Explorer").build(app)?;
            let quit = MenuItemBuilder::with_id("quit", "Quit Cortex").build(app)?;
            let menu = MenuBuilder::new(app)
                .items(&[&open, &explorer, &quit])
                .build()?;
            TrayIconBuilder::with_id("cortex")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .icon(icon_for("offline"))
                .tooltip("Cortex — offline")
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "open" => {
                        if let Some(window) = app.get_webview_window("tray") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "explorer" => {
                        let state = app.state::<ExplorerState>();
                        let _ = open_explorer(app.clone(), state);
                    }
                    "quit" => {
                        let state = app.state::<ExplorerState>();
                        stop_owned_explorer(&state);
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
                        toggle_tray(tray.app_handle(), rect);
                    }
                })
                .build(app)?;
            if let Some(window) = app.get_webview_window("tray") {
                let hidden = window.clone();
                window.on_window_event(move |event| match event {
                    tauri::WindowEvent::CloseRequested { api, .. } => {
                        api.prevent_close();
                        let _ = hidden.hide();
                    }
                    tauri::WindowEvent::Focused(false) => {
                        let _ = hidden.hide();
                    }
                    _ => {}
                });
                window.hide()?;
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("run Cortex tray");
}
