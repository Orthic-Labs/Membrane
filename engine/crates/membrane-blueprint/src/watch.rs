//! Native, polling-first Blueprint watcher and filesystem reconciler.
//!
//! Events are hints. A bounded deterministic snapshot is truth, so a dropped
//! or coalesced callback cannot make an index appear current indefinitely.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, Metadata};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use notify::{Event as NotifyEvent, EventKind as NotifyEventKind, RecommendedWatcher, RecursiveMode, Watcher};
use notify::event::RenameMode;

use crate::api::CancellationToken;
use crate::contracts::BarrierResult;
use crate::freshness::{content_digest, stable_read_with_limit, StableReadError, MAX_SOURCE_FILE_BYTES};
use crate::graph::RepoIgnore;

pub const DEFAULT_MAX_FILES: usize = 100_000;
pub const DEFAULT_MAX_SNAPSHOT_BYTES: u64 = 256 * 1024 * 1024;
/// Retained for wire/config compatibility. Native events are drained as soon
/// as supervision runs; known paths never wait for this interval.
pub const DEFAULT_DEBOUNCE_MS: u64 = 1_000;
pub const DEBOUNCE_MS: u64 = DEFAULT_DEBOUNCE_MS;
pub const MAX_DEBOUNCE_MS: u64 = 5_000;

#[derive(Debug, Clone)]
pub struct SnapshotConfig {
    pub root: PathBuf,
    pub exclusions: BTreeSet<String>,
    pub max_files: usize,
    pub max_bytes: u64,
    pub max_file_bytes: u64,
    pub max_events: usize,
    pub debounce_ms: u64,
}

impl SnapshotConfig {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            exclusions: BTreeSet::new(),
            max_files: DEFAULT_MAX_FILES,
            max_bytes: DEFAULT_MAX_SNAPSHOT_BYTES,
            max_file_bytes: MAX_SOURCE_FILE_BYTES,
            max_events: 256,
            debounce_ms: DEFAULT_DEBOUNCE_MS,
        }
    }

    pub fn exclude(mut self, path: impl AsRef<Path>) -> Self {
        if let Some(path) = normalized_relative(path.as_ref()) {
            self.exclusions.insert(path);
        }
        self
    }

    pub fn max_events(mut self, max_events: usize) -> Self {
        self.max_events = max_events.max(1);
        self
    }

    pub fn debounce_ms(mut self, debounce_ms: u64) -> Self {
        self.debounce_ms = debounce_ms.min(MAX_DEBOUNCE_MS);
        self
    }

    pub fn debounce(self, debounce: Duration) -> Self {
        self.debounce_ms(debounce.as_millis().min(u64::MAX as u128) as u64)
    }
}

