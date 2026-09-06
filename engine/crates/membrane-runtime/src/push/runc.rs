//! L2 command runner — deterministic wrapper around shell execution.
//!
//! This is the `runc.mjs` replacement: it executes a command string, captures
//! stdout+stderr, produces a head/tail-capped view for context injection, spills
//! the full output to disk when truncated, and preserves the child exit code.

use crate::push::truncate;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::ffi::{OsStr, OsString};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};

const CAPTURE_CHUNK_BYTES: usize = 8 * 1024;
const CAPTURE_CHANNEL_DEPTH: usize = 32;
const MAX_INLINE_CAPTURE_BYTES: usize = 256 * 1024;
const MAX_PREVIEW_EDGE_BYTES: usize = 32 * 1024;
static CAPTURE_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub struct RuncResult {
    pub capped: String,
    pub spill_path: Option<PathBuf>,
    pub anchor: String,
    pub exit_code: i32,
    pub recovery_marker: Option<crate::push::compress::RecoveryMarkerV1>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandAdapter {
    Git,
    RepositoryTestRunner,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandAdapterRejectionKind {
    UnsupportedProgram,
    UnsupportedInvocation,
    RootEscape,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandAdapterError {
    InvalidRoot(String),
    Rejected {
        adapter: CommandAdapter,
        kind: CommandAdapterRejectionKind,
    },
    Execution(String),
}

impl std::fmt::Display for CommandAdapterError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRoot(detail) => {
                write!(formatter, "command_adapter_repo_invalid:{detail}")
            }
            Self::Rejected { adapter, kind } => write!(
                formatter,
                "command_adapter_rejected:{kind:?};adapter={adapter:?}"
            ),
            Self::Execution(detail) => write!(formatter, "command_adapter_execution:{detail}"),
        }
    }
}

impl std::error::Error for CommandAdapterError {}

fn parse_shell_override(raw: &str) -> Vec<String> {
    raw.split_whitespace().map(|s| s.to_string()).collect()
}

fn default_shell_argv() -> Vec<String> {
    if cfg!(windows) {
        let bash = std::env::var_os("ProgramFiles")
            .map(PathBuf::from)
            .map(|root| root.join("Git").join("bin").join("bash.exe"))
            .filter(|path| path.is_file())
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| "bash".into());
        vec![bash, "-c".into()]
    } else {
        vec!["sh".into(), "-c".into()]
    }
}

