//! N5 Hub-native Adapt scheduling and installed launch authority.
//!
//! Canonical authority: the native-rust migration milestone N5 ("native
//! scheduler/lifecycle binding through Hub", "replace installed Python
//! shim"). Production Adapt scheduling and launching run exclusively through
//! the installed native `membrane adapt <verb>` CLI backed by the
//! membrane-adapt crate's frozen `adapt.cli.v1` surface.
//!
//! Invariants enforced here:
//!
//! * Mining executes in-process through membrane-runtime. Hub never spawns a
//!   second Membrane authority process, interpreter, or checkout command.
//! * Lifecycle/status stay typed: every run ends in a native
//!   `adapt.cli.v1` envelope or stable error code; scheduler records
//!   `ran` / `skipped` / `failed` statuses and never falls back silently.
//! * The installed `~/bin/adapt` launcher is Hub-owned and always execs the
//!   bundled binary's native `adapt` subcommand (the retired Python shim is
//!   replaced idempotently).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Frozen CLI API version emitted by every native Adapt response (the
/// membrane-adapt crate's cli_api surface).
pub const ADAPT_CLI_API_VERSION: &str = "adapt.cli.v1";
/// Scheduled Adapt cycle cadence. The previous external daily-sync schedule
/// ran once per day; the Hub binds the same production cadence to the
/// resident lifecycle instead of a shell scheduler.
pub const ADAPT_CYCLE_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// Native Adapt verbs surfaced in Hub telemetry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdaptVerb {
    Mine,
    Review,
    Apply,
}

impl AdaptVerb {
    pub fn as_str(self) -> &'static str {
        match self {
            AdaptVerb::Mine => "mine",
            AdaptVerb::Review => "review",
            AdaptVerb::Apply => "apply",
        }
    }
}

/// Stable failure codes. These strings are the typed status contract; they
/// are recorded verbatim in Hub telemetry and must never be reinterpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdaptErrorCode {
    NativeMineFailed,
    OutputInvalid,
    ApiVersionMismatch,
    ScheduleMissing,
    ScheduleInvalid,
    OutputWriteFailed,
}

impl AdaptErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            AdaptErrorCode::NativeMineFailed => "adapt_native_mine_failed",
            AdaptErrorCode::OutputInvalid => "adapt_output_invalid",
            AdaptErrorCode::ApiVersionMismatch => "adapt_api_version_mismatch",
            AdaptErrorCode::ScheduleMissing => "adapt_schedule_missing",
            AdaptErrorCode::ScheduleInvalid => "adapt_schedule_invalid",
            AdaptErrorCode::OutputWriteFailed => "adapt_output_write_failed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AdaptLaunchError {
    pub code: AdaptErrorCode,
    pub exit_code: Option<i32>,
    pub detail: String,
}

impl AdaptLaunchError {
    fn new(code: AdaptErrorCode, detail: impl Into<String>) -> Self {
        Self {
            code,
            exit_code: None,
            detail: detail.into(),
        }
    }

    /// Telemetry-facing reason: code plus optional short context.
    pub fn reason(&self) -> String {
        if self.detail.is_empty() {
            self.code.as_str().to_string()
        } else {
            format!(
                "{}:{}",
                self.code.as_str(),
                truncate_tail(&self.detail, 160)
            )
        }
    }
}

/// Typed in-process authority over native Adapt mining.
pub struct AdaptLauncher {
    _workspace_root: PathBuf,
}

impl AdaptLauncher {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            _workspace_root: workspace_root,
        }
    }

    /// Run one native mining cycle inside Hub over caller-selected transcripts.
    pub fn launch_mine(&self, schedule: &AdaptScheduleV1) -> Result<Value, AdaptLaunchError> {
        let response = membrane_runtime::cli::run_native_adapt_mine(
            membrane_runtime::cli::NativeAdaptMineRequest {
                transcripts: schedule.transcripts.clone(),
                host: schedule.host.clone(),
                scope: schedule.scope.clone(),
                min_recurrence: schedule.min_recurrence,
            },
        )
        .map_err(|error| AdaptLaunchError::new(AdaptErrorCode::NativeMineFailed, error))?;
        let api_version = response
            .get("api_version")
            .and_then(Value::as_str)
            .or_else(|| {
                response
                    .pointer("/response/api_version")
                    .and_then(Value::as_str)
            });
        if api_version != Some(ADAPT_CLI_API_VERSION) {
            return Err(AdaptLaunchError::new(
                AdaptErrorCode::ApiVersionMismatch,
                "",
            ));
        }
        Ok(response)
    }
}