#[derive(Debug, Error)]
pub enum SnapshotError {
    #[error("snapshot cancelled")]
    Cancelled,
    #[error("snapshot root is unavailable: {0}")]
    Root(#[from] std::io::Error),
    #[error("snapshot escaped repository root: {0}")]
    Escaped(PathBuf),
    #[error("snapshot limit exceeded: {kind} ({actual}, limit {limit})")]
    Limit { kind: &'static str, actual: u64, limit: u64 },
    #[error("stable read failed for {path}: {source}")]
    Read { path: PathBuf, source: StableReadError },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind { File, Directory }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotEntry {
    pub path: String,
    pub kind: EntryKind,
    pub size: u64,
    pub modified_ns: Option<u128>,
    pub digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Snapshot {
    pub entries: Vec<SnapshotEntry>,
    pub fingerprint: String,
}

fn canonical_root(root: &Path) -> Result<PathBuf, SnapshotError> {
    root.canonicalize().map_err(SnapshotError::Root)
}

fn normalized_relative(path: &Path) -> Option<String> {
    let mut result = String::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => {
                if !result.is_empty() { result.push('/'); }
                result.push_str(&value.to_string_lossy().replace('\\', "/"));
            }
            Component::CurDir => {}
            Component::ParentDir => return None,
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    (!result.is_empty()).then_some(result)
}

fn excluded(relative: &str, config: &SnapshotConfig) -> bool {
    config.exclusions.iter().any(|prefix| relative == prefix || relative.starts_with(&format!("{prefix}/")))
}

fn confined(root: &Path, path: &Path) -> Result<bool, SnapshotError> {
    let canonical = match path.canonicalize() {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(SnapshotError::Root(error)),
    };
    Ok(canonical == root || canonical.starts_with(root))
}

fn metadata_modified_ns(metadata: &Metadata) -> Option<u128> {
    metadata.modified().ok()?.duration_since(std::time::SystemTime::UNIX_EPOCH).ok().map(|v| v.as_nanos())
}

fn walk(root: &Path, current: &Path, config: &SnapshotConfig, ignore: &RepoIgnore, output: &mut Vec<SnapshotEntry>, bytes: &mut u64, cancellation: &CancellationToken) -> Result<(), SnapshotError> {
    if cancellation.is_cancelled() { return Err(SnapshotError::Cancelled); }
    let mut children = fs::read_dir(current)?.collect::<Result<Vec<_>, _>>()?;
    children.sort_by(|left, right| left.file_name().cmp(&right.file_name()));
    for child in children {
        if cancellation.is_cancelled() { return Err(SnapshotError::Cancelled); }
        let path = child.path();
        let relative = path.strip_prefix(root).ok().and_then(normalized_relative);
        let Some(relative) = relative else { continue; };
        if excluded(&relative, config) { continue; }
        if !confined(root, &path)? { continue; }
        let file_type = child.file_type()?;
        if file_type.is_symlink() { continue; }
        let metadata = child.metadata()?;
        if file_type.is_dir() {
            if ignore.dir_ignored(&relative, &child.file_name().to_string_lossy()) { continue; }
            if output.len() as u64 >= config.max_files as u64 { return Err(SnapshotError::Limit { kind: "entries", actual: output.len() as u64 + 1, limit: config.max_files as u64 }); }
            output.push(SnapshotEntry { path: relative.clone(), kind: EntryKind::Directory, size: 0, modified_ns: metadata_modified_ns(&metadata), digest: None, reason: None });
            walk(root, &path, config, ignore, output, bytes, cancellation)?;
        } else if file_type.is_file() {
            if output.len() as u64 >= config.max_files as u64 { return Err(SnapshotError::Limit { kind: "entries", actual: output.len() as u64 + 1, limit: config.max_files as u64 }); }
            if ignore.file_ignored(&relative, &child.file_name().to_string_lossy()) { continue; }
            if metadata.len() > config.max_file_bytes {
                output.push(SnapshotEntry { path: relative, kind: EntryKind::File, size: metadata.len(), modified_ns: metadata_modified_ns(&metadata), digest: None, reason: Some(format!("unsupported:file_bytes:{}>limit:{}", metadata.len(), config.max_file_bytes)) });
                continue;
            }
            *bytes = bytes.saturating_add(metadata.len());
            if *bytes > config.max_bytes { return Err(SnapshotError::Limit { kind: "snapshot_bytes", actual: *bytes, limit: config.max_bytes }); }
            let read = stable_read_with_limit(&path, config.max_file_bytes).map_err(|source| SnapshotError::Read { path: relative.clone().into(), source })?;
            if cancellation.is_cancelled() { return Err(SnapshotError::Cancelled); }
            output.push(SnapshotEntry { path: relative, kind: EntryKind::File, size: read.bytes.len() as u64, modified_ns: metadata_modified_ns(&metadata), digest: Some(read.content_digest), reason: None });
        }
    }
    Ok(())
}

pub fn snapshot(config: &SnapshotConfig) -> Result<Snapshot, SnapshotError> {
    snapshot_with_cancellation(config, &CancellationToken::new())
}

pub fn snapshot_with_cancellation(config: &SnapshotConfig, cancellation: &CancellationToken) -> Result<Snapshot, SnapshotError> {
    let root = canonical_root(&config.root)?;
    let ignore = RepoIgnore::for_root(&root);
    let mut entries = Vec::new();
    let mut bytes = 0;
    walk(&root, &root, config, &ignore, &mut entries, &mut bytes, cancellation)?;
    entries.sort_by(|left, right| left.path.as_bytes().cmp(right.path.as_bytes()));
    let mut canonical = Vec::new();
    for entry in &entries {
        canonical.extend_from_slice(entry.path.as_bytes());
        canonical.push(0);
        canonical.extend_from_slice(format!("{:?}:{}:{}:{}:{}\n", entry.kind, entry.size, entry.modified_ns.unwrap_or(0), entry.digest.as_deref().unwrap_or(""), entry.reason.as_deref().unwrap_or("")).as_bytes());
    }
    Ok(Snapshot { entries, fingerprint: content_digest(&canonical) })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind { Create, Modify, Delete, Rename }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchEvent {
    pub kind: EventKind,
    pub path: String,
    pub rename_to: Option<String>,
    pub source_clock: u64,
}

#[derive(Default)]
struct NativeEventQueue {
    events: Vec<WatchEvent>,
    keys: BTreeSet<(EventKind, String, Option<String>)>,
    next_source_clock: u64,
    overflow: bool,
    overflow_count: usize,
}

impl NativeEventQueue {
    fn enqueue(&mut self, mut event: WatchEvent, max_events: usize) {
        if event.rename_to.is_none() {
            if let Some(existing) = self.events.iter_mut().find(|existing| existing.path == event.path && existing.rename_to.is_none()) {
                let old_key = (existing.kind, existing.path.clone(), None);
                let merged_kind = match (existing.kind, event.kind) {
                    (EventKind::Create, EventKind::Modify) | (EventKind::Create, EventKind::Create) => EventKind::Create,
                    (_, EventKind::Create) => EventKind::Create,
                    (_, EventKind::Delete) => EventKind::Delete,
                    _ => EventKind::Modify,
                };
                existing.kind = merged_kind;
                self.keys.remove(&old_key);
                self.keys.insert((merged_kind, existing.path.clone(), None));
                return;
            }
        }
        let key = (event.kind, event.path.clone(), event.rename_to.clone());
        if !self.keys.insert(key) { return; }
        if self.events.len() >= max_events {
            self.overflow = true;
            self.overflow_count = self.overflow_count.saturating_add(1);
            return;
        }
        self.next_source_clock = self.next_source_clock.saturating_add(1);
        event.source_clock = self.next_source_clock;
        self.events.push(event);
    }
}

fn normalize_watch_path(root: &Path, path: &Path) -> Option<String> {
    let relative = if path.is_absolute() { path.strip_prefix(root).ok()? } else { path };
    normalized_relative(relative)
}

fn enqueue_notify_event(
    queue: &mut NativeEventQueue,
    root: &Path,
    max_events: usize,
    result: notify::Result<NotifyEvent>,
) {
    let event = match result {
        Ok(event) => event,
        Err(_) => {
            queue.overflow = true;
            queue.overflow_count = queue.overflow_count.saturating_add(1);
            return;
        }
    };
    let mut enqueue = |kind: EventKind, path: &Path, rename_to: Option<String>| {
        if let Some(path) = normalize_watch_path(root, path) {
            queue.enqueue(WatchEvent { kind, path, rename_to, source_clock: 0 }, max_events);
        }
    };
    match event.kind {
        NotifyEventKind::Create(_) => for path in event.paths { enqueue(EventKind::Create, &path, None); },
        NotifyEventKind::Remove(_) => for path in event.paths { enqueue(EventKind::Delete, &path, None); },
        NotifyEventKind::Modify(notify::event::ModifyKind::Name(mode)) => match mode {
            RenameMode::Both if event.paths.len() >= 2 => {
                enqueue(EventKind::Rename, &event.paths[0], normalize_watch_path(root, &event.paths[1]));
            }
            RenameMode::From => for path in event.paths { enqueue(EventKind::Delete, &path, None); },
            RenameMode::To => for path in event.paths { enqueue(EventKind::Create, &path, None); },
            _ => for path in event.paths { enqueue(EventKind::Modify, &path, None); },
        },
        NotifyEventKind::Modify(_) => for path in event.paths { enqueue(EventKind::Modify, &path, None); },
        NotifyEventKind::Access(_) | NotifyEventKind::Other | NotifyEventKind::Any => {}
    }
}

/// BPT-020/021 (selective invalidation): this reconciler is what makes a
/// watcher publish *selective* rather than a full-generation rebuild -- it
/// diffs the previous and current snapshot per path and emits exactly the
/// Create/Modify/Delete events for entries whose digest/kind/size actually
/// changed. What this module does NOT own: turning those per-path events
/// into a dependency-DAG invalidation over source/provider/config/schema/
/// generation parents (declared elsewhere in Blueprint's build/derivation
/// path) -- that projection consumes these events but is out of this
/// module's scope.
pub fn reconcile_snapshots(previous: &Snapshot, current: &Snapshot, source_clock: u64) -> Vec<WatchEvent> {
    // Directory metadata is useful for deterministic snapshot identity, but a
    // rebuild callback is only meaningful for source files.
    let old = previous.entries.iter().filter(|entry| entry.kind == EntryKind::File).map(|entry| (entry.path.as_str(), entry)).collect::<BTreeMap<_, _>>();
    let new = current.entries.iter().filter(|entry| entry.kind == EntryKind::File).map(|entry| (entry.path.as_str(), entry)).collect::<BTreeMap<_, _>>();
    let mut events = Vec::new();
    for path in old.keys().chain(new.keys()).copied().collect::<BTreeSet<_>>() {
        match (old.get(path), new.get(path)) {
            (None, Some(_)) => events.push(WatchEvent { kind: EventKind::Create, path: path.to_owned(), rename_to: None, source_clock: source_clock + events.len() as u64 + 1 }),
            (Some(_), None) => events.push(WatchEvent { kind: EventKind::Delete, path: path.to_owned(), rename_to: None, source_clock: source_clock + events.len() as u64 + 1 }),
            (Some(before), Some(after)) if before != after => events.push(WatchEvent { kind: EventKind::Modify, path: path.to_owned(), rename_to: None, source_clock: source_clock + events.len() as u64 + 1 }),
            _ => {}
        }
    }
    events
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GapReason { EventOverflow, SnapshotFailed, ReconciliationFailed, CallbackFailed }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventGap { pub reason: GapReason, pub source_clock: u64 }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarrierPoll { Waiting, Complete(BarrierResult) }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Barrier {
    pub target_source_clock: u64,
    pub deadline: Option<Duration>,
}

pub trait MonotonicClock: Send + Sync {
    fn now(&self) -> Duration;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SteadyClock { started: Option<Instant> }

impl SteadyClock { pub fn new() -> Self { Self { started: Some(Instant::now()) } } }
impl MonotonicClock for SteadyClock {
    fn now(&self) -> Duration { self.started.map(|start| start.elapsed()).unwrap_or_default() }
}

pub trait LifecycleSink: Send + Sync { fn observe(&self, observation: LifecycleObservation); }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LifecycleObservation {
    pub sequence: u64,
    pub kind: String,
    pub source_clock: u64,
    pub applied_clock: u64,
    pub detail: Option<String>,
}

#[derive(Debug, Error)]
pub enum WatchError {
    #[error("watcher is shut down")]
    Shutdown,
    #[error("snapshot failed: {0}")]
    Snapshot(#[from] SnapshotError),
    #[error("rebuild callback failed: {0}")]
    Callback(String),
    #[error("watcher event batch exceeded limit: {actual} (limit {limit})")]
    EventOverflow { actual: usize, limit: usize },
    #[error("native watcher unavailable: {0}")]
    Native(String),
}

pub struct NativeWatcher {
    config: SnapshotConfig,
    previous: Snapshot,
    source_clock: u64,
    applied_clock: u64,
    gap: Option<EventGap>,
    closed: bool,
    sequence: u64,
    clock: Arc<dyn MonotonicClock>,
    sink: Option<Arc<dyn LifecycleSink>>,
    ignore: RepoIgnore,
    native_queue: Arc<Mutex<NativeEventQueue>>,
    _native_watcher: RecommendedWatcher,
}

impl NativeWatcher {
    pub fn start(config: SnapshotConfig) -> Result<Self, WatchError> {
        Self::start_with_cancellation(config, &CancellationToken::new())
    }

    pub fn start_with_cancellation(config: SnapshotConfig, cancellation: &CancellationToken) -> Result<Self, WatchError> {
        let root = canonical_root(&config.root)?;
        let mut config = config;
        config.root = root.clone();
        let initial = snapshot_with_cancellation(&config, cancellation)?;
        let queue = Arc::new(Mutex::new(NativeEventQueue::default()));
        let callback_queue = Arc::clone(&queue);
        let callback_root = root.clone();
        let max_events = config.max_events;
        let native_watcher = notify::recommended_watcher(move |result| {
            if let Ok(mut queue) = callback_queue.lock() {
                enqueue_notify_event(&mut queue, &callback_root, max_events, result);
            }
        }).map_err(|error| WatchError::Native(error.to_string()))?;
        let mut native_watcher = native_watcher;
        native_watcher.watch(&root, RecursiveMode::Recursive).map_err(|error| WatchError::Native(error.to_string()))?;
        let mut watcher = Self { config, previous: initial, source_clock: 0, applied_clock: 0, gap: None, closed: false, sequence: 0, clock: Arc::new(SteadyClock::new()), sink: None, ignore: RepoIgnore::for_root(&root), native_queue: queue, _native_watcher: native_watcher };
        watcher.emit("started", None);
        Ok(watcher)
    }

    pub fn with_clock(mut self, clock: Arc<dyn MonotonicClock>) -> Self { self.clock = clock; self }
    pub fn with_sink(mut self, sink: Arc<dyn LifecycleSink>) -> Self { self.sink = Some(sink); self }
    pub fn new(config: SnapshotConfig) -> Result<Self, WatchError> { Self::start(config) }
    pub fn source_clock(&self) -> u64 { self.source_clock }
    pub fn applied_clock(&self) -> u64 { self.applied_clock }
    pub fn gap(&self) -> Option<&EventGap> { self.gap.as_ref() }
    pub fn is_shutdown(&self) -> bool { self.closed }
    pub fn root(&self) -> &Path { &self.config.root }
    pub fn debounce(&self) -> Duration { Duration::from_millis(self.config.debounce_ms) }

    /// Drain native events immediately. The debounce setting remains part of
    /// config compatibility, but native known-path events do not sleep.
    pub fn poll_debounced<F>(&mut self, schedule_rebuild: F, cancellation: &CancellationToken) -> Result<Vec<WatchEvent>, WatchError>
    where F: FnMut(&WatchEvent) -> Result<(), String> {
        self.poll_with_cancellation(schedule_rebuild, cancellation)
    }

    /// Polling is the reconciliation truth. A callback is only a rebuild hint;
    /// it cannot approve Phase 2 or mutate semantic policy.
    pub fn poll<F>(&mut self, schedule_rebuild: F) -> Result<Vec<WatchEvent>, WatchError>
    where F: FnMut(&WatchEvent) -> Result<(), String> {
        self.poll_with_cancellation(schedule_rebuild, &CancellationToken::new())
    }

    fn native_event_is_source(&self, event: &WatchEvent) -> bool {
        let path_is_source = |path: &str| {
            let name = Path::new(path)
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("");
            if self.ignore.file_ignored(path, name) {
                return false;
            }
            match fs::symlink_metadata(self.config.root.join(path)) {
                Ok(metadata) => metadata.is_file(),
                Err(_) => self
                    .previous
                    .entries
                    .iter()
                    .any(|entry| entry.path == path && entry.kind == EntryKind::File),
            }
        };
        path_is_source(&event.path)
            || event
                .rename_to
                .as_deref()
                .is_some_and(path_is_source)
    }

    fn take_native_events(&mut self) -> Result<(bool, Vec<WatchEvent>), WatchError> {
        let Ok(mut queue) = self.native_queue.lock() else {
            self.gap = Some(EventGap { reason: GapReason::EventOverflow, source_clock: self.source_clock });
            return Err(WatchError::EventOverflow { actual: self.config.max_events.saturating_add(1), limit: self.config.max_events });
        };
        if queue.overflow {
            let actual = queue.events.len().saturating_add(queue.overflow_count).max(self.config.max_events.saturating_add(1));
            queue.events.clear();
            queue.keys.clear();
            queue.overflow = false;
            queue.overflow_count = 0;
            drop(queue);
            self.gap = Some(EventGap { reason: GapReason::EventOverflow, source_clock: self.source_clock });
            self.emit("event_overflow", Some(format!("{actual}>{}", self.config.max_events)));
            return Err(WatchError::EventOverflow { actual, limit: self.config.max_events });
        }
        let events = std::mem::take(&mut queue.events);
        queue.keys.clear();
        drop(queue);
        let observed = !events.is_empty();
        let events = events
            .into_iter()
            .filter(|event| self.native_event_is_source(event))
            .collect();
        Ok((observed, events))
    }

    fn apply_known_event(&mut self, event: &WatchEvent) {
        let remove = |entries: &mut Vec<SnapshotEntry>, path: &str| {
            entries.retain(|entry| entry.path != path && !entry.path.starts_with(&format!("{path}/")));
        };
        remove(&mut self.previous.entries, &event.path);
        if let Some(rename_to) = &event.rename_to { remove(&mut self.previous.entries, rename_to); }
        if matches!(event.kind, EventKind::Delete) { return; }
        let path = event.rename_to.as_deref().unwrap_or(&event.path);
        let full_path = self.config.root.join(path);
        let Ok(metadata) = fs::symlink_metadata(&full_path) else { return; };
        let name = Path::new(path).file_name().and_then(|value| value.to_str()).unwrap_or("");
        let entry = if metadata.is_dir() {
            if self.ignore.dir_ignored(path, name) { return; }
            SnapshotEntry { path: path.to_owned(), kind: EntryKind::Directory, size: 0, modified_ns: metadata_modified_ns(&metadata), digest: None, reason: None }
        } else if metadata.is_file() {
            if self.ignore.file_ignored(path, name) { return; }
            if metadata.len() > self.config.max_file_bytes {
                SnapshotEntry { path: path.to_owned(), kind: EntryKind::File, size: metadata.len(), modified_ns: metadata_modified_ns(&metadata), digest: None, reason: Some(format!("unsupported:file_bytes:{}>limit:{}", metadata.len(), self.config.max_file_bytes)) }
            } else {
                let Ok(read) = stable_read_with_limit(&full_path, self.config.max_file_bytes) else { return; };
                SnapshotEntry { path: path.to_owned(), kind: EntryKind::File, size: read.bytes.len() as u64, modified_ns: metadata_modified_ns(&metadata), digest: Some(read.content_digest), reason: None }
            }
        } else { return; };
        self.previous.entries.push(entry);
        self.previous.entries.sort_by(|left, right| left.path.as_bytes().cmp(right.path.as_bytes()));
    }

    fn publish_native_events<F>(&mut self, events: Vec<WatchEvent>, mut schedule_rebuild: F) -> Result<Vec<WatchEvent>, WatchError>
    where F: FnMut(&WatchEvent) -> Result<(), String> {
        if events.len() > self.config.max_events {
            self.gap = Some(EventGap { reason: GapReason::EventOverflow, source_clock: self.source_clock });
            return Err(WatchError::EventOverflow { actual: events.len(), limit: self.config.max_events });
        }
        for event in &events {
            self.source_clock = self.source_clock.max(event.source_clock);
            if let Err(error) = schedule_rebuild(event) {
                self.gap = Some(EventGap { reason: GapReason::CallbackFailed, source_clock: self.source_clock });
                self.emit("callback_failed", Some(error.clone()));
                return Err(WatchError::Callback(error));
            }
            self.emit("rebuild_scheduled", Some(event.path.clone()));
        }
        for event in &events { self.apply_known_event(event); }
        self.applied_clock = self.source_clock;
        debug_assert!(self.applied_clock == self.source_clock, "published state must be committed before freshness is reported");
        if self.gap.is_none() { self.emit("caught_up", None); }
        Ok(events)
    }

    pub fn poll_with_cancellation<F>(&mut self, mut schedule_rebuild: F, cancellation: &CancellationToken) -> Result<Vec<WatchEvent>, WatchError>
    where F: FnMut(&WatchEvent) -> Result<(), String> {
        if self.closed { return Err(WatchError::Shutdown); }
        if cancellation.is_cancelled() {
            self.emit("poll_cancelled", None);
            return Err(WatchError::Snapshot(SnapshotError::Cancelled));
        }
        let (native_events_observed, native_events) = self.take_native_events()?;
        if native_events_observed {
            return self.publish_native_events(native_events, schedule_rebuild);
        }
        let current = match snapshot_with_cancellation(&self.config, cancellation) {
            Ok(current) => current,
            Err(SnapshotError::Cancelled) => {
                self.emit("poll_cancelled", None);
                return Err(WatchError::Snapshot(SnapshotError::Cancelled));
            }
            Err(error) => {
                self.gap = Some(EventGap { reason: GapReason::SnapshotFailed, source_clock: self.source_clock });
                self.emit("snapshot_failed", Some(error.to_string()));
                return Err(WatchError::Snapshot(error));
            }
        };
        let events = reconcile_snapshots(&self.previous, &current, self.source_clock);
        if events.len() > self.config.max_events {
            self.gap = Some(EventGap { reason: GapReason::EventOverflow, source_clock: self.source_clock });
            self.emit("event_overflow", Some(format!("{}>{}", events.len(), self.config.max_events)));
            return Err(WatchError::EventOverflow { actual: events.len(), limit: self.config.max_events });
        }
        self.source_clock = self.source_clock.saturating_add(events.len() as u64);
        for event in &events {
            if let Err(error) = schedule_rebuild(event) {
                self.gap = Some(EventGap { reason: GapReason::CallbackFailed, source_clock: self.source_clock });
                self.emit("callback_failed", Some(error.clone()));
                return Err(WatchError::Callback(error));
            }
            self.emit("rebuild_scheduled", Some(event.path.clone()));
        }
        // Invariant: a watcher publish becomes queryable before it is
        // reported fresh. `self.previous`/`self.applied_clock` (the state a
        // query reads) are committed here, strictly before the
        // "gap_cleared"/"caught_up" freshness observations are emitted below
        // -- never the other way around, so nothing can observe a "fresh"
        // signal against state that has not actually been published yet.
        self.previous = current;
        self.applied_clock = self.source_clock;
        if let Ok(mut queue) = self.native_queue.lock() {
            queue.next_source_clock = queue.next_source_clock.max(self.source_clock);
        }
        debug_assert!(self.applied_clock == self.source_clock, "published state must be committed before freshness is reported");
        if self.gap.as_ref().is_some_and(|gap| gap.reason != GapReason::CallbackFailed) {
            self.gap = None;
            self.emit("gap_cleared", None);
        }
        if self.gap.is_none() { self.emit("caught_up", None); }
        Ok(events)
    }

    pub fn report_event_gap(&mut self, reason: GapReason) -> Result<(), WatchError> {
        if self.closed { return Err(WatchError::Shutdown); }
        self.gap = Some(EventGap { reason, source_clock: self.source_clock });
        self.emit("event_gap", Some(format!("{reason:?}")));
        Ok(())
    }

    /// Reconcile immediately without requiring a resident holder.
    pub fn reconcile_now<F>(&mut self, schedule_rebuild: F) -> Result<Vec<WatchEvent>, WatchError>
    where F: FnMut(&WatchEvent) -> Result<(), String> { self.poll(schedule_rebuild) }

    pub fn barrier(&self, barrier: Barrier) -> BarrierPoll {
        if self.gap.is_some() { return BarrierPoll::Complete(BarrierResult::GapBlocked); }
        if self.applied_clock >= barrier.target_source_clock { return BarrierPoll::Complete(BarrierResult::CaughtUp); }
        if barrier.deadline.map(|deadline| self.clock.now() >= deadline).unwrap_or(false) { return BarrierPoll::Complete(BarrierResult::Timeout); }
        BarrierPoll::Waiting
    }

    pub fn clear_gap_after_reconcile(&mut self) { self.gap = None; self.emit("gap_cleared", None); }

    pub fn shutdown(&mut self) -> Result<(), WatchError> {
        if self.closed { return Ok(()); }
        self.emit("shutdown_requested", None);
        self.closed = true;
        self.emit("shutdown_complete", None);
        Ok(())
    }

    fn emit(&mut self, kind: &str, detail: Option<String>) {
        self.sequence = self.sequence.saturating_add(1);
        if let Some(sink) = &self.sink {
            sink.observe(LifecycleObservation { sequence: self.sequence, kind: kind.to_owned(), source_clock: self.source_clock, applied_clock: self.applied_clock, detail });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_ignores_generated_payloads_but_reports_oversized_source() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("target")).unwrap();
        fs::write(root.path().join("target/payload.bin"), vec![b'x'; 3 * 1024 * 1024]).unwrap();
        fs::write(root.path().join("src.rs"), "fn first() {}\n").unwrap();
        fs::write(root.path().join("large.rs"), vec![b'x'; 2 * 1024 * 1024 + 1]).unwrap();
        let current = snapshot(&SnapshotConfig::new(root.path())).unwrap();
        assert!(current.entries.iter().all(|entry| !entry.path.starts_with("target/")));
        let large = current.entries.iter().find(|entry| entry.path == "large.rs").unwrap();
        assert_eq!(large.digest, None);
        assert!(large.reason.as_deref().unwrap().starts_with("unsupported:file_bytes:"));
    }

    #[test]
    fn snapshot_edit_reconciles_real_source_file() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("src.rs");
        fs::write(&source, "fn first() {}\n").unwrap();
        let before = snapshot(&SnapshotConfig::new(root.path())).unwrap();
        fs::write(&source, "fn second() {}\n").unwrap();
        let after = snapshot(&SnapshotConfig::new(root.path())).unwrap();
        let events = reconcile_snapshots(&before, &after, 0);
        assert_eq!(events.iter().filter(|event| event.path == "src.rs" && event.kind == EventKind::Modify).count(), 1);
    }

    #[test]
    fn native_events_publish_only_source_files() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join(".agent/graph")).unwrap();
        fs::create_dir_all(root.path().join("src")).unwrap();
        fs::write(root.path().join("src/live.rs"), "fn live() {}\n").unwrap();
        fs::write(root.path().join("deleted.rs"), "fn deleted() {}\n").unwrap();
        let watcher = NativeWatcher::start(SnapshotConfig::new(root.path())).unwrap();
        fs::remove_file(root.path().join("deleted.rs")).unwrap();

        assert!(!watcher.native_event_is_source(&WatchEvent {
            kind: EventKind::Modify,
            path: ".agent".into(),
            rename_to: None,
            source_clock: 1,
        }));
        assert!(!watcher.native_event_is_source(&WatchEvent {
            kind: EventKind::Modify,
            path: "src".into(),
            rename_to: None,
            source_clock: 2,
        }));
        assert!(watcher.native_event_is_source(&WatchEvent {
            kind: EventKind::Modify,
            path: "src/live.rs".into(),
            rename_to: None,
            source_clock: 3,
        }));
        assert!(watcher.native_event_is_source(&WatchEvent {
            kind: EventKind::Delete,
            path: "deleted.rs".into(),
            rename_to: None,
            source_clock: 4,
        }));
    }
}
