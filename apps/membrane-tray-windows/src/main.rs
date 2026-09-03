#![cfg_attr(windows, windows_subsystem = "windows")]

mod instance;
mod ipc;
mod placement;
mod process;
mod renderer;
mod snapshot;
mod startup;
mod supervisor;
mod tray;
mod workspace;

slint::include_modules!();

use std::{
    cell::RefCell,
    fs::{create_dir_all, OpenOptions},
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    rc::Rc,
    time::Duration,
};

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::{CloseRequestResponse, ComponentHandle, Timer, TimerMode};
use tray_icon::{MouseButton, MouseButtonState, TrayIconEvent};

const POPOVER_WIDTH: i32 = 300;
// Keep shell geometry aligned with root heights in tray.slint. These are
// logical pixels; native placement converts them to monitor pixels.
const POPOVER_HEIGHT: i32 = 312;
const FIRST_RUN_HEIGHT: i32 = 336;
#[cfg(windows)]
const BASE_DPI: f64 = 96.0;
#[cfg(windows)]
const POPOVER_CORNER_RADIUS: i32 = 10;

#[cfg(windows)]
#[link(name = "user32")]
unsafe extern "system" {
    #[link_name = "GetDpiForWindow"]
    fn get_dpi_for_window(hwnd: windows_sys::Win32::Foundation::HWND) -> u32;
}

#[cfg(windows)]
#[link(name = "Shcore")]
unsafe extern "system" {
    #[link_name = "GetDpiForMonitor"]
    fn get_dpi_for_monitor(
        monitor: windows_sys::Win32::Graphics::Gdi::HMONITOR,
        dpi_type: u32,
        dpi_x: *mut u32,
        dpi_y: *mut u32,
    ) -> i32;
}

