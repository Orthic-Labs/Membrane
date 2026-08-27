//! Tray-owned daemon supervisor.
//!
//! The reducer is deliberately small and deterministic so crash-loop and
//! drain semantics remain testable without a desktop. The Windows process
//! plumbing lives in `process.rs`; this type owns its lifetime, protocol
//! reader, restart policy, and user-visible observation.

use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{
        atomic::AtomicBool,
        mpsc::{self, Receiver},
        Arc,
    },
};

use membrane_protocol::{
    DaemonCommandKind, DaemonCommandV1, DaemonEventKind, DaemonLaunchKind, DaemonLaunchV1,
    DAEMON_IPC_SCHEMA_VERSION,
};

use crate::{
    ipc::EventDecoder,
    process::{self, DaemonProcess, ProcessEvent},
    snapshot::{self, SnapshotUpdate},
};

pub const CRASH_LOOP_THRESHOLD: usize = 3;
pub const CRASH_LOOP_WINDOW_MS: u64 = 60_000;
pub const RESTART_BACKOFF_MS: u64 = 1_000;
pub const HANDSHAKE_TIMEOUT_MS: u64 = 10_000;
pub const DRAIN_TIMEOUT_MS: u64 = 7_000;

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
    pub fn is_quit_complete(&self) -> bool {
        self.drain_complete
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
        let process = match process::launch(&self.daemon_path) {
            Ok(process) => process,
            Err(_) => {
                return Some(self.fail_process(
                    now_ms,
                    Reason::DaemonSpawnFailed.as_str(),
                    None,
                    None,
                ));
            }
        };

        let mut token_bytes = [0_u8; 32];
        if getrandom::fill(&mut token_bytes).is_err() {
            let pid = Some(process.process_id());
            drop(process);
            return Some(self.fail_process(
                now_ms,
                Reason::DaemonProtocolInvalid.as_str(),
                pid,
                None,
            ));
        }
        let token = token_bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
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
        self.observation.pid = Some(process.process_id());
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
        _ => Reason::DaemonReadyFailed,
    }
}

pub fn default_daemon_path() -> PathBuf {
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
}
