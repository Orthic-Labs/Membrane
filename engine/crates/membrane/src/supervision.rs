//! Restart-supervision state shared by the engine binary and the installer.
//!
//! The OS supervisor launches `membrane.exe --os-supervised`, which records
//! an unclean start and clears it only on genuinely clean shutdown. The
//! failure this module prevents is counting *explicit stops* (installer
//! deactivate, user stop) as crashes: every `std::process::exit` and every
//! `taskkill /F` bypasses `Drop`, so a stop that does not mark itself clean
//! looks exactly like a crash to the next start. Three such false failures
//! suppress the restart task with exit 70.
//!
//! Contract:
//! - `begin` increments only on a previous *unclean* record and fails closed
//!   at three consecutive unclean exits (exit 70 + supervisor disable).
//! - Every explicit-stop path calls [`mark_clean`] *before* terminating the
//!   engine, so operator action never feeds the failure counter.
//! - (Re)activation calls [`reset`] and re-enables the supervisor task, which
//!   is the single supported suppression reset.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// Consecutive unclean exits before the supervisor task is suppressed.
pub const MAX_CONSECUTIVE_FAILURES: u64 = 3;

/// Freshness bound for the installer lock below: a crashed installer must
/// not suppress supervised starts forever.
const INSTALL_LOCK_MAX_AGE: Duration = Duration::from_secs(3600);

fn supervision_file(product_root: &Path) -> PathBuf {
    product_root.join("state/tools/.cache/memory/engine-supervision.json")
}

fn write_state(path: &Path, clean: bool, failures: u64) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let observed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default();
    std::fs::write(
        path,
        format!(
            "{{\"schemaVersion\":1,\"clean\":{clean},\"failures\":{failures},\"observedAtUnixMs\":{observed}}}\n"
        ),
    )
    .map_err(|error| error.to_string())
}

/// Resolve the installed product root from this executable's location:
/// `<root>/current/membrane.exe` (or the client) resolves to `<root>`.
pub fn product_root_from_exe() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    exe.parent()
        .and_then(std::path::Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| "installed engine has no product root".to_string())
}

/// Mark an explicit stop clean *before* the engine process is terminated.
/// Installer deactivate and user stop call this; crashes never reach it, so
/// only genuine unclean exits increment the failure counter.
pub fn mark_clean(product_root: &Path) -> Result<(), String> {
    write_state(&supervision_file(product_root), true, 0)
}

/// Supported suppression reset: clear failures and record clean. Called by
/// (re)activation alongside re-enabling the supervisor task.
pub fn reset(product_root: &Path) -> Result<(), String> {
    mark_clean(product_root)
}

/// True while the installer owns the product tree: it creates
/// `<root>/.install-lock` before touching files and removes it when done.
/// A supervised start during that window must exit quietly WITHOUT touching
/// supervision state — it is neither a crash nor an explicit stop, and
/// starting (mapping fresh images) would lock the files the installer is
/// replacing. Stale locks (crashed installer) are ignored after
/// `INSTALL_LOCK_MAX_AGE` so one bad install cannot suppress the engine.
pub fn install_in_progress(product_root: &Path) -> bool {
    let lock = product_root.join(".install-lock");
    let Ok(metadata) = std::fs::metadata(&lock) else {
        return false;
    };
    let fresh = metadata
        .created()
        .or_else(|_| metadata.modified())
        .ok()
        .and_then(|time| time.elapsed().ok())
        .is_some_and(|age| age < INSTALL_LOCK_MAX_AGE);
    fresh && lock.exists()
}

/// RAII guard for `--os-supervised` starts. `begin` records the unclean
/// start; dropping after a genuinely clean return clears it. Any
/// `std::process::exit` or external kill bypasses the drop, which is exactly
/// the crash signal — provided every explicit stop marked itself clean first.
pub struct SupervisionGuard {
    path: PathBuf,
}

impl SupervisionGuard {
    pub fn begin(product_root: &Path) -> Result<Self, String> {
        let path = supervision_file(product_root);
        let previous = std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok());
        let failures = if previous
            .as_ref()
            .and_then(|value| value.get("clean"))
            .and_then(serde_json::Value::as_bool)
            == Some(false)
        {
            previous
                .as_ref()
                .and_then(|value| value.get("failures"))
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
                + 1
        } else {
            0
        };
        if failures >= MAX_CONSECUTIVE_FAILURES {
            return Err(
                "membrane_engine_restart_suppressed: three consecutive unclean exits".into(),
            );
        }
        write_state(&path, false, failures)?;
        Ok(Self { path })
    }
}

impl Drop for SupervisionGuard {
    fn drop(&mut self) {
        let _ = write_state(&self.path, true, 0);
    }
}
