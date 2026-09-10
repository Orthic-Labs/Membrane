// Ported from blueprint/src/graph/git-source-observation.mjs (JS legacy
// source). Canonical git base-commit + working-tree observation, shared by
// build-time (records what a generation was indexed AT) and freshness-time
// (observes what is on disk NOW) call sites. Both MUST run the identical
// `git status` invocation and hash construction, or an unchanged worktree
// could report drift purely from formatting differences between two
// independently-written git observers — there is exactly one implementation.
//
// No native git helper already existed in this crate or in
// membrane-runtime for this exact `rev-parse HEAD` + bounded `git status
// --porcelain=v1 -z` pair (confirmed via grep across both crates' src/), so
// this is a new module. The legacy implementation used Node's
// `execFileSync` with a 5000ms `timeout` option; we reproduce that bound
// manually (no new "timeout" crate dependency) via a worker thread + an
// `mpsc` channel with `recv_timeout`.

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use xxhash_rust::xxh3::xxh3_128;

const GIT_TIMEOUT: Duration = Duration::from_millis(5000);
const MAX_GIT_OUTPUT_BYTES: usize = 16 * 1024 * 1024;

/// Runs `git <args>` in `cwd` with a bounded wall-clock timeout, returning
/// stdout bytes on success (exit code 0), or `None` on any failure: git
/// missing, non-zero exit, or timeout. Mirrors the legacy JS observer's
/// blanket try/catch-to-null behavior.
fn run_git_bounded(cwd: &str, args: &[&str]) -> Option<Vec<u8>> {
    let mut command = Command::new("git");
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = command.spawn().ok()?;
    let mut stdout = child.stdout.take()?;

    let (tx, rx) = mpsc::channel();
    let reader = thread::spawn(move || {
        let mut buf = Vec::new();
        let mut chunk = [0_u8; 8192];
        let mut read_result = Ok(());
        loop {
            match stdout.read(&mut chunk) {
                Ok(0) => break,
                Ok(count) => {
                    // Keep one byte past the bound as an overflow marker,
                    // while continuing to drain the pipe so git can exit.
                    let remaining = (MAX_GIT_OUTPUT_BYTES + 1).saturating_sub(buf.len());
                    if remaining > 0 { buf.extend_from_slice(&chunk[..count.min(remaining)]); }
                }
                Err(error) => { read_result = Err(error); break; }
            }
        }
        let _ = tx.send(read_result.map(|_| buf));
    });

    let bytes = match rx.recv_timeout(GIT_TIMEOUT) {
        Ok(Ok(bytes)) => {
            let _ = reader.join();
            match child.wait() {
                Ok(status) if status.success() => Some(bytes),
                _ => None,
            }
        }
        Ok(Err(_)) => {
            let _ = child.kill();
            let _ = child.wait();
            None
        }
        Err(_) => {
            // Timed out: best-effort kill; the reader thread is left to
            // finish/detach since stdout will close once the child dies.
            let _ = child.kill();
            let _ = child.wait();
            None
        }
    }?;
    (bytes.len() <= MAX_GIT_OUTPUT_BYTES).then_some(bytes)
}

fn looks_like_sha(value: &str) -> bool {
    let len = value.len();
    if !(40..=64).contains(&len) {
        return false;
    }
    value.chars().all(|c| c.is_ascii_hexdigit())
}

/// HEAD commit, or `None` when unavailable (not a git repo, no commits, git
/// missing).
pub fn git_base_commit(root: &str) -> Option<String> {
    let bytes = run_git_bounded(root, &["rev-parse", "HEAD"])?;
    let text = String::from_utf8(bytes).ok()?;
    let trimmed = text.trim();
    if looks_like_sha(trimmed) {
        Some(trimmed.to_lowercase())
    } else {
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitSourceObservation {
    pub head: String,
    pub dirty: bool,
    pub status_digest: String,
}

/// `{ head, dirty, statusDigest }` for `root`, or `None` when git is
/// unavailable. `status_digest` is an xxh3-128 hex digest of the exact
/// porcelain status bytes — the bounded, cheap worktree fingerprint every
/// freshness comparison in Blueprint is built on.
pub fn git_source_observation(root: &str) -> Option<GitSourceObservation> {
    let head = git_base_commit(root)?;
    let status = run_git_bounded(
        root,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--",
            ".",
            ":(exclude).agent",
            ":(exclude).agent/**",
            ":(exclude)docs/product.md",
            ":(exclude)docs/architecture.md",
        ],
    )?;
    let digest = xxh3_128(&status);
    Some(GitSourceObservation {
        head,
        dirty: !status.is_empty(),
        status_digest: format!("{digest:032x}"),
    })
}

/// Path-oriented convenience wrapper for Blueprint callers that already hold
/// a confined filesystem root. It intentionally delegates to the one string
/// implementation so build-time and freshness-time status bytes cannot drift.
pub fn git_source_observation_at(root: &std::path::Path) -> Option<GitSourceObservation> {
    git_source_observation(&root.to_string_lossy())
}
