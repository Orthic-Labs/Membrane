//! Tray-owned daemon supervisor.
//!
//! The reducer is deliberately small and deterministic so crash-loop and
//! drain semantics remain testable without a desktop. The Windows process
//! plumbing lives in `process.rs`; this type owns its lifetime, protocol
//! reader, restart policy, and user-visible observation.

use std::{
    collections::VecDeque,
    fs::{create_dir_all, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::AtomicBool,
        mpsc::{self, Receiver},
        Arc, Mutex, OnceLock,
    },
};

use serde_json::{json, Value};

use membrane_protocol::{
    DaemonCommandKind, DaemonCommandV1, DaemonEventKind, DaemonLaunchKind, DaemonLaunchV1,
    DAEMON_IPC_SCHEMA_VERSION,
};
use membrane_runtime::residency::{Holder, Identity, ResidentController};

use crate::{
    ipc::EventDecoder,
    process::{self, DaemonProcess, ProcessEvent},
    snapshot::{self, SnapshotUpdate},
    workspace,
};

pub const CRASH_LOOP_THRESHOLD: usize = 3;
pub const CRASH_LOOP_WINDOW_MS: u64 = 60_000;
pub const RESTART_BACKOFF_MS: u64 = 1_000;
pub const HANDSHAKE_TIMEOUT_MS: u64 = 35_000;
pub const DRAIN_TIMEOUT_MS: u64 = 7_000;
/// Bounded window a freshly launched daemon is kept alive without any valid
/// holder yet acquired. After this grace expires, only a valid holder keeps
/// the daemon running; a still-holderless daemon is drained as bounded
/// startup cleanup, never left resident indefinitely.
pub const PRE_HOLDER_GRACE_MS: u64 = 30_000;

static LIFECYCLE_LOG: OnceLock<Mutex<File>> = OnceLock::new();

/// Install the tray's durable diagnostic sink before instance/UI creation.
pub fn init_lifecycle_log() -> bool {
    let root = std::env::var_os("MEMBRANE_LOG_ROOT")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(|base| PathBuf::from(base).join("Membrane")));
    let Some(root) = root else { return false; };
    if create_dir_all(&root).is_err() { return false; }
    let Ok(file) = OpenOptions::new().create(true).append(true).open(root.join("membrane-tray.log")) else { return false; };
    let _ = LIFECYCLE_LOG.set(Mutex::new(file));
    lifecycle_event("tray_startup", json!({"stage":"log_initialized"}));
    true
}