fn main() -> Result<(), slint::PlatformError> {
    let args = std::env::args().collect::<Vec<_>>();
    if args.iter().any(|arg| arg == "--self-test") {
        println!("membrane-tray-windows: PASS");
        println!("native=slint-1.17.1 window=300px verdicts=filled-square,half-square,hollow-square,dash");
        println!("supervisor=job-object+3 exits / 60s placement=work-area-aware blur-grace=500ms startup=HKCU-Run");
        return Ok(());
    }

    let login_launch = args.iter().any(|arg| arg == startup::LOGIN_LAUNCH_ARG);
    let activation_launch = args.iter().any(|arg| arg == "--activate");
    let replacement_launch = args.iter().any(|arg| arg == "--replace");
    let open_dashboard_on_start = args.iter().any(|arg| arg == "--open-dashboard");
    let instance_event = instance::InstanceEvent::acquire()
        .map_err(|error| slint::PlatformError::Other(error.to_string()))?;
    if !instance_event.is_primary() {
        if replacement_launch {
            let _ = instance_event.signal(instance::InstanceSignal::Replace);
        } else if activation_launch {
            let _ = instance_event.signal(instance::InstanceSignal::Activate);
        } else if !login_launch {
            let _ = instance_event.signal(instance::InstanceSignal::OpenDashboard);
        }
        return Ok(());
    }
    if replacement_launch {
        return Ok(());
    }

    let demo_state = demo_state();
    let demo_mode = demo_state.is_some();
    let tray_status = demo_state
        .map(|state| tray::Status::from_state(state))
        .unwrap_or(tray::Status::Starting);
    let tray_icon = tray::create_tray(tray_status)
        .map_err(|error| slint::PlatformError::Other(error.to_string()))?;

    // Pick the renderer before the first window exists: keep the GPU renderer on
    // real desktops, use Slint's software renderer on GPU-less hosts.
    let mut selected_renderer = renderer::install();
    let popover = match TrayPopover::new() {
        Ok(popover) => popover,
        Err(error)
            if selected_renderer == renderer::Renderer::Default
                && renderer::looks_like_gpu_failure(&error.to_string()) =>
        {
            eprintln!(
                "membrane-tray: default renderer failed to initialize ({error}); retrying with software renderer"
            );
            selected_renderer = renderer::force_software();
            TrayPopover::new()?
        }
        Err(error) => return Err(error),
    };
    let _ = selected_renderer;
    popover.hide()?;

    let resolved_workspace = workspace::resolve();
    let daemon_path = resolved_workspace
        .as_ref()
        .ok()
        .and_then(workspace::Workspace::daemon_path)
        .unwrap_or_else(supervisor::default_daemon_path);
    let workspace_root = resolved_workspace
        .as_ref()
        .map(|workspace| workspace.root.clone())
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let http_port = std::env::var("MEMBRANE_HTTP_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .or_else(|| {
            resolved_workspace
                .as_ref()
                .ok()
                .map(|workspace| workspace.http_port)
        })
        .unwrap_or(47_851);
    let supervisor = Rc::new(RefCell::new(supervisor::Supervisor::new(
        workspace_root,
        daemon_path,
        http_port,
    )));
    if let Ok(workspace) = resolved_workspace.as_ref() {
        supervisor.borrow_mut().set_origin(workspace.origin);
    }
    let startup_path = resolved_workspace
        .as_ref()
        .ok()
        .filter(|workspace| workspace.origin == workspace::RuntimeOrigin::Installed)
        .and_then(workspace::Workspace::tray_path)
        .unwrap_or_else(|| {
            std::env::current_exe().unwrap_or_else(|_| PathBuf::from(if cfg!(windows) {
                "membrane-tray.exe"
            } else {
                "membrane-tray"
            }))
        });
    let installed_origin = resolved_workspace
        .as_ref()
        .is_ok_and(|workspace| workspace.origin == workspace::RuntimeOrigin::Installed);
    if activation_launch && installed_origin {
        let _ = startup::install_for_current_user(&startup_path);
    }
    let first_run = demo_state.is_none() && startup::should_show_first_run(login_launch);
    if first_run {
        let _ = startup::mark_first_run();
    }
    let login_enabled =
        startup::is_enabled_for_current_user(&startup_path).unwrap_or(false);

    if let Some(state) = demo_state {
        apply_demo_state(&popover, state);
    } else {
        let now = supervisor::now_unix_ms();
        if let Err(reason) = resolved_workspace {
            supervisor.borrow_mut().block_startup(reason, now);
        } else {
            supervisor.borrow_mut().start_process(now);
        }
        apply_observation(&popover, &supervisor.borrow(), first_run, login_enabled);
    }
    popover.set_first_run(first_run);
    popover.set_login_enabled(login_enabled);
    request_popover_size(&popover, first_run);

    let close_popover = popover.as_weak();
    popover.window().on_close_requested(move || {
        if let Some(window) = close_popover.upgrade() {
            let _ = window.hide();
        }
        CloseRequestResponse::KeepWindowShown
    });

    let callback_supervisor = supervisor.clone();
    popover.on_restart(move || {
        let now = supervisor::now_unix_ms();
        match workspace::resolve() {
            Ok(workspace) => {
                let mut supervisor = callback_supervisor.borrow_mut();
                supervisor.set_workspace(workspace.root, workspace.http_port);
                supervisor.set_origin(workspace.origin);
                supervisor.manual_restart_process(now);
            }
            Err(reason) => {
                callback_supervisor.borrow_mut().block_startup(reason, now);
            }
        };
    });

    let dashboard_supervisor = supervisor.clone();
    popover.on_open_dashboard(move || {
        let supervisor = dashboard_supervisor.borrow();
        let _ = launch_dashboard(&supervisor);
    });

    let login_popover = popover.as_weak();
    let startup_path_for_toggle = startup_path.clone();
    popover.on_toggle_login(move || {
        if !installed_origin {
            return;
        }
        let currently_enabled = login_popover
            .upgrade()
            .map(|window| window.get_login_enabled())
            .unwrap_or(false);
        let result = if currently_enabled {
            startup::remove_for_current_user()
        } else {
            startup::install_for_current_user(&startup_path_for_toggle)
        };
        if result.is_ok() {
            if let Some(window) = login_popover.upgrade() {
                window.set_login_enabled(!currently_enabled);
            }
        }
    });

    let finish_popover = popover.as_weak();
    popover.on_finish_first_run(move || {
        let _ = startup::mark_first_run();
        if let Some(window) = finish_popover.upgrade() {
            window.set_first_run(false);
            request_popover_size(&window, false);
            let _ = window.hide();
        }
    });

    let quit_supervisor = supervisor.clone();
    let quit_popover = popover.as_weak();
    popover.on_quit(move || {
        quit_supervisor
            .borrow_mut()
            .begin_drain(supervisor::now_unix_ms());
        if let Some(window) = quit_popover.upgrade() {
            let _ = window.hide();
        }
    });

    let dismiss_guard = Rc::new(RefCell::new(placement::DismissGuard::default()));
    let pointer_down_guard = Rc::clone(&dismiss_guard);
    popover.on_pointer_down(move || pointer_down_guard.borrow_mut().pointer_down());
    let pointer_up_guard = Rc::clone(&dismiss_guard);
    popover.on_pointer_up(move || pointer_up_guard.borrow_mut().pointer_up_or_cancel());
    let pointer_cancel_guard = Rc::clone(&dismiss_guard);
    popover.on_pointer_cancel(move || pointer_cancel_guard.borrow_mut().pointer_up_or_cancel());
    let dismiss_popover = popover.as_weak();
    popover.on_dismiss(move || {
        if let Some(window) = dismiss_popover.upgrade() {
            let _ = window.hide();
        }
    });

    // tray-icon requires its Win32 message loop to run on the same thread as
    // icon creation. Slint's native event loop services those messages while
    // this timer drains tray + daemon channels.
    let tray_events = TrayIconEvent::receiver();
    let mut last_status = tray_status;
    let timer = Timer::default();
    let timer_popover = popover.as_weak();
    let timer_supervisor = supervisor.clone();
    let timer_dismiss_guard = Rc::clone(&dismiss_guard);
    let mut focus_pending = first_run || demo_mode;
    let initial_show = first_run || demo_mode;
    let initial_anchor = if initial_show {
        tray_icon.rect().map(anchor_from_tray_rect)
    } else {
        None
    };
    // Re-apply once after show: Winit may queue pre-show position changes
    // before native HWND creation.
    let mut initial_anchor_pending = initial_show;
    let mut open_dashboard_deadline =
        open_dashboard_on_start.then(|| supervisor::now_unix_ms() + 20_000);
    timer.start(TimerMode::Repeated, Duration::from_millis(50), move || {
        let now = supervisor::now_unix_ms();
        while let Ok(event) = tray_events.try_recv() {
            if let TrayIconEvent::Click {
                button,
                button_state,
                rect,
                ..
            } = event
            {
                if button_state != MouseButtonState::Down
                    || !matches!(button, MouseButton::Left | MouseButton::Right)
                {
                    continue;
                }
                let anchor = anchor_from_tray_rect(rect);
                let first_run = timer_popover
                    .upgrade()
                    .map(|window| window.get_first_run())
                    .unwrap_or(false);
                timer_dismiss_guard.borrow_mut().tray_click(now);
                if let Some(window) = timer_popover.upgrade() {
                    place_popover(&window, anchor, first_run);
                    initial_anchor_pending = false;
                    if window.window().is_visible() {
                        let _ = window.hide();
                        focus_pending = false;
                    } else {
                        let _ = window.show();
                        if configure_native_popup(&window, first_run)
                            && focus_native_window(&window)
                        {
                            focus_pending = false;
                        } else {
                            focus_pending = true;
                        }
                    }
                }
            }
        }

        if !demo_mode {
            timer_supervisor.borrow_mut().tick(now);
        }
        if instance_event.take_signal(instance::InstanceSignal::Activate) {
            let should_restart = timer_supervisor.borrow().state() != supervisor::State::Running;
            if should_restart {
                match workspace::resolve() {
                    Ok(workspace) => {
                        let mut supervisor = timer_supervisor.borrow_mut();
                        supervisor.set_workspace(workspace.root, workspace.http_port);
                        supervisor.set_origin(workspace.origin);
                        supervisor.manual_restart_process(now);
                    }
                    Err(reason) => {
                        timer_supervisor.borrow_mut().block_startup(reason, now);
                    }
                };
            }
        }
        if instance_event.take_signal(instance::InstanceSignal::Replace) {
            timer_supervisor.borrow_mut().begin_drain(now);
            if let Some(window) = timer_popover.upgrade() {
                let _ = window.hide();
            }
        }
        if instance_event.take_signal(instance::InstanceSignal::OpenDashboard) {
            open_dashboard_deadline = Some(now + 20_000);
        }
        if open_dashboard_deadline.is_some()
            && timer_supervisor.borrow().state() == supervisor::State::Running
            && launch_dashboard(&timer_supervisor.borrow())
        {
            open_dashboard_deadline = None;
        } else if open_dashboard_deadline.is_some_and(|deadline| now >= deadline) {
            if let Some(window) = timer_popover.upgrade() {
                if let Some(anchor) = tray_icon.rect().map(anchor_from_tray_rect) {
                    place_popover(&window, anchor, window.get_first_run());
                }
                let _ = window.show();
                focus_pending = true;
            }
            open_dashboard_deadline = None;
        }
        if let Some(window) = timer_popover.upgrade() {
            let login = window.get_login_enabled();
            if !demo_mode {
                let supervisor = timer_supervisor.borrow();
                apply_observation(&window, &supervisor, window.get_first_run(), login);
                let status = tray::Status::from_state(supervisor.state());
                if status != last_status {
                    let _ = tray::update_tray(&tray_icon, status, &supervisor.observation().reason);
                    last_status = status;
                }
            }
            if initial_anchor_pending && window.window().is_visible() {
                if let Some(anchor) = tray_icon.rect().map(anchor_from_tray_rect) {
                    place_popover(&window, anchor, window.get_first_run());
                    initial_anchor_pending = false;
                }
            }
            if window.window().is_visible() {
                let configured = configure_native_popup(&window, window.get_first_run());
                if focus_pending && configured && focus_native_window(&window) {
                    focus_pending = false;
                }
            }
            if !demo_mode
                && window.window().is_visible()
                && !window.get_first_run()
                && !native_window_focused(&window)
                && timer_dismiss_guard.borrow().should_dismiss(now, true)
            {
                let _ = window.hide();
            }
            if supervisor.borrow().is_quit_complete() {
                let _ = slint::quit_event_loop();
            }
        }
    });

    if first_run || demo_state.is_some() {
        dismiss_guard
            .borrow_mut()
            .tray_click(supervisor::now_unix_ms());
        if let Some(anchor) = initial_anchor {
            place_popover(&popover, anchor, first_run);
        }
        popover.show()?;
        request_popover_size(&popover, first_run);
    }
    slint::run_event_loop_until_quit()
}