/// Render the Hub-owned installed launcher content for the native Adapt CLI.
/// POSIX sh on Unix, cmd on Windows. Contains no Python and no checkout path.
pub fn render_native_adapt_launcher(program: &Path) -> String {
    let quoted = quote_program(program);
    #[cfg(unix)]
    {
        format!("#!/bin/sh\nexec \"{quoted}\" adapt \"$@\"\n")
    }
    #[cfg(windows)]
    {
        format!("@echo off\r\n\"{quoted}\" adapt %*\r\n")
    }
}

fn quote_program(program: &Path) -> String {
    program
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

/// Idempotently replace the installed `~/bin/adapt` launcher with the native
/// one. The previous content (a Python shim pointing into a source checkout)
/// is overwritten atomically; permissions stay 0755 on Unix.
pub fn install_native_adapt_launcher(home: &Path, program: &Path) -> Result<PathBuf, String> {
    let dir = home.join("bin");
    fs::create_dir_all(&dir).map_err(|error| format!("adapt_launcher_dir_unavailable:{error}"))?;
    let name = if cfg!(windows) { "adapt.cmd" } else { "adapt" };
    let path = dir.join(name);
    let tmp = dir.join(format!("{name}.tmp"));
    fs::write(&tmp, render_native_adapt_launcher(program))
        .map_err(|error| format!("adapt_launcher_write_failed:{error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755))
            .map_err(|error| format!("adapt_launcher_write_failed:{error}"))?;
    }
    fs::rename(&tmp, &path).map_err(|error| format!("adapt_launcher_write_failed:{error}"))?;
    Ok(path)
}

/// Host user home, resolved the same split way workspace.rs does: HOME on
/// macOS/Linux, USERPROFILE on Windows; the two are never mixed.
pub fn host_home() -> Result<PathBuf, String> {
    #[cfg(windows)]
    let variable = "USERPROFILE";
    #[cfg(not(windows))]
    let variable = "HOME";
    std::env::var_os(variable)
        .map(PathBuf::from)
        .filter(|value| !value.as_os_str().is_empty())
        .ok_or_else(|| "adapt_launcher_home_unavailable".into())
}

pub fn default_schedule_path() -> Result<PathBuf, String> {
    Ok(host_home()?
        .join(".membrane")
        .join("adapt")
        .join("schedule.v1.json"))
}

/// Current Unix epoch milliseconds, saturating on clock prehistory.
pub fn unix_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// One scheduled cycle outcome. `state` is `ran`, `skipped`, or `failed`;
/// `reason` carries the typed skip/failure code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdaptCycleStatus {
    pub state: &'static str,
    pub verb: Option<AdaptVerb>,
    pub reason: Option<String>,
}

/// User-controlled native mining schedule. Semantic review/adjudication and
/// apply are intentionally not automatic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdaptScheduleV1 {
    pub schema_version: String,
    pub transcripts: Vec<PathBuf>,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default = "default_scope")]
    pub scope: String,
    #[serde(default = "default_min_recurrence")]
    pub min_recurrence: u32,
    pub output: PathBuf,
}

fn default_scope() -> String {
    "workspace".into()
}
fn default_min_recurrence() -> u32 {
    2
}

/// Hub-owned native mining schedule bound to resident lifecycle: cycles fire at
/// most once per interval and only while the resident service reports
/// Running. Failures are recorded, never retried inside the window, and
/// never fall back to retired Python paths.
pub struct AdaptScheduler {
    interval: Duration,
    next_due_ms: u64,
    schedule_path: PathBuf,
}

impl AdaptScheduler {
    pub fn new(interval: Duration, schedule_path: PathBuf) -> Self {
        Self {
            interval,
            next_due_ms: 0,
            schedule_path,
        }
    }

