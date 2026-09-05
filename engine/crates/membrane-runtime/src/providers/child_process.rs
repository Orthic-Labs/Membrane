//! Shared child-process plumbing for resident native language-service
//! providers (design §3 containment duties, §13 sanitized environment).
//!
//! Everything here is deliberately std-only: PATH probing runs over an
//! injected search-path list so tests stay deterministic and no toolchain is
//! ever installed implicitly, the environment handed to engine processes
//! carries only `PATH`, `HOME`, and `TMPDIR` (design §13), framing codecs cover
//! LSP 3.17 `Content-Length` bodies and newline-delimited tsserver JSON, and
//! one bounded reader thread per stream feeds an mpsc channel whose queue
//! drops on overflow behind a typed counter that providers surface as lane
//! omissions. All provider adapters stay synchronous: they block on these
//! channels; they never enter an async runtime.

use crate::live_diagnostics::AbsoluteDeadline;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

/// Environment variable allowlist handed to spawned engines (design §13:
/// sanitized environment and no implicit credentials).
pub const SANITIZED_ENV_KEYS: [&str; 3] = ["PATH", "HOME", "TMPDIR"];

/// Upper bound for one LSP header block before the stream is declared corrupt.
const MAX_LSP_HEADER_BYTES: usize = 64 * 1024;
/// Upper bound for one LSP body before the stream is declared corrupt.
const MAX_LSP_BODY_BYTES: usize = 32 * 1024 * 1024;
/// Upper bound for one newline-delimited frame before the stream is declared
/// corrupt.
const MAX_LINE_FRAME_BYTES: usize = 1024 * 1024;
/// Reader-thread chunk size.
const READ_CHUNK_BYTES: usize = 8 * 1024;

#[cfg(windows)]
const BINARY_SUFFIXES: [&str; 2] = [".exe", ".cmd"];
#[cfg(not(windows))]
const BINARY_SUFFIXES: [&str; 1] = [""];

/// Current wall-clock reading on the supervisor clock basis (milliseconds
/// since the Unix epoch), matching [`crate::live_diagnostics`]'s default clock.
pub fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// Default search path derived from the parent `PATH`; hosts inject explicit
/// allowlisted directories through [`probe_search_path`] instead whenever they
/// qualify installed runtimes.
pub fn default_search_path() -> Vec<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect()
}