fn anchor_from_tray_rect(rect: tray_icon::Rect) -> placement::Rect {
    let left = rect.position.x.round() as i32;
    let top = rect.position.y.round() as i32;
    placement::Rect {
        left,
        top,
        right: left.saturating_add(rect.size.width as i32),
        bottom: top.saturating_add(rect.size.height as i32),
    }
}

fn place_popover(window: &TrayPopover, anchor: placement::Rect, first_run: bool) {
    let work = placement::monitor_work_area(anchor).unwrap_or(placement::Rect {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1040,
    });
    let edge = placement::edge_for(anchor, work);
    let point = placement::origin(
        anchor,
        popup_physical_size(window, anchor, first_run),
        work,
        edge,
    );
    request_popover_size(window, first_run);
    window
        .window()
        .set_position(slint::PhysicalPosition::new(point.x, point.y));
    set_native_popup_position(window, point);
}

#[cfg(windows)]
fn set_native_popup_position(window: &TrayPopover, point: placement::Point) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOSIZE,
    };

    let Some(hwnd) = native_popup_hwnd(window) else {
        return;
    };
    unsafe {
        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            point.x,
            point.y,
            0,
            0,
            SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

#[cfg(not(windows))]
fn set_native_popup_position(_window: &TrayPopover, _point: placement::Point) {}

fn request_popover_size(window: &TrayPopover, first_run: bool) {
    let height = if first_run {
        FIRST_RUN_HEIGHT
    } else {
        POPOVER_HEIGHT
    };
    window
        .window()
        .set_size(slint::LogicalSize::new(POPOVER_WIDTH as f32, height as f32));
}

fn popup_physical_size(
    window: &TrayPopover,
    anchor: placement::Rect,
    first_run: bool,
) -> placement::Size {
    let scale = popup_scale_factor(window, anchor);
    placement::Size {
        width: logical_to_physical(POPOVER_WIDTH, scale),
        height: logical_to_physical(
            if first_run {
                FIRST_RUN_HEIGHT
            } else {
                POPOVER_HEIGHT
            },
            scale,
        ),
    }
}

fn logical_to_physical(value: i32, scale: f64) -> i32 {
    (f64::from(value) * scale.max(1.0)).round().max(1.0) as i32
}

fn popup_scale_factor(window: &TrayPopover, anchor: placement::Rect) -> f64 {
    #[cfg(windows)]
    if let Some(scale) = monitor_scale_factor(anchor) {
        return scale;
    }

    #[cfg(windows)]
    if let Some(hwnd) = native_popup_hwnd(window) {
        if let Some(scale) = native_popup_scale_factor(hwnd) {
            return scale;
        }
    }

    #[cfg(not(windows))]
    let _ = anchor;
    f64::from(window.window().scale_factor()).max(1.0)
}

#[cfg(windows)]
fn monitor_scale_factor(anchor: placement::Rect) -> Option<f64> {
    use windows_sys::Win32::{
        Foundation::POINT,
        Graphics::Gdi::{MonitorFromPoint, HMONITOR, MONITOR_DEFAULTTONEAREST},
    };

    let point = POINT {
        x: anchor
            .left
            .saturating_add(anchor.right.saturating_sub(anchor.left) / 2),
        y: anchor
            .top
            .saturating_add(anchor.bottom.saturating_sub(anchor.top) / 2),
    };
    let monitor: HMONITOR = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST) };
    if monitor.is_null() {
        return None;
    }

    // MDT_EFFECTIVE_DPI is the user-visible scale, unlike raw panel DPI.
    let mut dpi_x = 0_u32;
    let mut dpi_y = 0_u32;
    let result = unsafe { get_dpi_for_monitor(monitor, 0, &mut dpi_x, &mut dpi_y) };
    if result >= 0 && dpi_x > 0 {
        Some(f64::from(dpi_x) / BASE_DPI)
    } else {
        None
    }
}