/// Write content-free lifecycle diagnostics. Callers must pass only typed,
/// non-secret state such as stages, reasons, PIDs, and exit codes.
pub fn lifecycle_event(event: &str, details: Value) {
    let Some(log) = LIFECYCLE_LOG.get() else { return; };
    let mut record = match details {
        Value::Object(map) => map,
        _ => serde_json::Map::new(),
    };
    record.insert("event".into(), Value::String(event.to_owned()));
    record.insert("observedAtUnixMs".into(), json!(now_unix_ms()));
    if let Ok(mut log) = log.lock() {
        let _ = serde_json::to_writer(&mut *log, &Value::Object(record));
        let _ = log.write_all(b"\n");
        let _ = log.flush();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum State {
    #[default]
    Stopped,
    Starting,
    Running,
    Draining,
    Backoff,
    CrashLoop,
}

impl State {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Stopped => "Offline",
            Self::Starting => "Starting",
            Self::Running => "Running",
            Self::Draining => "Stopping",
            Self::Backoff => "Restarting",
            Self::CrashLoop => "Crash loop",
        }
    }

    pub const fn glyph(self) -> &'static str {
        match self {
            Self::Running => "filled-square",
            Self::Starting | Self::Draining => "half-square",
            Self::Stopped | Self::Backoff | Self::CrashLoop => "hollow-square",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    DaemonStarting,
    DaemonReady,
    DaemonDraining,
    DaemonExited,
    DaemonRestartBackoff,
    DaemonCrashLoop,
    DaemonProtocolInvalid,
    DaemonSpawnFailed,
    DaemonHandshakeTimeout,
    DaemonReadyFailed,
    DaemonDrainTimeout,
    DaemonPreHolderGraceExpired,
    DaemonJobEscapeDenied,
}

impl Reason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DaemonStarting => "daemon_starting",
            Self::DaemonReady => "daemon_ready",
            Self::DaemonDraining => "daemon_draining",
            Self::DaemonExited => "daemon_exited",
            Self::DaemonRestartBackoff => "daemon_restart_backoff",
            Self::DaemonCrashLoop => "daemon_crash_loop",
            Self::DaemonProtocolInvalid => "daemon_protocol_invalid",
            Self::DaemonSpawnFailed => "daemon_spawn_failed",
            Self::DaemonHandshakeTimeout => "daemon_handshake_timeout",
            Self::DaemonReadyFailed => "daemon_ready_failed",
            Self::DaemonDrainTimeout => "daemon_drain_timeout",
            Self::DaemonPreHolderGraceExpired => "daemon_pre_holder_grace_expired",
            Self::DaemonJobEscapeDenied => "daemon_job_escape_denied",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition {
    pub state: State,
    pub reason: Reason,
    pub generation: u64,
    pub pid: Option<u32>,
    pub observed_at_unix_ms: u64,
    pub exit_code: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation {
    pub state: State,
    pub reason: String,
    pub generation: u64,
    pub pid: Option<u32>,
    pub observed_at_unix_ms: u64,
    pub exit_code: Option<u32>,
    pub endpoint: Option<String>,
    pub admitted: String,
    pub withheld: String,
    pub budget: String,
    pub snapshot_observed: String,
    /// (pid, creation_time_ticks) fingerprint of the current daemon process.
    /// Combined with `generation`, this lets a caller that persisted the
    /// triple across an abrupt tray relaunch tell a survived daemon apart
    /// from an unrelated process that happens to reuse the same PID.
    pub process_identity: Option<(u32, u64)>,
}

impl Default for Observation {
    fn default() -> Self {
        Self {
            state: State::Stopped,
            reason: Reason::DaemonExited.as_str().to_owned(),
            generation: 0,
            pid: None,
            observed_at_unix_ms: 0,
            exit_code: None,
            endpoint: None,
            admitted: "Unknown · snapshot_unavailable".into(),
            withheld: "Unknown · snapshot_unavailable".into(),
            budget: "Unknown · snapshot_unavailable".into(),
            snapshot_observed: "Unknown · snapshot_unavailable".into(),
            process_identity: None,
        }
    }
}

#[derive(Debug)]
pub struct Supervisor {
    observation: Observation,
    failures: VecDeque<u64>,
    run_started_at: Option<u64>,
    retry_at: Option<u64>,
    handshake_deadline: Option<u64>,
    drain_deadline: Option<u64>,
    quit_requested: bool,
    drain_complete: bool,
    terminal_event: bool,
    process_exited: bool,
    control_sequence: u64,
    event_decoder: EventDecoder,
    event_rx: Option<Receiver<ProcessEvent>>,
    snapshot_rx: Option<Receiver<SnapshotUpdate>>,
    snapshot_stop: Option<Arc<AtomicBool>>,
    process: Option<DaemonProcess>,
    workspace_root: PathBuf,
    daemon_path: PathBuf,
    http_port: u16,
    bearer_token: Option<String>,
    installed_origin: bool,
    residency: ResidentController,
    /// Launch mode selected before the next spawn. Never mutated after a
    /// process has been acquired for the current generation — a change here
    /// only takes effect on the *next* `launch_process` call, so there is no
    /// retrofit of containment/escape onto an already-running daemon.
    pending_launch_mode: process::LaunchMode,
    /// Set when a process is launched with no valid holder acquired yet.
    /// Cleared the moment a holder is acquired. If it elapses first, the
    /// still-holderless daemon is bounded startup cleanup, not indefinite
    /// residency.
    pre_holder_deadline: Option<u64>,
    /// True once at least one valid holder has been acquired for the current
    /// process. After this, the daemon detaches from the pre-holder grace
    /// entirely and only holder bookkeeping governs its lifetime.
    holder_established: bool,
}

impl Default for Supervisor {
    fn default() -> Self {
        Self::new(
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            default_daemon_path(),
            4317,
        )
    }
}

impl Supervisor {
    pub fn new(workspace_root: PathBuf, daemon_path: PathBuf, http_port: u16) -> Self {
        Self {
            observation: Observation::default(),
            failures: VecDeque::new(),
            run_started_at: None,
            retry_at: None,
            handshake_deadline: None,
            drain_deadline: None,
            quit_requested: false,
            drain_complete: false,
            terminal_event: false,
            process_exited: false,
            control_sequence: 1,
            event_decoder: EventDecoder::default(),
            event_rx: None,
            snapshot_rx: None,
            snapshot_stop: None,
            process: None,
            workspace_root,
            daemon_path,
            http_port,
            bearer_token: None,
            installed_origin: false,
            residency: ResidentController::new(),
            pending_launch_mode: process::LaunchMode::Contained,
            pre_holder_deadline: None,
            holder_established: false,
        }
    }

    pub fn state(&self) -> State {
        self.observation.state
    }
    pub fn observation(&self) -> &Observation {
        &self.observation
    }
    pub fn endpoint(&self) -> Option<&str> {
        self.observation.endpoint.as_deref()
    }
    pub fn bearer_token(&self) -> Option<&str> {
        self.bearer_token.as_deref()
    }
    pub fn workspace_dashboard_path(&self) -> Option<PathBuf> {
        if self.daemon_path.file_name().is_some_and(|name| {
            name == "membrane-daemon.exe" || name == "membrane-daemon"
        }) {
            self.daemon_path.parent().map(|parent| {
                parent.join(if cfg!(windows) {
                    "membrane-hub.exe"
                } else {
                    "membrane-hub"
                })
            })
        } else {
            None
        }
    }
    pub fn is_installed_origin(&self) -> bool {
        self.installed_origin
    }
    pub fn set_origin(&mut self, origin: workspace::RuntimeOrigin) {
        self.installed_origin = origin == workspace::RuntimeOrigin::Installed;
    }
    pub fn is_quit_complete(&self) -> bool {
        self.drain_complete
    }

    pub fn set_workspace(&mut self, workspace: &workspace::Workspace) {
        self.workspace_root = workspace.root.clone();
        self.http_port = workspace.http_port;
        self.set_origin(workspace.origin);
        self.daemon_path = workspace.daemon_path().unwrap_or_else(default_daemon_path);
    }

    /// Acquire this tray's controller lease from a stable-current identity.
    /// A non-start decision denotes peer ownership; callers must then adopt
    /// that installed controller rather than launch a duplicate daemon.
    pub fn acquire_holder(
        &mut self,
        workspace: &workspace::Workspace,
        identity: Identity,
        holder: Holder,
        now_ms: u64,
        expires_at_ms: u64,
    ) -> Result<Transition, &'static str> {
        workspace.validate_controller_identity(&identity)?;
        let decision = self
            .residency
            .acquire(identity, holder, now_ms, expires_at_ms)
            .map_err(|_| "resident_controller_lease_rejected")?;
        // A valid holder is now established: the pre-holder startup grace no
        // longer governs this process, and it can no longer be reclaimed as
        // holderless startup cleanup. This must be recorded before any spawn
        // decision below, never retrofitted afterward.
        self.holder_established = true;
        self.pre_holder_deadline = None;
        // Select the launch mode from the *pre-spawn* residency snapshot —
        // before `start_process` runs — so the choice between an ordinary
        // contained launch and an escaping shared launch is made once, ahead
        // of spawn, and never adjusted after the process is acquired.
        let snapshot = decision.snapshot;
        self.pending_launch_mode = if snapshot.hub_holders > 0 && snapshot.coderight_daemon_holders > 0 {
            process::LaunchMode::Shared
        } else {
            process::LaunchMode::Contained
        };
        if decision.start_controller {
            Ok(self.start_process(now_ms))
        } else {
            Ok(self.transition(
                State::Running,
                "resident_controller_adopted",
                now_ms,
                self.observation.pid,
                None,
            ))
        }
    }

    pub fn renew_holder(
        &mut self,
        holder: &Holder,
        now_ms: u64,
        expires_at_ms: u64,
    ) -> Result<(), &'static str> {
        self.residency
            .renew(holder, now_ms, expires_at_ms)
            .map(|_| ())
            .map_err(|_| "resident_controller_renew_rejected")
    }

    /// Only final holder release drains tray-managed automatic work.
    pub fn release_holder(
        &mut self,
        holder: &Holder,
        now_ms: u64,
    ) -> Result<Transition, &'static str> {
        let release = self
            .residency
            .release(holder)
            .map_err(|_| "resident_controller_release_rejected")?;
        if release.drain_controller {
            Ok(self.begin_drain(now_ms))
        } else {
            Ok(self.transition(
                self.observation.state,
                "resident_controller_peer_retained",
                now_ms,
                self.observation.pid,
                None,
            ))
        }
    }

    pub fn reconcile_expired_holders(&mut self, now_ms: u64) -> Option<Transition> {
        self.residency
            .reconcile_expired(now_ms)
            .drain_controller
            .then(|| self.begin_drain(now_ms))
    }

    pub fn block_startup(&mut self, reason: &str, now_ms: u64) -> Transition {
        self.close_process();
        self.retry_at = None;
        self.handshake_deadline = None;
        self.transition(State::CrashLoop, reason, now_ms, None, None)
    }

    fn transition(
        &mut self,
        state: State,
        reason: impl Into<String>,
        now_ms: u64,
        pid: Option<u32>,
        exit_code: Option<u32>,
    ) -> Transition {
        self.observation.state = state;
        self.observation.reason = reason.into();
        self.observation.pid = pid;
        self.observation.observed_at_unix_ms = now_ms;
        self.observation.exit_code = exit_code;
        Transition {
            state,
            reason: reason_from_str(&self.observation.reason),
            generation: self.observation.generation,
            pid,
            observed_at_unix_ms: now_ms,
            exit_code,
        }
    }

    fn set_generation(&mut self, generation: u64) {
        self.observation.generation = generation;
    }

    /// Pure starting transition retained for deterministic reducer tests.
    /// Use [`start_process`] from the native tray.
    #[cfg(test)]
    pub fn start(&mut self) -> Transition {
        self.retry_at = None;
        self.handshake_deadline = None;
        self.terminal_event = false;
        self.process_exited = false;
        self.transition(
            State::Starting,
            Reason::DaemonStarting.as_str(),
            now_unix_ms(),
            None,
            None,
        )
    }

    pub fn start_process(&mut self, now_ms: u64) -> Transition {
        if self.observation.generation == 0 {
            self.set_generation(1);
        }
        self.retry_at = None;
        self.handshake_deadline = Some(now_ms.saturating_add(HANDSHAKE_TIMEOUT_MS));
        self.terminal_event = false;
        self.process_exited = false;
        // Grant a bounded pre-holder grace only while no valid holder has
        // been established yet. Once a holder exists this stays cleared, so
        // a later restart of the same controller never re-arms a grace
        // window a peer holder is already relying on.
        self.pre_holder_deadline = (!self.holder_established)
            .then(|| now_ms.saturating_add(PRE_HOLDER_GRACE_MS));
        let transition = self.transition(
            State::Starting,
            Reason::DaemonStarting.as_str(),
            now_ms,
            None,
            None,
        );
        self.launch_process(now_ms).unwrap_or(transition)
    }

    pub fn begin_drain(&mut self, now_ms: u64) -> Transition {
        // Final-holder drain (or explicit quit) ends this generation's
        // residency entirely; the next `start_process` must be free to arm a
        // fresh pre-holder grace rather than inherit a stale established flag
        // from a controller that no longer has any holder.
        self.holder_established = false;
        self.pre_holder_deadline = None;
        self.quit_requested = true;
        self.drain_complete = self.process.is_none();
        self.drain_deadline = Some(now_ms.saturating_add(DRAIN_TIMEOUT_MS));
        let mut transition = self.transition(
            State::Draining,
            Reason::DaemonDraining.as_str(),
            now_ms,
            self.observation.pid,
            None,
        );
        if let Some(process) = &self.process {
            self.control_sequence = self.control_sequence.saturating_add(1);
            let command = DaemonCommandV1 {
                schema_version: DAEMON_IPC_SCHEMA_VERSION,
                sequence: self.control_sequence,
                kind: DaemonCommandKind::Drain,
            };
            if process.send_command(&command).is_err() {
                let pid = self.observation.pid;
                transition =
                    self.fail_process(now_ms, Reason::DaemonDrainTimeout.as_str(), pid, None);
            }
        }
        transition
    }

    /// Pure crash-loop transition retained for tests and portable semantics.
    #[cfg(test)]
    pub fn unexpected_exit(&mut self, now_ms: u64) -> Transition {
        self.record_unexpected_exit(now_ms, None)
    }

    fn record_unexpected_exit(&mut self, now_ms: u64, exit_code: Option<u32>) -> Transition {
        self.record_failure(now_ms, exit_code, None)
    }

    fn record_failure(
        &mut self,
        now_ms: u64,
        exit_code: Option<u32>,
        pid: Option<u32>,
    ) -> Transition {
        if let Some(started) = self.run_started_at.take() {
            if now_ms.saturating_sub(started) >= CRASH_LOOP_WINDOW_MS {
                self.failures.clear();
            }
        }
        self.failures
            .retain(|time| now_ms.saturating_sub(*time) <= CRASH_LOOP_WINDOW_MS);
        self.failures.push_back(now_ms);
        let state = if self.failures.len() >= CRASH_LOOP_THRESHOLD {
            State::CrashLoop
        } else {
            State::Backoff
        };
        let reason = if state == State::CrashLoop {
            Reason::DaemonCrashLoop
        } else {
            Reason::DaemonRestartBackoff
        };
        self.retry_at =
            (state == State::Backoff).then_some(now_ms.saturating_add(RESTART_BACKOFF_MS));
        self.transition(state, reason.as_str(), now_ms, pid, exit_code)
    }

    fn fail_process(
        &mut self,
        now_ms: u64,
        reason: &str,
        pid: Option<u32>,
        exit_code: Option<u32>,
    ) -> Transition {
        lifecycle_event(
            "membrane_tray_daemon_failure",
            serde_json::json!({"reason": reason, "pid": pid, "exitCode": exit_code, "observedAtUnixMs": now_ms}),
        );
        self.process_exited = true;
        self.terminal_event = false;
        self.handshake_deadline = None;
        self.close_process();
        if self.quit_requested {
            self.drain_complete = true;
            self.transition(State::Stopped, reason, now_ms, pid, exit_code)
        } else {
            self.record_failure(now_ms, exit_code, pid)
        }
    }

    /// Pure manual restart transition. Native restart uses
    /// [`manual_restart_process`] to relaunch exactly one child.
    #[cfg(test)]
    pub fn manual_restart(&mut self) -> Transition {
        self.failures.clear();
        self.run_started_at = None;
        self.retry_at = None;
        self.handshake_deadline = None;
        self.terminal_event = false;
        self.process_exited = false;
        self.set_generation(self.observation.generation.saturating_add(1));
        self.transition(
            State::Starting,
            Reason::DaemonStarting.as_str(),
            now_unix_ms(),
            None,
            None,
        )
    }

    pub fn manual_restart_process(&mut self, now_ms: u64) -> Transition {
        self.close_process();
        self.failures.clear();
        self.run_started_at = None;
        self.retry_at = None;
        self.handshake_deadline = Some(now_ms.saturating_add(HANDSHAKE_TIMEOUT_MS));
        self.terminal_event = false;
        self.process_exited = false;
        // Activation may signal a long-lived tray after its original
        // holderless startup allowance expired. Re-arm that bounded allowance
        // for this explicit restart; otherwise the next timer tick immediately
        // kills the new daemon against the stale deadline.
        self.pre_holder_deadline = (!self.holder_established)
            .then(|| now_ms.saturating_add(PRE_HOLDER_GRACE_MS));
        self.set_generation(self.observation.generation.saturating_add(1));
        let transition = self.transition(
            State::Starting,
            Reason::DaemonStarting.as_str(),
            now_ms,
            None,
            None,
        );
        self.launch_process(now_ms).unwrap_or(transition)
    }

    /// Drain process events, enforce handshake/drain deadlines, and perform
    /// one automatic restart when backoff expires. Called from Slint's UI timer.
    pub fn tick(&mut self, now_ms: u64) {
        let mut snapshots = Vec::new();
        if let Some(receiver) = &self.snapshot_rx {
            while let Ok(update) = receiver.try_recv() {
                snapshots.push(update);
            }
        }
        for update in snapshots {
            if update.generation == self.observation.generation && !self.process_exited {
                self.observation.admitted = update.values.admitted;
                self.observation.withheld = update.values.withheld;
                self.observation.budget = update.values.budget;
                self.observation.snapshot_observed = update.values.observed;
                if let Some(remote) = update.resident_holder.as_ref() {
                    self.note_remote_holder(remote);
                }
            }
        }

        let mut events = Vec::new();
        if let Some(receiver) = &self.event_rx {
            while let Ok(event) = receiver.try_recv() {
                events.push(event);
            }
        }
        for event in events {
            match event {
                ProcessEvent::Event(frame) => self.handle_frame(&frame, now_ms),
                ProcessEvent::ProtocolInvalid => {
                    let pid = self.observation.pid;
                    self.fail_process(now_ms, Reason::DaemonProtocolInvalid.as_str(), pid, None);
                }
                ProcessEvent::Exited { code } => self.handle_exit(code, now_ms),
            }
        }

        if self.observation.state == State::Starting
            && self
                .handshake_deadline
                .is_some_and(|deadline| now_ms >= deadline)
        {
            let pid = self.observation.pid;
            self.fail_process(now_ms, Reason::DaemonHandshakeTimeout.as_str(), pid, None);
        }

        // Pre-holder grace: a daemon started with no valid holder yet is kept
        // alive only for a bounded window. Once a holder is established
        // (`acquire_holder` clears this deadline) the daemon detaches from
        // this check entirely; a receiver-gone/EOF loss before that point
        // still routes through the ordinary process-exit/protocol-invalid
        // paths above, so this only catches a daemon that is still running
        // but has outlived its holderless startup allowance.
        if matches!(self.observation.state, State::Starting | State::Running)
            && !self.holder_established
            && self
                .pre_holder_deadline
                .is_some_and(|deadline| now_ms >= deadline)
        {
            self.pre_holder_deadline = None;
            let pid = self.observation.pid;
            self.fail_process(now_ms, Reason::DaemonPreHolderGraceExpired.as_str(), pid, None);
        }

        if self.observation.state == State::Draining
            && self
                .drain_deadline
                .is_some_and(|deadline| now_ms >= deadline)
        {
            self.process_exited = true;
            self.transition(
                State::Stopped,
                Reason::DaemonDrainTimeout.as_str(),
                now_ms,
                self.observation.pid,
                None,
            );
            self.close_process();
            self.drain_complete = true;
        }

        if self.observation.state == State::Backoff
            && self.process.is_none()
            && self.retry_at.is_some_and(|retry_at| now_ms >= retry_at)
        {
            self.retry_at = None;
            self.handshake_deadline = Some(now_ms.saturating_add(HANDSHAKE_TIMEOUT_MS));
            self.terminal_event = false;
            self.process_exited = false;
            self.transition(
                State::Starting,
                Reason::DaemonStarting.as_str(),
                now_ms,
                None,
                None,
            );
            let _ = self.launch_process(now_ms);
        }
    }

    /// Observe an already-authoritative daemon lease without mutating this
    /// tray's local registry. Only a fenced status with any nonzero Hub or
    /// CodeRight holder can satisfy startup grace; zero, failed, or mismatched
    /// observations remain non-authoritative.
    fn note_remote_holder(&mut self, remote: &snapshot::RemoteHolderObservation) {
        let status = &remote.status;
        if matches!(self.observation.state, State::Starting | State::Running)
            && (status.hub_holders > 0 || status.coderight_daemon_holders > 0)
            && status.controller_active
        {
            self.holder_established = true;
            self.pre_holder_deadline = None;
        }
    }

    fn handle_frame(&mut self, frame: &[u8], now_ms: u64) {
        // A child can close stdout before its wait notification reaches this
        // queue. Ignore frames queued after process exit.
        if self.process_exited {
            return;
        }
        let event = match self.event_decoder.decode(frame) {
            Ok(event) => event,
            Err(_) => {
                let pid = self.observation.pid;
                self.fail_process(now_ms, Reason::DaemonProtocolInvalid.as_str(), pid, None);
                return;
            }
        };
        self.observation.pid = Some(event.pid);
        self.observation.observed_at_unix_ms = event.observed_at_unix_ms;
        match event.kind {
            DaemonEventKind::Ready => {
                self.run_started_at = Some(now_ms);
                self.failures
                    .retain(|time| now_ms.saturating_sub(*time) <= CRASH_LOOP_WINDOW_MS);
                self.handshake_deadline = None;
                self.observation.endpoint = event.endpoint;
                if let (Some(endpoint), Some(token)) =
                    (self.observation.endpoint.clone(), self.bearer_token.clone())
                {
                    self.start_snapshot_polling(endpoint, token);
                }
                self.transition(
                    State::Running,
                    Reason::DaemonReady.as_str(),
                    now_ms,
                    Some(event.pid),
                    None,
                );
            }
            DaemonEventKind::Draining => {
                self.transition(
                    State::Draining,
                    event
                        .reason
                        .as_deref()
                        .unwrap_or(Reason::DaemonDraining.as_str()),
                    now_ms,
                    Some(event.pid),
                    None,
                );
            }
            DaemonEventKind::Drained => {
                self.process_exited = true;
                self.transition(
                    State::Stopped,
                    Reason::DaemonExited.as_str(),
                    now_ms,
                    Some(event.pid),
                    None,
                );
                self.drain_complete = self.quit_requested;
            }
            DaemonEventKind::Fatal => {
                let reason = event
                    .reason
                    .as_deref()
                    .unwrap_or(Reason::DaemonReadyFailed.as_str());
                self.fail_process(now_ms, reason, Some(event.pid), None);
            }
        }
    }

    fn handle_exit(&mut self, code: u32, now_ms: u64) {
        if self.process_exited {
            return;
        }
        self.process_exited = true;
        self.close_process();
        self.handshake_deadline = None;
        if self.observation.state == State::Draining || self.quit_requested {
            self.transition(
                State::Stopped,
                if self.observation.reason == Reason::DaemonDrainTimeout.as_str() {
                    Reason::DaemonDrainTimeout.as_str()
                } else {
                    Reason::DaemonExited.as_str()
                },
                now_ms,
                self.observation.pid,
                Some(code),
            );
            self.drain_complete = self.quit_requested;
        } else {
            self.terminal_event = false;
            self.record_unexpected_exit(now_ms, Some(code));
        }
    }

    fn launch_process(&mut self, now_ms: u64) -> Option<Transition> {
        self.close_process();
        self.process_exited = false;
        if workspace::installed_tray_path().is_some() {
            if let Err(error) = membrane_runtime::serve::prepare_installed_credential_for_exe(&self.daemon_path) {
                eprintln!("[startup] installed credential preparation failed: {error}");
            return Some(self.fail_process(now_ms, "workspace_api_token_migration_required", None, None));
            }
        }
        // The mode selected in `acquire_holder` (before this spawn, from the
        // pre-spawn residency snapshot) governs this launch. It is read, not
        // recomputed, here — there is no retrofit of containment/escape
        // after the process already exists.
        let launch_result = match self.pending_launch_mode {
            process::LaunchMode::Contained => process::launch(&self.daemon_path)
                .map_err(process::LaunchError::from),
            process::LaunchMode::Shared => process::launch_shared(&self.daemon_path),
        };
        let process = match launch_result {
            Ok(process) => process,
            Err(process::LaunchError::JobEscapeDenied) => {
                // Typed failure: a shared launch must not silently fall back
                // to a contained spawn, because that would retrofit
                // containment onto a process the caller already decided
                // must escape. Ordinary contained containment paths are
                // unaffected by this branch.
                return Some(self.fail_process(
                    now_ms,
                    Reason::DaemonJobEscapeDenied.as_str(),
                    None,
                    None,
                ));
            }
            Err(process::LaunchError::Io(error)) => {
                let reason = error
                    .raw_os_error()
                    .map(|code| format!("daemon_spawn_failed_windows_{code}"))
                    .unwrap_or_else(|| Reason::DaemonSpawnFailed.as_str().to_owned());
                return Some(self.fail_process(now_ms, &reason, None, None));
            }
        };

        let token = match workspace::api_token(&self.workspace_root) {
            Ok(token) => token,
            Err(reason) => {
                let pid = Some(process.process_id());
                drop(process);
                return Some(self.fail_process(now_ms, reason, pid, None));
            }
        };
        if token.len() != 64 {
            let pid = Some(process.process_id());
            drop(process);
            return Some(self.fail_process(
                now_ms,
                Reason::DaemonProtocolInvalid.as_str(),
                pid,
                None,
            ));
        }
        let launch = DaemonLaunchV1 {
            schema_version: DAEMON_IPC_SCHEMA_VERSION,
            sequence: 1,
            kind: DaemonLaunchKind::Launch,
            workspace_root: self.workspace_root.to_string_lossy().into_owned(),
            http_port: self.http_port,
            bearer_token: token.clone(),
            parent_pid: std::process::id(),
        };
        if process.send_launch(&launch).is_err() {
            let pid = Some(process.process_id());
            drop(process);
            return Some(self.fail_process(
                now_ms,
                Reason::DaemonProtocolInvalid.as_str(),
                pid,
                None,
            ));
        }
        let (sender, receiver) = mpsc::channel();
        process.start_readers(sender);
        self.event_rx = Some(receiver);
        self.event_decoder = EventDecoder::default();
        self.control_sequence = 1;
        self.bearer_token = Some(token);
        let pid = process.process_id();
        self.observation.pid = Some(pid);
        self.observation.process_identity = process
            .creation_time_ticks()
            .map(|creation_ticks| (pid, creation_ticks));
        self.process = Some(process);
        None
    }

    fn close_process(&mut self) {
        if let Some(stop) = self.snapshot_stop.take() {
            stop.store(true, std::sync::atomic::Ordering::Release);
        }
        self.snapshot_rx = None;
        self.event_rx = None;
        self.event_decoder = EventDecoder::default();
        self.bearer_token = None;
        self.observation.endpoint = None;
        let unknown = snapshot::SnapshotValues::unknown("snapshot_unavailable");
        self.observation.admitted = unknown.admitted;
        self.observation.withheld = unknown.withheld;
        self.observation.budget = unknown.budget;
        self.observation.snapshot_observed = unknown.observed;
        self.observation.process_identity = None;
        self.process.take(); // Drop closes job, coupling daemon lifetime.
    }

    fn start_snapshot_polling(&mut self, endpoint: String, token: String) {
        if let Some(stop) = self.snapshot_stop.take() {
            stop.store(true, std::sync::atomic::Ordering::Release);
        }
        let (receiver, stop) =
            snapshot::start_polling(endpoint, token, self.observation.generation);
        self.snapshot_rx = Some(receiver);
        self.snapshot_stop = Some(stop);
    }
}

