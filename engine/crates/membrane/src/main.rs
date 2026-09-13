//! Membrane singleton engine entrypoint.
//!
//! All stateful service ownership lives in `membrane-runtime`. The companion
//! `membrane-client` binary owns compatibility transport for CLI, hook, and
//! stdio callers; this engine serves the resident singleton and never spawns
//! hook worker children: ordinary hook dispatch runs in-process under a
//! per-module deadline with a bounded leaf-process scope.

use membrane::dispatch::parse_mode;
use membrane::modes::{dispatch, DispatchOutcome};
use membrane::supervision::SupervisionGuard;
use membrane_runtime::service::{run_installed_runtime, LifecycleControl};

#[cfg(windows)]
fn disable_supervisor_task() {
    use std::os::windows::process::CommandExt;
    let _ = std::process::Command::new("schtasks.exe")
        .args(["/Change", "/TN", "Membrane Engine", "/Disable"])
        .creation_flags(0x0800_0000).status();
}

#[cfg(not(windows))]
fn disable_supervisor_task() {}

/// Detach a supervisor-launched resident from any visible console and keep
/// its diagnostics in a log file instead.
///
/// The OS supervisor starts this binary attached to an interactive console,
/// so without this every (re)start opens a visible window that persists for
/// the engine's whole lifetime and reopens on every restart. Background
/// automation must never present UI: redirect stderr to the product log
/// file, then free the console. Foreground runs (bare resident, one-shot
/// CLI) keep their console untouched; only `--os-supervised` detaches.
#[cfg(windows)]
fn detach_supervised_console(product_root: &std::path::Path) {
    use std::os::windows::io::AsRawHandle;
    let log_path = product_root.join("state/tools/.cache/memory/engine-stderr.log");
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(file) = std::fs::OpenOptions::new().create(true).append(true).open(&log_path) {
        // Keep the file alive for the process lifetime so the redirected
        // handle stays valid; dropping it would invalidate stderr.
        let file = Box::leak(Box::new(file));
        unsafe {
            windows_sys::Win32::System::Console::SetStdHandle(
                windows_sys::Win32::System::Console::STD_ERROR_HANDLE,
                file.as_raw_handle() as _,
            );
        }
    }
    // Always detach: a background restart must never present a window even
    // if the log file could not be opened (writes then go nowhere, which is
    // still strictly better than a visible console).
    unsafe {
        windows_sys::Win32::System::Console::FreeConsole();
    }
}

fn main() {
    let first = std::env::args().nth(1);
    match first.as_deref() {
        Some("--help") | Some("-h") => {
            println!("membrane — installed singleton engine");
            return;
        }
        Some("--version") | Some("-V") => {
            println!("membrane {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        _ => {}
    }
    // No `hook-module` child exists anymore: hook dispatch runs in-process
    // under a per-module deadline with a bounded leaf scope, so there is no
    // private worker entry point ahead of resident startup.
    //
    // Explicit stops never reach the failure counter: deactivate and the
    // installer mark supervision clean before terminating the engine, so a
    // `clean: false` record here means a genuine crash or startup failure.
    // After three consecutive unclean exits the task suppresses itself with
    // exit 70; (re)activation resets the state and re-enables the task,
    // which is the single supported suppression reset.
    let _supervision = if first.as_deref() == Some("--os-supervised") {
        let product_root = match membrane::supervision::product_root_from_exe() {
            Ok(root) => root,
            Err(error) => { eprintln!("membrane: {error}"); std::process::exit(70); }
        };
        #[cfg(windows)]
        detach_supervised_console(&product_root);
        // The installer owns the tree during file replacement: a trigger
        // that fires mid-install must not map fresh images (locking them)
        // and must not count as a failure. Exit quietly; the installer
        // re-arms supervision when it finishes.
        if membrane::supervision::install_in_progress(&product_root) {
            std::process::exit(0);
        }
        match SupervisionGuard::begin(&product_root) {
            Ok(guard) => Some(guard),
            Err(error) => {
                disable_supervisor_task();
                eprintln!("membrane: {error}");
                std::process::exit(70);
            }
        }
    } else { None };
    // Bare invocation (no transport mode, no supervision flag) starts the
    // resident singleton, as does `--os-supervised` after the guard above.
    // Anything else is one-shot installer control: install, activation,
    // migration, init, and explicit local CLI run through the command
    // dispatcher below. Compat transport modes (`hook`, `stdio-mcp`) are
    // rejected there: they belong to the transport-only `membrane-client`,
    // which forwards them to the resident engine instead of constructing
    // local state.
    let supervised = first.as_deref() == Some("--os-supervised");
    if first.is_none() || supervised {
        if let Err(error) = run_installed_runtime(LifecycleControl::default()) {
            eprintln!("membrane: engine startup failed: {error}");
            std::process::exit(1);
        }
        return;
    }
    dispatch_control_plane();
}

fn dispatch_control_plane() -> ! {
    let invocation = match parse_mode(std::env::args_os()) {
        Ok(invocation) => invocation,
        Err(error) => {
            eprintln!("membrane: {error}");
            std::process::exit(membrane::EXIT_USER_ERROR);
        }
    };
    let outcome = dispatch(&invocation);
    match outcome {
        DispatchOutcome::Ok => std::process::exit(membrane::EXIT_OK),
        DispatchOutcome::UserError(error) => {
            eprintln!("membrane: {error}");
            std::process::exit(membrane::EXIT_USER_ERROR);
        }
        DispatchOutcome::InternalError(error) => {
            eprintln!("membrane: internal: {error}");
            std::process::exit(membrane::EXIT_INTERNAL_ERROR);
        }
    }
}