#[cfg(windows)]
fn native_popup_hwnd(window: &TrayPopover) -> Option<windows_sys::Win32::Foundation::HWND> {
    let native_handle = window.window().window_handle();
    let handle = native_handle.window_handle().ok()?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return None;
    };
    let hwnd = handle.hwnd.get() as windows_sys::Win32::Foundation::HWND;
    (!hwnd.is_null()).then_some(hwnd)
}

#[cfg(windows)]
fn native_popup_scale_factor(hwnd: windows_sys::Win32::Foundation::HWND) -> Option<f64> {
    let dpi = unsafe { get_dpi_for_window(hwnd) };
    (dpi > 0).then_some(f64::from(dpi) / BASE_DPI)
}

#[cfg(windows)]
fn apply_native_round_region(
    hwnd: windows_sys::Win32::Foundation::HWND,
    width: i32,
    height: i32,
    scale: f64,
) -> bool {
    use windows_sys::Win32::Graphics::Gdi::{
        CreateRoundRectRgn, DeleteObject, SetWindowRgn, HGDIOBJ,
    };

    let width = width.max(1);
    let height = height.max(1);
    let radius = logical_to_physical(POPOVER_CORNER_RADIUS, scale)
        .min(width.min(height) / 2)
        .max(1);
    let diameter = radius.saturating_mul(2);
    let region = unsafe { CreateRoundRectRgn(0, 0, width, height, diameter, diameter) };
    if region.is_null() {
        return false;
    }
    let applied = unsafe { SetWindowRgn(hwnd, region, 1) != 0 };
    if !applied {
        unsafe {
            DeleteObject(region as HGDIOBJ);
        }
    }
    applied
}

