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
use std::process::Stdio;
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
    run_git_bounded_timeout(cwd, args, GIT_TIMEOUT)
}

/// `run_git_bounded` with a caller-chosen wall-clock bound. Short retrieval
/// paths (freshness reads under a request deadline) pass a fraction of their
/// remaining budget instead of the full observation timeout, so an expensive
/// or hung status read degrades to an honest incomplete observation instead
/// of consuming the caller's entire window.
fn run_git_bounded_timeout(cwd: &str, args: &[&str], timeout: Duration) -> Option<Vec<u8>> {
    let mut command = crate::hidden_command("git");
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

    let bytes = match rx.recv_timeout(timeout) {
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
    git_base_commit_bounded(root, GIT_TIMEOUT)
}

/// HEAD commit under a caller-chosen wall-clock bound. `rev-parse` is a
/// fast index read; the bound covers process spawn on loaded systems.
pub fn git_base_commit_bounded(root: &str, timeout: Duration) -> Option<String> {
    let bytes = run_git_bounded_timeout(root, &["rev-parse", "HEAD"], timeout)?;
    let text = String::from_utf8(bytes).ok()?;
    let trimmed = text.trim();
    if looks_like_sha(trimmed) {
        Some(trimmed.to_lowercase())
    } else {
        None
    }
}

/// `git rev-list --count base..HEAD` under a caller-chosen bound — how many
/// commits the sealed base lags current HEAD. `None` when unmeasurable
/// (shallow clone, unrelated history, git failure, timeout); callers must
/// treat that as unknown lag, never as zero.
pub fn git_commit_distance_bounded(root: &str, base: &str, timeout: Duration) -> Option<u64> {
    let range = format!("{base}..HEAD");
    let bytes = run_git_bounded_timeout(root, &["rev-list", "--count", &range], timeout)?;
    String::from_utf8(bytes).ok()?.trim().parse().ok()
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
            ":(exclude)docs/product/README.md",
            ":(exclude)docs/architecture/membrane.md",
        ],
    )?;
    let digest = xxh3_128(&status);
    Some(GitSourceObservation {
        head,
        dirty: !status.is_empty(),
        status_digest: format!("{digest:032x}"),
    })
}

/// Worktree fingerprint under a caller-chosen wall-clock bound. `None` means
/// the observation did not complete — timed out, failed, or git absent —
/// which callers must surface as an *incomplete* observation, never as a
/// clean tree. Splitting this from `git_base_commit_bounded` lets freshness
/// reads short-circuit on a HEAD drift before paying for `status`.
pub fn git_worktree_fingerprint_bounded(root: &str, timeout: Duration) -> Option<(bool, String)> {
    let status = run_git_bounded_timeout(
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
            ":(exclude)docs/product/README.md",
            ":(exclude)docs/architecture/membrane.md",
        ],
        timeout,
    )?;
    let digest = xxh3_128(&status);
    Some((!status.is_empty(), format!("{digest:032x}")))
}

/// Path-oriented convenience wrapper for Blueprint callers that already hold
/// a confined filesystem root. It intentionally delegates to the one string
/// implementation so build-time and freshness-time status bytes cannot drift.
pub fn git_source_observation_at(root: &std::path::Path) -> Option<GitSourceObservation> {
    git_source_observation(&root.to_string_lossy())
}
