//! One content-free freshness verdict for every RightContext consumer.
//!
//! The evaluator uses an epoch sandwich: snapshot/skills epoch A, a self-stable
//! bounded working-tree overlay, then epoch B. A verdict is returned only when
//! both epochs match. This prevents callers from observing a commit, Blueprint
//! reindex, or skills ingest assembled from different moments in time.

use crate::MemoryStore;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const FRESHNESS_SCHEMA_VERSION: u32 = 1;
pub const MAX_FRESHNESS_ATTEMPTS: usize = 3;
pub const MAX_RETURNED_OVERLAY_ENTRIES: usize = 64;
const MAX_OVERLAY_FILES: usize = 512;
const MAX_OVERLAY_BYTES: u64 = 64 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 2 * 1024 * 1024;
const GRAPH_HEADER_BYTES: u64 = 128 * 1024;
const MAX_GIT_STATUS_BYTES: u64 = 8 * 1024 * 1024;
const MAX_GIT_TEXT_BYTES: u64 = 64 * 1024;
const GIT_CHILD_TIMEOUT: Duration = Duration::from_secs(2);
const CHILD_POLL_INTERVAL: Duration = Duration::from_millis(5);
const FIRST_AFTER_IDLE_THRESHOLD: Duration = Duration::from_secs(5 * 60);

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FreshnessEpoch {
    pub head_commit: Option<String>,
    pub base_commit: Option<String>,
    pub manifest_digest: Option<String>,
    pub blueprint_generation: Option<String>,
    pub graph_manifest_generation: Option<String>,
    pub graph_body_generation: Option<String>,
    pub skills_generation: Option<String>,
}