fn native_window_focused(window: &TrayPopover) -> bool {
    #[cfg(windows)]
    {
        let Some(hwnd) = native_popup_hwnd(window) else {
            // A hidden Slint window can have no HWND until the event loop
            // creates it; don't start the blur timer in that interval.
            return true;
        };
        return unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow() == hwnd
        };
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        true
    }
}

fn focus_native_window(window: &TrayPopover) -> bool {
    #[cfg(windows)]
    {
        let Some(hwnd) = native_popup_hwnd(window) else {
            return false;
        };
        return unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::SetForegroundWindow(hwnd) != 0
        };
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        false
    }
}

/// Match native menu semantics: no caption/frame, no taskbar button, and
/// above the shell while visible. Slint still owns painting and input.
fn configure_native_popup(window: &TrayPopover, first_run: bool) -> bool {
    #[cfg(windows)]
    {
        let Some(hwnd) = native_popup_hwnd(window) else {
            // `show()` may only queue native creation. The timer retries once
            // Winit has installed the HWND in the active event loop.
            return false;
        };
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, GWL_STYLE,
            HWND_TOPMOST, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, WS_BORDER, WS_CAPTION,
            WS_DLGFRAME, WS_EX_ACCEPTFILES, WS_EX_APPWINDOW, WS_EX_CLIENTEDGE, WS_EX_STATICEDGE,
            WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_WINDOWEDGE, WS_MAXIMIZE, WS_MAXIMIZEBOX,
            WS_MINIMIZE, WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU, WS_THICKFRAME,
        };

        let scale = native_popup_scale_factor(hwnd)
            .unwrap_or_else(|| f64::from(window.window().scale_factor()).max(1.0));
        let width = logical_to_physical(POPOVER_WIDTH, scale);
        let height = logical_to_physical(
            if first_run {
                FIRST_RUN_HEIGHT
            } else {
                POPOVER_HEIGHT
            },
            scale,
        );

        let (style_applied, position_applied) = unsafe {
            let old_style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
            let style = (old_style
                & !(WS_BORDER
                    | WS_CAPTION
                    | WS_DLGFRAME
                    | WS_THICKFRAME
                    | WS_MINIMIZE
                    | WS_MINIMIZEBOX
                    | WS_MAXIMIZE
                    | WS_MAXIMIZEBOX
                    | WS_SYSMENU))
                | WS_POPUP;
            if old_style != style {
                SetWindowLongPtrW(hwnd, GWL_STYLE, style as _);
            }

            let old_ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            let ex_style = (old_ex_style
                & !(WS_EX_ACCEPTFILES
                    | WS_EX_APPWINDOW
                    | WS_EX_CLIENTEDGE
                    | WS_EX_STATICEDGE
                    | WS_EX_WINDOWEDGE))
                | WS_EX_TOOLWINDOW
                | WS_EX_TOPMOST;
            if old_ex_style != ex_style {
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style as _);
            }

            let position_result = SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                0,
                0,
                width,
                height,
                SWP_NOMOVE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
            let region_applied = apply_native_round_region(hwnd, width, height, scale);
            let applied_style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
            let applied_ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            (
                applied_style == style && applied_ex_style == ex_style,
                position_result != 0 && region_applied,
            )
        };
        return style_applied && position_applied;
    }
    #[cfg(not(windows))]
    {
        let _ = (window, first_run);
        true
    }
}

