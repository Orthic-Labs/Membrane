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
    let mut env: Vec<(String, String)> = sanitize_env_pairs(std::env::vars_os().map(|(key, value)| {
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

/// Spawn `binary` with piped stdio under the given environment.
///
/// The child sees exactly `env` — nothing else leaks from the parent process.
/// On Unix the child is placed in a new session/process group via `setsid`
/// (`pre_exec`) so that [`kill_direct_child`] can terminate the whole tree
/// with `killpg`. This is best-effort: descendants that explicitly escape the
/// group may survive, but configuration disables long-lived children (build
/// scripts, flycheck, watch) so well-behaved engines hold none.
pub fn spawn_sanitized(
    binary: &Path,
    args: &[String],
    working_dir: &Path,
    env: &[(String, String)],
) -> std::io::Result<Child> {
    let mut command = Command::new(binary);
    command
        .args(args)
        .current_dir(working_dir)
        .env_clear()
        .envs(env.iter().map(|(key, value)| (key.as_str(), value.as_str())))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
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
                // Ignore error: if setsid fails we still exec; cleanup falls
                // back to direct child kill.
                let _ = setsid();
                Ok(())
            });
        }
    }
    command.spawn()
}

/// Terminate a process tree via process-group kill on Unix, falling back to
/// direct child kill.
///
/// On Unix the child was spawned in its own process group (see
/// [`spawn_sanitized`]), so `killpg(pgid, SIGTERM/SIGKILL)` terminates the
/// group. On Windows or if the group kill fails, falls back to
/// [`Child::kill`] which terminates only the direct child. This is best-effort:
/// descendants that double-forked or changed groups may not be covered. Adapters
/// mitigate by disabling build scripts, flycheck, watch and proc macros so
/// well-behaved engines spawn no long-lived children.
pub fn kill_direct_child(child: &mut Child) {
    #[cfg(unix)]
    {
        let pid = child.id() as i32;
        unsafe {
            extern "C" {
                fn killpg(pgrp: i32, sig: i32) -> i32;
            }
            const SIGTERM: i32 = 15;
            const SIGKILL: i32 = 9;
            // Best-effort: TERM then KILL the whole group. Errors mean the
            // group already exited or we lack permission; fall through to
            // direct kill below.
            let _ = killpg(pid, SIGTERM);
            let _ = killpg(pid, SIGKILL);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
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
        decoder.push(lsp_frame_bytes(b"{}"));
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
        let left_binary = left.join("membrane-fake-engine");
        let right_binary = right.join("membrane-fake-engine");
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
        assert_eq!(probe_search_path("definitely-absent-binary", &[bin_dir]), None);
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
        assert!(env.iter().all(|(key, _)| SANITIZED_ENV_KEYS.contains(&key.as_str())));
        let path = env.iter().find(|(key, _)| key == "PATH").map(|(_, value)| value);
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
            recv_with_deadline(&receiver_closed, AbsoluteDeadline::after(now_unix_ms(), 60_000)),
            FrameOutcome::QueueClosed
        );
    }
}
