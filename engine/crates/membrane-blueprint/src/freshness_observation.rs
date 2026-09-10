// Ported from blueprint/src/sources/freshness-observation.mjs (JS legacy
// source), function `observeRepositoryFreshness`. Content-free repository
// freshness evidence owned by Blueprint: per-file overlay enumeration (from
// `git status --porcelain=v1 -z --untracked-files=all`) with per-entry
// content hashes, a before/after re-check for stability, an optional
// commit-distance count against a caller-supplied base commit, and bounded
// file-count/byte limits. Git never crosses the Blueprint boundary beyond
// this module: only bounded commit, status, path, and content-digest
// evidence leaves it.
//
// The bounded-timeout git invocation pattern mirrors
// `crate::git_source_observation`'s `run_git_bounded` (that module's helper
// is private to it, and this module needs slightly different invocations
// -- a `-z`-parsed status plus `rev-list --count` -- so a small local copy
// is kept here rather than reaching into a sibling module's private API).

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

const MAX_OVERLAY_FILES: usize = 64;
const MAX_OVERLAY_BYTES: u64 = 64 * 1024 * 1024;
const MAX_GIT_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
const GIT_TIMEOUT: Duration = Duration::from_millis(2_000);
const IGNORED_OVERLAY_PREFIXES: [&str; 3] = [".agent/", ".blueprint/", "memory-mirror/"];

fn digest_bytes(value: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value);
    format!("sha256:{}", hasher.finalize().iter().map(|b| format!("{b:02x}")).collect::<String>())
}

fn run_git_bounded(root: &Path, args: &[&str], max_buffer: usize) -> Option<Vec<u8>> {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(root)
        .args(args)
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
                    // Keep one byte past bound as overflow marker, while
                    // continuing to drain pipe so git can exit cleanly.
                    let remaining = (max_buffer + 1).saturating_sub(buf.len());
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
            let _ = child.kill();
            let _ = child.wait();
            None
        }
    }?;
    if bytes.len() > max_buffer {
        return None;
    }
    Some(bytes)
}

