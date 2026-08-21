//! Membrane Hub's direct resident supervisor.
//!
//! Provenance: adapted from Orthic `feed3a0` `src-tauri/src/supervisor.rs`.
//! Orthic's product-manifest fan-out is intentionally not retained: this Hub
//! owns one Membrane supervisor-child through a typed inherited-stdio lease.

use membrane_protocol::{
    ResidentHelloV1, ResidentLeaseV1, ResidentLifecycleFrameV1, RESIDENT_LEASE_SCHEMA_VERSION,
};
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver},
        Mutex,
    },
    time::{Duration, Instant},
};

const MAX_START_ATTEMPTS: usize = 5;
const BASE_BACKOFF: Duration = Duration::from_millis(250);
const MAX_BACKOFF: Duration = Duration::from_secs(8);
const CRASH_WINDOW: Duration = Duration::from_secs(60);
const CRASH_LOOP_THRESHOLD: usize = 3;
const LIFECYCLE_TIMEOUT: Duration = Duration::from_secs(8);
const DRAIN_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_RESIDENT_LOG_BYTES: u64 = 2 * 1024 * 1024;
static NEXT_FENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceStatus {
    Running,
    Unavailable,
    CrashLoop,
}

struct ManagedService {
    child: Child,
    stdin: Option<ChildStdin>,
    frames: Receiver<Result<ResidentLifecycleFrameV1, String>>,
    lease: ResidentLeaseV1,
    started_at: Instant,
}

#[derive(Default)]
struct State {
    service: Option<ManagedService>,
    crashes: VecDeque<Instant>,
    crash_loop: bool,
}

/// One Hub owns one Membrane supervisor-child. Existing processes are never
/// adopted: a conflicting endpoint is an explicit unavailable state, which
/// prevents two resident Hubs from silently claiming one service.
pub struct Supervisor {
    state: Mutex<State>,
    executable: PathBuf,
    workspace_root: PathBuf,
    resident_log_path: PathBuf,
}

impl Supervisor {
    pub fn new(executable: PathBuf, workspace_root: PathBuf, resident_log_path: PathBuf) -> Self {
        Self {
            state: Mutex::new(State::default()),
            executable,
            workspace_root,
            resident_log_path,
        }
    }