    fn load_schedule(&self) -> Result<AdaptScheduleV1, AdaptLaunchError> {
        if !self.schedule_path.is_file() {
            return Err(AdaptLaunchError::new(
                AdaptErrorCode::ScheduleMissing,
                self.schedule_path.display().to_string(),
            ));
        }
        let body = fs::read_to_string(&self.schedule_path).map_err(|error| {
            AdaptLaunchError::new(AdaptErrorCode::ScheduleInvalid, error.to_string())
        })?;
        let schedule: AdaptScheduleV1 = serde_json::from_str(&body).map_err(|error| {
            AdaptLaunchError::new(AdaptErrorCode::ScheduleInvalid, error.to_string())
        })?;
        if schedule.schema_version != "adapt.schedule.v1"
            || schedule.transcripts.is_empty()
            || schedule.transcripts.iter().any(|path| !path.is_file())
            || schedule.output.as_os_str().is_empty()
            || schedule.scope.trim().is_empty()
            || schedule.min_recurrence < 1
        {
            return Err(AdaptLaunchError::new(
                AdaptErrorCode::ScheduleInvalid,
                "contract",
            ));
        }
        Ok(schedule)
    }

    /// Return a status exactly when a cycle fires (the first tick is due
    /// immediately so Hub start converges to a known status fast).
    pub fn tick(
        &mut self,
        now_ms: u64,
        service_running: bool,
        launcher: &AdaptLauncher,
    ) -> Option<AdaptCycleStatus> {
        if now_ms < self.next_due_ms {
            return None;
        }
        self.next_due_ms = now_ms.saturating_add(self.interval.as_millis() as u64);
        if !service_running {
            return Some(AdaptCycleStatus {
                state: "skipped",
                verb: None,
                reason: Some("resident_not_running".into()),
            });
        }
        let schedule = match self.load_schedule() {
            Ok(schedule) => schedule,
            Err(error) if error.code == AdaptErrorCode::ScheduleMissing => {
                return Some(AdaptCycleStatus {
                    state: "skipped",
                    verb: None,
                    reason: Some(error.reason()),
                })
            }
            Err(error) => {
                return Some(AdaptCycleStatus {
                    state: "failed",
                    verb: Some(AdaptVerb::Mine),
                    reason: Some(error.reason()),
                })
            }
        };
        let response = match launcher.launch_mine(&schedule) {
            Ok(response) => response,
            Err(error) => {
                return Some(AdaptCycleStatus {
                    state: "failed",
                    verb: Some(AdaptVerb::Mine),
                    reason: Some(error.reason()),
                })
            }
        };
        if let Some(parent) = schedule.output.parent() {
            if let Err(error) = fs::create_dir_all(parent) {
                return Some(AdaptCycleStatus {
                    state: "failed",
                    verb: Some(AdaptVerb::Mine),
                    reason: Some(
                        AdaptLaunchError::new(AdaptErrorCode::OutputWriteFailed, error.to_string())
                            .reason(),
                    ),
                });
            }
        }
        let temporary = schedule.output.with_extension("json.tmp");
        let encoded = serde_json::to_vec_pretty(&response).expect("Adapt response serializes");
        if let Err(error) =
            fs::write(&temporary, encoded).and_then(|_| fs::rename(&temporary, &schedule.output))
        {
            let _ = fs::remove_file(&temporary);
            return Some(AdaptCycleStatus {
                state: "failed",
                verb: Some(AdaptVerb::Mine),
                reason: Some(
                    AdaptLaunchError::new(AdaptErrorCode::OutputWriteFailed, error.to_string())
                        .reason(),
                ),
            });
        }
        Some(AdaptCycleStatus {
            state: "ran",
            verb: None,
            reason: None,
        })
    }
}