impl FreshnessEpoch {
    /// Convenience constructor for deterministic probes and downstream conformance tests.
    pub fn coherent_for_test(head: &str, generation: &str, skills: &str) -> Self {
        Self {
            head_commit: Some(head.to_string()),
            base_commit: Some(head.to_string()),
            manifest_digest: Some(format!(
                "sha256:{:x}",
                Sha256::digest(generation.as_bytes())
            )),
            blueprint_generation: Some(generation.to_string()),
            graph_manifest_generation: Some(generation.to_string()),
            graph_body_generation: Some(generation.to_string()),
            skills_generation: Some(skills.to_string()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayEntry {
    pub path: String,
    pub status: String,
    pub content_hash: String,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayObservation {
    pub stable: bool,
    #[serde(default)]
    pub entries: Vec<OverlayEntry>,
    #[serde(default)]
    pub limit_exceeded: bool,
    #[serde(default)]
    pub stage_elapsed_ms: BTreeMap<String, u64>,
}

pub trait FreshnessProbe {
    fn read_epoch(&mut self) -> Result<FreshnessEpoch, String>;
    fn read_overlay(&mut self) -> Result<OverlayObservation, String>;
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphState {
    Clean,
    DirtyOverlay,
    StaleSnapshot,
    MissingSnapshot,
    PartialReindex,
    ConcurrentUpdate,
    Indeterminate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReindexState {
    Idle,
    Partial,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderVerdict {
    pub freshness_class: String,
    pub usable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderVerdicts {
    pub blueprint: ProviderVerdict,
    pub dirty_overlay: ProviderVerdict,
    pub skills: ProviderVerdict,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FreshnessVerdict {
    pub schema_version: u32,
    pub checked_at: String,
    pub service_generation: String,
    pub release_generation: String,
    pub first_after_idle: bool,
    pub idle_gap_ms: Option<u64>,
    pub cache_age_ms: Option<u64>,
    pub refresh_in_flight: bool,
    pub stage_elapsed_ms: BTreeMap<String, u64>,
    pub snapshot_id: String,
    pub stable: bool,
    pub attempts: usize,
    pub graph_state: GraphState,
    pub head_commit: Option<String>,
    pub base_commit: Option<String>,
    pub blueprint_base_commit: Option<String>,
    pub manifest_digest: Option<String>,
    pub blueprint_generation: Option<String>,
    pub skills_generation: Option<String>,
    pub overlay_digest: String,
    pub overlay_count: usize,
    pub overlay_truncated: bool,
    pub reindex_state: ReindexState,
    pub overlay_entries: Vec<OverlayEntry>,
    pub providers: ProviderVerdicts,
    pub reasons: Vec<String>,
}

/// Hash-bound barrier proof consumed by Membrane adapters. The working-tree
/// graph remains session-neutral; overlay identity is carried only here.
pub fn source_barrier_receipt(
    verdict: &FreshnessVerdict,
    repository_id: &str,
    session_id: &str,
    worktree_path: &str,
) -> serde_json::Value {
    let generation_id = contract_digest(
        verdict
            .blueprint_generation
            .clone()
            .unwrap_or_else(|| digest_bytes(verdict.snapshot_id.as_bytes())),
    );
    let manifest_digest = contract_digest(
        verdict
            .manifest_digest
            .clone()
            .unwrap_or_else(|| digest_bytes(verdict.snapshot_id.as_bytes())),
    );
    let source_observation_digest = digest_bytes(
        serde_json::json!({
            "head": verdict.head_commit,
            "base": verdict.base_commit,
            "blueprint_base": verdict.blueprint_base_commit,
            "generation": generation_id,
        })
        .to_string()
        .as_bytes(),
    );
    let status = match verdict.graph_state {
        GraphState::Clean | GraphState::DirtyOverlay => "current",
        GraphState::StaleSnapshot => "stale",
        GraphState::ConcurrentUpdate => "drifted",
        GraphState::PartialReindex => "degraded",
        GraphState::MissingSnapshot | GraphState::Indeterminate => "blocked",
    };
    serde_json::json!({
        "schema": "orthic.source-barrier-receipt.v1",
        "repository_id": repository_id,
        "barrier_clock": 0,
        "applied_graph_clock": 0,
        "event_gap": verdict.reasons.iter().any(|reason| reason == "event_gap"),
        "generation_id": generation_id,
        "manifest_digest": manifest_digest,
        "source_observation_digest": source_observation_digest,
        "dirty_overlay_digest": contract_digest(verdict.overlay_digest.clone()),
        "overlay_identity": { "session_id": session_id, "worktree_path": worktree_path },
        "status": status,
    })
}

fn contract_digest(value: String) -> String {
    if value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        value
    } else {
        digest_bytes(value.as_bytes())
    }
}

#[derive(Clone, Default)]
pub struct FreshnessCoordinator {
    state: Arc<Mutex<HashMap<PathBuf, FreshnessCacheEntry>>>,
}

#[derive(Clone, Default)]
struct FreshnessCacheEntry {
    verdict: Option<FreshnessVerdict>,
    observed_at: Option<Instant>,
    refresh_in_flight: bool,
}

impl FreshnessCoordinator {
    /// Return the most recent generation-stamped verdict immediately and refresh it on a
    /// background thread. Repository observation is never executed by the prompt HTTP waiter.
    pub fn latest_or_schedule(&self, store: MemoryStore, repo_root: PathBuf) -> FreshnessVerdict {
        let refresh_root = repo_root.clone();
        self.latest_or_schedule_with(repo_root, move || {
            evaluate_repository_freshness(&store, refresh_root)
        })
    }

    fn latest_or_schedule_with<F>(&self, repo_root: PathBuf, evaluator: F) -> FreshnessVerdict
    where
        F: FnOnce() -> FreshnessVerdict + Send + 'static,
    {
        let (mut returned, launch) = {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            let entry = state.entry(repo_root.clone()).or_default();
            let launch = !entry.refresh_in_flight;
            if launch {
                entry.refresh_in_flight = true;
            }
            let mut returned = entry
                .verdict
                .clone()
                .unwrap_or_else(refresh_pending_verdict);
            returned.cache_age_ms = entry.observed_at.map(elapsed_ms);
            returned.refresh_in_flight = entry.refresh_in_flight;
            (returned, launch)
        };

        if launch {
            let coordinator = self.clone();
            std::thread::spawn(move || {
                let evaluated = std::panic::catch_unwind(std::panic::AssertUnwindSafe(evaluator));
                let mut state = coordinator
                    .state
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                let entry = state.entry(repo_root).or_default();
                if let Ok(mut verdict) = evaluated {
                    verdict.cache_age_ms = Some(0);
                    verdict.refresh_in_flight = false;
                    entry.verdict = Some(verdict);
                    entry.observed_at = Some(Instant::now());
                }
                entry.refresh_in_flight = false;
            });
        }
        returned.refresh_in_flight = true;
        returned
    }

    #[cfg(test)]
    fn cached_for_test(&self, repo_root: &Path) -> Option<FreshnessVerdict> {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(repo_root)
            .and_then(|entry| entry.verdict.clone())
    }
}

fn refresh_pending_verdict() -> FreshnessVerdict {
    let mut verdict = unavailable(
        GraphState::Indeterminate,
        0,
        "refresh_pending",
        BTreeMap::new(),
    );
    verdict.refresh_in_flight = true;
    verdict
}

pub fn evaluate_freshness(
    probe: &mut impl FreshnessProbe,
    requested_attempts: usize,
) -> FreshnessVerdict {
    let attempts = requested_attempts.clamp(1, MAX_FRESHNESS_ATTEMPTS);
    let mut stage_elapsed_ms = BTreeMap::new();
    for attempt in 1..=attempts {
        let before = match probe.read_epoch() {
            Ok(epoch) => epoch,
            Err(_) => return indeterminate(attempt, "epoch_unavailable", stage_elapsed_ms),
        };
        let overlay = match probe.read_overlay() {
            Ok(overlay) => overlay,
            Err(_) => return indeterminate(attempt, "overlay_unavailable", stage_elapsed_ms),
        };
        merge_stage_elapsed(&mut stage_elapsed_ms, &overlay.stage_elapsed_ms);
        let after = match probe.read_epoch() {
            Ok(epoch) => epoch,
            Err(_) => return indeterminate(attempt, "epoch_unavailable", stage_elapsed_ms),
        };

        if overlay.limit_exceeded {
            return indeterminate(attempt, "overlay_limit_exceeded", stage_elapsed_ms);
        }
        if overlay.stable && before == after {
            return classify(before, overlay.entries, attempt, stage_elapsed_ms);
        }
    }
    unavailable(
        GraphState::ConcurrentUpdate,
        attempts,
        "concurrent_update",
        stage_elapsed_ms,
    )
}

fn classify(
    epoch: FreshnessEpoch,
    mut entries: Vec<OverlayEntry>,
    attempts: usize,
    stage_elapsed_ms: BTreeMap<String, u64>,
) -> FreshnessVerdict {
    entries.sort();
    let overlay_count = entries.len();
    let overlay_digest = digest_overlay(&entries);
    let overlay_truncated = overlay_count > MAX_RETURNED_OVERLAY_ENTRIES;
    let returned_entries = entries
        .into_iter()
        .take(MAX_RETURNED_OVERLAY_ENTRIES)
        .collect::<Vec<_>>();

    let generations = [
        epoch.blueprint_generation.as_ref(),
        epoch.graph_manifest_generation.as_ref(),
        epoch.graph_body_generation.as_ref(),
    ];
    let present_generations = generations.iter().filter(|value| value.is_some()).count();
    let generations_match = present_generations == generations.len()
        && generations.windows(2).all(|pair| pair[0] == pair[1]);
    let snapshot_present = epoch.manifest_digest.is_some() && present_generations > 0;

    let (graph_state, reindex_state, blueprint_class, blueprint_usable, mut reasons) =
        if !snapshot_present && present_generations == 0 {
            (
                GraphState::MissingSnapshot,
                ReindexState::Unknown,
                "unknown",
                false,
                vec!["snapshot_missing".to_string()],
            )
        } else if !generations_match || epoch.manifest_digest.is_none() {
            (
                GraphState::PartialReindex,
                ReindexState::Partial,
                "unknown",
                false,
                vec!["generation_mismatch".to_string()],
            )
        } else if epoch.head_commit.is_none() || epoch.base_commit.is_none() {
            (
                GraphState::Indeterminate,
                ReindexState::Unknown,
                "unknown",
                false,
                vec!["commit_epoch_missing".to_string()],
            )
        } else if epoch.head_commit != epoch.base_commit {
            (
                GraphState::StaleSnapshot,
                ReindexState::Idle,
                "stale_snapshot",
                true,
                vec!["snapshot_base_differs_from_head".to_string()],
            )
        } else if overlay_count > 0 {
            (
                GraphState::DirtyOverlay,
                ReindexState::Idle,
                "committed_snapshot",
                true,
                vec!["working_tree_dirty".to_string()],
            )
        } else {
            (
                GraphState::Clean,
                ReindexState::Idle,
                "committed_snapshot",
                true,
                Vec::new(),
            )
        };

    if epoch.skills_generation.is_none() {
        reasons.push("skills_generation_missing".to_string());
    }
    // Provider availability is lane-local once the sandwich itself is stable. A partial
    // Blueprint publish must not disable a verified Git overlay or a coherent skills snapshot.
    let skills_usable = epoch.skills_generation.is_some();
    let overlay_base_commit = epoch.head_commit.clone();
    let blueprint_base_commit = epoch.base_commit.clone();
    let overlay_usable = overlay_count > 0 && overlay_base_commit.is_some();
    let snapshot_id = digest_snapshot(&epoch, &overlay_digest);

    let (first_after_idle, idle_gap_ms) = idle_observation();
    FreshnessVerdict {
        schema_version: FRESHNESS_SCHEMA_VERSION,
        checked_at: crate::time::now_iso(),
        service_generation: crate::release_identity::service_generation().to_string(),
        release_generation: crate::release_identity::release_generation(),
        first_after_idle,
        idle_gap_ms,
        cache_age_ms: Some(0),
        refresh_in_flight: false,
        stage_elapsed_ms,
        snapshot_id,
        stable: graph_state != GraphState::Indeterminate,
        attempts,
        graph_state,
        head_commit: epoch.head_commit,
        base_commit: overlay_base_commit,
        blueprint_base_commit,
        manifest_digest: epoch.manifest_digest,
        blueprint_generation: epoch.blueprint_generation,
        skills_generation: epoch.skills_generation,
        overlay_digest,
        overlay_count,
        overlay_truncated,
        reindex_state,
        overlay_entries: returned_entries,
        providers: ProviderVerdicts {
            blueprint: ProviderVerdict {
                freshness_class: blueprint_class.to_string(),
                usable: blueprint_usable,
            },
            dirty_overlay: ProviderVerdict {
                freshness_class: if overlay_count == 0 {
                    "current"
                } else {
                    "dirty_overlay"
                }
                .to_string(),
                usable: overlay_usable,
            },
            skills: ProviderVerdict {
                freshness_class: if skills_usable { "current" } else { "unknown" }.to_string(),
                usable: skills_usable,
            },
        },
        reasons,
    }
}

fn indeterminate(
    attempts: usize,
    reason: &str,
    stage_elapsed_ms: BTreeMap<String, u64>,
) -> FreshnessVerdict {
    unavailable(
        GraphState::Indeterminate,
        attempts,
        reason,
        stage_elapsed_ms,
    )
}

fn unavailable(
    state: GraphState,
    attempts: usize,
    reason: &str,
    stage_elapsed_ms: BTreeMap<String, u64>,
) -> FreshnessVerdict {
    let class = "unknown";
    let empty_overlay_digest = digest_overlay(&[]);
    let (first_after_idle, idle_gap_ms) = idle_observation();
    FreshnessVerdict {
        schema_version: FRESHNESS_SCHEMA_VERSION,
        checked_at: crate::time::now_iso(),
        service_generation: crate::release_identity::service_generation().to_string(),
        release_generation: crate::release_identity::release_generation(),
        first_after_idle,
        idle_gap_ms,
        cache_age_ms: Some(0),
        refresh_in_flight: false,
        stage_elapsed_ms,
        snapshot_id: digest_bytes(format!("{class}:{attempts}").as_bytes()),
        stable: false,
        attempts,
        graph_state: state,
        head_commit: None,
        base_commit: None,
        blueprint_base_commit: None,
        manifest_digest: None,
        blueprint_generation: None,
        skills_generation: None,
        overlay_digest: empty_overlay_digest,
        overlay_count: 0,
        overlay_truncated: false,
        reindex_state: ReindexState::Unknown,
        overlay_entries: Vec::new(),
        providers: ProviderVerdicts {
            blueprint: ProviderVerdict {
                freshness_class: class.to_string(),
                usable: false,
            },
            dirty_overlay: ProviderVerdict {
                freshness_class: class.to_string(),
                usable: false,
            },
            skills: ProviderVerdict {
                freshness_class: class.to_string(),
                usable: false,
            },
        },
        reasons: vec![reason.to_string()],
    }
}

fn merge_stage_elapsed(target: &mut BTreeMap<String, u64>, source: &BTreeMap<String, u64>) {
    for (stage, elapsed_ms) in source {
        let total = target.entry(stage.clone()).or_default();
        *total = total.saturating_add(*elapsed_ms);
    }
}

fn idle_observation() -> (bool, Option<u64>) {
    static LAST_CHECK: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
    let now = Instant::now();
    let mut last = LAST_CHECK
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let gap = last.as_ref().map(|previous| now.duration_since(*previous));
    let first = gap.is_none_or(|elapsed| elapsed >= FIRST_AFTER_IDLE_THRESHOLD);
    *last = Some(now);
    (
        first,
        gap.map(|elapsed| elapsed.as_millis().min(u128::from(u64::MAX)) as u64),
    )
}

fn digest_snapshot(epoch: &FreshnessEpoch, overlay_digest: &str) -> String {
    let encoded = serde_json::to_vec(&(epoch, overlay_digest)).unwrap_or_default();
    digest_bytes(&encoded)
}

fn digest_overlay(entries: &[OverlayEntry]) -> String {
    let mut hasher = Sha256::new();
    for entry in entries {
        update_field(&mut hasher, entry.path.as_bytes());
        update_field(&mut hasher, entry.status.as_bytes());
        update_field(&mut hasher, entry.content_hash.as_bytes());
    }
    format!("sha256:{:x}", hasher.finalize())
}

fn update_field(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

fn digest_bytes(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

pub struct FilesystemFreshnessProbe<'a> {
    repo_root: PathBuf,
    store: &'a MemoryStore,
}

impl<'a> FilesystemFreshnessProbe<'a> {
    pub fn new(repo_root: PathBuf, store: &'a MemoryStore) -> Self {
        Self { repo_root, store }
    }
}

impl FreshnessProbe for FilesystemFreshnessProbe<'_> {
    fn read_epoch(&mut self) -> Result<FreshnessEpoch, String> {
        let head_commit = git_text(&self.repo_root, &["rev-parse", "HEAD"])?;
        let portable_path = self.repo_root.join(".blueprint/manifest.json");
        let graph_db_path = self.repo_root.join(".agent/graph/graph.db");
        let graph_manifest_path = self.repo_root.join(".agent/graph/manifest.json");
        let graph_body_path = self.repo_root.join(".agent/graph/graph.json");

        let (manifest_digest, portable) = read_optional_json(&portable_path)?
            .map(|(bytes, value)| (Some(digest_bytes(&bytes)), Some(value)))
            .unwrap_or((None, None));
        let graph_db = read_graph_db_metadata(&graph_db_path)?;
        let graph_manifest = read_optional_json(&graph_manifest_path)?.map(|(_, value)| value);
        let legacy_graph_body_generation = read_graph_body_generation(&graph_body_path)?;

        // graph.db is the sealed generation wherever Blueprint has migrated to
        // it; the JSON manifests are a compatibility fallback for repositories
        // that predate the migration. The precedence must therefore be
        // graph.db FIRST. Reading the legacy `.blueprint/manifest.json` first
        // meant a stale doc-run generation outranked the freshly sealed store,
        // so blueprint_generation and graph_body_generation disagreed and every
        // verdict became a false `partial_reindex` that made the lane unusable.
        let blueprint_generation = graph_db
            .as_ref()
            .and_then(|value| value.generation_id.clone())
            .or_else(|| {
                portable.as_ref().and_then(|value| {
                    json_string(value, &[&["generation", "id"], &["generationId"]])
                })
            });
        let base_commit = graph_db
            .as_ref()
            .and_then(|value| value.base_commit.clone())
            .or_else(|| {
                portable.as_ref().and_then(|value| {
                    json_string(
                        value,
                        &[
                            &["generation", "baseCommit"],
                            &["baseCommit"],
                            &["repo", "baseCommit"],
                        ],
                    )
                })
            });
        let graph_manifest_generation = graph_db
            .as_ref()
            .and_then(|value| value.generation_id.clone())
            .or_else(|| {
                graph_manifest
                    .as_ref()
                    .and_then(|value| json_string(value, &[&["generationId"]]))
            });
        let graph_body_generation = graph_db
            .as_ref()
            .and_then(|value| value.generation_id.clone())
            .or(legacy_graph_body_generation);

        Ok(FreshnessEpoch {
            head_commit: Some(head_commit),
            base_commit,
            manifest_digest: graph_db
                .as_ref()
                .and_then(|value| value.manifest_digest.clone())
                .or(manifest_digest),
            blueprint_generation,
            graph_manifest_generation,
            graph_body_generation,
            skills_generation: Some(self.store.skills_generation()?),
        })
    }

    fn read_overlay(&mut self) -> Result<OverlayObservation, String> {
        read_stable_overlay(&self.repo_root)
    }
}

pub fn evaluate_repository_freshness(store: &MemoryStore, repo_root: PathBuf) -> FreshnessVerdict {
    let mut probe = FilesystemFreshnessProbe::new(repo_root, store);
    evaluate_freshness(&mut probe, MAX_FRESHNESS_ATTEMPTS)
}

/// Canonicalize a requested repository and confine it to the configured workspace.
/// Errors intentionally omit filesystem paths so the HTTP surface stays content-free.
pub fn canonical_repo_root(
    requested: &Path,
    configured_workspace: &Path,
) -> Result<PathBuf, String> {
    let allowed = configured_workspace
        .canonicalize()
        .map_err(|_| "configured workspace is unavailable".to_string())?;
    let requested = requested
        .canonicalize()
        .map_err(|_| "repoRoot is not an existing directory".to_string())?;
    if !requested.is_dir() {
        return Err("repoRoot is not an existing directory".to_string());
    }
    if requested == allowed || requested.starts_with(&allowed) {
        Ok(requested)
    } else {
        Err("repoRoot is outside the configured workspace".to_string())
    }
}

fn read_optional_json(path: &Path) -> Result<Option<(Vec<u8>, serde_json::Value)>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("manifest metadata unavailable: {error}"))?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_MANIFEST_BYTES
    {
        return Err("manifest is not a bounded regular file".to_string());
    }
    let bytes = std::fs::read(path).map_err(|error| format!("manifest unavailable: {error}"))?;
    let value =
        serde_json::from_slice(&bytes).map_err(|_| "manifest is invalid JSON".to_string())?;
    Ok(Some((bytes, value)))
}

fn json_string(value: &serde_json::Value, paths: &[&[&str]]) -> Option<String> {
    paths.iter().find_map(|path| {
        let mut cursor = value;
        for component in *path {
            cursor = cursor.get(*component)?;
        }
        cursor.as_str().map(str::to_string)
    })
}

#[derive(Debug)]
struct GraphDbMetadata {
    manifest_digest: Option<String>,
    generation_id: Option<String>,
    base_commit: Option<String>,
}

/// Open a Blueprint graph store for reading without ever writing to it.
///
/// A WAL database opened READ_ONLY needs its `-shm` file to already exist,
/// because a read-only connection is not allowed to create one. `blueprint
/// graph build` checkpoints and removes the `-wal`/`-shm` sidecars when it
/// finishes, so the store left behind by a *fresh* build cannot be opened
/// read-only at all — the prompt path then degraded the blueprint lane with
/// "graph database unavailable" precisely when the graph was most current.
///
/// Fall back to `immutable=1`, which reads the main database directly and
/// needs no shared-memory file. Order matters: the read-only attempt comes
/// first so that whenever sidecars DO exist we honour the WAL and observe the
/// last committed generation rather than a pre-checkpoint main image.
fn open_graph_db_read_only(path: &Path) -> Result<Connection, String> {
    match Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY) {
        Ok(connection) => Ok(connection),
        Err(read_only_error) => {
            let uri = format!(
                "file:{}?immutable=1",
                path.to_string_lossy()
                    .replace('?', "%3f")
                    .replace('#', "%23")
            );
            Connection::open_with_flags(
                uri,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
            )
            .map_err(|immutable_error| {
                format!(
                    "graph database unavailable: {read_only_error}; \
                     immutable retry failed: {immutable_error}"
                )
            })
        }
    }
}

fn read_graph_db_metadata(path: &Path) -> Result<Option<GraphDbMetadata>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("graph database metadata unavailable: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("graph database is not a regular file".to_string());
    }
    let connection = open_graph_db_read_only(path)?;
    let manifest: Option<String> = connection
        .query_row(
            "SELECT value FROM generation WHERE key='manifest'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| format!("graph database manifest unavailable: {error}"))?;
    let Some(manifest) = manifest else {
        return Ok(Some(GraphDbMetadata {
            manifest_digest: None,
            generation_id: None,
            base_commit: None,
        }));
    };
    let manifest_digest = Some(digest_bytes(manifest.as_bytes()));
    let manifest: serde_json::Value = serde_json::from_str(&manifest)
        .map_err(|_| "graph database manifest is invalid JSON".to_string())?;
    let source_observation: Option<String> = connection
        .query_row(
            "SELECT value FROM generation WHERE key='sourceObservation'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| format!("graph database source observation unavailable: {error}"))?;
    let base_commit = source_observation
        .as_deref()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .and_then(|value| json_string(&value, &[&["head"], &["baseCommit"]]));
    Ok(Some(GraphDbMetadata {
        manifest_digest,
        generation_id: json_string(&manifest, &[&["generationId"], &["generation", "id"]]),
        base_commit,
    }))
}

fn read_graph_body_generation(path: &Path) -> Result<Option<String>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("graph metadata unavailable: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("graph body is not a regular file".to_string());
    }
    let mut file = File::open(path).map_err(|error| format!("graph body unavailable: {error}"))?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(GRAPH_HEADER_BYTES)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("graph header unavailable: {error}"))?;
    let header = String::from_utf8_lossy(&bytes);
    Ok(extract_json_string(&header, "generationId"))
}

fn extract_json_string(input: &str, key: &str) -> Option<String> {
    let key = format!("\"{key}\"");
    let after_key = input.split_once(&key)?.1;
    let after_colon = after_key.split_once(':')?.1.trim_start();
    let quoted = after_colon.strip_prefix('"')?;
    Some(quoted.split_once('"')?.0.to_string())
}

fn read_stable_overlay(repo_root: &Path) -> Result<OverlayObservation, String> {
    let before_started = Instant::now();
    let before = git_status(repo_root)?;
    let mut git_status_elapsed_ms = elapsed_ms(before_started);
    if before.limit_exceeded {
        return Ok(overlay_limit_exceeded(git_status_elapsed_ms));
    }
    let mut status_paths = parse_status(&before.bytes)?;
    // A nested Git repository is a separate freshness scope. Git reports an untracked nested
    // repository as one directory even with `--untracked-files=all`; omit it from this repo's
    // overlay so callers can request that canonical child root independently.
    status_paths.retain(|(path, _)| {
        let absolute = repo_root.join(path);
        !(absolute.is_dir() && absolute.join(".git").exists())
    });
    if status_paths.len() > MAX_OVERLAY_FILES {
        return Ok(OverlayObservation {
            stable: false,
            entries: Vec::new(),
            limit_exceeded: true,
            stage_elapsed_ms: git_status_stage(git_status_elapsed_ms),
        });
    }
    // Decide the cumulative byte cap from metadata before reading file bodies. Over-limit dirty
    // trees need only an indeterminate verdict; hashing up to 64 MiB first made freshness latency
    // depend on unrelated build artifacts and could outlive the HTTP request.
    if !overlay_within_byte_limit(repo_root, &status_paths)? {
        return Ok(overlay_limit_exceeded(git_status_elapsed_ms));
    }

    let mut total_bytes = 0u64;
    let mut entries = Vec::with_capacity(status_paths.len());
    let mut stable = true;
    for (path, status) in status_paths {
        let absolute = safe_join(repo_root, &path)?;
        let (content_hash, len, unchanged) = hash_overlay_path(&absolute, &status)?;
        total_bytes = total_bytes.saturating_add(len);
        if total_bytes > MAX_OVERLAY_BYTES {
            return Ok(OverlayObservation {
                stable: false,
                entries: Vec::new(),
                limit_exceeded: true,
                stage_elapsed_ms: git_status_stage(git_status_elapsed_ms),
            });
        }
        stable &= unchanged;
        entries.push(OverlayEntry {
            path,
            status,
            content_hash,
        });
    }
    let after_started = Instant::now();
    let after = git_status(repo_root)?;
    git_status_elapsed_ms = git_status_elapsed_ms.saturating_add(elapsed_ms(after_started));
    if after.limit_exceeded {
        return Ok(overlay_limit_exceeded(git_status_elapsed_ms));
    }
    stable &= before.bytes == after.bytes;
    entries.sort();
    Ok(OverlayObservation {
        stable,
        entries,
        limit_exceeded: false,
        stage_elapsed_ms: git_status_stage(git_status_elapsed_ms),
    })
}

fn overlay_limit_exceeded(git_status_elapsed_ms: u64) -> OverlayObservation {
    OverlayObservation {
        stable: false,
        entries: Vec::new(),
        limit_exceeded: true,
        stage_elapsed_ms: git_status_stage(git_status_elapsed_ms),
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn git_status_stage(elapsed_ms: u64) -> BTreeMap<String, u64> {
    BTreeMap::from([("git_status".to_string(), elapsed_ms)])
}

struct GitStatusOutput {
    bytes: Vec<u8>,
    limit_exceeded: bool,
}

fn git_status(repo_root: &Path) -> Result<GitStatusOutput, String> {
    let mut command = hidden_command("git");
    command.arg("-C").arg(repo_root).args([
        "status",
        "--porcelain=v1",
        "-z",
        "--untracked-files=all",
    ]);
    let output = bounded_child_output(
        &mut command,
        GIT_CHILD_TIMEOUT,
        MAX_GIT_STATUS_BYTES,
        "git status",
    )?;
    if output.limit_exceeded {
        return Ok(GitStatusOutput {
            bytes: Vec::new(),
            limit_exceeded: true,
        });
    }
    if !output.status.success() {
        return Err("git status failed".to_string());
    }
    Ok(GitStatusOutput {
        bytes: output.stdout,
        limit_exceeded: false,
    })
}

fn git_text(repo_root: &Path, args: &[&str]) -> Result<String, String> {
    let mut command = hidden_command("git");
    command.arg("-C").arg(repo_root).args(args);
    let output = bounded_child_output(&mut command, GIT_CHILD_TIMEOUT, MAX_GIT_TEXT_BYTES, "git")?;
    if output.limit_exceeded {
        return Err("git output exceeded its byte limit".to_string());
    }
    if !output.status.success() {
        return Err("git failed".to_string());
    }
    let value =
        String::from_utf8(output.stdout).map_err(|_| "git output is not UTF-8".to_string())?;
    let value = value.trim();
    if value.is_empty() {
        Err("git returned an empty value".to_string())
    } else {
        Ok(value.to_string())
    }
}

#[derive(Debug)]
struct BoundedChildOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    limit_exceeded: bool,
}

fn bounded_child_output(
    command: &mut Command,
    timeout: Duration,
    max_stdout_bytes: u64,
    label: &str,
) -> Result<BoundedChildOutput, String> {
    command.stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|error| format!("{label} unavailable: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("{label} output unavailable"))?;
    let (reader_tx, reader_rx) = mpsc::sync_channel(1);
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout
            .take(max_stdout_bytes.saturating_add(1))
            .read_to_end(&mut bytes)
            .map(|_| bytes);
        let _ = reader_tx.send(result);
    });
    let started = Instant::now();
    let mut status = None;
    let mut stdout_result = None;

    loop {
        if stdout_result.is_none() {
            match reader_rx.try_recv() {
                Ok(result) => stdout_result = Some(result),
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("{label} output reader stopped"));
                }
            }
        }

        if let Some(Ok(bytes)) = stdout_result.as_ref() {
            if bytes.len() as u64 > max_stdout_bytes {
                let _ = child.kill();
                let terminated = child
                    .wait()
                    .map_err(|error| format!("{label} termination failed: {error}"))?;
                let _ = reader.join();
                return Ok(BoundedChildOutput {
                    status: terminated,
                    stdout: bytes.clone(),
                    limit_exceeded: true,
                });
            }
        }
        if stdout_result.as_ref().is_some_and(Result::is_err) {
            let error = stdout_result
                .take()
                .expect("checked child output result")
                .expect_err("checked child output error");
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            return Err(format!("{label} unavailable: {error}"));
        }

        if status.is_none() {
            status = child
                .try_wait()
                .map_err(|error| format!("{label} wait failed: {error}"))?;
        }
        if let Some(status) = status {
            if let Some(stdout_result) = stdout_result.take() {
                let _ = reader.join();
                let stdout =
                    stdout_result.map_err(|error| format!("{label} unavailable: {error}"))?;
                return Ok(BoundedChildOutput {
                    status,
                    stdout,
                    limit_exceeded: false,
                });
            }
        }

        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("{label} timed out"));
        }
        std::thread::sleep(CHILD_POLL_INTERVAL);
    }
}

fn hidden_command(program: &str) -> Command {
    // `mut` is load-bearing only under `cfg(windows)` below.
    #[allow(unused_mut)]
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command
}

fn parse_status(raw: &[u8]) -> Result<Vec<(String, String)>, String> {
    let records = raw
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .collect::<Vec<_>>();
    let mut index = 0usize;
    let mut paths = BTreeMap::<String, String>::new();
    while index < records.len() {
        let record = records[index];
        if record.len() < 4 || record[2] != b' ' {
            return Err("git status returned an invalid record".to_string());
        }
        let code = &record[..2];
        let path = normalize_git_path(&record[3..])?;
        let status =
            std::str::from_utf8(code).map_err(|_| "git status code is not UTF-8".to_string())?;
        if !is_internal_snapshot_path(&path) {
            paths.insert(path, status.to_string());
        }
        if code.iter().any(|byte| matches!(*byte, b'R' | b'C')) {
            index += 1;
            if index >= records.len() {
                return Err("git status rename record is incomplete".to_string());
            }
            let other = normalize_git_path(records[index])?;
            if code.contains(&b'R') && !is_internal_snapshot_path(&other) {
                paths.insert(other, "D ".to_string());
            }
        }
        index += 1;
    }
    Ok(paths.into_iter().collect())
}

fn normalize_git_path(bytes: &[u8]) -> Result<String, String> {
    let path = std::str::from_utf8(bytes)
        .map_err(|_| "git status path is not UTF-8".to_string())?
        .replace('\\', "/");
    if path.is_empty()
        || path.chars().any(|character| character.is_control())
        || Path::new(&path).is_absolute()
        || Path::new(&path).components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err("git status path escaped the repository".to_string());
    }
    Ok(path)
}