/// Probe an injected search-path list for `name`, returning the first match.
///
/// Deterministic by construction: directories are tried in list order and the
/// first directory containing an executable `name` (plus platform binary
/// suffixes) wins. Probing never installs anything (design §13).
pub fn probe_search_path(name: &str, search_path: &[PathBuf]) -> Option<PathBuf> {
    for directory in search_path {
        for suffix in BINARY_SUFFIXES {
            let candidate = directory.join(format!("{name}{suffix}"));
            if is_executable_file(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Keep only the [`SANITIZED_ENV_KEYS`] allowlist entries from `pairs`,
/// preserving input order.
pub fn sanitize_env_pairs<I>(pairs: I) -> Vec<(String, String)>
where
    I: IntoIterator<Item = (String, String)>,
{
    pairs
        .into_iter()
        .filter(|(key, _)| SANITIZED_ENV_KEYS.contains(&key.as_str()))
        .collect()
}

/// Build the exact environment for one engine process: the allowlisted parent
/// variables with `PATH` overridden to the injected search path joined by the
/// platform separator, so child toolchain resolution stays allowlisted
/// (design §13). Output is sorted by key for determinism.
pub fn sanitized_child_env(search_path: &[PathBuf]) -> Vec<(String, String)> {
    let separator = if cfg!(windows) { ";" } else { ":" };
    let path_value = search_path
        .iter()
        .map(|directory| directory.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(separator);
    let mut env: Vec<(String, String)> =
        sanitize_env_pairs(std::env::vars_os().map(|(key, value)| {
            (
                key.to_string_lossy().into_owned(),
                value.to_string_lossy().into_owned(),
            )
        }))
        .into_iter()
        .filter(|(key, _)| key != "PATH")
        .collect();
    env.push(("PATH".to_string(), path_value));
    env.sort_by(|left, right| left.0.cmp(&right.0));
    env
}

/// One sanitized engine process plus its governed process tree (design §13:
/// "process-tree termination on timeout/shutdown").
///
/// On Unix the child was spawned into its own session/process group via
/// `setsid`, so [`SanitizedProcess::kill_tree`] terminates the whole group
/// with `killpg`. On Windows the child is assigned to a Job Object with
/// `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, so [`SanitizedProcess::kill_tree`]
/// terminates every descendant and dropping the handle reaps any survivor.
pub struct SanitizedProcess {
    pub child: Child,
    #[cfg(windows)]
    job: Option<windows_job::WindowsJob>,
}

impl SanitizedProcess {
    /// Terminate the entire process tree: TERM then KILL on Unix, Job Object
    /// termination on Windows, always followed by the direct-child kill/wait
    /// fallback so no path leaves an unwaited zombie.
    pub fn kill_tree(&mut self) {
        #[cfg(unix)]
        {
            let pid = self.child.id() as i32;
            // SAFETY: killpg takes only a pid and a signal number. The group
            // was created by setsid at spawn; failures mean the group already
            // exited or we lack permission, and the direct kill below still runs.
            unsafe {
                extern "C" {
                    fn killpg(pgrp: i32, sig: i32) -> i32;
                }
                const SIGTERM: i32 = 15;
                const SIGKILL: i32 = 9;
                let _ = killpg(pid, SIGTERM);
                let _ = killpg(pid, SIGKILL);
            }
        }
        #[cfg(windows)]
        if let Some(job) = &self.job {
            job.terminate();
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Spawn `binary` with piped stdio under the given environment.
///
/// The child sees exactly `env` — nothing else leaks from the parent process.
/// On Unix the child is placed in a new session/process group via `setsid`
/// (`pre_exec`). On Windows the child joins a dedicated Job Object with
/// kill-on-close semantics so shutdown covers the full tree even when the
/// engine spawned descendants of its own.
pub fn spawn_sanitized(
    binary: &Path,
    args: &[String],
    working_dir: &Path,
    env: &[(String, String)],
) -> std::io::Result<SanitizedProcess> {
    let mut command = Command::new(binary);
    command
        .args(args)
        .current_dir(working_dir)
        .env_clear()
        .envs(
            env.iter()
                .map(|(key, value)| (key.as_str(), value.as_str())),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    spawn_contained_command(command)
}

/// Use the same process-tree containment for already-authorized command owners.
/// Environment and argv remain the caller's responsibility.
pub fn spawn_contained_command(mut command: Command) -> std::io::Result<SanitizedProcess> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        // SAFETY: setsid is async-signal-safe and has no Rust-specific
        // preconditions. Called in the child after fork, before exec.
        unsafe {
            command.pre_exec(|| {
                extern "C" {
                    fn setsid() -> i32;
                }
                if setsid() < 0 { return Err(std::io::Error::last_os_error()); }
                Ok(())
            });
        }
    }
    let mut child = command.spawn()?;
    #[cfg(windows)]
    let job = {
        use std::os::windows::io::AsRawHandle as _;
        let job = windows_job::WindowsJob::create();
        // Best-effort assignment; on failure the direct-child kill below
        // remains, matching the historical containment floor.
        match &job {
            Some(job) => {
                job.assign(child.as_raw_handle());
            }
            None => {}
        }
        job
    };
    Ok(SanitizedProcess {
        child,
        #[cfg(windows)]
        job,
    })
}

// ---------------------------------------------------------------------------
// Windows Job Object containment (design §13 process-tree shutdown)
// ---------------------------------------------------------------------------

/// Governed process-tree handle for engine children on Windows.
///
/// A Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` is created per
/// child; the child (and every descendant that does not explicitly break
/// away) is assigned to it. [`WindowsJob::terminate`] ends the whole tree;
/// dropping the handle closes the last reference and the kernel then kills
/// any remaining member, so no descendant can outlive supervisor teardown.
///
/// Declared via raw FFI to keep this crate std-only: no external Windows
/// crate is pulled in, and none of these calls have Rust-specific safety
/// preconditions beyond valid handles, which are only ever produced by
/// [`WindowsJob::create`] or borrowed from a live child.
#[cfg(windows)]
mod windows_job {
    use std::ffi::{c_void, CString};
    use std::os::windows::io::RawHandle;

    type Handle = *mut c_void;

    const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;
    const JOBOBJECT_EXTENDED_LIMIT_INFORMATION_CLASS: i32 = 9;
    const NO_ERROR_EXIT_CODE: u32 = 0;
    const INVALID_HANDLE_VALUE: Handle = std::ptr::null_mut();

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct IoCounters {
        read_operation_count: u64,
        write_operation_count: u64,
        other_operation_count: u64,
        read_transfer_count: u64,
        write_transfer_count: u64,
        other_transfer_count: u64,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct BasicLimitInformation {
        per_process_user_time_limit: i64,
        per_job_user_time_limit: i64,
        limit_flags: u32,
        minimum_working_set_size: usize,
        maximum_working_set_size: usize,
        active_process_limit: u32,
        affinity: usize,
        priority_class: u32,
        scheduling_class: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct ExtendedLimitInformation {
        basic_limit_information: BasicLimitInformation,
        io_info: IoCounters,
        process_memory_limit: usize,
        job_memory_limit: usize,
        peak_process_memory_used: usize,
        peak_job_memory_used: usize,
    }

    impl Default for ExtendedLimitInformation {
        fn default() -> Self {
            // SAFETY: all-zero bit pattern is valid for this POD struct; only
            // LimitFlags is set explicitly afterwards.
            let mut info: Self = unsafe { std::mem::zeroed() };
            info.basic_limit_information.limit_flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            info
        }
    }

    extern "system" {
        fn CreateJobObjectW(job_attributes: *mut c_void, name: *const u16) -> Handle;
        fn SetInformationJobObject(
            job: Handle,
            information_class: i32,
            information: *mut c_void,
            information_length: u32,
        ) -> i32;
        fn AssignProcessToJobObject(job: Handle, process: RawHandle) -> i32;
        fn TerminateJobObject(job: Handle, exit_code: u32) -> i32;
        fn CloseHandle(object: Handle) -> i32;
    }

    pub struct WindowsJob {
        handle: Handle,
    }

    // The raw HANDLE is only used from whichever thread owns the value and
    // every call here is an atomic kernel transition; sharing across threads
    // is safe and destruction runs wherever the value drops.
    unsafe impl Send for WindowsJob {}
    unsafe impl Sync for WindowsJob {}

    impl WindowsJob {
        /// Create one kill-on-close job with extended limit information
        /// applied. `None` means creation failed and the caller must fall
        /// back to direct-child termination.
        pub fn create() -> Option<Self> {
            // SAFETY: no attributes and no name; we own the returned handle.
            let handle = unsafe {
                let name = CString::new("membrane-live-diagnostics-job").ok()?;
                CreateJobObjectW(std::ptr::null_mut(), name.as_ptr() as *const u16)
            };
            if handle.is_null() || handle == INVALID_HANDLE_VALUE {
                return None;
            }
            let job = Self { handle };
            let info = ExtendedLimitInformation::default();
            // SAFETY: `info` is a valid POD of the exact expected layout and
            // size for class JobObjectExtendedLimitInformation (9).
            let applied = unsafe {
                SetInformationJobObject(
                    job.handle,
                    JOBOBJECT_EXTENDED_LIMIT_INFORMATION_CLASS,
                    &info as *const ExtendedLimitInformation as *mut c_void,
                    std::mem::size_of::<ExtendedLimitInformation>() as u32,
                )
            };
            if applied == 0 {
                return None;
            }
            Some(job)
        }

        /// Place one live process into the job tree.
        pub fn assign(&self, process: RawHandle) {
            if process.is_null() {
                return;
            }
            // SAFETY: both handles are valid kernel handles owned by this
            // call; assignment has no aliasing preconditions.
            unsafe {
                AssignProcessToJobObject(self.handle, process);
            }
        }

        /// Kill every process currently in the job.
        pub fn terminate(&self) {
            // SAFETY: self.handle is a valid job handle.
            unsafe {
                TerminateJobObject(self.handle, NO_ERROR_EXIT_CODE);
            }
        }
    }

    impl Drop for WindowsJob {
        fn drop(&mut self) {
            // SAFETY: dropping relinquishes our reference; kill-on-close
            // semantics terminate any surviving members.
            unsafe {
                CloseHandle(self.handle);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Framing codecs
// ---------------------------------------------------------------------------

/// Incremental byte-stream to UTF-8 frame decoder shared by reader threads.
pub trait FrameDecoder: Send {
    /// Feed one raw read chunk into the decoder buffer.
    fn push(&mut self, chunk: &[u8]);
    /// Pop the next complete frame, if one is buffered. Malformed streams flip
    /// the decoder into a sticky failed state yielding `None` forever.
    fn pop_frame(&mut self) -> Option<String>;
}

/// Encode one payload as an LSP `Content-Length` framed message.
pub fn lsp_frame_bytes(payload: &[u8]) -> Vec<u8> {
    let mut framed = format!("Content-Length: {}\r\n\r\n", payload.len()).into_bytes();
    framed.extend_from_slice(payload);
    framed
}

/// Encode one payload as a newline-delimited tsserver message.
pub fn tsserver_line_bytes(payload: &str) -> Vec<u8> {
    let mut framed = payload.trim_end().as_bytes().to_vec();
    framed.push(b'\n');
    framed
}

/// LSP 3.17 `Content-Length` framing decoder over stdin byte chunks.
///
/// Header parsing is case-insensitive, tolerates extra headers such as
/// `Content-Type`, enforces the header/body size bounds above, and converts
/// bodies lossily to UTF-8. A missing or invalid `Content-Length` poisons the
/// decoder permanently: the stream can no longer be trusted.
#[derive(Debug, Default)]
pub struct LspDecoder {
    buf: Vec<u8>,
    failed: bool,
}

impl LspDecoder {
    pub fn new() -> Self {
        Self::default()
    }
}

impl FrameDecoder for LspDecoder {
    fn push(&mut self, chunk: &[u8]) {
        if !self.failed {
            self.buf.extend_from_slice(chunk);
        }
    }

    fn pop_frame(&mut self) -> Option<String> {
        if self.failed {
            return None;
        }
        let Some(separator) = find_header_terminator(&self.buf, MAX_LSP_HEADER_BYTES) else {
            if self.buf.len() > MAX_LSP_HEADER_BYTES {
                self.failed = true;
            }
            return None;
        };
        let headers = match std::str::from_utf8(&self.buf[..separator]) {
            Ok(headers) => headers,
            Err(_) => {
                self.failed = true;
                return None;
            }
        };
        let Some(length) = parse_content_length(headers) else {
            self.failed = true;
            return None;
        };
        if length > MAX_LSP_BODY_BYTES as u64 {
            self.failed = true;
            return None;
        }
        let Some(body_start) = separator.checked_add(4) else {
            self.failed = true;
            return None;
        };
        let Some(body_end) = usize::try_from(length)
            .ok()
            .and_then(|length| body_start.checked_add(length))
        else {
            self.failed = true;
            return None;
        };
        if self.buf.len() < body_end {
            return None;
        }
        let body = String::from_utf8_lossy(&self.buf[body_start..body_end]).into_owned();
        self.buf.drain(..body_end);
        Some(body)
    }
}

/// Find the `\r\n\r\n` header terminator within the first `limit` bytes.
fn find_header_terminator(buf: &[u8], limit: usize) -> Option<usize> {
    let end = buf.len().min(limit);
    if end < 4 {
        return None;
    }
    for index in 0..=(end - 4) {
        if &buf[index..index + 4] == b"\r\n\r\n" {
            return Some(index);
        }
    }
    None
}

/// Extract the first case-insensitive `Content-Length` header value.
fn parse_content_length(headers: &str) -> Option<u64> {
    headers.split("\r\n").find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if name.trim().eq_ignore_ascii_case("content-length") {
            value.trim().parse::<u64>().ok()
        } else {
            None
        }
    })
}

/// Newline-delimited JSON frame decoder for the tsserver stdout protocol.
///
/// Blank lines are skipped; a single line exceeding [`MAX_LINE_FRAME_BYTES`]
/// poisons the decoder.
#[derive(Debug, Default)]
pub struct LineFrameDecoder {
    buf: Vec<u8>,
    failed: bool,
}

impl LineFrameDecoder {
    pub fn new() -> Self {
        Self::default()
    }
}

impl FrameDecoder for LineFrameDecoder {
    fn push(&mut self, chunk: &[u8]) {
        if !self.failed {
            self.buf.extend_from_slice(chunk);
        }
    }

    fn pop_frame(&mut self) -> Option<String> {
        loop {
            if self.failed {
                return None;
            }
            let Some(position) = self.buf.iter().position(|byte| *byte == b'\n') else {
                if self.buf.len() > MAX_LINE_FRAME_BYTES {
                    self.failed = true;
                }
                return None;
            };
            let line_bytes: Vec<u8> = self.buf.drain(..=position).collect();
            let text = String::from_utf8_lossy(&line_bytes[..line_bytes.len() - 1]);
            let trimmed = text.trim();
            if trimmed.is_empty() {
                continue;
            }
            return Some(trimmed.to_string());
        }
    }
}

// ---------------------------------------------------------------------------
// Bounded reader threads
// ---------------------------------------------------------------------------

/// Handles for one bounded reader thread: the frame receiver plus the thread
/// join handle so shutdowns can prove the thread exited.
pub struct ReaderPump {
    /// Decoded frames. Closed once the stream ends or the decoder fails.
    pub frames: Receiver<String>,
    /// Producer thread handle; join after killing the child.
    pub handle: JoinHandle<()>,
}

/// Spawn one bounded reader thread decoding `input` with `decoder`.
///
/// Frames flow through a `sync_channel(capacity)` queue; when the consumer
/// stalls and the queue fills, further frames are dropped and counted in
/// `overflow` so providers can surface typed omissions instead of silently
/// losing diagnostics. Channel closure ends production immediately.
pub fn spawn_bounded_reader<D, R>(
    input: R,
    decoder: D,
    capacity: usize,
    overflow: Arc<AtomicUsize>,
) -> ReaderPump
where
    D: FrameDecoder + 'static,
    R: Read + Send + 'static,
{
    let (sender, receiver): (SyncSender<String>, Receiver<String>) =
        std::sync::mpsc::sync_channel(capacity.max(1));
    let handle = std::thread::spawn(move || {
        let mut decoder = decoder;
        let mut input = input;
        let mut chunk = [0u8; READ_CHUNK_BYTES];
        'produce: loop {
            match input.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    decoder.push(&chunk[..read]);
                    while let Some(frame) = decoder.pop_frame() {
                        match sender.try_send(frame) {
                            Ok(()) => {}
                            Err(TrySendError::Full(_)) => {
                                overflow.fetch_add(1, Ordering::Relaxed);
                            }
                            Err(TrySendError::Disconnected(_)) => break 'produce,
                        }
                    }
                }
            }
        }
    });
    ReaderPump {
        frames: receiver,
        handle,
    }
}

/// Spawn one thread draining stderr so a full pipe cannot stall the engine.
pub fn spawn_stderr_drainer<R>(input: R) -> JoinHandle<()>
where
    R: Read + Send + 'static,
{
    std::thread::spawn(move || {
        let mut input = input;
        let mut chunk = [0u8; READ_CHUNK_BYTES];
        loop {
            match input.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
        }
    })
}

/// Outcome of one deadline-bounded receive from a reader queue.
#[derive(Debug, PartialEq, Eq)]
pub enum FrameOutcome {
    /// One decoded frame arrived in time.
    Frame(String),
    /// The producer closed the queue (stream ended or decoder poisoned).
    QueueClosed,
    /// The absolute deadline expired before any frame arrived.
    DeadlineExceeded,
}

/// Receive one frame honoring the supervisor's absolute deadline.
///
/// Providers call this in their pump loops so a stalled engine surfaces as
/// [`crate::live_diagnostics::ProviderError::DeadlineExceeded`] even though the
/// underlying reader thread stays blocked in `read`.
pub fn recv_with_deadline(frames: &Receiver<String>, deadline: AbsoluteDeadline) -> FrameOutcome {
    let remaining_ms = deadline.at_monotonic_ms.saturating_sub(now_unix_ms());
    recv_within(frames, remaining_ms)
}

/// Receive one frame within a plain millisecond budget (handshake waits).
pub fn recv_within(frames: &Receiver<String>, budget_ms: u64) -> FrameOutcome {
    match frames.recv_timeout(Duration::from_millis(budget_ms)) {
        Ok(frame) => FrameOutcome::Frame(frame),
        Err(RecvTimeoutError::Timeout) => FrameOutcome::DeadlineExceeded,
        Err(RecvTimeoutError::Disconnected) => FrameOutcome::QueueClosed,
    }
}

/// Drain every frame already flowing until the queue closes or `budget_ms`
/// elapses; used while awaiting best-effort handshake replies on shutdown.
pub fn drain_frames_until(frames: &Receiver<String>, budget_ms: u64) {
    loop {
        match recv_within(frames, budget_ms) {
            FrameOutcome::Frame(_) => continue,
            FrameOutcome::QueueClosed | FrameOutcome::DeadlineExceeded => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn executable(path: &Path) {
        std::fs::write(path, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    #[test]
    fn lsp_round_trip_preserves_bodies_across_chunk_boundaries() {
        let first = lsp_frame_bytes(b"{\"jsonrpc\":\"2.0\"}");
        let second = lsp_frame_bytes(b"[1,2,3]");
        let mut wire = Vec::new();
        wire.extend_from_slice(&first);
        wire.extend_from_slice(&second);

        let mut decoder = LspDecoder::new();
        let mut pushed = 0usize;
        // Feed in awkward chunk sizes to prove reassembly across boundaries.
        while pushed < wire.len() {
            let take = (wire.len() - pushed).min(3);
            decoder.push(&wire[pushed..pushed + take]);
            pushed += take;
        }
        assert_eq!(decoder.pop_frame().unwrap(), "{\"jsonrpc\":\"2.0\"}");
        assert_eq!(decoder.pop_frame().unwrap(), "[1,2,3]");
        assert_eq!(decoder.pop_frame(), None);
    }

    #[test]
    fn lsp_decoder_tolerates_extra_headers_and_is_case_insensitive() {
        let mut wire = Vec::new();
        wire.extend_from_slice(b"content-type: application/vscode-jsonrpc\r\n");
        wire.extend_from_slice(b"CONTENT-LENGTH: 2\r\n\r\n{}");
        let mut decoder = LspDecoder::new();
        decoder.push(&wire);
        assert_eq!(decoder.pop_frame().unwrap(), "{}");
    }

    #[test]
    fn lsp_decoder_fails_sticky_on_missing_content_length() {
        let mut decoder = LspDecoder::new();
        decoder.push(b"Nope: 1\r\n\r\n{}");
        assert_eq!(decoder.pop_frame(), None);
        decoder.push(&lsp_frame_bytes(b"{}"));
        assert_eq!(decoder.pop_frame(), None);
    }

    #[test]
    fn line_decoder_round_trips_partial_chunks_and_skips_blank_lines() {
        let payload = "{\"seq\":1}\r\n\n{\"seq\":2}\n";
        let mut decoder = LineFrameDecoder::new();
        let bytes = payload.as_bytes();
        decoder.push(&bytes[..5]);
        assert_eq!(decoder.pop_frame(), None);
        decoder.push(&bytes[5..]);
        assert_eq!(decoder.pop_frame().unwrap(), "{\"seq\":1}");
        assert_eq!(decoder.pop_frame().unwrap(), "{\"seq\":2}");
        assert_eq!(decoder.pop_frame(), None);
    }

    #[test]
    fn probe_search_path_finds_first_match_deterministically() {
        let dir = tempfile::tempdir().unwrap();
        let left = dir.path().join("left");
        let right = dir.path().join("right");
        std::fs::create_dir_all(&left).unwrap();
        std::fs::create_dir_all(&right).unwrap();
        let name = format!("membrane-fake-engine{}", BINARY_SUFFIXES[0]);
        let left_binary = left.join(&name);
        let right_binary = right.join(&name);
        executable(&left_binary);
        executable(&right_binary);

        let order_left_first = vec![left.clone(), right.clone()];
        let order_right_first = vec![right.clone(), left.clone()];
        assert_eq!(
            probe_search_path("membrane-fake-engine", &order_left_first),
            Some(left_binary.clone())
        );
        assert_eq!(
            probe_search_path("membrane-fake-engine", &order_right_first),
            Some(right_binary.clone())
        );
        // Repeat: same input, same answer.
        assert_eq!(
            probe_search_path("membrane-fake-engine", &order_left_first),
            probe_search_path("membrane-fake-engine", &order_left_first)
        );
    }

    #[test]
    fn probe_search_path_misses_return_none_without_installing() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        assert_eq!(
            probe_search_path("definitely-absent-binary", &[bin_dir]),
            None
        );
    }

    #[test]
    fn sanitize_env_pairs_keeps_only_the_allowlist() {
        let sanitized = sanitize_env_pairs([
            ("PATH".to_string(), "/bin".to_string()),
            ("AWS_SECRET_ACCESS_KEY".to_string(), "leak".to_string()),
            ("HOME".to_string(), "/home/u".to_string()),
            ("TMPDIR".to_string(), "/tmp".to_string()),
            ("GITHUB_TOKEN".to_string(), "leak".to_string()),
        ]);
        assert_eq!(
            sanitized,
            vec![
                ("PATH".to_string(), "/bin".to_string()),
                ("HOME".to_string(), "/home/u".to_string()),
                ("TMPDIR".to_string(), "/tmp".to_string()),
            ]
        );
    }

    #[test]
    fn sanitized_child_env_overrides_path_sorts_and_drops_credentials() {
        let search = vec![PathBuf::from("/opt/allowlisted"), PathBuf::from("/usr/bin")];
        std::env::set_var("MEMBRANE_TEST_CREDENTIAL", "leak");
        let env = sanitized_child_env(&search);
        std::env::remove_var("MEMBRANE_TEST_CREDENTIAL");
        assert!(
            env.windows(2).all(|pair| pair[0].0 < pair[1].0),
            "env must be sorted by key: {env:?}"
        );
        assert!(env
            .iter()
            .all(|(key, _)| SANITIZED_ENV_KEYS.contains(&key.as_str())));
        let path = env
            .iter()
            .find(|(key, _)| key == "PATH")
            .map(|(_, value)| value);
        assert_eq!(
            path.map(|value| value.starts_with("/opt/allowlisted")),
            Some(true)
        );
    }

    #[test]
    fn bounded_reader_counts_overflow_and_closes_queue_at_eof() {
        let overflow = Arc::new(AtomicUsize::new(0));
        let pump = spawn_bounded_reader(
            Cursor::new(b"a\nb\nc\n".to_vec()),
            LineFrameDecoder::new(),
            1,
            Arc::clone(&overflow),
        );
        pump.handle.join().unwrap();
        assert_eq!(overflow.load(Ordering::Relaxed), 2);
        assert_eq!(pump.frames.recv().unwrap(), "a");
        assert!(pump.frames.recv().is_err());
    }

    #[test]
    fn recv_with_deadline_enforces_deadline_with_stubbed_reader() {
        let (_held_sender, receiver) = std::sync::mpsc::sync_channel::<String>(1);
        let expired = AbsoluteDeadline::after(now_unix_ms(), 0);
        assert!(expired.expired(now_unix_ms()));
        assert_eq!(
            recv_with_deadline(&receiver, expired),
            FrameOutcome::DeadlineExceeded
        );

        let (sender, receiver_closed) = std::sync::mpsc::sync_channel::<String>(1);
        drop(sender);
        assert_eq!(
            recv_with_deadline(
                &receiver_closed,
                AbsoluteDeadline::after(now_unix_ms(), 60_000)
            ),
            FrameOutcome::QueueClosed
        );
    }
}
