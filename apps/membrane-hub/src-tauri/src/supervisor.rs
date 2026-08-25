//! Hub-owned in-process Membrane runtime lifecycle.
//!
//! The active Hub process owns the only resident runtime. This module never
//! launches or adopts a Membrane child process; it starts the runtime library
//! on a Hub-owned thread and drains that thread during Hub shutdown.

use membrane_runtime::service::{run_hub_runtime, LifecycleControl};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{Mutex, MutexGuard},
    thread::JoinHandle,
    time::{Duration, Instant},
};

const CRASH_WINDOW: Duration = Duration::from_secs(60);
const CRASH_LOOP_THRESHOLD: usize = 3;
#[cfg(not(test))]
const DRAIN_TIMEOUT: Duration = Duration::from_secs(7);
#[cfg(test)]
const DRAIN_TIMEOUT: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceStatus {
    Running,
    Unavailable,
    CrashLoop,
}

struct ManagedRuntime {
    control: LifecycleControl,
    thread: JoinHandle<Result<(), String>>,
    started_at: Instant,
    drain_kind: Option<DrainKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DrainKind {
    StartupFailed,
    OperatorStop,
}

#[derive(Default)]
struct State {
    runtime: Option<ManagedRuntime>,
    crashes: VecDeque<Instant>,
    crash_loop: bool,
    last_error: Option<String>,
}

/// One Hub owns one in-process Membrane runtime. The runtime crate enforces
/// the same invariant process-wide before it binds storage or a loopback port.
pub struct Supervisor {
    state: Mutex<State>,
    workspace_root: PathBuf,
}

impl Supervisor {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            state: Mutex::new(State::default()),
            workspace_root,
        }
    }

    pub fn start(&self) -> Result<ServiceStatus, String> {
        let mut state = self.lock_state()?;
        if state.crash_loop {
            return Ok(ServiceStatus::CrashLoop);
        }
        if state
            .runtime
            .as_ref()
            .is_some_and(|runtime| !runtime.thread.is_finished() && runtime.drain_kind.is_none())
        {
            return Ok(ServiceStatus::Running);
        }
        if state
            .runtime
            .as_ref()
            .is_some_and(|runtime| !runtime.thread.is_finished())
        {
            return Ok(ServiceStatus::Unavailable);
        }
        if state.runtime.is_some() {
            Self::collect_finished(&mut state);
            if state.crash_loop {
                return Ok(ServiceStatus::CrashLoop);
            }
        }

        let control = LifecycleControl::default();
        let thread_control = control.clone();
        let failure_control = control.clone();
        let root = self.workspace_root.clone();
        let thread = std::thread::Builder::new()
            .name("membrane-hub-runtime".into())
            .spawn(move || {
                let result = run_hub_runtime(&root, thread_control);
                if let Err(error) = &result {
                    failure_control.fail(error.clone());
                }
                result
            })
            .map_err(|_| "membrane_hub_runtime_thread_unavailable")?;
        let mut managed = ManagedRuntime {
            control,
            thread,
            started_at: Instant::now(),
            drain_kind: None,
        };
        let ready = managed.control.wait_until_ready();
        match ready {
            Ok(_) => {
                state.runtime = Some(managed);
                state.last_error = None;
                Ok(ServiceStatus::Running)
            }
            Err(error) => {
                let started_at = managed.started_at;
                managed.control.request_drain(Some("startup_failed"));
                managed.drain_kind = Some(DrainKind::StartupFailed);
                if Self::wait_until_finished(&managed.thread) {
                    let _ = managed.thread.join();
                    Self::record_exit(&mut state, started_at);
                } else {
                    state.runtime = Some(managed);
                }
                state.last_error = Some(error.clone());
                Err(error)
            }
        }
    }

    pub fn supervise(&self) -> ServiceStatus {
        let mut state = match self.lock_state() {
            Ok(state) => state,
            Err(_) => return ServiceStatus::Unavailable,
        };
        if state.crash_loop {
            return ServiceStatus::CrashLoop;
        }
        let Some(runtime) = state.runtime.as_ref() else {
            return ServiceStatus::Unavailable;
        };
        if !runtime.thread.is_finished() && runtime.drain_kind.is_none() {
            return ServiceStatus::Running;
        }
        if !runtime.thread.is_finished() {
            return ServiceStatus::Unavailable;
        }
        Self::collect_finished(&mut state)
    }

    pub fn stop(&self) {
        let runtime = self
            .state
            .lock()
            .ok()
            .and_then(|mut state| state.runtime.take());
        let Some(mut runtime) = runtime else {
            return;
        };
        runtime.control.request_drain(Some("stop"));
        runtime.drain_kind = Some(DrainKind::OperatorStop);
        if Self::wait_until_finished(&runtime.thread) {
            let _ = runtime.thread.join();
        } else {
            runtime.control.fail("membrane_hub_runtime_drain_timeout");
            if let Ok(mut state) = self.state.lock() {
                state.last_error = Some("membrane_hub_runtime_drain_timeout".into());
                state.runtime = Some(runtime);
            }
        }
    }

    pub fn last_error(&self) -> Option<String> {
        self.state
            .lock()
            .ok()
            .and_then(|state| state.last_error.clone())
    }

    fn lock_state(&self) -> Result<MutexGuard<'_, State>, String> {
        self.state
            .lock()
            .map_err(|_| "service_state_unavailable".to_string())
    }

    fn collect_finished(state: &mut State) -> ServiceStatus {
        let Some(runtime) = state.runtime.take() else {
            return ServiceStatus::Unavailable;
        };
        let started_at = runtime.started_at;
        let drain_kind = runtime.drain_kind;
        match runtime.thread.join() {
            Ok(Ok(())) => state.last_error = Some("membrane_hub_runtime_stopped".into()),
            Ok(Err(error)) => state.last_error = Some(error),
            Err(_) => state.last_error = Some("membrane_hub_runtime_panicked".into()),
        }
        if drain_kind == Some(DrainKind::OperatorStop) {
            return ServiceStatus::Unavailable;
        }
        Self::record_exit(state, started_at)
    }

    fn wait_until_finished(thread: &JoinHandle<Result<(), String>>) -> bool {
        let deadline = Instant::now() + DRAIN_TIMEOUT;
        while !thread.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(25));
        }
        thread.is_finished()
    }

    fn record_exit(state: &mut State, started_at: Instant) -> ServiceStatus {
        let now = Instant::now();
        if now.duration_since(started_at) >= CRASH_WINDOW {
            state.crashes.clear();
        }
        state.crashes.push_back(now);
        while state
            .crashes
            .front()
            .is_some_and(|time| now.duration_since(*time) > CRASH_WINDOW)
        {
            state.crashes.pop_front();
        }
        if state.crashes.len() >= CRASH_LOOP_THRESHOLD {
            state.crash_loop = true;
            ServiceStatus::CrashLoop
        } else {
            ServiceStatus::Unavailable
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inactive_supervisor_is_typed_unavailable() {
        let supervisor = Supervisor::new(PathBuf::from("missing"));
        assert_eq!(supervisor.supervise(), ServiceStatus::Unavailable);
        assert!(supervisor.last_error().is_none());
    }

    #[test]
    fn repeated_fast_exits_enter_crash_loop() {
        let mut state = State::default();
        assert_eq!(
            Supervisor::record_exit(&mut state, Instant::now()),
            ServiceStatus::Unavailable
        );
        assert_eq!(
            Supervisor::record_exit(&mut state, Instant::now()),
            ServiceStatus::Unavailable
        );
        assert_eq!(
            Supervisor::record_exit(&mut state, Instant::now()),
            ServiceStatus::CrashLoop
        );
    }

    #[test]
    fn timed_out_stop_retains_draining_thread_until_it_can_be_joined() {
        let supervisor = Supervisor::new(PathBuf::from("missing"));
        let control = LifecycleControl::default();
        let thread_control = control.clone();
        let thread = std::thread::spawn(move || {
            while !thread_control.shutdown_requested() {
                std::thread::sleep(Duration::from_millis(5));
            }
            std::thread::sleep(Duration::from_millis(200));
            Ok(())
        });
        supervisor.state.lock().unwrap().runtime = Some(ManagedRuntime {
            control,
            thread,
            started_at: Instant::now(),
            drain_kind: None,
        });

        supervisor.stop();
        assert_eq!(supervisor.supervise(), ServiceStatus::Unavailable);
        assert!(supervisor.state.lock().unwrap().runtime.is_some());
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(supervisor.supervise(), ServiceStatus::Unavailable);
        assert!(supervisor.state.lock().unwrap().runtime.is_none());
    }

    #[test]
    fn retained_startup_failures_still_enter_crash_loop_after_join() {
        let mut state = State::default();
        for expected in [
            ServiceStatus::Unavailable,
            ServiceStatus::Unavailable,
            ServiceStatus::CrashLoop,
        ] {
            state.runtime = Some(ManagedRuntime {
                control: LifecycleControl::default(),
                thread: std::thread::spawn(|| Err("startup failed".into())),
                started_at: Instant::now(),
                drain_kind: Some(DrainKind::StartupFailed),
            });
            while !state.runtime.as_ref().unwrap().thread.is_finished() {
                std::thread::yield_now();
            }
            assert_eq!(Supervisor::collect_finished(&mut state), expected);
        }
        assert!(state.crash_loop);
    }
}