fn is_internal_snapshot_path(path: &str) -> bool {
    path == ".agent"
        || path.starts_with(".agent/")
        || path == ".blueprint"
        || path.starts_with(".blueprint/")
}

fn safe_join(repo_root: &Path, relative: &str) -> Result<PathBuf, String> {
    let joined = repo_root.join(relative);
    if joined.starts_with(repo_root) {
        Ok(joined)
    } else {
        Err("overlay path escaped the repository".to_string())
    }
}

fn overlay_within_byte_limit(
    repo_root: &Path,
    status_paths: &[(String, String)],
) -> Result<bool, String> {
    let mut total_bytes = 0u64;
    for (path, status) in status_paths {
        let absolute = safe_join(repo_root, path)?;
        let len = if status.contains('D') && !absolute.exists() {
            0
        } else {
            let metadata = std::fs::symlink_metadata(&absolute)
                .map_err(|error| format!("overlay metadata unavailable: {error}"))?;
            if metadata.file_type().is_symlink() {
                std::fs::read_link(&absolute)
                    .map_err(|error| format!("overlay symlink unavailable: {error}"))?
                    .as_os_str()
                    .len() as u64
            } else if metadata.is_file() {
                metadata.len()
            } else {
                return Err("overlay entry is not a regular file".to_string());
            }
        };
        total_bytes = total_bytes.saturating_add(len);
        if total_bytes > MAX_OVERLAY_BYTES {
            return Ok(false);
        }
    }
    Ok(true)
}