fn reason_from_str(value: &str) -> Reason {
    match value {
        "daemon_starting" => Reason::DaemonStarting,
        "daemon_ready" => Reason::DaemonReady,
        "daemon_draining" => Reason::DaemonDraining,
        "daemon_exited" => Reason::DaemonExited,
        "daemon_restart_backoff" => Reason::DaemonRestartBackoff,
        "daemon_crash_loop" => Reason::DaemonCrashLoop,
        "daemon_protocol_invalid" => Reason::DaemonProtocolInvalid,
        "daemon_spawn_failed" => Reason::DaemonSpawnFailed,
        "daemon_handshake_timeout" => Reason::DaemonHandshakeTimeout,
        "daemon_ready_failed" => Reason::DaemonReadyFailed,
        "daemon_drain_timeout" => Reason::DaemonDrainTimeout,
        "daemon_pre_holder_grace_expired" => Reason::DaemonPreHolderGraceExpired,
        "daemon_job_escape_denied" => Reason::DaemonJobEscapeDenied,
        _ => Reason::DaemonReadyFailed,
    }
}

pub fn default_daemon_path() -> PathBuf {
    // A production tray is always launched through stable `current`. Never
    // honor a development override in that process; version-backed binaries
    // remain an implementation detail behind this stable projection.
    if let Some(installed) = workspace::installed_tray_path() {
        if std::env::current_exe()
            .ok()
            .is_some_and(|exe| same_path(&exe, &installed))
        {
            return installed.with_file_name(if cfg!(windows) {
                "membrane-daemon.exe"
            } else {
                "membrane-daemon"
            });
        }
    }
    if let Some(path) = std::env::var_os("MEMBRANE_DAEMON_PATH") {
        return PathBuf::from(path);
    }
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .map(|path| {
            path.join(if cfg!(windows) {
                "membrane-daemon.exe"
            } else {
                "membrane-daemon"
            })
        })
        .unwrap_or_else(|| {
            PathBuf::from(if cfg!(windows) {
                "membrane-daemon.exe"
            } else {
                "membrane-daemon"
            })
        })
}

