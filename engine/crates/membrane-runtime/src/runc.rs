//! L2 command runner — deterministic wrapper around shell execution.
//!
//! This is the `runc.mjs` replacement: it executes a command string, captures
//! stdout+stderr, produces a head/tail-capped view for context injection, spills
//! the full output to disk when truncated, and preserves the child exit code.

use crate::truncate;
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone)]
pub struct RuncResult {
    pub capped: String,
    pub spill_path: Option<PathBuf>,
    pub anchor: String,
    pub exit_code: i32,
}

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
    std::env::var("CRYPT_ANCHOR_TTL_MS")
        .ok()
        .and_then(|value| value.parse::<u128>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(7 * 24 * 60 * 60 * 1_000)
}

/// Resolve the shell argv prefix used to execute `cmd`.
///
/// Order:
/// 1. `CRYPT_RUNC_SHELL` if set (split by whitespace into argv). If the value
///    has no explicit `-c`/`/C`, we append the platform default switch.
/// 2. Platform default: POSIX `sh -c`, Windows Git Bash `bash -c`.
pub fn resolve_shell_argv() -> Vec<String> {
    if let Ok(raw) = std::env::var("CRYPT_RUNC_SHELL") {
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

enum StreamBody {
    Memory(Vec<u8>),
    Spill(PathBuf),
}

struct StreamCapture {
    body: StreamBody,
    byte_count: usize,
    newline_count: usize,
    ends_with_newline: bool,
}

impl StreamCapture {
    fn read_with(
        &self,
        mut consume: impl FnMut(&[u8]) -> Result<(), String>,
    ) -> Result<(), String> {
        match &self.body {
            StreamBody::Memory(bytes) => consume(bytes),
            StreamBody::Spill(path) => {
                let mut file = File::open(path)
                    .map_err(|error| format!("temporary capture reopen failed: {error}"))?;
                let mut chunk = [0u8; 64 * 1024];
                loop {
                    let read = file
                        .read(&mut chunk)
                        .map_err(|error| format!("temporary capture read failed: {error}"))?;
                    if read == 0 {
                        return Ok(());
                    }
                    consume(&chunk[..read])?;
                }
            }
        }
    }

    fn cleanup(&self) {
        if let StreamBody::Spill(path) = &self.body {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn capture_stream(
    mut reader: impl Read,
    line_limit: usize,
    spill_dir: &Path,
    lane: &str,
) -> Result<StreamCapture, String> {
    let mut memory = Vec::new();
    let mut spill: Option<(PathBuf, File)> = None;
    let mut byte_count = 0usize;
    let mut newline_count = 0usize;
    let mut ends_with_newline = false;
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut chunk)
            .map_err(|error| format!("{lane} capture failed: {error}"))?;
        if read == 0 {
            break;
        }
        let part = &chunk[..read];
        byte_count = byte_count.saturating_add(read);
        newline_count =
            newline_count.saturating_add(part.iter().filter(|byte| **byte == b'\n').count());
        ends_with_newline = part.last() == Some(&b'\n');
        if spill.is_none() && newline_count > line_limit {
            std::fs::create_dir_all(spill_dir)
                .map_err(|error| format!("spill_dir create failed: {error}"))?;
            let path = spill_dir.join(format!(
                ".capture-{}-{lane}-{}.tmp",
                std::process::id(),
                crate::time::now_millis()
            ));
            let mut file = File::create(&path)
                .map_err(|error| format!("temporary capture create failed: {error}"))?;
            file.write_all(&memory)
                .and_then(|_| file.write_all(part))
                .map_err(|error| format!("temporary capture write failed: {error}"))?;
            memory.clear();
            spill = Some((path, file));
        } else if let Some((_, file)) = spill.as_mut() {
            file.write_all(part)
                .map_err(|error| format!("temporary capture write failed: {error}"))?;
        } else {
            memory.extend_from_slice(part);
        }
    }
    let body = match spill {
        Some((path, mut file)) => {
            file.flush()
                .map_err(|error| format!("temporary capture flush failed: {error}"))?;
            StreamBody::Spill(path)
        }
        None => StreamBody::Memory(memory),
    };
    Ok(StreamCapture {
        body,
        byte_count,
        newline_count,
        ends_with_newline,
    })
}

struct LineWindow {
    head_limit: usize,
    tail_limit: usize,
    head: Vec<String>,
    tail: VecDeque<String>,
    line_count: usize,
    pending: Vec<u8>,
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
        }
    }

    fn push(&mut self, bytes: &[u8]) {
        let mut start = 0;
        for (index, byte) in bytes.iter().enumerate() {
            if *byte != b'\n' {
                continue;
            }
            self.pending.extend_from_slice(&bytes[start..index]);
            if self.pending.last() == Some(&b'\r') {
                self.pending.pop();
            }
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
        let line = String::from_utf8_lossy(&self.pending).into_owned();
        self.pending.clear();
        self.line_count += 1;
        if self.head.len() < self.head_limit {
            self.head.push(line);
        } else if self.tail_limit > 0 {
            if self.tail.len() == self.tail_limit {
                self.tail.pop_front();
            }
            self.tail.push_back(line);
        }
    }

    fn render(self) -> String {
        let elided = self
            .line_count
            .saturating_sub(self.head_limit.saturating_add(self.tail_limit));
        let mut lines = self.head;
        lines.push(format!("… {elided} lines elided …"));
        lines.extend(self.tail);
        lines.join("\n")
    }
}

fn publish_spill(
    stdout: &StreamCapture,
    stderr: &StreamCapture,
    head: usize,
    tail: usize,
    spill_dir: &Path,
) -> Result<(String, PathBuf, String), String> {
    std::fs::create_dir_all(spill_dir)
        .map_err(|error| format!("spill_dir create failed: {error}"))?;
    let temp = spill_dir.join(format!(
        ".combined-{}-{}.tmp",
        std::process::id(),
        crate::time::now_millis()
    ));
    let mut file = File::create(&temp).map_err(|error| format!("spill create failed: {error}"))?;
    let mut hasher = Sha256::new();
    let mut size_bytes = 0usize;
    let mut window = LineWindow::new(head, tail);
    let mut pending = Vec::new();
    let mut write_lossy = |bytes: &[u8]| -> Result<(), String> {
        pending.extend_from_slice(bytes);
        let Some(end) = pending.iter().rposition(|byte| *byte == b'\n') else {
            return Ok(());
        };
        let complete = pending.drain(..=end).collect::<Vec<_>>();
        let text = String::from_utf8_lossy(&complete);
        file.write_all(text.as_bytes())
            .map_err(|error| format!("spill write failed: {error}"))?;
        hasher.update(text.as_bytes());
        size_bytes = size_bytes.saturating_add(text.len());
        window.push(&complete);
        Ok(())
    };
    stdout.read_with(&mut write_lossy)?;
    stderr.read_with(&mut write_lossy)?;
    drop(write_lossy);
    if !pending.is_empty() {
        let text = String::from_utf8_lossy(&pending);
        file.write_all(text.as_bytes())
            .map_err(|error| format!("spill write failed: {error}"))?;
        hasher.update(text.as_bytes());
        size_bytes = size_bytes.saturating_add(text.len());
        window.push(&pending);
    }
    file.flush()
        .map_err(|error| format!("spill flush failed: {error}"))?;
    let digest = format!("{:x}", hasher.finalize());
    let path = spill_dir.join(format!("{digest}.log"));
    std::fs::rename(&temp, &path)
        .or_else(|error| {
            if path.is_file() {
                let _ = std::fs::remove_file(&temp);
                Ok(())
            } else {
                Err(error)
            }
        })
        .map_err(|error| format!("spill publish failed: {error}"))?;
    let metadata = spill_dir.join(format!("{digest}.json"));
    let metadata_temp = spill_dir.join(format!(
        ".{digest}.metadata.{}.tmp",
        crate::time::now_millis()
    ));
    let created_at_millis = crate::time::now_millis();
    let record = serde_json::json!({
        "schemaVersion": 1,
        "anchor": format!("mr://anchor/{digest}"),
        "sha256": digest,
        "createdAtMillis": created_at_millis,
        "expiresAtMillis": created_at_millis.saturating_add(anchor_ttl_millis()),
        "sizeBytes": size_bytes,
    });
    std::fs::write(&metadata_temp, record.to_string())
        .map_err(|error| format!("anchor metadata write failed: {error}"))?;
    std::fs::rename(&metadata_temp, &metadata)
        .map_err(|error| format!("anchor metadata publish failed: {error}"))?;
    Ok((window.finish().render(), path, digest))
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
    c.arg(cmd).stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = c
        .spawn()
        .map_err(|error| format!("spawn failed: {error}"))?;
    let stdout = child.stdout.take().ok_or("stdout capture unavailable")?;
    let stderr = child.stderr.take().ok_or("stderr capture unavailable")?;
    let line_limit = head.saturating_add(tail);
    let stdout_dir = spill_dir.to_path_buf();
    let stderr_dir = spill_dir.to_path_buf();
    let stdout_thread =
        std::thread::spawn(move || capture_stream(stdout, line_limit, &stdout_dir, "stdout"));
    let stderr_thread =
        std::thread::spawn(move || capture_stream(stderr, line_limit, &stderr_dir, "stderr"));
    let status = child
        .wait()
        .map_err(|error| format!("wait failed: {error}"))?;
    let stdout = stdout_thread
        .join()
        .map_err(|_| "stdout capture panicked".to_owned())??;
    let stderr = stderr_thread
        .join()
        .map_err(|_| "stderr capture panicked".to_owned())??;

    let total_bytes = stdout.byte_count.saturating_add(stderr.byte_count);
    let total_newlines = stdout.newline_count.saturating_add(stderr.newline_count);
    let ends_with_newline = if stderr.byte_count > 0 {
        stderr.ends_with_newline
    } else {
        stdout.ends_with_newline
    };
    let total_lines =
        total_newlines.saturating_add(usize::from(total_bytes > 0 && !ends_with_newline));
    let result = if total_lines > line_limit {
        publish_spill(&stdout, &stderr, head, tail, spill_dir)
            .map(|(capped, path, digest)| (capped, Some(path), digest))
    } else {
        let mut combined = Vec::with_capacity(total_bytes);
        stdout.read_with(|chunk| {
            combined.extend_from_slice(chunk);
            Ok(())
        })?;
        stderr.read_with(|chunk| {
            combined.extend_from_slice(chunk);
            Ok(())
        })?;
        let full = String::from_utf8_lossy(&combined).into_owned();
        let (capped, was_truncated) = truncate::head_tail(&full, head, tail);
        debug_assert!(!was_truncated);
        let digest = format!("{:x}", Sha256::digest(full.as_bytes()));
        Ok((capped, None, digest))
    };
    stdout.cleanup();
    stderr.cleanup();
    let (capped, spill_path, digest) = result?;
    let exit_code = status.code().unwrap_or(1);

    Ok(RuncResult {
        capped,
        spill_path,
        anchor: format!("mr://anchor/{digest}"),
        exit_code,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    /// Every test in this module reads or writes the process-global
    /// `CRYPT_RUNC_SHELL` env var. Rust runs tests concurrently within one
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

    #[test]
    fn default_shell_preserves_the_legacy_bash_contract() {
        let _guard = lock_env();
        let prior = std::env::var_os("CRYPT_RUNC_SHELL");
        unsafe {
            std::env::remove_var("CRYPT_RUNC_SHELL");
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
                Some(v) => std::env::set_var("CRYPT_RUNC_SHELL", v),
                None => std::env::remove_var("CRYPT_RUNC_SHELL"),
            }
        }
    }

    /// `CRYPT_RUNC_SHELL` with just a program (no switch) — the resolver
    /// should append the platform-correct switch automatically.
    #[test]
    fn shell_override_program_only_appends_platform_switch() {
        let _guard = lock_env();
        let prior = std::env::var_os("CRYPT_RUNC_SHELL");
        // SAFETY: `_guard` serializes all env-touching tests in this module, so
        // this test owns `CRYPT_RUNC_SHELL` for its duration.
        unsafe {
            std::env::set_var("CRYPT_RUNC_SHELL", "bash");
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
                Some(v) => std::env::set_var("CRYPT_RUNC_SHELL", v),
                None => std::env::remove_var("CRYPT_RUNC_SHELL"),
            }
        }
    }

    /// `CRYPT_RUNC_SHELL` with both program and switch — use as-is.
    #[test]
    fn shell_override_program_and_switch_used_verbatim() {
        let _guard = lock_env();
        let prior = std::env::var_os("CRYPT_RUNC_SHELL");
        unsafe {
            std::env::set_var("CRYPT_RUNC_SHELL", "bash -c");
        }
        let argv = resolve_shell_argv();
        assert_eq!(argv, vec!["bash".to_string(), "-c".to_string()]);
        unsafe {
            match prior {
                Some(v) => std::env::set_var("CRYPT_RUNC_SHELL", v),
                None => std::env::remove_var("CRYPT_RUNC_SHELL"),
            }
        }
    }

    #[test]
    fn run_capped_preserves_exit_and_spills() {
        let _guard = lock_env();
        // Pin the shell to the platform default so a leaked override from another
        // process can't change what runs here.
        let prior = std::env::var_os("CRYPT_RUNC_SHELL");
        unsafe {
            std::env::remove_var("CRYPT_RUNC_SHELL");
        }
        let dir = tempfile::tempdir().unwrap();

        let long_cmd = "i=1; while [ $i -le 100 ]; do echo l$i; i=$((i+1)); done".to_string();

        let r = run_capped(&long_cmd, 3, 3, dir.path()).expect("run_capped ok");
        assert_eq!(
            r.exit_code, 0,
            "expected exit 0; got {}: capped output:\n{}",
            r.exit_code, r.capped
        );
        assert!(r.spill_path.is_some());
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
                Some(v) => std::env::set_var("CRYPT_RUNC_SHELL", v),
                None => std::env::remove_var("CRYPT_RUNC_SHELL"),
            }
        }
    }

    /// Non-truncated output stays in memory & creates no spill artifact.
    #[test]
    fn run_capped_does_not_spill_when_output_fits() {
        let _guard = lock_env();
        let prior = std::env::var_os("CRYPT_RUNC_SHELL");
        unsafe {
            std::env::remove_var("CRYPT_RUNC_SHELL");
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
                Some(v) => std::env::set_var("CRYPT_RUNC_SHELL", v),
                None => std::env::remove_var("CRYPT_RUNC_SHELL"),
            }
        }
    }

    #[test]
    fn run_capped_freezes_stream_order_lossy_utf8_head_tail_and_exit_status() {
        let _guard = lock_env();
        let prior = std::env::var_os("CRYPT_RUNC_SHELL");
        unsafe {
            std::env::remove_var("CRYPT_RUNC_SHELL");
        }
        let dir = tempfile::tempdir().unwrap();

        let ordered = run_capped("printf out; printf err >&2; exit 7", 100, 100, dir.path())
            .expect("ordered command");
        assert_eq!(ordered.exit_code, 7);
        assert_eq!(ordered.capped, "outerr");
        assert!(ordered.spill_path.is_none());
        assert_eq!(
            ordered.anchor,
            format!("mr://anchor/{:x}", Sha256::digest(b"outerr"))
        );

        let invalid = run_capped("printf '\\377'", 100, 100, dir.path()).expect("invalid utf8");
        assert_eq!(invalid.capped, "\u{fffd}");
        assert!(invalid.spill_path.is_none());
        assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());

        let truncated = run_capped(
            "printf 'o1\\no2\\no3\\n'; printf 'e1\\ne2\\ne3\\n' >&2",
            2,
            2,
            dir.path(),
        )
        .expect("truncated command");
        assert_eq!(
            truncated.capped.lines().collect::<Vec<_>>(),
            ["o1", "o2", "… 2 lines elided …", "e2", "e3"]
        );
        let spill = truncated.spill_path.expect("truncated output spill");
        assert_eq!(
            std::fs::read_to_string(&spill).unwrap(),
            "o1\no2\no3\ne1\ne2\ne3\n"
        );
        assert!(spill
            .file_stem()
            .and_then(|name| name.to_str())
            .is_some_and(|digest| truncated.anchor.ends_with(digest)));

        unsafe {
            match prior {
                Some(v) => std::env::set_var("CRYPT_RUNC_SHELL", v),
                None => std::env::remove_var("CRYPT_RUNC_SHELL"),
            }
        }
    }
}