    pub fn backoff_delay(attempt: usize) -> Duration {
        let multiplier = 1_u32.checked_shl(attempt as u32).unwrap_or(u32::MAX);
        BASE_BACKOFF
            .checked_mul(multiplier)
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
                    self.state
                        .lock()
                        .map_err(|_| "service_state_unavailable")?
                        .service = Some(service);
                    return Ok(ServiceStatus::Running);
                }
                Err(error) if attempt + 1 == MAX_START_ATTEMPTS => return Err(error),
                Err(_) => std::thread::sleep(Self::backoff_delay(attempt)),
            }
        }
        Err("membrane_hub_start_failed".into())
    }

    pub fn supervise(&self) -> ServiceStatus {
        let exited_at = {
            let mut state = match self.state.lock() {
                Ok(state) => state,
                Err(_) => return ServiceStatus::Unavailable,
            };
            if state.crash_loop {
                return ServiceStatus::CrashLoop;
            }
            let lifecycle_invalid = {
                let Some(service) = state.service.as_mut() else {
                    return ServiceStatus::Unavailable;
                };
                let mut invalid = false;
                while let Ok(frame) = service.frames.try_recv() {
                    match frame {
                        Ok(frame)
                            if frame.kind == "register"
                                && frame.state.as_deref() == Some("failed") =>
                        {
                            invalid = true;
                            break;
                        }
                        Ok(frame) if frame.kind == "ack" && frame.fence == service.lease.fence => {}
                        Ok(_) | Err(_) => {
                            invalid = true;
                            break;
                        }
                    }
                }
                invalid
            };
            if lifecycle_invalid {
                return Self::mark_unavailable(&mut state);
            }
            let service = state.service.as_mut().expect("service checked above");
            if service.child.try_wait().ok().flatten().is_none() {
                return ServiceStatus::Running;
            }
            service.started_at
        };
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => return ServiceStatus::Unavailable,
        };
        Self::record_exit(&mut state, exited_at)
    }

    pub fn stop(&self) {
        let service = self
            .state
            .lock()
            .ok()
            .and_then(|mut state| state.service.take());
        if let Some(mut service) = service {
            let command = ResidentLifecycleFrameV1 {
                kind: "command".into(),
                state: None,
                command: Some("drain".into()),
                fence: service.lease.fence,
                endpoint: None,
                capability: Some(service.lease.capability.clone()),
            };
            if let Some(mut stdin) = service.stdin.take() {
                let _ = write_frame(&mut stdin, &command);
                // Dropping the sole write end is the authoritative EOF drain signal.
            }
            let deadline = Instant::now() + DRAIN_TIMEOUT;
            while Instant::now() < deadline {
                if service.child.try_wait().ok().flatten().is_some() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            let _ = service.child.kill();
            let _ = service.child.wait();
        }
    }

    fn mark_unavailable(state: &mut State) -> ServiceStatus {
        let started_at = state
            .service
            .take()
            .map(|mut service| {
                let started_at = service.started_at;
                let _ = service.child.kill();
                let _ = service.child.wait();
                started_at
            })
            .unwrap_or_else(Instant::now);
        Self::record_exit(state, started_at)
    }

    fn record_exit(state: &mut State, exited_at: Instant) -> ServiceStatus {
        let now = Instant::now();
        if now.duration_since(exited_at) >= CRASH_WINDOW {
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
        state.service = None;
        if state.crashes.len() >= CRASH_LOOP_THRESHOLD {
            state.crash_loop = true;
            ServiceStatus::CrashLoop
        } else {
            ServiceStatus::Unavailable
        }
    }

    fn spawn(&self) -> Result<ManagedService, String> {
        if !self.executable.is_file() {
            return Err("membrane_hub_missing".into());
        }
        let root =
            fs::canonicalize(&self.workspace_root).map_err(|_| "workspace_root_unavailable")?;
        let lease = self.mint_lease(&root)?;
        let hello = ResidentHelloV1 {
            kind: "hello".into(),
            lifecycle_version: 1,
            fence: lease.fence,
            installation_id: sha256_text(&root.to_string_lossy()),
            product_id: "membrane".into(),
            instance_id: lease.instance_id.clone(),
            release_generation: lease.release_generation.clone(),
            artifact_digest: lease.artifact_digest.clone(),
            declared_data_root: lease.declared_data_root.clone(),
            capability: lease.capability.clone(),
        };
        let resident_log = self.open_resident_log()?;
        let mut child = Command::new(&self.executable)
            .arg("supervisor-child")
            .env("MEMBRANE_LIFECYCLE_STDIO", "1")
            .env("WORKSPACE_ROOT", &root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(resident_log))
            .spawn()
            .map_err(|_| "membrane_hub_start_failed")?;
        let setup = (|| {
            let mut stdin = child.stdin.take().ok_or("membrane_hub_stdin_missing")?;
            write_frame(&mut stdin, &hello)?;
            let stdout = child.stdout.take().ok_or("membrane_hub_stdout_missing")?;
            let frames = read_lifecycle_frames(stdout);
            wait_for_startup(&frames, &lease)?;
            Ok((stdin, frames))
        })();
        match setup {
            Ok((stdin, frames)) => Ok(ManagedService {
                child,
                stdin: Some(stdin),
                frames,
                lease,
                started_at: Instant::now(),
            }),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                Err(error)
            }
        }
    }

    fn open_resident_log(&self) -> Result<fs::File, String> {
        if let Some(parent) = self.resident_log_path.parent() {
            fs::create_dir_all(parent).map_err(|_| "membrane_hub_log_unavailable")?;
        }
        if fs::metadata(&self.resident_log_path)
            .is_ok_and(|metadata| metadata.len() >= MAX_RESIDENT_LOG_BYTES)
        {
            let previous = self.resident_log_path.with_extension("previous.log");
            let _ = fs::remove_file(&previous);
            fs::rename(&self.resident_log_path, previous)
                .map_err(|_| "membrane_hub_log_rotate_failed")?;
        }
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.resident_log_path)
            .map_err(|_| "membrane_hub_log_unavailable".into())
    }

    fn mint_lease(&self, root: &Path) -> Result<ResidentLeaseV1, String> {
        let mut instance = [0_u8; 32];
        let mut capability = [0_u8; 32];
        getrandom::fill(&mut instance).map_err(|_| "membrane_hub_random_unavailable")?;
        getrandom::fill(&mut capability).map_err(|_| "membrane_hub_random_unavailable")?;
        Ok(ResidentLeaseV1 {
            schema_version: RESIDENT_LEASE_SCHEMA_VERSION,
            instance_id: format!("sha256:{}", hex(&instance)),
            capability: hex(&capability),
            release_generation: self.release_generation()?,
            declared_data_root: root.to_string_lossy().into_owned(),
            artifact_digest: sha256_file(&self.executable)?,
            fence: NEXT_FENCE.fetch_add(1, Ordering::Relaxed).max(1),
        })
    }

    fn release_generation(&self) -> Result<String, String> {
        let output = Command::new(&self.executable)
            .args(["cli", "build-info"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .map_err(|_| "membrane_hub_release_unavailable")?;
        if !output.status.success() {
            return Err("membrane_hub_release_unavailable".into());
        }
        serde_json::from_slice::<serde_json::Value>(&output.stdout)
            .ok()
            .and_then(|value| {
                value
                    .get("release_generation")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
            .filter(|value| is_sha256_digest(value))
            .ok_or("membrane_hub_release_invalid".into())
    }
}

fn write_frame<T: serde::Serialize>(stdin: &mut ChildStdin, frame: &T) -> Result<(), String> {
    serde_json::to_writer(&mut *stdin, frame).map_err(|_| "membrane_hub_frame_invalid")?;
    stdin
        .write_all(b"\n")
        .map_err(|_| "membrane_hub_pipe_unavailable")?;
    stdin
        .flush()
        .map_err(|_| "membrane_hub_pipe_unavailable".into())
}

fn read_lifecycle_frames(
    stdout: ChildStdout,
) -> Receiver<Result<ResidentLifecycleFrameV1, String>> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut output = BufReader::new(stdout);
        let mut line = String::new();
        loop {
            line.clear();
            match output.read_line(&mut line) {
                Ok(0) => {
                    let _ = sender.send(Err("membrane_hub_lifecycle_eof".into()));
                    return;
                }
                Ok(_) => {
                    let frame = serde_json::from_str::<ResidentLifecycleFrameV1>(&line)
                        .map_err(|_| "membrane_hub_lifecycle_invalid".into());
                    if sender.send(frame).is_err() {
                        return;
                    }
                }
                Err(_) => {
                    let _ = sender.send(Err("membrane_hub_lifecycle_read_failed".into()));
                    return;
                }
            }
        }
    });
    receiver
}