fn apply_observation(
    popover: &TrayPopover,
    supervisor: &supervisor::Supervisor,
    first_run: bool,
    login_enabled: bool,
) {
    let observation = supervisor.observation();
    popover.set_current_state(observation.state.label().into());
    popover.set_state_kind(observation.state.glyph().into());
    popover.set_reason(observation.reason.clone().into());
    popover.set_observed(format_observed(observation.observed_at_unix_ms).into());
    popover.set_generation(format!("generation {}", observation.generation).into());
    popover.set_pid(
        observation
            .pid
            .map(|pid| format!("PID {pid}"))
            .unwrap_or_else(|| "PID —".to_owned())
            .into(),
    );
    popover.set_endpoint(
        observation
            .endpoint
            .clone()
            .unwrap_or_else(|| "endpoint —".to_owned())
            .into(),
    );
    popover.set_admitted(observation.admitted.clone().into());
    popover.set_withheld(observation.withheld.clone().into());
    popover.set_budget(observation.budget.clone().into());
    popover.set_snapshot_observed(observation.snapshot_observed.clone().into());
    popover.set_can_restart(
        matches!(
            observation.state,
            supervisor::State::Stopped | supervisor::State::Backoff | supervisor::State::CrashLoop
        ) && !first_run,
    );
    popover.set_first_run(first_run);
    popover.set_login_enabled(login_enabled);
}

fn apply_demo_state(popover: &TrayPopover, state: supervisor::State) {
    popover.set_current_state(state.label().into());
    popover.set_state_kind(state.glyph().into());
    let reason = match state {
        supervisor::State::Running => "daemon_ready",
        supervisor::State::Starting => "daemon_starting",
        supervisor::State::Draining => "daemon_draining",
        supervisor::State::Stopped => "daemon_exited",
        supervisor::State::Backoff => "daemon_restart_backoff",
        supervisor::State::CrashLoop => "daemon_crash_loop",
    };
    popover.set_reason(reason.into());
    popover.set_observed("observed now".into());
    popover.set_generation("generation 3".into());
    match state {
        supervisor::State::Running => {
            popover.set_admitted("128".into());
            popover.set_withheld("4".into());
            popover.set_budget("0".into());
            popover.set_snapshot_observed("fixture · observed now".into());
        }
        supervisor::State::CrashLoop => {
            popover.set_admitted("Unknown · daemon_crash_loop".into());
            popover.set_withheld("Unknown · daemon_crash_loop".into());
            popover.set_budget("Unknown · daemon_crash_loop".into());
            popover.set_snapshot_observed("Unknown · daemon_crash_loop".into());
        }
        _ => {
            popover.set_admitted("Unknown · snapshot_unavailable".into());
            popover.set_withheld("Unknown · snapshot_unavailable".into());
            popover.set_budget("Unknown · snapshot_unavailable".into());
            popover.set_snapshot_observed("Unknown · snapshot_unavailable".into());
        }
    }
    popover.set_can_restart(!matches!(
        state,
        supervisor::State::Running | supervisor::State::Starting | supervisor::State::Draining
    ));
}

