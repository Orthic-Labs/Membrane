// Test-support binary for engine/crates/membrane/tests/hook_containment.rs
// ONLY. It stands in for `git.exe` on PATH inside a temp, non-repo
// directory so `bounded_git`'s `Command::new("git")` (Windows resolves a
// bare program name to `<name>.exe` via CreateProcess, never `.cmd`/`.bat`)
// actually spawns this binary instead of the real system git.
//
// It must never ship: packaging (apps/membrane-hub/scripts/
// package-portable-windows.mjs) copies the release payload by explicit
// source filename (only "membrane.exe"), so this bin is excluded from any
// shipped payload the same way the existing `cli-parity` dev bin already is
// - no additional opt-out mechanism was invented or is needed.
use std::{env, process::{Command, Stdio}, thread, time::Duration};

/// Spawns a detached grandchild that sleeps ~4s then writes "late" to the
/// marker path named by `marker_env`, mirroring the previous `git.cmd`
/// shim's `start "" /b cmd /c "ping -n 5 127.0.0.1 >nul & echo late>..."`
/// line. Using ping+cmd (not a Rust sleep) keeps the descendant a genuine OS
/// process-tree member for the job-object containment assertion, exactly as
/// before.
fn spawn_delayed_marker(marker_env: &str) {
    let Ok(marker) = env::var(marker_env) else { return };
    let system_root = env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
    let cmd_line = format!(
        "{system_root}\\System32\\ping.exe -n 5 127.0.0.1 >nul & echo late>{marker}"
    );
    let _ = Command::new("cmd")
        .args(["/c", &cmd_line])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

fn main() {
    let early_exit = env::var("MEMBRANE_HOOK_EARLY_EXIT").as_deref() == Ok("1");
    if early_exit {
        spawn_delayed_marker("MEMBRANE_FAKE_GIT_EARLY_LATE_MARKER");
        return;
    }
    spawn_delayed_marker("MEMBRANE_FAKE_GIT_LATE_MARKER");
    // Block well past HOOK_MODULE_DEADLINE_MS (3000ms, hook_diagnostics.rs)
    // so the containing module genuinely crosses its deadline, mirroring
    // the previous script's `ping -n 11` (~10s) blocking line.
    thread::sleep(Duration::from_secs(11));
}
