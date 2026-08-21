//! One content-free freshness verdict for every Membrane consumer.
//!
//! The evaluator uses an epoch sandwich: snapshot/skills epoch A, bounded overlay
//! evidence, then epoch B. A verdict is returned only when
//! both epochs match. This prevents callers from observing a commit, Blueprint
//! reindex, or skills ingest assembled from different moments in time.

use crate::MemoryStore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const FRESHNESS_SCHEMA_VERSION: u32 = 1;
pub const MAX_FRESHNESS_ATTEMPTS: usize = 3;
pub const MAX_RETURNED_OVERLAY_ENTRIES: usize = 64;
const BLUEPRINT_FRAME_BYTES: usize = 16 * 1024;
const BLUEPRINT_REQUEST_TIMEOUT: Duration = Duration::from_secs(2);
const FIRST_AFTER_IDLE_THRESHOLD: Duration = Duration::from_secs(5 * 60);

/// Commits the sealed generation may lag HEAD before the graph is called stale.
///
/// Plan 1.2: staleness is "behind HEAD by more than N", default 1. Treating any
/// difference as stale made an actively-committed worktree permanently alarmed.
const MAX_GENERATION_COMMIT_LAG: u32 = 1;

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
    /// Commits HEAD is ahead of the sealed generation's base commit.
    ///
    /// `Some(0)` means the generation describes HEAD exactly. `None` means the
    /// distance could not be measured (shallow clone, unrelated histories, or
    /// a git failure) — callers must treat that as "unknown", not "current".
    pub commit_distance: Option<u32>,
}