fn git_text(root: &Path, args: &[&str]) -> Option<String> {
    let bytes = run_git_bounded(root, args, 64 * 1024)?;
    let text = String::from_utf8(bytes).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Confines `value` (a git-status-reported path, using `/` separators after
/// legacy's `replaceAll("\\", "/")`) to `root`, rejecting absolute paths and
/// any `..` component -- mirroring `normalizePath` in the legacy module.
fn normalize_path(root: &Path, value: &str) -> Option<String> {
    let normalized = value.replace('\\', "/");
    if normalized.is_empty() {
        return None;
    }
    if Path::new(&normalized).is_absolute() {
        return None;
    }
    if normalized.split('/').any(|segment| segment == "..") {
        return None;
    }
    let absolute = root.join(&normalized);
    let confined = pathdiff(&absolute, root)?;
    if confined.is_empty() || confined.starts_with("../") {
        return None;
    }
    Some(confined)
}

/// Minimal relative-path computation for two paths that are already known
/// to share `root` as an ancestor prefix (the caller always joins onto
/// `root`), avoiding a new `pathdiff` crate dependency.
fn pathdiff(absolute: &Path, root: &Path) -> Option<String> {
    let abs_components: Vec<Component> = absolute.components().collect();
    let root_components: Vec<Component> = root.components().collect();
    if abs_components.len() < root_components.len() {
        return None;
    }
    if abs_components[..root_components.len()] != root_components[..] {
        return None;
    }
    let rest: Vec<String> = abs_components[root_components.len()..]
        .iter()
        .map(|c| c.as_os_str().to_string_lossy().replace('\\', "/"))
        .collect();
    Some(rest.join("/"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StatusEntry {
    path: String,
    status: String,
}

/// Parses NUL-separated `git status --porcelain=v1 -z` output, mirroring
/// `parseStatus`: rejects malformed records and paths that escape the
/// repository, skips nested git-repo directories and ignored overlay
/// prefixes, and consumes the extra rename/copy source field.
fn parse_status(root: &Path, bytes: &[u8]) -> Result<Vec<StatusEntry>, &'static str> {
    let text = String::from_utf8_lossy(bytes);
    let fields: Vec<&str> = text.split('\0').collect();
    let mut entries = Vec::new();
    let mut index = 0usize;
    while index < fields.len() {
        let field = fields[index];
        if field.is_empty() {
            index += 1;
            continue;
        }
        if field.len() < 4 || field.as_bytes()[2] != b' ' {
            return Err("invalid git status record");
        }
        let status = field[0..2].to_string();
        let path = normalize_path(root, &field[3..]).ok_or("git status path escaped repository")?;
        let first_char = status.chars().next().unwrap_or(' ');
        if first_char == 'R' || first_char == 'C' {
            index += 1; // consume rename/copy source field
        }
        let absolute = root.join(&path);
        if absolute.is_dir() && absolute.join(".git").exists() {
            index += 1;
            continue;
        }
        if IGNORED_OVERLAY_PREFIXES.iter().any(|prefix| path.starts_with(prefix)) {
            index += 1;
            continue;
        }
        entries.push(StatusEntry { path, status });
        index += 1;
    }
    Ok(entries)
}

struct HashedEntry {
    path: String,
    status: String,
    content_hash: Option<String>,
    bytes: u64,
    stable: bool,
    observation_failed: bool,
}

#[cfg(unix)]
fn metadata_mode(metadata: &std::fs::Metadata) -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    Some(metadata.mode())
}

#[cfg(not(unix))]
fn metadata_mode(metadata: &std::fs::Metadata) -> Option<u32> {
    Some(if metadata.permissions().readonly() { 1 } else { 0 })
}

fn same_metadata(before: &std::fs::Metadata, after: &std::fs::Metadata) -> bool {
    before.len() == after.len()
        && before.modified().ok() == after.modified().ok()
        && metadata_mode(before) == metadata_mode(after)
}

/// Mirrors `hashEntry`: deleted entries hash to a fixed `"deleted"` digest,
/// symlinks hash their target text, oversized regular files are reported
/// unstable with no digest, and everything else is a stable-or-not read of
/// the full file body with a before/after metadata re-check.
fn hash_entry(root: &Path, entry: StatusEntry) -> HashedEntry {
    let absolute = root.join(&entry.path);
    if entry.status.contains('D') && !absolute.exists() {
        return HashedEntry {
            path: entry.path,
            status: entry.status,
            content_hash: Some(digest_bytes(b"deleted")),
            bytes: 0,
            stable: true,
            observation_failed: false,
        };
    }
    let before = match std::fs::symlink_metadata(&absolute) {
        Ok(meta) => meta,
        Err(_) => {
            return HashedEntry {
                path: entry.path,
                status: entry.status,
                content_hash: None,
                bytes: 0,
                stable: false,
                observation_failed: true,
            };
        }
    };
    if before.file_type().is_symlink() {
        let stable = match std::fs::read_link(&absolute) {
            Ok(target) => {
                let target_str = target.to_string_lossy().to_string();
                let after = std::fs::symlink_metadata(&absolute).ok();
                let hash = digest_bytes(target_str.as_bytes());
                let stable = after.map(|a| same_metadata(&before, &a)).unwrap_or(false);
                return HashedEntry {
                    path: entry.path,
                    status: entry.status,
                    content_hash: Some(hash),
                    bytes: target_str.len() as u64,
                    stable,
                    observation_failed: false,
                };
            }
            Err(_) => false,
        };
        return HashedEntry {
            path: entry.path,
            status: entry.status,
            content_hash: None,
            bytes: 0,
            stable,
            observation_failed: true,
        };
    }
    if !before.is_file() {
        return HashedEntry {
            path: entry.path,
            status: entry.status,
            content_hash: None,
            bytes: 0,
            stable: false,
            observation_failed: true,
        };
    }
    if before.len() > MAX_OVERLAY_BYTES {
        return HashedEntry {
            path: entry.path,
            status: entry.status,
            content_hash: None,
            bytes: before.len(),
            stable: false,
            observation_failed: false,
        };
    }
    let body = match std::fs::read(&absolute) {
        Ok(body) => body,
        Err(_) => {
            return HashedEntry {
                path: entry.path,
                status: entry.status,
                content_hash: None,
                bytes: 0,
                stable: false,
                observation_failed: true,
            };
        }
    };
    let after = std::fs::symlink_metadata(&absolute).ok();
    let stable = after.map(|a| same_metadata(&before, &a)).unwrap_or(false);
    HashedEntry {
        path: entry.path,
        status: entry.status,
        content_hash: Some(digest_bytes(&body)),
        bytes: before.len(),
        stable,
        observation_failed: false,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FreshnessOverlayEntry {
    pub path: String,
    pub status: String,
    pub content_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryFreshnessObservation {
    pub available: bool,
    pub stable: bool,
    pub revision: Option<String>,
    pub commit_distance: Option<u64>,
    pub entries: Vec<FreshnessOverlayEntry>,
    pub limit_exceeded: bool,
    /// Stage timings use same map shape as runtime `OverlayObservation`.
    /// Legacy observer has one bounded git stage today.
    pub stage_elapsed_ms: BTreeMap<String, u64>,
    pub reason: Option<String>,
}

fn unavailable(reason: &'static str, elapsed: Instant) -> RepositoryFreshnessObservation {
    RepositoryFreshnessObservation {
        available: false,
        stable: false,
        revision: None,
        commit_distance: None,
        entries: Vec::new(),
        limit_exceeded: false,
        stage_elapsed_ms: stage_elapsed(elapsed),
        reason: Some(reason.to_string()),
    }
}

fn stage_elapsed(started: Instant) -> BTreeMap<String, u64> {
    BTreeMap::from([(String::from("git_status"), started.elapsed().as_millis() as u64)])
}

/// Convert canonical git observation into freshness-receipt current-state
/// input. No freshness verdict is inferred here; unavailable git remains
/// `available: false`, while dirty is informational evidence.
pub fn observe_current_vcs_state(repo_root: &Path) -> crate::freshness::CurrentSourceState {
    match crate::git_source_observation::git_source_observation(&repo_root.to_string_lossy()) {
        Some(observation) => crate::freshness::CurrentSourceState {
            available: true,
            vcs_revision: Some(observation.head),
            dirty: Some(observation.dirty),
            worktree_fingerprint: Some(observation.status_digest),
        },
        None => crate::freshness::CurrentSourceState::default(),
    }
}

/// Changed source paths used by query-time freshness suppression. This is
/// evidence about paths only, never a second freshness verdict. Empty git
/// output is a successful empty list (not an unavailable observation).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangedPathsObservation {
    pub complete: bool,
    pub paths: Vec<String>,
    pub reason: Option<String>,
}

impl ChangedPathsObservation {
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self { complete: false, paths: Vec::new(), reason: Some(reason.into()) }
    }
}

impl From<ChangedPathsObservation> for crate::freshness_receipt::ChangedPaths {
    fn from(value: ChangedPathsObservation) -> Self {
        Self { complete: value.complete, paths: value.paths, reason: value.reason }
    }
}

fn git_lines(root: &Path, args: &[&str]) -> Option<Vec<String>> {
    let bytes = run_git_bounded(root, args, 4 * 1024 * 1024)?;
    let text = String::from_utf8(bytes).ok()?;
    Some(text
        .lines()
        .map(|line| line.trim().replace('\\', "/"))
        .filter(|line| !line.is_empty())
        .collect())
}

/// Port of legacy `changedPathsSinceGeneration`. Git command failures are
/// represented as `complete:false`; callers must then suppress whole
/// generation rather than guessing path freshness.
pub fn changed_paths_since_generation(
    repo_root: &Path,
    indexed_revision: Option<&str>,
    current_available: bool,
) -> ChangedPathsObservation {
    let Some(indexed_revision) = indexed_revision.filter(|value| !value.is_empty()) else {
        return ChangedPathsObservation::unavailable("comparison_unavailable");
    };
    if !current_available {
        return ChangedPathsObservation::unavailable("comparison_unavailable");
    }
    let committed = git_lines(repo_root, &["diff", "--no-renames", "--name-only", "--diff-filter=ACDMRTUXB", indexed_revision, "--"]);
    let worktree = git_lines(repo_root, &["diff", "--no-renames", "--name-only", "--diff-filter=ACDMRTUXB", "HEAD", "--"]);
    let untracked = git_lines(repo_root, &["ls-files", "--others", "--exclude-standard"]);
    let (Some(committed), Some(worktree), Some(untracked)) = (committed, worktree, untracked) else {
        return ChangedPathsObservation::unavailable("comparison_failed");
    };
    let mut paths = BTreeMap::new();
    for path in committed.into_iter().chain(worktree).chain(untracked) {
        paths.insert(path, ());
    }
    ChangedPathsObservation { complete: true, paths: paths.into_keys().collect(), reason: None }
}

/// Receipt-oriented adapter that accepts the canonical freshness types used
/// by `build_freshness_receipt`, avoiding duplicated extraction of the indexed
/// revision and availability gate at production call sites.
pub fn changed_paths_for_freshness(
    repo_root: &Path,
    generation: &crate::freshness::GenerationFreshnessBasis,
    current: &crate::freshness::CurrentSourceState,
) -> crate::freshness_receipt::ChangedPaths {
    changed_paths_since_generation(
        repo_root,
        generation.indexed_revision.as_deref(),
        current.available,
    )
    .into()
}

/// Native port of `observeRepositoryFreshness(repoRoot, { baseCommit })`.
///
/// Enumerates the working-tree overlay (uncommitted + untracked changes)
/// against the last-known-good HEAD, content-hashing every overlay entry
/// and re-checking git status/HEAD after hashing to detect a change that
/// raced the observation (`stable: false` in that case, matching the
/// legacy double-read design). Bounded to `MAX_OVERLAY_FILES` entries and
/// `MAX_OVERLAY_BYTES` total content bytes; either bound trips
/// `limit_exceeded: true` with an empty entry list rather than a partial,
/// silently-truncated one. `base_commit`, when given, yields
/// `commit_distance` via `git rev-list --count base..HEAD` (0 when HEAD
/// already equals `base_commit`).
pub fn observe_repository_freshness(
    repo_root: &Path,
    base_commit: Option<&str>,
) -> RepositoryFreshnessObservation {
    let started = Instant::now();
    let root = match repo_root.canonicalize() {
        Ok(root) => root,
        Err(_) => return unavailable("git_status_unavailable", started),
    };

    let revision_before = git_text(&root, &["rev-parse", "HEAD"]);
    let status_before = run_git_bounded(
        &root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        MAX_GIT_OUTPUT_BYTES,
    );
    let (revision_before, status_before) = match (revision_before, status_before) {
        (Some(rev), Some(status)) => (rev, status),
        _ => return unavailable("git_status_unavailable", started),
    };

    let status_entries = match parse_status(&root, &status_before) {
        Ok(entries) => entries,
        Err(_) => return unavailable("overlay_observation_failed", started),
    };

    if status_entries.len() > MAX_OVERLAY_FILES {
        return RepositoryFreshnessObservation {
            available: true,
            stable: false,
            revision: Some(revision_before),
            commit_distance: None,
            entries: Vec::new(),
            limit_exceeded: true,
            stage_elapsed_ms: stage_elapsed(started),
            reason: Some("overlay_file_limit_exceeded".to_string()),
        };
    }

    let mut total_bytes: u64 = 0;
    let mut stable = true;
    let mut missing_hash = false;
    let mut observation_failed = false;
    let mut entries = Vec::with_capacity(status_entries.len());
    for entry in status_entries {
        let hashed = hash_entry(&root, entry);
        total_bytes = total_bytes.saturating_add(hashed.bytes);
        stable &= hashed.stable;
        observation_failed |= hashed.observation_failed;
        if hashed.content_hash.is_none() {
            missing_hash = true;
        }
        entries.push(FreshnessOverlayEntry {
            path: hashed.path,
            status: hashed.status,
            content_hash: hashed.content_hash,
        });
    }

    if observation_failed {
        return unavailable("overlay_observation_failed", started);
    }
    if total_bytes > MAX_OVERLAY_BYTES || missing_hash {
        return RepositoryFreshnessObservation {
            available: true,
            stable: false,
            revision: Some(revision_before),
            commit_distance: None,
            entries: Vec::new(),
            limit_exceeded: true,
            stage_elapsed_ms: stage_elapsed(started),
            reason: Some("overlay_byte_limit_exceeded".to_string()),
        };
    }

    let status_after = run_git_bounded(
        &root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        MAX_GIT_OUTPUT_BYTES,
    );
    let revision_after = git_text(&root, &["rev-parse", "HEAD"]);
    let (status_after, revision_after) = match (status_after, revision_after) {
        (Some(status), Some(rev)) => (status, rev),
        _ => return unavailable("git_status_unavailable", started),
    };

    stable &= status_before == status_after && revision_before == revision_after;
    entries.sort_by(|a, b| a.path.cmp(&b.path).then_with(|| a.status.cmp(&b.status)));

    let commit_distance = base_commit.and_then(|base| {
        if revision_after == base {
            Some(0)
        } else {
            git_text(&root, &["rev-list", "--count", &format!("{base}..{revision_after}")])
                .and_then(|value| value.parse::<u64>().ok())
        }
    });

    RepositoryFreshnessObservation {
        available: true,
        stable,
        revision: Some(revision_after),
        commit_distance,
        entries,
        limit_exceeded: false,
        stage_elapsed_ms: stage_elapsed(started),
        reason: None,
    }
}

/// Test-only accessor kept for parity tests, mirroring the legacy module's
/// exported `_internals`.
#[doc(hidden)]
pub fn _internal_normalize_path(root: &Path, value: &str) -> Option<String> {
    normalize_path(root, value)
}

#[allow(dead_code)]
fn _unused_pathbuf_hint(_p: PathBuf) {}