fn anchor_ttl_millis() -> u128 {
    std::env::var("MEMBRANE_PUSH_ANCHOR_TTL_MS")
        .ok()
        .and_then(|value| value.parse::<u128>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(7 * 24 * 60 * 60 * 1_000)
}

/// Resolve the shell argv prefix used to execute `cmd`.
///
/// Order:
/// 1. `MEMBRANE_PUSH_RUNC_SHELL` if set (split by whitespace into argv). If the value
///    has no explicit `-c`/`/C`, we append the platform default switch.
/// 2. Platform default: POSIX `sh -c`, Windows Git Bash `bash -c`.
pub fn resolve_shell_argv() -> Vec<String> {
    if let Ok(raw) = std::env::var("MEMBRANE_PUSH_RUNC_SHELL") {
        let mut argv = parse_shell_override(&raw);
        if argv.is_empty() {
            return default_shell_argv();
        }
        // If the override provides only the program, add the platform switch.
        if argv.len() == 1 {
            if cfg!(windows) {
                argv.push("/C".into());
            } else {
                argv.push("-c".into());
            }
        }
        return argv;
    }
    default_shell_argv()
}

struct OrderedCapture {
    path: PathBuf,
    byte_count: usize,
    newline_count: usize,
    ends_with_newline: bool,
    digest: String,
}

impl OrderedCapture {
    fn cleanup(&self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[derive(Debug)]
enum CaptureMessage {
    Chunk {
        sequence: u64,
        lane: &'static str,
        bytes: Vec<u8>,
    },
    Failed {
        lane: &'static str,
        reason: String,
    },
    Finished,
}

fn read_capture_lane(
    mut reader: impl Read,
    lane: &'static str,
    sequence: Arc<AtomicU64>,
    sender: mpsc::SyncSender<CaptureMessage>,
) {
    let mut chunk = [0u8; CAPTURE_CHUNK_BYTES];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => {
                let sequence = sequence.fetch_add(1, Ordering::Relaxed);
                if sender
                    .send(CaptureMessage::Chunk {
                        sequence,
                        lane,
                        bytes: chunk[..read].to_vec(),
                    })
                    .is_err()
                {
                    return;
                }
            }
            Err(error) => {
                let _ = sender.send(CaptureMessage::Failed {
                    lane,
                    reason: error.to_string(),
                });
                return;
            }
        }
    }
    let _ = sender.send(CaptureMessage::Finished);
}

fn capture_ordered_with_control(
    stdout: impl Read + Send + 'static,
    stderr: impl Read + Send + 'static,
    spill_dir: &Path,
    mut check: impl FnMut() -> Result<(), String>,
    mut abort: impl FnMut(),
) -> Result<
    (
        OrderedCapture,
        std::thread::JoinHandle<()>,
        std::thread::JoinHandle<()>,
    ),
    String,
> {
    std::fs::create_dir_all(spill_dir)
        .map_err(|error| format!("spill_dir create failed: {error}"))?;
    let path = spill_dir.join(format!(
        ".ordered-{}-{}-{}.tmp",
        std::process::id(),
        crate::time::now_millis(),
        CAPTURE_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed),
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| format!("ordered capture create failed: {error}"))?;
    let (sender, receiver) = mpsc::sync_channel(CAPTURE_CHANNEL_DEPTH);
    let sequence = Arc::new(AtomicU64::new(0));
    let stdout_sequence = Arc::clone(&sequence);
    let stdout_sender = sender.clone();
    let stdout_thread = std::thread::spawn(move || {
        read_capture_lane(stdout, "stdout", stdout_sequence, stdout_sender)
    });
    let stderr_thread =
        std::thread::spawn(move || read_capture_lane(stderr, "stderr", sequence, sender));

    let mut next_sequence = 0u64;
    let mut pending = BTreeMap::<u64, (&'static str, Vec<u8>)>::new();
    let mut finished = 0usize;
    let mut byte_count = 0usize;
    let mut newline_count = 0usize;
    let mut ends_with_newline = false;
    let mut hasher = Sha256::new();
    let mut failure = None;
    while finished < 2 {
        if let Err(reason) = check() { failure = Some(reason); break; }
        match receiver.recv_timeout(std::time::Duration::from_millis(25)) {
            Ok(CaptureMessage::Chunk {
                sequence,
                lane,
                bytes,
            }) => {
                pending.insert(sequence, (lane, bytes));
                if pending.len() > CAPTURE_CHANNEL_DEPTH * 2 {
                    failure = Some("push_capture_reorder_limit".to_string()); break;
                }
                while let Some((_lane, bytes)) = pending.remove(&next_sequence) {
                    if byte_count.saturating_add(bytes.len()) > super::recovery::MAX_ARTIFACT_BYTES {
                        failure = Some("push_capture_byte_limit".to_string()); break;
                    }
                    if let Err(error) = file.write_all(&bytes) {
                        failure = Some(format!("ordered capture write failed: {error}")); break;
                    }
                    hasher.update(&bytes);
                    byte_count = byte_count.saturating_add(bytes.len());
                    newline_count = newline_count
                        .saturating_add(bytes.iter().filter(|byte| **byte == b'\n').count());
                    ends_with_newline = bytes.last() == Some(&b'\n');
                    next_sequence = next_sequence.saturating_add(1);
                }
            }
            Ok(CaptureMessage::Failed { lane, reason }) => {
                failure.get_or_insert_with(|| format!("{lane} capture failed: {reason}"));
                finished += 1;
            }
            Ok(CaptureMessage::Finished) => finished += 1,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                if finished < 2 { failure = Some("push_capture_incomplete".into()); }
                break;
            }
        }
        if failure.is_some() { break; }
    }
    if let Some(reason) = failure {
        abort();
        drop(receiver);
        let _ = stdout_thread.join();
        let _ = stderr_thread.join();
        drop(file);
        let _ = std::fs::remove_file(&path);
        return Err(reason);
    }
    if !pending.is_empty() {
        let _ = std::fs::remove_file(&path);
        return Err("ordered capture sequence gap".to_string());
    }
    file.flush()
        .map_err(|error| format!("ordered capture flush failed: {error}"))?;
    Ok((
        OrderedCapture {
            path,
            byte_count,
            newline_count,
            ends_with_newline,
            digest: hex::encode(hasher.finalize()),
        },
        stdout_thread,
        stderr_thread,
    ))
}

#[cfg(test)]
fn capture_ordered(stdout: impl Read + Send + 'static, stderr: impl Read + Send + 'static, spill_dir: &Path)
    -> Result<(OrderedCapture, std::thread::JoinHandle<()>, std::thread::JoinHandle<()>), String> {
    capture_ordered_with_control(stdout, stderr, spill_dir, || Ok(()), || {})
}

struct LineWindow {
    head_limit: usize,
    tail_limit: usize,
    head: Vec<Vec<u8>>,
    tail: VecDeque<Vec<u8>>,
    line_count: usize,
    pending: Vec<u8>,
    dropped_hasher: Sha256,
    dropped_len: usize,
}

impl LineWindow {
    fn new(head_limit: usize, tail_limit: usize) -> Self {
        Self {
            head_limit,
            tail_limit,
            head: Vec::with_capacity(head_limit),
            tail: VecDeque::with_capacity(tail_limit),
            line_count: 0,
            pending: Vec::new(),
            dropped_hasher: Sha256::new(),
            dropped_len: 0,
        }
    }

    fn push(&mut self, bytes: &[u8]) {
        let mut start = 0;
        for (index, byte) in bytes.iter().enumerate() {
            if *byte != b'\n' {
                continue;
            }
            // Preserve exact source bytes, including line terminators, so
            // retained byte counts & omitted-byte digest describe the durable
            // spill rather than a normalized rendering.
            self.pending.extend_from_slice(&bytes[start..=index]);
            self.finish_line();
            start = index + 1;
        }
        self.pending.extend_from_slice(&bytes[start..]);
    }

    fn finish(mut self) -> Self {
        if !self.pending.is_empty() {
            self.finish_line();
        }
        self
    }

    fn finish_line(&mut self) {
        let line = std::mem::take(&mut self.pending);
        self.line_count += 1;
        let dropped = if self.head.len() < self.head_limit {
            self.head.push(line);
            None
        } else if self.tail_limit > 0 {
            let evicted = if self.tail.len() == self.tail_limit {
                self.tail.pop_front()
            } else {
                None
            };
            self.tail.push_back(line);
            evicted
        } else {
            Some(line)
        };
        if let Some(dropped) = dropped {
            self.dropped_hasher.update(&dropped);
            self.dropped_len = self.dropped_len.saturating_add(dropped.len());
        }
    }

    fn render(self) -> (String, String, usize, usize, usize) {
        let head_bytes = self.head.iter().map(|line| line.len()).sum();
        let tail_bytes = self.tail.iter().map(|line| line.len()).sum();
        let elided = self
            .line_count
            .saturating_sub(self.head_limit.saturating_add(self.tail_limit));
        let display = |mut line: Vec<u8>| {
            if line.last() == Some(&b'\n') {
                line.pop();
            }
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            String::from_utf8_lossy(&line).into_owned()
        };
        let mut lines = self.head.into_iter().map(display).collect::<Vec<_>>();
        lines.push(format!("… {elided} lines elided …"));
        lines.extend(self.tail.into_iter().map(display));
        let rendered = lines.join("\n");
        let dropped_digest = hex::encode(self.dropped_hasher.finalize());
        (
            rendered,
            dropped_digest,
            self.dropped_len,
            head_bytes,
            tail_bytes,
        )
    }
}

fn hash_file_range(path: &Path, start: u64, length: usize) -> Result<String, String> {
    let mut file = File::open(path).map_err(|error| format!("capture reopen failed: {error}"))?;
    file.seek(SeekFrom::Start(start))
        .map_err(|error| format!("capture seek failed: {error}"))?;
    let mut remaining = length;
    let mut chunk = [0u8; 64 * 1024];
    let mut hasher = Sha256::new();
    while remaining > 0 {
        let requested = remaining.min(chunk.len());
        let read = file
            .read(&mut chunk[..requested])
            .map_err(|error| format!("capture range read failed: {error}"))?;
        if read == 0 {
            return Err("capture range ended early".to_string());
        }
        hasher.update(&chunk[..read]);
        remaining -= read;
    }
    Ok(hex::encode(hasher.finalize()))
}

fn byte_preview(capture: &OrderedCapture) -> Result<(String, String, usize, usize, usize), String> {
    let head_bytes = capture.byte_count.min(MAX_PREVIEW_EDGE_BYTES);
    let tail_bytes = capture
        .byte_count
        .saturating_sub(head_bytes)
        .min(MAX_PREVIEW_EDGE_BYTES);
    let dropped_len = capture
        .byte_count
        .saturating_sub(head_bytes.saturating_add(tail_bytes));
    let mut file = File::open(&capture.path)
        .map_err(|error| format!("capture preview reopen failed: {error}"))?;
    let mut head = vec![0u8; head_bytes];
    file.read_exact(&mut head)
        .map_err(|error| format!("capture head read failed: {error}"))?;
    let mut tail = vec![0u8; tail_bytes];
    if tail_bytes > 0 {
        file.seek(SeekFrom::End(-(tail_bytes as i64)))
            .and_then(|_| file.read_exact(&mut tail))
            .map_err(|error| format!("capture tail read failed: {error}"))?;
    }
    let dropped_digest = hash_file_range(&capture.path, head_bytes as u64, dropped_len)?;
    let capped = format!(
        "{}\n… {dropped_len} bytes elided …\n{}",
        String::from_utf8_lossy(&head),
        String::from_utf8_lossy(&tail)
    );
    Ok((capped, dropped_digest, dropped_len, head_bytes, tail_bytes))
}

fn line_preview(
    capture: &OrderedCapture,
    head: usize,
    tail: usize,
) -> Result<(String, String, usize, usize, usize), String> {
    if capture.byte_count > MAX_INLINE_CAPTURE_BYTES {
        return byte_preview(capture);
    }
    let bytes = std::fs::read(&capture.path)
        .map_err(|error| format!("capture preview read failed: {error}"))?;
    let mut window = LineWindow::new(head, tail);
    window.push(&bytes);
    Ok(window.finish().render())
}

fn publish_spill_scoped(
    capture: &OrderedCapture,
    head: usize,
    tail: usize,
    spill_dir: &Path,
    scope: &super::recovery::RecoveryScope,
) -> Result<
    (
        String,
        PathBuf,
        String,
        crate::push::compress::RecoveryMarkerV1,
    ),
    String,
> {
    let (mut capped, dropped_digest, dropped_len, head_bytes, tail_bytes) =
        line_preview(capture, head, tail)?;
    if dropped_len == 0 {
        // Binary-only publication is not truncation; never print a fictitious
        // zero-line elision marker for a complete lossy UTF-8 preview.
        let bytes = std::fs::read(&capture.path).map_err(|e| e.to_string())?;
        capped = String::from_utf8_lossy(&bytes).into_owned();
    }
    if capture.byte_count > super::recovery::MAX_ARTIFACT_BYTES { return Err("push_capture_byte_limit".into()); }
    let original = std::fs::read(&capture.path).map_err(|e| format!("capture read: {e}"))?;
    let retained = super::recovery::RecoveryStore::at(spill_dir).publish(scope, &original,
        anchor_ttl_millis().min(u64::MAX as u128) as u64, super::recovery::now_ms()).map_err(|e| e.to_string())?;
    let digest = capture.digest.clone();
    if retained.source_digest != format!("sha256:{digest}") { return Err("push_capture_corrupt".into()); }
    let path = spill_dir.join(format!("{digest}.log"));
    if let Ok(metadata) = std::fs::symlink_metadata(&path) {
        if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > super::recovery::MAX_ARTIFACT_BYTES as u64 {
            return Err("push_legacy_export_invalid".into());
        }
        let existing = std::fs::read(&path).map_err(|e| e.to_string())?;
        if existing != original { return Err("push_legacy_export_corrupt".into()); }
    }
    OpenOptions::new()
        .write(true)
        .open(&capture.path)
        .and_then(|file| file.sync_all())
        .map_err(|error| format!("spill sync failed: {error}"))?;
    std::fs::rename(&capture.path, &path)
        .or_else(|error| {
            if path.is_file() {
                let _ = std::fs::remove_file(&capture.path);
                Ok(())
            } else {
                Err(error)
            }
        })
        .map_err(|error| format!("spill publish failed: {error}"))?;
    sync_spill_directory(spill_dir)?;
    let metadata = spill_dir.join(format!("{digest}.json"));
    let metadata_temp = spill_dir.join(format!(
        ".{digest}.metadata.{}.tmp",
        crate::time::now_millis()
    ));
    let created_at_millis = crate::time::now_millis();
    let expires_at_millis = retained.expires_at as u128;
    let recovery = crate::push::compress::RecoveryMarkerV1 {
        schema_version: crate::push::compress::RECOVERY_MARKER_SCHEMA_VERSION,
        source_digest: format!("sha256:{digest}"),
        transform: "head_tail_spill".to_string(),
        transform_version: 1,
        kept_head_bytes: head_bytes,
        kept_tail_bytes: tail_bytes,
        protected_spans: Vec::new(),
        dropped_bytes_digest: format!("sha256:{dropped_digest}"),
        recovery_handle: retained.handle.clone(),
        expires_at_millis: expires_at_millis as u64,
    };
    let record = serde_json::json!({
        "schemaVersion": 1,
        "anchor": retained.handle,
        "sha256": digest,
        "createdAtMillis": created_at_millis,
        "expiresAtMillis": expires_at_millis,
        "sizeBytes": capture.byte_count,
        "droppedBytes": dropped_len,
        "recovery": &recovery,
    });
    {
        let mut metadata_file = File::create(&metadata_temp)
            .map_err(|error| format!("anchor metadata create failed: {error}"))?;
        metadata_file
            .write_all(record.to_string().as_bytes())
            .map_err(|error| format!("anchor metadata write failed: {error}"))?;
        metadata_file
            .sync_all()
            .map_err(|error| format!("anchor metadata sync failed: {error}"))?;
    }
    std::fs::rename(&metadata_temp, &metadata)
        .map_err(|error| format!("anchor metadata publish failed: {error}"))?;
    sync_spill_directory(spill_dir)?;
    Ok((capped, path, digest, recovery))
}

#[cfg(test)]
fn publish_spill(capture: &OrderedCapture, head: usize, tail: usize, spill_dir: &Path)
    -> Result<(String, PathBuf, String, super::compress::RecoveryMarkerV1), String> {
    let scope = super::recovery::RecoveryScope::local().map_err(|e| e.to_string())?;
    publish_spill_scoped(capture, head, tail, spill_dir, &scope)
}

fn sync_spill_directory(spill_dir: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        File::open(spill_dir)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| format!("spill directory sync failed: {error}"))?;
    }
    #[cfg(not(unix))]
    {
        let _ = spill_dir;
    }
    Ok(())
}