fn truncate_tail(value: &str, max: usize) -> String {
    let trimmed = value.trim();
    if trimmed.len() <= max {
        trimmed.to_string()
    } else {
        let start = trimmed.len() - max;
        // Start on a char boundary.
        let start = (0..=start)
            .rev()
            .find(|index| trimmed.is_char_boundary(*index))
            .unwrap_or(start);
        trimmed[start..].to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbs_map_to_canonical_argv() {
        assert_eq!(AdaptVerb::Mine.as_str(), "mine");
        assert_eq!(AdaptVerb::Review.as_str(), "review");
        assert_eq!(AdaptVerb::Apply.as_str(), "apply");
    }

    #[test]
    fn native_launcher_content_has_no_python_and_no_checkout_paths() {
        let rendered = render_native_adapt_launcher(Path::new(
            "/Applications/Membrane Hub.app/Contents/MacOS/membrane",
        ));
        #[cfg(unix)]
        assert_eq!(
            rendered,
            "#!/bin/sh\nexec \"/Applications/Membrane Hub.app/Contents/MacOS/membrane\" adapt \"$@\"\n"
        );
        let lowered = rendered.to_lowercase();
        assert!(
            !lowered.contains("python"),
            "retired interpreter referenced"
        );
        assert!(!rendered.contains("adapt/src"), "checkout path referenced");
        assert!(rendered.contains("\" adapt "));
    }

    #[test]
    fn installed_launcher_replaces_python_shim_atomically_and_executably() {
        let home = tempfile::tempdir().unwrap();
        let program = home.path().join("bundle").join("membrane");
        let installed = install_native_adapt_launcher(home.path(), &program).unwrap();
        let expected_name = if cfg!(windows) { "adapt.cmd" } else { "adapt" };
        assert_eq!(installed, home.path().join("bin").join(expected_name));
        let content = fs::read_to_string(&installed).unwrap();
        assert_eq!(content, render_native_adapt_launcher(&program));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&installed).unwrap().permissions().mode() & 0o777,
                0o755
            );
        }
        // Re-install is idempotent and leaves no temp artifacts.
        install_native_adapt_launcher(home.path(), &program).unwrap();
        assert!(fs::read_dir(home.path().join("bin")).unwrap().all(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("adapt")
        }));
    }

    #[test]
    fn scheduler_skips_without_resident_and_gates_on_interval() {
        let launcher = AdaptLauncher::new(PathBuf::from("/nonexistent/membrane-binary"));
        let temp = tempfile::tempdir().unwrap();
        let mut scheduler =
            AdaptScheduler::new(Duration::from_secs(3600), temp.path().join("missing.json"));
        let skipped = scheduler
            .tick(1_000, false, &launcher)
            .expect("first tick due immediately");
        assert_eq!(skipped.state, "skipped");
        assert_eq!(skipped.reason.as_deref(), Some("resident_not_running"));
        // Interval gating: nothing due until the hour elapses.
        assert!(scheduler.tick(1_001, true, &launcher).is_none());
        // A missing user schedule is an explicit skip, never implicit work.
        let skipped = scheduler
            .tick(1_000 + 3_600_000, true, &launcher)
            .expect("due after interval");
        assert_eq!(skipped.state, "skipped");
        assert!(skipped
            .reason
            .as_deref()
            .unwrap()
            .starts_with("adapt_schedule_missing:"));
    }

    #[test]
    fn configured_native_mine_cycle_writes_review_input() {
        let dir = tempfile::tempdir().unwrap();
        let transcript = dir.path().join("one.jsonl");
        fs::write(&transcript, "{}\n").unwrap();
        let output = dir.path().join("out").join("mine.json");
        let schedule_path = dir.path().join("schedule.json");
        fs::write(
            &schedule_path,
            serde_json::to_vec(&AdaptScheduleV1 {
                schema_version: "adapt.schedule.v1".into(),
                transcripts: vec![transcript],
                host: Some("pi".into()),
                scope: "repo".into(),
                min_recurrence: 2,
                output: output.clone(),
            })
            .unwrap(),
        )
        .unwrap();
        let mut scheduler = AdaptScheduler::new(Duration::from_secs(3600), schedule_path);
        let status = scheduler
            .tick(unix_now_ms(), true, &AdaptLauncher::new(dir.path().into()))
            .expect("first tick due");
        assert_eq!(status.state, "ran");
        assert_eq!(status.reason, None);
        let saved: Value = serde_json::from_slice(&fs::read(output).unwrap()).unwrap();
        assert_eq!(
            saved
                .pointer("/response/api_version")
                .and_then(Value::as_str),
            Some(ADAPT_CLI_API_VERSION)
        );
    }
}