fn demo_state() -> Option<supervisor::State> {
    let value = std::env::args()
        .skip(1)
        .find_map(|arg| arg.strip_prefix("--demo=").map(str::to_owned))?;
    match value.to_ascii_lowercase().as_str() {
        "healthy" | "running" => Some(supervisor::State::Running),
        "offline" | "stopped" => Some(supervisor::State::Stopped),
        "crash-loop" | "crash_loop" => Some(supervisor::State::CrashLoop),
        "starting" => Some(supervisor::State::Starting),
        _ => None,
    }
}


/// Append target for the Hub's stdout and stderr: `<log root>/membrane-hub.log`.
/// The log root matches `membrane_runtime::paths::log_root()` on Windows
/// (`%LOCALAPPDATA%\Membrane`, or `MEMBRANE_LOG_ROOT` when set), so `membrane
/// cli doctor paths` names the directory these files actually appear in.
/// Falls back to a discarded stream so a logging failure never stops the Hub.
fn hub_log_target() -> Stdio {
    let root = std::env::var_os("MEMBRANE_LOG_ROOT")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(|base| PathBuf::from(base).join("Membrane")));
    let Some(root) = root else {
        return Stdio::null();
    };
    if create_dir_all(&root).is_err() {
        return Stdio::null();
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("membrane-hub.log"))
        .map(Stdio::from)
        .unwrap_or_else(|_| Stdio::null())
}

fn launch_dashboard(supervisor: &supervisor::Supervisor) -> bool {
    let (Some(endpoint), Some(token)) = (supervisor.endpoint(), supervisor.bearer_token()) else {
        return false;
    };
    let path = if supervisor.is_installed_origin() {
        supervisor.workspace_dashboard_path()
    } else {
        std::env::var_os("MEMBRANE_DASHBOARD_PATH")
            .map(PathBuf::from)
            .or_else(|| supervisor.workspace_dashboard_path())
            .or_else(|| {
                std::env::current_exe()
                    .ok()
                    .and_then(|exe| exe.parent().map(|parent| parent.join(if cfg!(windows) {
                        "membrane-hub.exe"
                    } else {
                        "membrane-hub"
                    })))
            })
    }
        .unwrap_or_else(|| PathBuf::from("membrane-hub.exe"));
    // The Hub reports its own failures on stdout and stderr. Discarding them
    // left the product with no diagnosis path at all: after thirty minutes of
    // uptime the only file on disk was the installer's log, while log_root()
    // stayed empty. Append both streams to that root instead; if the log
    // cannot be opened the Hub still starts, just as before.
    let mut child = match Command::new(path)
        .stdin(Stdio::piped())
        .stdout(hub_log_target())
        .stderr(hub_log_target())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return false,
    };
    let payload = serde_json::json!({
        "endpoint": endpoint,
        "token": token,
    });
    let Ok(mut bytes) = serde_json::to_vec(&payload) else {
        return false;
    };
    bytes.push(b'\n');
    if let Some(mut stdin) = child.stdin.take() {
        return stdin.write_all(&bytes).is_ok();
    }
    false
}

fn format_observed(observed_ms: u64) -> String {
    if observed_ms == 0 {
        return "observed —".to_owned();
    }
    let now = supervisor::now_unix_ms();
    let age = now.saturating_sub(observed_ms);
    if age < 1_000 {
        "observed now".to_owned()
    } else if age < 60_000 {
        format!("observed {}s ago", age / 1_000)
    } else {
        format!("observed {}m ago", age / 60_000)
    }
}