fn command_name(program: &OsStr) -> String {
    Path::new(program)
        .file_name()
        .unwrap_or(program)
        .to_string_lossy()
        .to_ascii_lowercase()
        .trim_end_matches(".exe")
        .to_owned()
}

fn has_root_escape(value: &str) -> bool {
    let path = Path::new(value);
    path.is_absolute()
        || value.starts_with('/')
        || value.starts_with('\\')
        || value.as_bytes().get(1) == Some(&b':')
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
        || value.contains("../")
        || value.contains("..\\")
}

fn validate_existing_repo_path(
    root: &Path,
    value: &str,
) -> Result<(), CommandAdapterRejectionKind> {
    let value = value.split("::").next().unwrap_or(value);
    if has_root_escape(value) {
        return Err(CommandAdapterRejectionKind::RootEscape);
    }
    let path = std::fs::canonicalize(root.join(value))
        .map_err(|_| CommandAdapterRejectionKind::UnsupportedInvocation)?;
    if path.starts_with(root) {
        Ok(())
    } else {
        Err(CommandAdapterRejectionKind::RootEscape)
    }
}

fn validate_git_args(
    args: &[std::borrow::Cow<'_, str>],
) -> Result<(), CommandAdapterRejectionKind> {
    let Some(subcommand) = args.first() else {
        return Err(CommandAdapterRejectionKind::UnsupportedInvocation);
    };
    if !matches!(
        subcommand.as_ref(),
        "status" | "diff" | "log" | "show" | "rev-parse" | "ls-files"
    ) {
        return Err(CommandAdapterRejectionKind::UnsupportedInvocation);
    }
    for argument in &args[1..] {
        let argument = argument.as_ref();
        if matches!(
            argument,
            "-C" | "-c"
                | "--git-dir"
                | "--work-tree"
                | "--config-env"
                | "--no-index"
                | "--ext-diff"
                | "--textconv"
                | "--output"
        ) || [
            "-C=",
            "-c=",
            "--git-dir=",
            "--work-tree=",
            "--config-env=",
            "--exec-path=",
            "--namespace=",
            "--output=",
        ]
        .iter()
        .any(|prefix| argument.starts_with(prefix))
        {
            return Err(CommandAdapterRejectionKind::UnsupportedInvocation);
        }
        if has_root_escape(argument) {
            return Err(CommandAdapterRejectionKind::RootEscape);
        }
    }
    Ok(())
}

fn validate_cargo_test_args(
    args: &[std::borrow::Cow<'_, str>],
) -> Result<(), CommandAdapterRejectionKind> {
    if args.first().is_none_or(|argument| argument != "test") {
        return Err(CommandAdapterRejectionKind::UnsupportedInvocation);
    }
    let mut index = 1;
    while index < args.len() {
        let argument = args[index].as_ref();
        if matches!(
            argument,
            "--workspace"
                | "--locked"
                | "--all-targets"
                | "--all-features"
                | "--no-default-features"
                | "--lib"
                | "--bins"
                | "--tests"
                | "--benches"
                | "--doc"
                | "--quiet"
                | "-q"
        ) {
            index += 1;
            continue;
        }
        if matches!(argument, "-p" | "--package" | "--test" | "--features") {
            let Some(value) = args.get(index + 1) else {
                return Err(CommandAdapterRejectionKind::UnsupportedInvocation);
            };
            if has_root_escape(&value) || value.contains(['/', '\\']) || value.starts_with('-') {
                return Err(CommandAdapterRejectionKind::RootEscape);
            }
            index += 2;
            continue;
        }
        if argument.starts_with('-') || has_root_escape(argument) || argument.contains(['/', '\\'])
        {
            return Err(CommandAdapterRejectionKind::UnsupportedInvocation);
        }
        index += 1;
    }
    Ok(())
}

fn validate_test_runner_args(
    root: &Path,
    program: &str,
    args: &[std::borrow::Cow<'_, str>],
) -> Result<(), CommandAdapterRejectionKind> {
    match program {
        "cargo" => validate_cargo_test_args(args),
        "pnpm" => match args {
            [command] if command == "test" => Ok(()),
            [command, script] if command == "run" && script.starts_with("test") => Ok(()),
            _ => Err(CommandAdapterRejectionKind::UnsupportedInvocation),
        },
        "node" => {
            if args.first().is_none_or(|argument| argument != "--test") {
                return Err(CommandAdapterRejectionKind::UnsupportedInvocation);
            }
            for target in &args[1..] {
                if target.starts_with('-') {
                    return Err(CommandAdapterRejectionKind::UnsupportedInvocation);
                }
                validate_existing_repo_path(root, target)?;
            }
            Ok(())
        }
        "python" | "python3" => {
            if args.len() < 2 || args[0] != "-m" || args[1] != "pytest" {
                return Err(CommandAdapterRejectionKind::UnsupportedInvocation);
            }
            validate_pytest_tail(root, &args[2..])
        }
        "py" => {
            let mut index = 0;
            if args.first().is_some_and(|argument| {
                argument.starts_with('-')
                    && argument[1..]
                        .chars()
                        .all(|character| character.is_ascii_digit() || character == '.')
            }) {
                index += 1;
            }
            if args.get(index).is_none_or(|argument| argument != "-m")
                || args
                    .get(index + 1)
                    .is_none_or(|argument| argument != "pytest")
            {
                return Err(CommandAdapterRejectionKind::UnsupportedInvocation);
            }
            validate_pytest_tail(root, &args[index + 2..])
        }
        _ => Err(CommandAdapterRejectionKind::UnsupportedProgram),
    }
}

fn validate_pytest_tail(
    root: &Path,
    args: &[std::borrow::Cow<'_, str>],
) -> Result<(), CommandAdapterRejectionKind> {
    for argument in args {
        if matches!(argument.as_ref(), "-q" | "-x" | "--quiet")
            || argument.starts_with("--maxfail=")
        {
            continue;
        }
        if argument.starts_with('-') {
            return Err(CommandAdapterRejectionKind::UnsupportedInvocation);
        }
        validate_existing_repo_path(root, argument)?;
    }
    Ok(())
}

fn validate_adapter(
    adapter: CommandAdapter,
    root: &Path,
    program: &OsStr,
    args: &[OsString],
) -> Result<(), CommandAdapterRejectionKind> {
    if Path::new(program).components().count() != 1 {
        return Err(CommandAdapterRejectionKind::RootEscape);
    }
    let program = command_name(program);
    let args = args
        .iter()
        .map(|argument| argument.to_string_lossy())
        .collect::<Vec<_>>();
    match adapter {
        CommandAdapter::Git => {
            if program != "git" {
                return Err(CommandAdapterRejectionKind::UnsupportedProgram);
            }
            validate_git_args(&args)
        }
        CommandAdapter::RepositoryTestRunner => validate_test_runner_args(root, &program, &args),
    }
}

fn run_command_capped(command: Command, head: usize, tail: usize, spill_dir: &Path) -> Result<RuncResult, String> {
    run_command_capped_with_limits(command, head, tail, spill_dir, std::time::Duration::from_secs(120), &tokio_util::sync::CancellationToken::new())
}

/// Limits are enforced during pipe draining and after EOF, not only during wait.
pub fn run_command_capped_with_limits(mut command: Command, head: usize, tail: usize, spill_dir: &Path,
    timeout: std::time::Duration, cancellation: &tokio_util::sync::CancellationToken) -> Result<RuncResult, String> {
    if head > 4096 || tail > 4096 || timeout.is_zero() || timeout > std::time::Duration::from_secs(120) {
        return Err("push_capture_limit_invalid".into());
    }
    let started = std::time::Instant::now();
    let check = || {
        if cancellation.is_cancelled() { Err("push_cancelled".to_string()) }
        else if started.elapsed() >= timeout { Err("push_capture_timeout".to_string()) }
        else { Ok(()) }
    };
    check()?;
    let root = command.get_current_dir().map(Path::to_path_buf).unwrap_or_else(super::recovery::workspace_root);
    let session = std::env::var("MEMBRANE_PUSH_SESSION").unwrap_or_else(|_| "local".into());
    let scope = super::recovery::RecoveryScope::new(&root, &session).map_err(|e| e.to_string())?;
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut process = crate::providers::child_process::spawn_contained_command(command)
        .map_err(|error| format!("spawn failed: {error}"))?;
    let stdout = process.child.stdout.take().ok_or("stdout capture unavailable")?;
    let stderr = process.child.stderr.take().ok_or("stderr capture unavailable")?;
    let capture_result = capture_ordered_with_control(stdout, stderr, spill_dir, check, || process.kill_tree());
    let (capture, stdout_thread, stderr_thread) = match capture_result {
        Ok(result) => result, Err(error) => { process.kill_tree(); return Err(error); }
    };
    let status = loop {
        if let Err(error) = check() { process.kill_tree(); capture.cleanup(); return Err(error); }
        match process.child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(10)),
            Err(error) => { process.kill_tree(); capture.cleanup(); return Err(format!("wait failed: {error}")); }
        }
    };
    stdout_thread.join().map_err(|_| "stdout capture panicked")?;
    stderr_thread.join().map_err(|_| "stderr capture panicked")?;
    let total_lines = capture.newline_count.saturating_add(usize::from(capture.byte_count > 0 && !capture.ends_with_newline));
    let original = std::fs::read(&capture.path).map_err(|e| format!("capture read failed: {e}"))?;
    let result = if total_lines > head.saturating_add(tail) || capture.byte_count > MAX_INLINE_CAPTURE_BYTES || std::str::from_utf8(&original).is_err() {
        publish_spill_scoped(&capture, head, tail, spill_dir, &scope)
            .map(|(capped, path, digest, marker)| (capped, Some(path), digest, Some(marker)))
    } else {
        let full = std::str::from_utf8(&original).map_err(|_| "push_capture_encoding")?;
        // Preserve exact inline bytes; head_tail's normalization is lossy even
        // when there are no omitted lines.
        Ok((full.to_owned(), None, capture.digest.clone(), None))
    };
    capture.cleanup();
    let (capped, spill_path, digest, recovery_marker) = result?;
    Ok(RuncResult { capped, spill_path,
        anchor: if recovery_marker.is_some() { format!("mr://anchor/{digest}") } else { String::new() },
        exit_code: status.code().unwrap_or(1), recovery_marker })
}

/// Run one explicit Git or repository-test command without shell expansion.
pub fn run_adapter_capped(
    adapter: CommandAdapter,
    repo_root: &Path,
    program: &OsStr,
    args: &[OsString],
    head: usize,
    tail: usize,
    spill_dir: &Path,
) -> Result<RuncResult, CommandAdapterError> {
    let root = std::fs::canonicalize(repo_root)
        .map_err(|error| CommandAdapterError::InvalidRoot(error.to_string()))?;
    if !root.is_dir() {
        return Err(CommandAdapterError::InvalidRoot(
            "canonical root is not a directory".to_owned(),
        ));
    }
    validate_adapter(adapter, &root, program, args)
        .map_err(|kind| CommandAdapterError::Rejected { adapter, kind })?;
    let mut command = Command::new(program);
    command.args(args).current_dir(root);
    if adapter == CommandAdapter::Git {
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("GIT_") {
                command.env_remove(key);
            }
        }
        command.env("GIT_PAGER", "cat");
    }
    run_command_capped(command, head, tail, spill_dir).map_err(CommandAdapterError::Execution)
}

/// Execute `cmd` via a platform-resolved shell, returning a capped view and
/// (if truncated) a spill file path containing the full output.
pub fn run_capped(
    cmd: &str,
    head: usize,
    tail: usize,
    spill_dir: &Path,
) -> Result<RuncResult, String> {
    let argv = resolve_shell_argv();
    let program = argv
        .first()
        .ok_or_else(|| "resolved shell argv unexpectedly empty".to_string())?;

    let mut c = Command::new(program);
    if argv.len() > 1 {
        c.args(&argv[1..]);
    }
    c.arg(cmd);
    run_command_capped(c, head, tail, spill_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    /// Every test in this module reads or writes the process-global
    /// `MEMBRANE_PUSH_RUNC_SHELL` env var. Rust runs tests concurrently within one
    /// binary, and `std::env::set_var` mutates the whole process, so without a
    /// shared lock the `shell_override_*` tests race the `run_capped_*` tests —
    /// the latter would read a `bash`-polluted value and run under the wrong
    /// shell (the exit-127 flake this guard fixes). Hold this for the whole test.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn lock_env() -> MutexGuard<'static, ()> {
        // Recover from a poisoned lock: a prior test panicking while holding it
        // must not cascade-fail every other test in the module.
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn hundred_line_command() -> String {
        (0..100).map(|_| "echo l").collect::<Vec<_>>().join("; ")
    }

    #[test]
    fn default_shell_preserves_the_legacy_bash_contract() {
        let _guard = lock_env();
        let prior = std::env::var_os("MEMBRANE_PUSH_RUNC_SHELL");
        unsafe {
            std::env::remove_var("MEMBRANE_PUSH_RUNC_SHELL");
        }
        let argv = resolve_shell_argv();
        assert_eq!(argv.last().map(String::as_str), Some("-c"));
        assert!(
            argv.first().is_some_and(|program| {
                let normalized = program.replace('\\', "/").to_ascii_lowercase();
                normalized.ends_with("/bash.exe") || normalized == "bash" || normalized == "sh"
            }),
            "legacy runc contract requires a Bourne-compatible shell, got {argv:?}"
        );
        unsafe {
            match prior {
                Some(v) => std::env::set_var("MEMBRANE_PUSH_RUNC_SHELL", v),
                None => std::env::remove_var("MEMBRANE_PUSH_RUNC_SHELL"),
            }
        }
    }

    /// `MEMBRANE_PUSH_RUNC_SHELL` with just a program (no switch) — the resolver
    /// should append the platform-correct switch automatically.
    #[test]
    fn shell_override_program_only_appends_platform_switch() {
        let _guard = lock_env();
        let prior = std::env::var_os("MEMBRANE_PUSH_RUNC_SHELL");
        // SAFETY: `_guard` serializes all env-touching tests in this module, so
        // this test owns `MEMBRANE_PUSH_RUNC_SHELL` for its duration.
        unsafe {
            std::env::set_var("MEMBRANE_PUSH_RUNC_SHELL", "bash");
        }
        let argv = resolve_shell_argv();
        if cfg!(windows) {
            // On Windows, "bash" is interpreted as the program; we append /C
            // because cfg!(windows). Note: bash.exe may not exist on Windows
            // (test environments vary) — this test only checks argv parsing,
            // not whether the shell can actually run.
            assert_eq!(argv, vec!["bash".to_string(), "/C".to_string()]);
        } else {
            assert_eq!(argv, vec!["bash".to_string(), "-c".to_string()]);
        }
        unsafe {
            match prior {
                Some(v) => std::env::set_var("MEMBRANE_PUSH_RUNC_SHELL", v),
                None => std::env::remove_var("MEMBRANE_PUSH_RUNC_SHELL"),
            }
        }
    }

    /// `MEMBRANE_PUSH_RUNC_SHELL` with both program and switch — use as-is.
    #[test]
    fn shell_override_program_and_switch_used_verbatim() {
        let _guard = lock_env();
        let prior = std::env::var_os("MEMBRANE_PUSH_RUNC_SHELL");
        unsafe {
            std::env::set_var("MEMBRANE_PUSH_RUNC_SHELL", "bash -c");
        }
        let argv = resolve_shell_argv();
        assert_eq!(argv, vec!["bash".to_string(), "-c".to_string()]);
        unsafe {
            match prior {
                Some(v) => std::env::set_var("MEMBRANE_PUSH_RUNC_SHELL", v),
                None => std::env::remove_var("MEMBRANE_PUSH_RUNC_SHELL"),
            }
        }
    }

    #[test]
    fn run_capped_preserves_exit_and_spills() {
        let _guard = lock_env();
        // Pin the shell to the platform default so a leaked override from another
        // process can't change what runs here.
        let prior = std::env::var_os("MEMBRANE_PUSH_RUNC_SHELL");
        unsafe {
            std::env::remove_var("MEMBRANE_PUSH_RUNC_SHELL");
        }
        let dir = tempfile::tempdir().unwrap();

        let long_cmd = hundred_line_command();

        let r = run_capped(&long_cmd, 3, 3, dir.path()).expect("run_capped ok");
        assert_eq!(
            r.exit_code, 0,
            "expected exit 0; got {}: capped output:\n{}",
            r.exit_code, r.capped
        );
        assert!(
            r.spill_path.is_some(),
            "expected spill; capped output:\n{}",
            r.capped
        );
        let capped_lines: Vec<&str> = r.capped.lines().collect();
        assert_eq!(capped_lines.len(), 7, "capped:\n{}", r.capped);
        assert!(capped_lines[3].contains("lines elided"));

        let spill_path = r.spill_path.unwrap();
        let spilled = std::fs::read_to_string(&spill_path).unwrap();
        assert_eq!(spilled.lines().count(), 100);
        assert!(r.anchor.starts_with("mr://anchor/"));
        assert!(spill_path
            .file_stem()
            .and_then(|name| name.to_str())
            .is_some_and(|name| r.anchor.ends_with(name)));
        let metadata: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(spill_path.with_extension("json")).unwrap(),
        )
        .unwrap();
        assert_eq!(metadata["anchor"], r.anchor);
        assert!(metadata["expiresAtMillis"].as_u64() > metadata["createdAtMillis"].as_u64());

        let r2 = run_capped("exit 7", 3, 3, dir.path()).expect("run_capped ok");
        assert_eq!(r2.exit_code, 7);

        unsafe {
            match prior {
                Some(v) => std::env::set_var("MEMBRANE_PUSH_RUNC_SHELL", v),
                None => std::env::remove_var("MEMBRANE_PUSH_RUNC_SHELL"),
            }
        }
    }

    /// P2 §8.7: the spill metadata carries the one canonical context-recovery
    /// marker so truncated output can be bound to its full bytes by digest and
    /// restored exactly.
    #[test]
    fn run_capped_spill_metadata_carries_recovery_marker() {
        let _guard = lock_env();
        let prior = std::env::var_os("MEMBRANE_PUSH_RUNC_SHELL");
        unsafe {
            std::env::remove_var("MEMBRANE_PUSH_RUNC_SHELL");
        }
        let dir = tempfile::tempdir().unwrap();
        let long_cmd = hundred_line_command();
        let r = run_capped(&long_cmd, 3, 3, dir.path()).expect("run_capped ok");
        let spill = r
            .spill_path
            .as_ref()
            .unwrap_or_else(|| panic!("expected spill; capped output:\n{}", r.capped));
        let metadata: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(spill.with_extension("json")).unwrap())
                .unwrap();
        let recovery = &metadata["recovery"];
        assert_eq!(recovery["transform"], "head_tail_spill");
        assert_eq!(recovery["transformVersion"], 1);
        assert!(recovery["keptHeadBytes"]
            .as_u64()
            .is_some_and(|value| value > 0));
        assert!(recovery["keptTailBytes"]
            .as_u64()
            .is_some_and(|value| value > 0));
        assert_eq!(recovery["recoveryHandle"], metadata["anchor"]);
        assert_eq!(recovery["expiresAtMillis"], metadata["expiresAtMillis"]);
        assert_eq!(
            recovery["sourceDigest"],
            format!("sha256:{}", spill.file_stem().unwrap().to_string_lossy())
        );
        assert!(metadata["droppedBytes"]
            .as_u64()
            .is_some_and(|bytes| bytes > 0));
        assert!(recovery["droppedBytesDigest"]
            .as_str()
            .is_some_and(|d| d.starts_with("sha256:")));
        let marker = r
            .recovery_marker
            .as_ref()
            .expect("canonical recovery marker");
        let source = std::fs::read(spill).unwrap();
        let dropped_end = source.len().saturating_sub(marker.kept_tail_bytes);
        assert!(marker.kept_head_bytes <= dropped_end);
        assert_eq!(
            marker.dropped_bytes_digest,
            crate::digest::digest_bytes(&source[marker.kept_head_bytes..dropped_end]),
            "marker must bind exact omitted spill bytes, including line terminators"
        );

        unsafe {
            match prior {
                Some(v) => std::env::set_var("MEMBRANE_PUSH_RUNC_SHELL", v),
                None => std::env::remove_var("MEMBRANE_PUSH_RUNC_SHELL"),
            }
        }
    }

    /// Non-truncated output stays in memory & creates no spill artifact.
    #[test]
    fn run_capped_does_not_spill_when_output_fits() {
        let _guard = lock_env();
        let prior = std::env::var_os("MEMBRANE_PUSH_RUNC_SHELL");
        unsafe {
            std::env::remove_var("MEMBRANE_PUSH_RUNC_SHELL");
        }
        let dir = tempfile::tempdir().unwrap();
        let r = run_capped("echo hi", 100, 100, dir.path()).expect("run_capped ok");
        assert_eq!(r.exit_code, 0, "expected exit 0; got {}", r.exit_code);
        assert!(r.spill_path.is_none());
        assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());
        // `echo hi` emits "hi\n" on every shell. We trim because the truncated
        // output is what's printed (not the spilled raw output).
        assert_eq!(r.capped.trim_end(), "hi");

        unsafe {
            match prior {
                Some(v) => std::env::set_var("MEMBRANE_PUSH_RUNC_SHELL", v),
                None => std::env::remove_var("MEMBRANE_PUSH_RUNC_SHELL"),
            }
        }
    }

    #[test]
    fn newline_free_output_spills_on_byte_ceiling_with_exact_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let source = vec![b'x'; MAX_INLINE_CAPTURE_BYTES + 1];
        let (capture, stdout_thread, stderr_thread) = capture_ordered(
            std::io::Cursor::new(source.clone()),
            std::io::Cursor::new(Vec::<u8>::new()),
            dir.path(),
        )
        .expect("capture newline-free bytes");
        stdout_thread.join().unwrap();
        stderr_thread.join().unwrap();
        assert_eq!(capture.newline_count, 0);
        assert_eq!(capture.byte_count, source.len());

        let (preview, spill, digest, marker) =
            publish_spill(&capture, 100, 100, dir.path()).expect("publish exact spill");
        assert_eq!(std::fs::read(&spill).unwrap(), source);
        assert_eq!(digest, hex::encode(Sha256::digest(&source)));
        assert!(preview.len() <= (MAX_PREVIEW_EDGE_BYTES * 2) + 64);
        let dropped_end = source.len() - marker.kept_tail_bytes;
        assert_eq!(
            marker.dropped_bytes_digest,
            crate::digest::digest_bytes(&source[marker.kept_head_bytes..dropped_end])
        );
    }

    #[test]
    fn run_capped_freezes_stream_order_lossy_utf8_head_tail_and_exit_status() {
        let _guard = lock_env();
        let prior = std::env::var_os("MEMBRANE_PUSH_RUNC_SHELL");
        unsafe {
            std::env::remove_var("MEMBRANE_PUSH_RUNC_SHELL");
        }
        let dir = tempfile::tempdir().unwrap();

        let ordered = run_capped("printf out; printf err >&2; exit 7", 100, 100, dir.path())
            .expect("ordered command");
        assert_eq!(ordered.exit_code, 7);
        assert!(matches!(ordered.capped.as_str(), "outerr" | "errout"));
        assert!(ordered.spill_path.is_none());
        assert!(ordered.anchor.is_empty(), "an inline identity is not a recovery handle");

        let invalid = run_capped("printf '\\377'", 100, 100, dir.path()).expect("invalid utf8");
        assert_eq!(invalid.capped, "\u{fffd}");
        assert!(invalid.spill_path.is_some(), "lossy UTF-8 preview requires the original");
        let scope = super::super::recovery::RecoveryScope::local().unwrap();
        let exact = super::super::recovery::RecoveryStore::at(dir.path()).resolve(&scope, &invalid.anchor,
            &super::super::recovery::Selector::Whole, 16, super::super::recovery::now_ms()).unwrap();
        assert_eq!(exact.bytes().unwrap(), [255]);

        let truncated = run_capped(
            "printf 'o1\\no2\\no3\\n'; printf 'e1\\ne2\\ne3\\n' >&2",
            2,
            2,
            dir.path(),
        )
        .expect("truncated command");
        let spill = truncated.spill_path.expect("truncated output spill");
        let spilled = std::fs::read_to_string(&spill).unwrap();
        let spill_lines: Vec<&str> = spilled.lines().collect();
        // The two lanes are drained by concurrent readers and sequenced on
        // arrival, so which lane's chunk lands first is not fixed. What is
        // frozen is that every line survives, each lane keeps its own relative
        // order, and the capped view is exactly the spill's head and tail.
        assert_eq!(spill_lines.len(), 6, "spill:\n{spilled}");
        let lane = |prefix: char| {
            spill_lines
                .iter()
                .copied()
                .filter(|line| line.starts_with(prefix))
                .collect::<Vec<_>>()
        };
        assert_eq!(lane('o'), ["o1", "o2", "o3"], "spill:\n{spilled}");
        assert_eq!(lane('e'), ["e1", "e2", "e3"], "spill:\n{spilled}");
        let capped_lines: Vec<&str> = truncated.capped.lines().collect();
        assert_eq!(capped_lines.len(), 5, "capped:\n{}", truncated.capped);
        assert_eq!(&capped_lines[..2], &spill_lines[..2]);
        assert!(capped_lines[2].contains("lines elided"));
        assert_eq!(&capped_lines[3..], &spill_lines[4..]);
        assert!(truncated.anchor.starts_with("mr://anchor/"));
        let scope = super::super::recovery::RecoveryScope::local().unwrap();
        let exact = super::super::recovery::RecoveryStore::at(dir.path()).resolve(
            &scope, &truncated.anchor, &super::super::recovery::Selector::Whole,
            super::super::recovery::MAX_RESTORE_BYTES, super::super::recovery::now_ms(),
        ).unwrap();
        assert_eq!(exact.bytes().unwrap(), std::fs::read(&spill).unwrap());

        unsafe {
            match prior {
                Some(v) => std::env::set_var("MEMBRANE_PUSH_RUNC_SHELL", v),
                None => std::env::remove_var("MEMBRANE_PUSH_RUNC_SHELL"),
            }
        }
    }

    #[test]
    fn captured_original_resolves_through_shared_store_without_rerun() {
        let _guard = lock_env();
        let dir = tempfile::tempdir().unwrap();
        let out = run_capped("printf 'first\\r\\nsecond\\r\\nthird\\r\\n'", 1, 1, dir.path()).unwrap();
        let scope = super::super::recovery::RecoveryScope::local().unwrap();
        let slice = super::super::recovery::RecoveryStore::at(dir.path()).resolve(&scope, &out.anchor,
            &super::super::recovery::Selector::Lines {start:2,end:2}, 64, super::super::recovery::now_ms()).unwrap();
        assert_eq!(slice.bytes().unwrap(), b"second\r\n");
    }
    #[test]
    fn capture_deadline_and_pre_cancel_do_not_publish_partial_originals() {
        let _guard = lock_env();
        let dir = tempfile::tempdir().unwrap();
        let mut command = Command::new("node");
        command.args(["-e", "process.stdout.write('partial'); setInterval(()=>{},1000)"]);
        let started = std::time::Instant::now();
        let result = run_command_capped_with_limits(command, 1, 1, dir.path(), std::time::Duration::from_millis(100), &tokio_util::sync::CancellationToken::new());
        assert!(result.unwrap_err().contains("timeout"));
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());
        let token = tokio_util::sync::CancellationToken::new(); token.cancel();
        assert!(run_command_capped_with_limits(Command::new("nonexistent-program"), 1, 1, dir.path(), std::time::Duration::from_secs(1), &token).unwrap_err().contains("cancelled"));
    }

    #[test]
    fn adapters_run_git_and_repository_node_tests_without_shell_expansion() {
        let _guard = lock_env();
        let repo = tempfile::tempdir().unwrap();
        let spills = tempfile::tempdir().unwrap();
        let initialized = Command::new("git")
            .args(["init", "-q"])
            .current_dir(repo.path())
            .status()
            .unwrap();
        assert!(initialized.success());
        let prior_git_dir = std::env::var_os("GIT_DIR");
        unsafe {
            std::env::set_var("GIT_DIR", repo.path().join("missing-redirection"));
        }
        let git_probe = run_adapter_capped(
            CommandAdapter::Git,
            repo.path(),
            OsStr::new("git"),
            &["rev-parse".into(), "--is-inside-work-tree".into()],
            20,
            20,
            spills.path(),
        )
        .unwrap();
        assert_eq!(git_probe.capped.trim(), "true");
        unsafe {
            match prior_git_dir {
                Some(value) => std::env::set_var("GIT_DIR", value),
                None => std::env::remove_var("GIT_DIR"),
            }
        }

        let test = repo.path().join("adapter.test.mjs");
        std::fs::write(
            &test,
            "import assert from 'node:assert/strict'; import test from 'node:test'; test('adapter', () => assert.equal(1, 1));",
        )
        .unwrap();
        let node_test = run_adapter_capped(
            CommandAdapter::RepositoryTestRunner,
            repo.path(),
            OsStr::new("node"),
            &["--test".into(), "adapter.test.mjs".into()],
            100,
            100,
            spills.path(),
        )
        .unwrap();
        assert_eq!(node_test.exit_code, 0, "{}", node_test.capped);
    }

    #[test]
    fn adapters_reject_shells_and_non_test_subcommands_before_spawn() {
        let repo = tempfile::tempdir().unwrap();
        let spills = tempfile::tempdir().unwrap();
        std::fs::write(repo.path().join("inside.test.mjs"), "").unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        for (adapter, program, args) in [
            (CommandAdapter::Git, "sh", vec!["status".into()]),
            (CommandAdapter::Git, "/tmp/git", vec!["status".into()]),
            (
                CommandAdapter::Git,
                "git",
                vec!["-C".into(), "..".into(), "status".into()],
            ),
            (
                CommandAdapter::Git,
                "git",
                vec!["status".into(), "--work-tree=..".into()],
            ),
            (
                CommandAdapter::Git,
                "git",
                vec![
                    "diff".into(),
                    "--no-index".into(),
                    "inside".into(),
                    "../outside".into(),
                ],
            ),
            (
                CommandAdapter::Git,
                "git",
                vec!["diff".into(), "--output=../outside".into()],
            ),
            (
                CommandAdapter::Git,
                "git",
                vec!["show".into(), outside.path().as_os_str().to_owned()],
            ),
            (
                CommandAdapter::Git,
                "git",
                vec!["status".into(), "-c".into(), "core.fsmonitor=true".into()],
            ),
            (
                CommandAdapter::RepositoryTestRunner,
                "cargo",
                vec!["build".into()],
            ),
            (
                CommandAdapter::RepositoryTestRunner,
                "cargo",
                vec![
                    "test".into(),
                    "--manifest-path".into(),
                    "../Cargo.toml".into(),
                ],
            ),
            (
                CommandAdapter::RepositoryTestRunner,
                "cargo",
                vec!["test".into(), "--config".into(), "../cargo.toml".into()],
            ),
            (
                CommandAdapter::RepositoryTestRunner,
                "cargo",
                vec!["test".into(), "--target-dir".into(), "../target".into()],
            ),
            (
                CommandAdapter::RepositoryTestRunner,
                "pnpm",
                vec!["run".into(), "build".into()],
            ),
            (
                CommandAdapter::RepositoryTestRunner,
                "pnpm",
                vec!["--dir".into(), "..".into(), "test".into()],
            ),
            (
                CommandAdapter::RepositoryTestRunner,
                "node",
                vec!["--test".into(), "../outside.test.mjs".into()],
            ),
            (
                CommandAdapter::RepositoryTestRunner,
                "node",
                vec!["--test".into(), outside.path().as_os_str().to_owned()],
            ),
            (
                CommandAdapter::RepositoryTestRunner,
                "python3",
                vec!["script.py".into()],
            ),
            (
                CommandAdapter::RepositoryTestRunner,
                "python3",
                vec!["-m".into(), "pytest".into(), "../outside.py".into()],
            ),
            (
                CommandAdapter::RepositoryTestRunner,
                "python3",
                vec!["-m".into(), "pytest".into(), "--rootdir=..".into()],
            ),
            (
                CommandAdapter::RepositoryTestRunner,
                "py",
                vec![
                    "-3.11".into(),
                    "-m".into(),
                    "pytest".into(),
                    "../outside.py".into(),
                ],
            ),
        ] {
            let error = run_adapter_capped(
                adapter,
                repo.path(),
                OsStr::new(program),
                &args,
                20,
                20,
                spills.path(),
            )
            .unwrap_err();
            assert!(
                matches!(error, CommandAdapterError::Rejected { adapter: observed, .. } if observed == adapter),
                "{error}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_adapter_rejects_symlink_target_outside_canonical_root() {
        let repo = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        std::os::unix::fs::symlink(outside.path(), repo.path().join("escape.test.mjs")).unwrap();

        let rejection = validate_adapter(
            CommandAdapter::RepositoryTestRunner,
            &std::fs::canonicalize(repo.path()).unwrap(),
            OsStr::new("node"),
            &["--test".into(), "escape.test.mjs".into()],
        );

        assert_eq!(rejection, Err(CommandAdapterRejectionKind::RootEscape));
    }
}