impl FreshnessEpoch {
    /// Convenience constructor for deterministic probes and downstream conformance tests.
    pub fn coherent_for_test(head: &str, generation: &str, skills: &str) -> Self {
        Self {
            head_commit: Some(head.to_string()),
            base_commit: Some(head.to_string()),
            manifest_digest: Some(format!(
                "sha256:{}",
                hex::encode(Sha256::digest(generation.as_bytes()))
            )),
            blueprint_generation: Some(generation.to_string()),
            graph_manifest_generation: Some(generation.to_string()),
            graph_body_generation: Some(generation.to_string()),
            skills_generation: Some(skills.to_string()),
            commit_distance: Some(0),
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
        "schema": "membrane.source-barrier-receipt.v1",
        "repository_id": repository_id,
        "barrier_clock": 0,
        "applied_graph_clock": 0,
        "event_gap": verdict.reasons.iter().any(|reason| reason == "event_gap"),
        "generation_id": generation_id.clone(),
        "manifest_digest": manifest_digest,
        "source_observation_digest": source_observation_digest,
        "dirty_overlay_digest": contract_digest(verdict.overlay_digest.clone()),
        "overlay_identity": {
            "session_id": session_id,
            "worktree_path": worktree_path,
            "generation_id": generation_id,
            "overlay_digest": contract_digest(verdict.overlay_digest.clone()),
        },
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
        } else if epoch.head_commit != epoch.base_commit
            && epoch
                .commit_distance
                .is_none_or(|distance| distance > MAX_GENERATION_COMMIT_LAG)
        {
            // Plan 1.2: stale means the generation is behind HEAD by more than
            // MAX_GENERATION_COMMIT_LAG commits — not merely "different from
            // HEAD". A single commit past the sealed generation is the normal
            // steady state of an active worktree and used to trip a permanent
            // alarm. An unmeasurable distance stays stale: unknown lag is not
            // evidence of freshness.
            (
                GraphState::StaleSnapshot,
                ReindexState::Idle,
                "stale_snapshot",
                true,
                vec!["snapshot_base_differs_from_head".to_string()],
            )
        } else if overlay_count > 0 {
            // Plan 1.2: a dirty worktree is an informational observation, never
            // a staleness verdict. `DirtyOverlay` maps to the "current" class
            // (see freshness_class) and the reason below is what surfaces it.
            (
                GraphState::DirtyOverlay,
                ReindexState::Idle,
                "committed_snapshot",
                true,
                vec!["dirty_overlay_observed".to_string()],
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
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

fn update_field(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

fn digest_bytes(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

pub struct FilesystemFreshnessProbe<'a> {
    repo_root: PathBuf,
    store: &'a MemoryStore,
    blueprint_endpoint: Option<PathBuf>,
    pending_overlay: Option<OverlayObservation>,
}

impl<'a> FilesystemFreshnessProbe<'a> {
    pub fn new(repo_root: PathBuf, store: &'a MemoryStore) -> Self {
        Self {
            repo_root,
            store,
            blueprint_endpoint: None,
            pending_overlay: None,
        }
    }

    /// Bind a Blueprint endpoint explicitly for isolated conformance tests.
    pub fn with_blueprint_endpoint(mut self, endpoint: PathBuf) -> Self {
        self.blueprint_endpoint = Some(endpoint);
        self
    }
}

impl FreshnessProbe for FilesystemFreshnessProbe<'_> {
    fn read_epoch(&mut self) -> Result<FreshnessEpoch, String> {
        let status = self
            .blueprint_endpoint
            .as_deref()
            .map(|endpoint| read_blueprint_status_at(endpoint, &self.repo_root))
            .unwrap_or_else(|| read_blueprint_status(&self.repo_root))?;
        self.pending_overlay = status
            .get("result")
            .and_then(|value| value.get("overlay"))
            .filter(|value| {
                value.get("available").and_then(serde_json::Value::as_bool) == Some(true)
            })
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok());
        let head_commit = json_string(&status, &[&["result", "repository", "revision"]]);
        let manifest = status.get("result").and_then(|value| value.get("manifest"));
        let blueprint_generation = json_string(
            &status,
            &[
                &["generation"],
                &["result", "generation"],
                &["result", "manifest", "generationId"],
            ],
        );
        let base_commit = manifest
            .and_then(|value| json_string(value, &[&["baseCommit"], &["repo", "baseCommit"]]));
        let manifest_digest = manifest.and_then(|value| {
            json_string(value, &[&["manifestDigest"]]).or_else(|| {
                serde_json::to_vec(value)
                    .ok()
                    .map(|bytes| digest_bytes(&bytes))
            })
        });
        let graph_manifest_generation = blueprint_generation.clone();
        let graph_body_generation = blueprint_generation.clone();

        Ok(FreshnessEpoch {
            head_commit,
            base_commit: base_commit.clone(),
            manifest_digest,
            blueprint_generation,
            graph_manifest_generation,
            graph_body_generation,
            commit_distance: status
                .get("result")
                .and_then(|value| value.get("overlay"))
                .and_then(|value| value.get("commitDistance"))
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| u32::try_from(value).ok()),
            skills_generation: Some(self.store.skills_generation()?),
        })
    }

    fn read_overlay(&mut self) -> Result<OverlayObservation, String> {
        self.pending_overlay
            .take()
            .ok_or_else(|| "Blueprint overlay evidence unavailable".to_string())
    }
}

fn blueprint_daemon_endpoint() -> Result<PathBuf, String> {
    if let Some(endpoint) = std::env::var_os("BLUEPRINT_DAEMON_ENDPOINT") {
        return Ok(PathBuf::from(endpoint));
    }
    #[cfg(windows)]
    {
        let home =
            std::env::var("USERPROFILE").map_err(|_| "USERPROFILE is unavailable".to_string())?;
        let suffix = hex::encode(Sha256::digest(home.as_bytes()));
        return Ok(PathBuf::from(format!(
            r"\\.\pipe\membrane-blueprint-{}",
            &suffix[..16]
        )));
    }
    #[cfg(not(windows))]
    {
        let home = std::env::var_os("HOME").ok_or_else(|| "HOME is unavailable".to_string())?;
        Ok(PathBuf::from(home).join(".blueprint/blueprint.sock"))
    }
}

fn blueprint_status_request(repo_root: &Path) -> Result<Vec<u8>, String> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let request = serde_json::json!({
        "protocolVersion": 1,
        "requestId": format!("membrane-freshness-{}-{nonce}", std::process::id()),
        "repoId": serde_json::Value::Null,
        "generation": serde_json::Value::Null,
        "method": "status",
        "deadlineMs": 2000,
        "input": { "repoRoot": repo_root },
    });
    let mut bytes = serde_json::to_vec(&request).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn exchange_blueprint_status<T: Read + Write>(
    stream: &mut T,
    repo_root: &Path,
) -> Result<serde_json::Value, String> {
    stream
        .write_all(&blueprint_status_request(repo_root)?)
        .map_err(|error| error.to_string())?;
    stream.flush().map_err(|error| error.to_string())?;
    let mut frame = Vec::new();
    let mut byte = [0_u8; 1];
    while frame.len() < BLUEPRINT_FRAME_BYTES {
        let count = stream.read(&mut byte).map_err(|error| error.to_string())?;
        if count == 0 || byte[0] == b'\n' {
            break;
        }
        frame.push(byte[0]);
    }
    if frame.is_empty() {
        return Err("Blueprint daemon returned no status frame".to_string());
    }
    if frame.len() >= BLUEPRINT_FRAME_BYTES {
        return Err("Blueprint status frame exceeds limit".to_string());
    }
    let value: serde_json::Value =
        serde_json::from_slice(&frame).map_err(|error| error.to_string())?;
    if value
        .get("protocolVersion")
        .and_then(serde_json::Value::as_u64)
        != Some(1)
    {
        return Err("Blueprint protocol version mismatch".to_string());
    }
    if value.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
        return Err(format!(
            "Blueprint status failed: {}",
            value.get("error").unwrap_or(&serde_json::Value::Null)
        ));
    }
    Ok(value)
}

#[cfg(unix)]
fn read_blueprint_status_at(
    endpoint: &Path,
    repo_root: &Path,
) -> Result<serde_json::Value, String> {
    use std::os::unix::net::UnixStream;
    let mut stream = UnixStream::connect(endpoint).map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(BLUEPRINT_REQUEST_TIMEOUT))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(BLUEPRINT_REQUEST_TIMEOUT))
        .map_err(|error| error.to_string())?;
    exchange_blueprint_status(&mut stream, repo_root)
}