fn same_path(left: &Path, right: &Path) -> bool {
    left.to_string_lossy().eq_ignore_ascii_case(&right.to_string_lossy())
}

pub fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_supervisor() -> Supervisor {
        Supervisor::new(
            PathBuf::from(r"C:\workspace"),
            PathBuf::from(r"C:\daemon.exe"),
            4317,
        )
    }

    #[test]
    fn repaired_installed_workspace_replaces_previous_executable_and_port() {
        let mut supervisor = test_supervisor();
        let current = PathBuf::from("/installed/Membrane/current");
        let resolved = workspace::Workspace {
            root: PathBuf::from("/installed/Membrane/state"),
            http_port: workspace::INSTALLED_PORT,
            origin: workspace::RuntimeOrigin::Installed,
            product_root: Some(PathBuf::from("/installed/Membrane")),
            stable_current: Some(current.clone()),
            version_root: Some(PathBuf::from("/installed/Membrane/versions/0.1.24")),
            state_root: Some(PathBuf::from("/installed/Membrane/state")),
        };
        supervisor.set_workspace(&resolved);
        assert_eq!(supervisor.daemon_path, resolved.daemon_path().unwrap());
        assert_eq!(supervisor.workspace_root, resolved.root);
        assert_eq!(supervisor.http_port, workspace::INSTALLED_PORT);
        assert!(supervisor.is_installed_origin());
    }

    #[test]
    fn third_fast_exit_enters_crash_loop() {
        let mut supervisor = test_supervisor();
        assert_eq!(supervisor.start().state, State::Starting);
        assert_eq!(supervisor.unexpected_exit(1_000).state, State::Backoff);
        assert_eq!(supervisor.unexpected_exit(2_000).state, State::Backoff);
        let transition = supervisor.unexpected_exit(3_000);
        assert_eq!(
            (transition.state, transition.reason.as_str()),
            (State::CrashLoop, "daemon_crash_loop")
        );
    }

    #[test]
    fn manual_restart_clears_history_and_increments_generation() {
        let mut supervisor = test_supervisor();
        supervisor.unexpected_exit(1);
        let transition = supervisor.manual_restart();
        assert_eq!(
            (transition.state, transition.generation),
            (State::Starting, 1)
        );
        assert_eq!(supervisor.unexpected_exit(2).state, State::Backoff);
    }

    #[test]
    fn long_run_expires_old_failures() {
        let mut supervisor = test_supervisor();
        supervisor.unexpected_exit(1);
        supervisor.run_started_at = Some(1_002);
        supervisor.record_unexpected_exit(61_002, None);
        assert_eq!(supervisor.unexpected_exit(61_003).state, State::Backoff);
    }

    #[test]
    fn stable_reason_vocabulary_covers_spawn_and_handshake_failures() {
        assert_eq!(Reason::DaemonSpawnFailed.as_str(), "daemon_spawn_failed");
        assert_eq!(
            Reason::DaemonHandshakeTimeout.as_str(),
            "daemon_handshake_timeout"
        );
        assert_eq!(Reason::DaemonReadyFailed.as_str(), "daemon_ready_failed");
        assert_eq!(Reason::DaemonDrainTimeout.as_str(), "daemon_drain_timeout");
    }

    #[test]
    fn explicit_restart_rearms_only_holderless_startup_grace() {
        let mut supervisor = test_supervisor();
        supervisor.pre_holder_deadline = Some(1);
        supervisor.manual_restart_process(10_000);
        assert_eq!(
            supervisor.pre_holder_deadline,
            Some(10_000 + PRE_HOLDER_GRACE_MS)
        );

        supervisor.holder_established = true;
        supervisor.manual_restart_process(20_000);
        assert_eq!(supervisor.pre_holder_deadline, None);
    }

    #[test]
    fn remote_both_holder_status_clears_grace_without_local_acquire() {
        let mut supervisor = test_supervisor();
        supervisor.observation.state = State::Running;
        supervisor.pre_holder_deadline = Some(30_000);
        let remote = snapshot::RemoteHolderObservation {
            controller: membrane_protocol::ResidentControllerIdentityV1 {
                installation_id: "installation".into(),
                cortex_store_id: "store".into(),
                release_generation: "release".into(),
                startup_generation: 1,
                stable_current: "C:/Membrane/current".into(),
            },
            status: membrane_protocol::ResidentHolderStatusV1 {
                controller_active: true,
                services_ready: true,
                services_unavailable_reason: None,
                hub_holders: 1,
                coderight_daemon_holders: 1,
            },
        };
        supervisor.note_remote_holder(&remote);
        assert!(supervisor.holder_established);
        assert_eq!(supervisor.pre_holder_deadline, None);
        assert_eq!(supervisor.residency.snapshot().hub_holders, 0);
    }

    fn remote_status(
        hub_holders: u32,
        coderight_daemon_holders: u32,
        services_ready: bool,
        services_unavailable_reason: Option<membrane_protocol::ResidentServicesUnavailableV1>,
    ) -> snapshot::RemoteHolderObservation {
        snapshot::RemoteHolderObservation {
            controller: membrane_protocol::ResidentControllerIdentityV1 {
                installation_id: "installation".into(),
                cortex_store_id: "store".into(),
                release_generation: "release".into(),
                startup_generation: 1,
                stable_current: "C:/Membrane/current".into(),
            },
            status: membrane_protocol::ResidentHolderStatusV1 {
                controller_active: true,
                services_ready,
                services_unavailable_reason,
                hub_holders,
                coderight_daemon_holders,
            },
        }
    }

    #[test]
    fn remote_single_holder_status_establishes_without_local_acquire() {
        let cases = [
            (1, 0),
            (0, 1),
        ];
        for (hub_holders, coderight_daemon_holders) in cases {
            let mut supervisor = test_supervisor();
            supervisor.observation.state = State::Running;
            supervisor.pre_holder_deadline = Some(30_000);
            supervisor.note_remote_holder(&remote_status(
                hub_holders,
                coderight_daemon_holders,
                true,
                None,
            ));
            assert!(supervisor.holder_established);
            assert_eq!(supervisor.pre_holder_deadline, None);
            assert_eq!(supervisor.residency.snapshot().hub_holders, 0);
        }
    }

    #[test]
    fn remote_active_holder_preserves_startup_while_services_catch_up() {
        for (hub, coderight) in [(1, 0), (0, 1)] {
            let mut supervisor = test_supervisor();
            supervisor.observation.state = State::Running;
            supervisor.pre_holder_deadline = Some(30_000);
            supervisor.note_remote_holder(&remote_status(
                hub, coderight, false,
                Some(membrane_protocol::ResidentServicesUnavailableV1::BlueprintWatcherUnavailable),
            ));
            assert!(supervisor.holder_established);
            assert_eq!(supervisor.pre_holder_deadline, None);
            assert_eq!(supervisor.residency.snapshot().hub_holders, 0);
        }
    }

    #[test]
    fn remote_zero_or_inactive_never_establishes() {
        let cases = [
            (0, 0, true, None),
            (
                0,
                0,
                false,
                Some(membrane_protocol::ResidentServicesUnavailableV1::CatalogUnavailable),
            ),
        ];
        for (hub_holders, coderight_daemon_holders, services_ready, reason) in cases {
            let mut supervisor = test_supervisor();
            supervisor.observation.state = State::Running;
            supervisor.pre_holder_deadline = Some(30_000);
            supervisor.note_remote_holder(&remote_status(
                hub_holders,
                coderight_daemon_holders,
                services_ready,
                reason,
            ));
            assert!(!supervisor.holder_established);
            assert_eq!(supervisor.pre_holder_deadline, Some(30_000));
            assert_eq!(supervisor.residency.snapshot().hub_holders, 0);
        }

        let mut inactive = test_supervisor();
        inactive.observation.state = State::Running;
        inactive.pre_holder_deadline = Some(30_000);
        let mut remote = remote_status(1, 0, true, None);
        remote.status.controller_active = false;
        inactive.note_remote_holder(&remote);
        assert!(!inactive.holder_established);
        assert_eq!(inactive.pre_holder_deadline, Some(30_000));
    }
}
