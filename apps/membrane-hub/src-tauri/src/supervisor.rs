//! Membrane Hub's direct service supervisor.
//!
//! Provenance: adapted from Orthic `feed3a0` `src-tauri/src/supervisor.rs`.
//! Orthic's product-manifest fan-out is intentionally not retained: this Hub
//! owns one Membrane Cortex service, while Blueprint, Guide, Pull, Push, &
//! Adapt are exposed through Membrane's one snapshot contract.

use std::{
    collections::VecDeque,
    io::Read,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::Mutex,
    time::{Duration, Instant},
};

const MAX_START_ATTEMPTS: usize = 5;
const BASE_BACKOFF: Duration = Duration::from_millis(250);
const MAX_BACKOFF: Duration = Duration::from_secs(8);
const CRASH_WINDOW: Duration = Duration::from_secs(60);
const CRASH_LOOP_THRESHOLD: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceStatus {
    Running,
    Unavailable,
    CrashLoop,
}

struct ManagedService {
    child: Child,
    started_at: Instant,
}

#[derive(Default)]
struct State {
    service: Option<ManagedService>,
    crashes: VecDeque<Instant>,
    crash_loop: bool,
}

/// One Hub owns one Cortex service process.  Existing processes are never
/// adopted: a conflicting endpoint is an explicit unavailable state, which
/// prevents two resident Hubs from silently claiming one service.
pub struct Supervisor {
    state: Mutex<State>,
    executable: PathBuf,
    workspace_root: PathBuf,
}

impl Supervisor {
    pub fn new(executable: PathBuf, workspace_root: PathBuf) -> Self {
        Self { state: Mutex::new(State::default()), executable, workspace_root }
    }

    pub fn backoff_delay(attempt: usize) -> Duration {
        let multiplier = 1_u64 << attempt.min(32);
        BASE_BACKOFF
            .checked_mul(multiplier as u32)
            .unwrap_or(MAX_BACKOFF)
            .min(MAX_BACKOFF)
    }

    pub fn start(&self) -> Result<ServiceStatus, String> {
        {
            let mut state = self.state.lock().map_err(|_| "service_state_unavailable")?;
            if state.crash_loop {
                return Ok(ServiceStatus::CrashLoop);
            }
            if state
                .service
                .as_mut()
                .is_some_and(|managed| managed.child.try_wait().ok().flatten().is_none())
            {
                return Ok(ServiceStatus::Running);
            }
            state.service = None;
        }
        for attempt in 0..MAX_START_ATTEMPTS {
            match self.spawn() {
                Ok(service) => {
                    self.state.lock().map_err(|_| "service_state_unavailable")?.service = Some(service);
                    return Ok(ServiceStatus::Running);
                }
                Err(error) if attempt + 1 == MAX_START_ATTEMPTS => return Err(error),
                Err(_) => std::thread::sleep(Self::backoff_delay(attempt)),
            }
        }
        Err("cortex_service_start_failed".into())
    }

    pub fn supervise(&self) -> ServiceStatus {
        let exited_at = {
            let mut state = match self.state.lock() { Ok(state) => state, Err(_) => return ServiceStatus::Unavailable };
            if state.crash_loop { return ServiceStatus::CrashLoop; }
            let Some(service) = state.service.as_mut() else { return ServiceStatus::Unavailable; };
            if service.child.try_wait().ok().flatten().is_none() { return ServiceStatus::Running; }
            service.started_at
        };
        let mut state = match self.state.lock() { Ok(state) => state, Err(_) => return ServiceStatus::Unavailable };
        let now = Instant::now();
        if now.duration_since(exited_at) >= CRASH_WINDOW { state.crashes.clear(); }
        state.crashes.push_back(now);
        while state.crashes.front().is_some_and(|time| now.duration_since(*time) > CRASH_WINDOW) { state.crashes.pop_front(); }
        state.service = None;
        if state.crashes.len() >= CRASH_LOOP_THRESHOLD {
            state.crash_loop = true;
            ServiceStatus::CrashLoop
        } else {
            ServiceStatus::Unavailable
        }
    }

    pub fn stop(&self) {
        let service = self.state.lock().ok().and_then(|mut state| state.service.take());
        if let Some(mut service) = service {
            let _ = service.child.kill();
            let _ = service.child.wait();
        }
    }

    fn spawn(&self) -> Result<ManagedService, String> {
        if !self.executable.is_file() { return Err("cortex_service_missing".into()); }
        let mut child = Command::new(&self.executable)
            .env("MEMBRANE_OWNER_PIPE", "1")
            .env("WORKSPACE_ROOT", &self.workspace_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| "cortex_service_start_failed")?;
        std::thread::sleep(Duration::from_millis(120));
        if child.try_wait().map_err(|_| "cortex_service_wait_failed")?.is_some() {
            let mut stderr = String::new();
            if let Some(mut pipe) = child.stderr.take() { let _ = pipe.read_to_string(&mut stderr); }
            // Do not adopt an existing process: the visible state remains
            // unavailable & only this supervisor ever owns a child handle.
            return Err(if stderr.to_lowercase().contains("already in use") { "cortex_service_already_owned" } else { "cortex_service_start_failed" }.into());
        }
        Ok(ManagedService { child, started_at: Instant::now() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn backoff_is_capped() {
        assert_eq!(Supervisor::backoff_delay(0), Duration::from_millis(250));
        assert_eq!(Supervisor::backoff_delay(5), Duration::from_secs(8));
        assert_eq!(Supervisor::backoff_delay(50), Duration::from_secs(8));
    }
}