fn wait_for_startup(
    frames: &Receiver<Result<ResidentLifecycleFrameV1, String>>,
    lease: &ResidentLeaseV1,
) -> Result<(), String> {
    let deadline = Instant::now() + LIFECYCLE_TIMEOUT;
    let starting = recv_frame(frames, deadline)?;
    if starting.kind != "register"
        || starting.state.as_deref() != Some("starting")
        || starting.command.is_some()
        || starting.endpoint.is_some()
        || starting.capability.is_some()
        || starting.fence != lease.fence
    {
        return Err("membrane_hub_starting_invalid".into());
    }
    let ready = recv_frame(frames, deadline)?;
    let endpoint = ready.endpoint.as_ref();
    if ready.kind != "register"
        || ready.state.as_deref() != Some("ready")
        || ready.command.is_some()
        || ready.fence != lease.fence
        || ready.capability.as_deref() != Some(&lease.capability)
        || endpoint.is_none_or(|value| value.host != "127.0.0.1" || value.port < 1024)
    {
        return Err("membrane_hub_ready_invalid".into());
    }
    Ok(())
}

fn recv_frame(
    frames: &Receiver<Result<ResidentLifecycleFrameV1, String>>,
    deadline: Instant,
) -> Result<ResidentLifecycleFrameV1, String> {
    frames
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|_| "membrane_hub_lifecycle_timeout")?
}

fn sha256_text(value: &str) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(value.as_bytes())))
}
fn sha256_file(path: &Path) -> Result<String, String> {
    fs::read(path)
        .map(|bytes| format!("sha256:{}", hex::encode(Sha256::digest(bytes))))
        .map_err(|_| "membrane_hub_artifact_unavailable".into())
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|value| format!("{value:02x}")).collect()
}
fn is_sha256_digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
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
    #[test]
    fn lease_material_is_typed_and_cryptographically_shaped() {
        assert!(is_sha256_digest(&format!("sha256:{}", hex(&[7_u8; 32]))));
        assert_eq!(hex(&[9_u8; 32]).len(), 64);
    }
}