fn hash_overlay_path(path: &Path, status: &str) -> Result<(String, u64, bool), String> {
    if status.contains('D') && !path.exists() {
        return Ok((digest_bytes(b"deleted"), 0, true));
    }
    let before = std::fs::symlink_metadata(path)
        .map_err(|error| format!("overlay metadata unavailable: {error}"))?;
    if before.file_type().is_symlink() {
        let target = std::fs::read_link(path)
            .map_err(|error| format!("overlay symlink unavailable: {error}"))?;
        let after = std::fs::symlink_metadata(path)
            .map_err(|error| format!("overlay metadata unavailable: {error}"))?;
        return Ok((
            digest_bytes(target.to_string_lossy().as_bytes()),
            target.as_os_str().len() as u64,
            same_metadata(&before, &after),
        ));
    }
    if !before.is_file() {
        return Err("overlay entry is not a regular file".to_string());
    }
    if before.len() > MAX_OVERLAY_BYTES {
        return Ok((String::new(), before.len(), false));
    }
    let mut file =
        File::open(path).map_err(|error| format!("overlay file unavailable: {error}"))?;
    file.seek(SeekFrom::Start(0))
        .map_err(|error| format!("overlay file unavailable: {error}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 32 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("overlay file unavailable: {error}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let after = std::fs::symlink_metadata(path)
        .map_err(|error| format!("overlay metadata unavailable: {error}"))?;
    Ok((
        format!("sha256:{:x}", hasher.finalize()),
        before.len(),
        same_metadata(&before, &after),
    ))
}

fn same_metadata(before: &std::fs::Metadata, after: &std::fs::Metadata) -> bool {
    before.len() == after.len()
        && before.modified().ok() == after.modified().ok()
        && before.file_type() == after.file_type()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    struct SequenceProbe {
        epochs: Vec<Result<FreshnessEpoch, String>>,
        overlays: Vec<Result<OverlayObservation, String>>,
    }

    impl FreshnessProbe for SequenceProbe {
        fn read_epoch(&mut self) -> Result<FreshnessEpoch, String> {
            self.epochs.remove(0)
        }

        fn read_overlay(&mut self) -> Result<OverlayObservation, String> {
            self.overlays.remove(0)
        }
    }

    fn child_fixture_command(mode: &str) -> Command {
        let executable = std::env::current_exe().expect("current test executable");
        let mut command = hidden_command(executable.to_str().expect("UTF-8 test executable path"));
        command
            .args([
                "--exact",
                "freshness::tests::bounded_child_fixture",
                "--nocapture",
            ])
            .env("CRYPT_FRESHNESS_CHILD_FIXTURE", mode);
        command
    }

    #[test]
    fn bounded_child_fixture() {
        let Ok(mode) = std::env::var("CRYPT_FRESHNESS_CHILD_FIXTURE") else {
            return;
        };
        match mode.as_str() {
            "normal" => {
                std::io::stdout()
                    .write_all(b"fixture-normal-output")
                    .unwrap();
                std::io::stdout().flush().unwrap();
            }
            "oversized" => {
                std::io::stdout().write_all(&vec![b'x'; 8 * 1024]).unwrap();
                std::io::stdout().flush().unwrap();
            }
            "output_hang" => {
                std::io::stdout().write_all(b"fixture-before-hang").unwrap();
                std::io::stdout().flush().unwrap();
                std::thread::sleep(Duration::from_secs(30));
            }
            "silent_hang" => std::thread::sleep(Duration::from_secs(30)),
            other => panic!("unknown child fixture mode: {other}"),
        }
    }

    #[test]
    fn bounded_child_kills_a_silent_process_at_the_deadline() {
        let started = Instant::now();
        let error = bounded_child_output(
            &mut child_fixture_command("silent_hang"),
            Duration::from_millis(100),
            1024,
            "fixture",
        )
        .unwrap_err();

        assert!(error.contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn bounded_child_kills_a_process_that_writes_then_hangs() {
        let started = Instant::now();
        let error = bounded_child_output(
            &mut child_fixture_command("output_hang"),
            Duration::from_millis(100),
            1024,
            "fixture",
        )
        .unwrap_err();

        assert!(error.contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn bounded_child_stops_reading_and_kills_on_the_output_cap() {
        let output = bounded_child_output(
            &mut child_fixture_command("oversized"),
            Duration::from_secs(2),
            64,
            "fixture",
        )
        .unwrap();

        assert!(output.limit_exceeded);
        assert!(output.stdout.len() <= 65);
    }

    #[test]
    fn bounded_child_returns_normal_output_and_exit_status() {
        let output = bounded_child_output(
            &mut child_fixture_command("normal"),
            Duration::from_secs(2),
            4096,
            "fixture",
        )
        .unwrap();

        assert!(!output.limit_exceeded);
        assert!(output.status.success());
        assert!(String::from_utf8(output.stdout)
            .unwrap()
            .contains("fixture-normal-output"));
    }

    #[test]
    fn freshness_exposes_distinct_boot_and_release_generations() {
        let mut probe = SequenceProbe {
            epochs: vec![
                Ok(FreshnessEpoch::coherent_for_test("head", "graph", "skills")),
                Ok(FreshnessEpoch::coherent_for_test("head", "graph", "skills")),
            ],
            overlays: vec![Ok(OverlayObservation {
                stable: true,
                ..OverlayObservation::default()
            })],
        };

        let verdict = evaluate_freshness(&mut probe, 1);

        assert!(verdict.service_generation.starts_with("sha256:"));
        assert_eq!(
            verdict.release_generation,
            format!(
                "sha256:{}",
                option_env!("CRYPT_SOURCE_TREE_SHA256").unwrap_or("unknown")
            )
        );
        assert_ne!(verdict.service_generation, verdict.release_generation);
    }

    #[test]
    fn parser_excludes_blueprint_outputs_and_names_statuses() {
        let raw = b" M src/lib.rs\0?? new.txt\0 M .agent/graph/graph.json\0";
        assert_eq!(
            parse_status(raw).unwrap(),
            vec![
                ("new.txt".to_string(), "??".to_string()),
                ("src/lib.rs".to_string(), " M".to_string()),
            ]
        );
    }

    #[test]
    fn rename_status_hashes_the_new_path_and_marks_the_missing_old_path_deleted() {
        let raw = b"R  src/new.rs\0src/old.rs\0";
        assert_eq!(
            parse_status(raw).unwrap(),
            vec![
                ("src/new.rs".to_string(), "R ".to_string()),
                ("src/old.rs".to_string(), "D ".to_string()),
            ]
        );
    }

    #[test]
    fn copy_status_does_not_rehash_the_unchanged_source_path() {
        let raw = b"C  src/copy.rs\0src/original.rs\0";
        assert_eq!(
            parse_status(raw).unwrap(),
            vec![("src/copy.rs".to_string(), "C ".to_string())]
        );
    }

    #[test]
    fn cumulative_overlay_limit_is_detected_from_metadata_before_hashing() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first.bin");
        let second = directory.path().join("second.bin");
        File::create(&first)
            .unwrap()
            .set_len(40 * 1024 * 1024)
            .unwrap();
        File::create(&second)
            .unwrap()
            .set_len(30 * 1024 * 1024)
            .unwrap();
        let paths = vec![
            ("first.bin".to_string(), "??".to_string()),
            ("second.bin".to_string(), "??".to_string()),
        ];

        assert!(!overlay_within_byte_limit(directory.path(), &paths).unwrap());
    }

    #[test]
    fn coordinator_returns_immediately_while_repository_refresh_runs_off_path() {
        let coordinator = FreshnessCoordinator::default();
        let root = PathBuf::from("coordinator-off-path-fixture");
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let started = Instant::now();

        let pending = coordinator.latest_or_schedule_with(root.clone(), move || {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            let epoch = FreshnessEpoch::coherent_for_test("head", "graph", "skills");
            let mut probe = SequenceProbe {
                epochs: vec![Ok(epoch.clone()), Ok(epoch)],
                overlays: vec![Ok(OverlayObservation {
                    stable: true,
                    ..OverlayObservation::default()
                })],
            };
            evaluate_freshness(&mut probe, 1)
        });

        assert!(started.elapsed() < Duration::from_millis(50));
        assert_eq!(pending.graph_state, GraphState::Indeterminate);
        assert!(pending.refresh_in_flight);
        assert!(pending
            .reasons
            .iter()
            .any(|reason| reason == "refresh_pending"));
        started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("background refresh starts");
        release_tx.send(()).unwrap();

        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            if coordinator
                .cached_for_test(&root)
                .is_some_and(|verdict| verdict.graph_state == GraphState::Clean)
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "background freshness did not publish"
            );
            std::thread::yield_now();
        }
    }

    #[test]
    fn within_limit_overlay_and_deleted_paths_pass_metadata_preflight() {
        let directory = tempfile::tempdir().unwrap();
        File::create(directory.path().join("small.bin"))
            .unwrap()
            .set_len(1024)
            .unwrap();
        let paths = vec![
            ("small.bin".to_string(), " M".to_string()),
            ("deleted.bin".to_string(), " D".to_string()),
        ];

        assert!(overlay_within_byte_limit(directory.path(), &paths).unwrap());
    }

    #[test]
    fn source_barrier_receipt_normalizes_provider_generation_digest() {
        let mut verdict = refresh_pending_verdict();
        verdict.graph_state = GraphState::Clean;
        verdict.blueprint_generation = Some("xxh128:provider-generation".to_string());
        verdict.manifest_digest = Some("manifest-generation".to_string());
        let receipt = source_barrier_receipt(&verdict, "repo-a", "session-a", "/workspace");
        for field in [
            "generation_id",
            "manifest_digest",
            "source_observation_digest",
            "dirty_overlay_digest",
        ] {
            assert!(receipt[field]
                .as_str()
                .is_some_and(|value| value.starts_with("sha256:") && value.len() == 71));
        }
        assert_eq!(receipt["overlay_identity"]["session_id"], "session-a");
        assert_eq!(receipt["overlay_identity"]["worktree_path"], "/workspace");
    }
}
