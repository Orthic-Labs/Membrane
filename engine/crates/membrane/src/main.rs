//! Membrane singleton engine entrypoint.
//!
//! All stateful service ownership lives in `membrane-runtime`. The companion
//! `membrane-client` binary owns compatibility transport for CLI, hook, and
//! stdio callers; this engine only dispatches its private hook worker.

use membrane::dispatch::parse_mode;
use membrane::modes::{dispatch, DispatchOutcome};
use membrane_runtime::service::{run_installed_runtime, LifecycleControl};

struct SupervisionGuard {
    path: std::path::PathBuf,
    failures: u64,
}

impl SupervisionGuard {
    fn begin() -> Result<Self, String> {
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        let product = exe.parent().and_then(std::path::Path::parent)
            .ok_or_else(|| "installed engine has no product root".to_string())?;
        let path = product.join("state/tools/.cache/memory/engine-supervision.json");
        let previous = std::fs::read_to_string(&path).ok()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok());
        let failures = if previous.as_ref().and_then(|value| value.get("clean")).and_then(serde_json::Value::as_bool) == Some(false) {
            previous.as_ref().and_then(|value| value.get("failures")).and_then(serde_json::Value::as_u64).unwrap_or(0) + 1
        } else { 0 };
        if failures >= 3 {
            disable_supervisor_task();
            return Err("membrane_engine_restart_suppressed: three consecutive unclean exits".into());
        }
        write_supervision(&path, false, failures)?;
        Ok(Self { path, failures })
    }
}

impl Drop for SupervisionGuard {
    fn drop(&mut self) { let _ = write_supervision(&self.path, true, 0); }
}

fn write_supervision(path: &std::path::Path, clean: bool, failures: u64) -> Result<(), String> {
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).map_err(|error| error.to_string())?; }
    let observed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64).unwrap_or_default();
    std::fs::write(path, format!("{{\"schemaVersion\":1,\"clean\":{clean},\"failures\":{failures},\"observedAtUnixMs\":{observed}}}\n"))
        .map_err(|error| error.to_string())
}

#[cfg(windows)]
fn disable_supervisor_task() {
    use std::os::windows::process::CommandExt;
    let _ = std::process::Command::new("schtasks.exe")
        .args(["/Change", "/TN", "Membrane Engine", "/Disable"])
        .creation_flags(0x0800_0000).status();
}

#[cfg(not(windows))]
fn disable_supervisor_task() {}

fn dispatch_hook_invocation() -> ! {
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
    // HookHost's contained child invokes the engine directly. Keep this path
    // ahead of resident startup so `hook-module` reaches the existing native
    // module entry point instead of recursively starting a resident engine.
    if first.as_deref() == Some("hook-module") {
        dispatch_hook_invocation();
    }
    let _supervision = if first.as_deref() == Some("--os-supervised") {
        match SupervisionGuard::begin() {
            Ok(guard) => Some(guard),
            Err(error) => { eprintln!("membrane: {error}"); std::process::exit(70); }
        }
    } else { None };
    if let Err(error) = run_installed_runtime(LifecycleControl::default()) {
        eprintln!("membrane: engine startup failed: {error}");
        std::process::exit(1);
    }
}