#[cfg(windows)]
fn read_blueprint_status_at(
    endpoint: &Path,
    repo_root: &Path,
) -> Result<serde_json::Value, String> {
    let endpoint = endpoint.to_path_buf();
    let repo_root = repo_root.to_path_buf();
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let result = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(endpoint)
            .map_err(|error| error.to_string())
            .and_then(|mut pipe| exchange_blueprint_status(&mut pipe, &repo_root));
        let _ = sender.send(result);
    });
    receiver
        .recv_timeout(BLUEPRINT_REQUEST_TIMEOUT)
        .map_err(|_| "Blueprint status request timed out".to_string())?
}

fn read_blueprint_status(repo_root: &Path) -> Result<serde_json::Value, String> {
    read_blueprint_status_at(&blueprint_daemon_endpoint()?, repo_root)
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

fn json_string(value: &serde_json::Value, paths: &[&[&str]]) -> Option<String> {
    paths.iter().find_map(|path| {
        let mut cursor = value;
        for component in *path {
            cursor = cursor.get(*component)?;
        }
        cursor.as_str().map(str::to_string)
    })
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}
#[cfg(test)]
mod tests {
    use super::*;

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
                option_env!("MEMBRANE_SOURCE_TREE_SHA256").unwrap_or("unknown")
            )
        );
        assert_ne!(verdict.service_generation, verdict.release_generation);
    }

    /// Build an epoch whose generation is coherent but sits `distance` commits
    /// behind HEAD.
    fn epoch_behind_head(distance: Option<u32>) -> FreshnessEpoch {
        FreshnessEpoch {
            base_commit: Some("base".to_string()),
            commit_distance: distance,
            ..FreshnessEpoch::coherent_for_test("head", "graph", "skills")
        }
    }

    #[test]
    fn generation_one_commit_behind_head_is_not_stale() {
        // Plan 1.2: stale means behind HEAD by MORE than MAX_GENERATION_COMMIT_LAG.
        // A single commit past the sealed generation is the ordinary steady
        // state of an active worktree, and treating it as stale is exactly the
        // always-on alarm Phase 1 removes.
        let verdict = classify(epoch_behind_head(Some(1)), Vec::new(), 1, BTreeMap::new());
        assert_eq!(verdict.graph_state, GraphState::Clean);
    }

    #[test]
    fn generation_further_behind_head_than_the_lag_budget_is_stale() {
        let verdict = classify(
            epoch_behind_head(Some(MAX_GENERATION_COMMIT_LAG + 1)),
            Vec::new(),
            1,
            BTreeMap::new(),
        );
        assert_eq!(verdict.graph_state, GraphState::StaleSnapshot);
    }

    #[test]
    fn unmeasurable_commit_distance_stays_stale() {
        // Unknown lag is not evidence of freshness: a shallow clone or a failed
        // rev-list must not be reported as current.
        let verdict = classify(epoch_behind_head(None), Vec::new(), 1, BTreeMap::new());
        assert_eq!(verdict.graph_state, GraphState::StaleSnapshot);
    }

    #[test]
    fn dirty_worktree_is_observed_but_never_stale() {
        // Plan 1.2: dirty worktree -> informational `dirty_overlay_observed`,
        // and DirtyOverlay maps to the "current" freshness class.
        let verdict = classify(
            FreshnessEpoch::coherent_for_test("head", "graph", "skills"),
            vec![OverlayEntry {
                path: "src/edited.rs".to_string(),
                status: " M".to_string(),
                content_hash: "sha256:abc".to_string(),
            }],
            1,
            BTreeMap::new(),
        );
        assert_eq!(verdict.graph_state, GraphState::DirtyOverlay);
        // DirtyOverlay is one of the two states that map to the "current"
        // freshness class (see the `status` match in the receipt builder), so a
        // dirty tree never reports as stale or blocked.
        assert!(
            verdict.stable,
            "a dirty worktree must remain a stable verdict"
        );
        assert!(
            verdict
                .reasons
                .iter()
                .any(|reason| reason == "dirty_overlay_observed"),
            "expected the informational dirty_overlay_observed reason, got {:?}",
            verdict.reasons
        );
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
        assert_eq!(
            receipt["overlay_identity"]["generation_id"],
            receipt["generation_id"]
        );
        assert_eq!(
            receipt["overlay_identity"]["overlay_digest"],
            receipt["dirty_overlay_digest"]
        );
    }
}
