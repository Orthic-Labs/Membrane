//! CTX-023/CTX-024 background-review **input producer** (tray-owned).
//!
//! The daemon's background scheduler consumes a
//! [`BackgroundReviewInputSnapshotV1`] from a workspace file. Before this
//! module existed nothing in the product ever wrote that file, so the
//! deterministic semantic provider and the CTX-024 foreground-coverage gate
//! were live code over data that never arrived.
//!
//! This module is the missing producer. It runs **inside the tray-owned serve
//! process**, on a real turn boundary (a completed Adapt observation window),
//! and it publishes only state Cortex already holds:
//!
//! * the append-only session-event stream that
//!   `adapt_observations::execute` wrote for this scope/session/task, and
//! * whatever durable Cortex knowledge already covers that seq range.
//!
//! It introduces no resident process of its own, it cannot run while the Hub
//! is inactive (there is no serve path to call it), and it writes nothing when
//! there is no new recorded activity: an empty window stays an honest no-op.

use crate::MemoryStore;
use cortex_store::{AbsorbedStore, MemDb, SessionEvent};
use membrane_protocol::background_review::{
    BackgroundReviewActivitySignalV1, BackgroundReviewEventRangeV1,
    BackgroundReviewForegroundMemoryStateV1, BackgroundReviewJobKindV1,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Env override honoured by the daemon reader; mirrored here so producer and
/// consumer can never resolve different files.
pub const BACKGROUND_REVIEW_INPUT_ENV: &str = "MEMBRANE_BACKGROUND_REVIEW_INPUT";
pub const DEFAULT_BACKGROUND_REVIEW_INPUT: &str = ".membrane/background-review-input.json";
pub const SNAPSHOT_SCHEMA_VERSION: u32 = 1;

/// Detector identity used by `adapt_observations` to name the per-consumer
/// event stream. Duplicated deliberately (the constant is private to that
/// module) and pinned by
/// `derived_stream_matches_the_stream_adapt_observations_writes`, so drift
/// fails a test instead of silently producing snapshots for an empty stream.
const ADAPT_VERIFICATION_DETECTOR: &str = "required_verification_completion.v1";

/// Upper bound on one published window. Matches the daemon loader's own
/// `cursor.last_seq + 256` bound, so a published window is never larger than
/// the window the consumer will read.
const MAX_WINDOW_EVENTS: u64 = 256;

/// How long a published window stays reviewable. The daemon treats this as a
/// hard deadline for one review attempt.
const REVIEW_DEADLINE_MS: u64 = 60_000;

/// Host activity/foreground snapshot the daemon scheduler consumes. The field
/// set and wire names are exactly the daemon's reader contract
/// (`deny_unknown_fields`, camelCase, `taskId` optional).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackgroundReviewInputSnapshotV1 {
    pub schema_version: u32,
    pub activity: BackgroundReviewActivitySignalV1,
    pub job_kind: BackgroundReviewJobKindV1,
    pub foreground_memory_state: BackgroundReviewForegroundMemoryStateV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    pub deadline_unix_ms: u64,
}

/// What one successful publication announced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedWindow {
    pub path: PathBuf,
    pub session_id: String,
    pub from_seq: u64,
    pub to_seq: u64,
}

/// Resolve the snapshot path. Installed state (`.../Membrane/state`) is
/// authoritative and ignores the env override; every other root honours it.
/// The daemon calls this same function, so the two cannot drift.
pub fn input_path(root: &Path) -> PathBuf {
    if root.file_name().is_some_and(|name| name == "state")
        && root
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name == "Membrane")
    {
        return root.join(DEFAULT_BACKGROUND_REVIEW_INPUT);
    }
    std::env::var_os(BACKGROUND_REVIEW_INPUT_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join(DEFAULT_BACKGROUND_REVIEW_INPUT))
}

/// Durable producer cursor: which seq of each stream has already been
/// announced. Kept beside the snapshot so a crash cannot lose it.
fn cursor_path(root: &Path) -> PathBuf {
    let snapshot = input_path(root);
    let mut name = snapshot
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "background-review-input.json".to_string());
    name.push_str(".cursor");
    snapshot.with_file_name(name)
}

/// Event stream `adapt_observations::execute` appends a completed window to.
pub fn adapt_consumer_stream(scope: &str, session_id: &str, task_id: &str) -> String {
    format!(
        "adapt:consumer:{}",
        membrane_adapt::canonical::sha256_canonical(&json!([
            scope,
            session_id,
            task_id,
            ADAPT_VERIFICATION_DETECTOR
        ]))
    )
}

/// Temp file + fsync + rename, so a reader never observes a partial snapshot.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let parent = path
        .parent()
        .ok_or_else(|| "snapshot path has no parent directory".to_string())?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    let temp = parent.join(format!(
        ".{}.{}.{unique}.tmp",
        path.file_name()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_else(|| "snapshot".into()),
        std::process::id()
    ));
    {
        let mut file = std::fs::File::create(&temp).map_err(|error| error.to_string())?;
        file.write_all(bytes).map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
    }
    match std::fs::rename(&temp, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = std::fs::remove_file(&temp);
            Err(error.to_string())
        }
    }
}

fn read_published_cursors(root: &Path) -> std::collections::BTreeMap<String, u64> {
    std::fs::read_to_string(cursor_path(root))
        .ok()
        .and_then(|value| serde_json::from_str(&value).ok())
        .unwrap_or_default()
}

/// Publish the pending review window for one completed observation window.
///
/// Returns `Ok(None)` when there is genuinely nothing to report (no recorded
/// stream, or no event past the last announced seq) — the honest no-op. It
/// returns `Err` only when real recorded activity exists but could not be
/// published, so the caller can log a refusal instead of silently degrading.
#[allow(clippy::too_many_arguments)]
pub fn publish_observation_window(
    store: &MemoryStore,
    root: &Path,
    scope: &str,
    session_id: &str,
    task_id: &str,
    window_id: &str,
    foreground_active: bool,
    observed_at_unix_ms: u64,
) -> Result<Option<PublishedWindow>, String> {
    let stream = adapt_consumer_stream(scope, session_id, task_id);
    publish_stream_window(
        store,
        root,
        &stream,
        task_id,
        window_id,
        foreground_active,
        observed_at_unix_ms,
    )
}

#[allow(clippy::too_many_arguments)]
fn publish_stream_window(
    store: &MemoryStore,
    root: &Path,
    stream: &str,
    task_id: &str,
    window_id: &str,
    foreground_active: bool,
    observed_at_unix_ms: u64,
) -> Result<Option<PublishedWindow>, String> {
    // One publisher at a time: the producer cursor is read-modify-written, and
    // two turn boundaries completing together must not lose an advance.
    static PUBLISH: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _serialized = PUBLISH.lock().unwrap_or_else(|error| error.into_inner());
    let events = AbsorbedStore::new(store.db().clone()).map_err(|error| error.to_string())?;
    let high_water = events.cursor(stream).map_err(|error| error.to_string())?;
    if high_water.last_seq == 0 {
        // No recorded activity for this stream: never fabricate a window.
        return Ok(None);
    }
    let published = read_published_cursors(root);
    let last_published = published.get(stream).copied().unwrap_or(0);
    if high_water.last_seq <= last_published {
        // Replayed or idempotent window; nothing new was appended.
        return Ok(None);
    }
    let from_seq = last_published.saturating_add(1);
    let to_seq = high_water
        .last_seq
        .min(last_published.saturating_add(MAX_WINDOW_EVENTS));
    let window = events
        .events_range(stream, from_seq, to_seq.saturating_add(1))
        .map_err(|error| error.to_string())?;
    if window.is_empty() {
        return Ok(None);
    }
    // Binding check: the stream we derived must be the stream the window we
    // were told about actually landed in. If the detector identity ever drifts
    // this refuses loudly rather than publishing a snapshot over a stream that
    // holds someone else's events.
    let holds_window = window
        .iter()
        .any(|event| event.payload.get("window_id").and_then(Value::as_str) == Some(window_id));
    if !holds_window {
        return Err(format!(
            "derived observation stream does not hold window {window_id}"
        ));
    }

    let activity_units = window.len() as u64;
    let input_tokens: u64 = window
        .iter()
        .map(|event| cortex_core::estimate_tokens(&event.payload.to_string()) as u64)
        .sum::<u64>()
        .max(1);
    let observed_at = window
        .iter()
        .map(|event| event.occurred_at_ms)
        .max()
        .filter(|value| *value != 0)
        .unwrap_or(observed_at_unix_ms);

    let snapshot = BackgroundReviewInputSnapshotV1 {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        activity: BackgroundReviewActivitySignalV1 {
            schema_version: BackgroundReviewActivitySignalV1::SCHEMA_VERSION,
            session_id: stream.to_string(),
            turn_id: window_id.to_string(),
            activity_units,
            input_tokens: Some(input_tokens),
            foreground_active,
            observed_at_unix_ms: observed_at,
        },
        job_kind: BackgroundReviewJobKindV1::CortexMemoryCandidateExtraction,
        foreground_memory_state: foreground_memory_state(
            &store.db().clone(),
            stream,
            from_seq,
            to_seq.saturating_add(1),
        ),
        task_id: (!task_id.trim().is_empty()).then(|| task_id.to_string()),
        deadline_unix_ms: observed_at.saturating_add(REVIEW_DEADLINE_MS),
    };
    snapshot
        .activity
        .validate()
        .map_err(|error| error.to_string())?;
    snapshot
        .foreground_memory_state
        .validate()
        .map_err(|error| error.to_string())?;

    let path = input_path(root);
    let encoded = serde_json::to_vec(&snapshot).map_err(|error| error.to_string())?;
    write_atomic(&path, &encoded)?;

    // The producer cursor advances only after the snapshot is durably in
    // place. A crash in between republishes the identical window on the next
    // boundary (level-triggered, idempotent); the daemon's own cursor decides
    // what it has already consumed, so no window is lost or double-consumed.
    let mut published = published;
    published.insert(stream.to_string(), to_seq);
    let encoded_cursor = serde_json::to_vec(&published).map_err(|error| error.to_string())?;
    write_atomic(&cursor_path(root), &encoded_cursor)?;

    Ok(Some(PublishedWindow {
        path,
        session_id: stream.to_string(),
        from_seq,
        to_seq,
    }))
}

/// Durable knowledge Cortex already holds for a session's seq range.
///
/// This reports **what Cortex knows**, never a decision: the CTX-024 gate
/// re-derives its own window and re-checks overlap independently, so the two
/// cannot disagree about admission. Coverage means a durable knowledge
/// emission for this session that was **not** produced by a background review
/// job (background emissions carry `jobId`); those are foreground memory.
///
/// * proposal registry unreadable → `Unavailable` (the gate then fails closed);
/// * registry present/never initialised and nothing covers the range →
///   `AvailableNoEmission`;
/// * a covering foreground emission → `AvailableEmission { range }`.
pub fn foreground_memory_state(
    db: &MemDb,
    session_id: &str,
    from_seq: u64,
    to_seq: u64,
) -> BackgroundReviewForegroundMemoryStateV1 {
    match foreground_coverage(db, session_id, from_seq, to_seq) {
        Ok(Some(range)) => BackgroundReviewForegroundMemoryStateV1::AvailableEmission { range },
        Ok(None) => BackgroundReviewForegroundMemoryStateV1::AvailableNoEmission,
        Err(_) => BackgroundReviewForegroundMemoryStateV1::Unavailable,
    }
}

fn foreground_coverage(
    db: &MemDb,
    session_id: &str,
    from_seq: u64,
    to_seq: u64,
) -> Result<Option<BackgroundReviewEventRangeV1>, String> {
    for emission in session_emissions(db, session_id)? {
        // Background-produced candidates carry the originating jobId; they are
        // not foreground memory and must not suppress the background lane.
        if emission.get("jobId").is_some_and(|value| !value.is_null()) {
            continue;
        }
        let (Some(start), Some(end)) = (
            emission.get("fromSeq").and_then(Value::as_u64),
            emission.get("toSeq").and_then(Value::as_u64),
        ) else {
            continue;
        };
        let range = BackgroundReviewEventRangeV1 {
            start_seq: start,
            end_seq: end,
        };
        if range.validate().is_err() {
            continue;
        }
        if range.overlaps(BackgroundReviewEventRangeV1 {
            start_seq: from_seq,
            end_seq: to_seq,
        }) {
            return Ok(Some(range));
        }
    }
    Ok(None)
}

/// Highest seq of this session that durable Cortex knowledge already covers,
/// counting every producer. Used to seed a restarted daemon's review cursor so
/// a crash does not replay windows that already produced durable knowledge.
pub fn durable_reviewed_through_seq(db: &MemDb, session_id: &str) -> Option<u64> {
    session_emissions(db, session_id)
        .ok()?
        .into_iter()
        .filter_map(|emission| emission.get("toSeq").and_then(Value::as_u64))
        .filter(|value| *value > 0)
        .max()
        .map(|to_seq| to_seq.saturating_sub(1))
}

/// Knowledge emissions recorded for one session. `Err` means the registry
/// could not be consulted; a missing table means it was never initialised, so
/// no emission exists and the honest answer is an empty list.
fn session_emissions(db: &MemDb, session_id: &str) -> Result<Vec<Value>, String> {
    let conn = db.lock_events();
    let mut statement = match conn.prepare(
        "SELECT emission_json FROM membrane_knowledge_proposal
         WHERE emission_json LIKE ?1 ORDER BY created_at ASC LIMIT 512",
    ) {
        Ok(statement) => statement,
        Err(rusqlite::Error::SqliteFailure(_, Some(message)))
            if message.contains("no such table") =>
        {
            return Ok(Vec::new())
        }
        Err(rusqlite::Error::SqlInputError { ref msg, .. }) if msg.contains("no such table") => {
            return Ok(Vec::new())
        }
        Err(error) => return Err(error.to_string()),
    };
    let pattern = format!("%{session_id}%");
    let rows = statement
        .query_map([pattern], |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?;
    let mut emissions = Vec::new();
    for row in rows {
        let text = row.map_err(|error| error.to_string())?;
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        if value.get("sessionId").and_then(Value::as_str) == Some(session_id) {
            emissions.push(value);
        }
    }
    Ok(emissions)
}

/// Test/consumer helper: the events one published window refers to.
pub fn window_events(
    store: &MemoryStore,
    session_id: &str,
    from_seq: u64,
    to_seq: u64,
) -> Result<Vec<SessionEvent>, String> {
    AbsorbedStore::new(store.db().clone())
        .map_err(|error| error.to_string())?
        .events_range(session_id, from_seq, to_seq.saturating_add(1))
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryStore;
    use cortex_core::review::{
        bound_memory_candidate_extraction_window_with_state, ForegroundMemoryEmissionV1,
        ForegroundMemoryStateV1, MemoryCandidateExtractionDecisionV1,
        MemoryCandidateExtractionLimitsV1, MemoryCandidateExtractionSkipV1,
    };
    use cortex_core::EventCursor;

    const SCOPE: &str = "repo";
    const SESSION: &str = "session";
    const TASK: &str = "task";

    /// `input_path` reads a process-global environment variable, so tests that
    /// set it and tests that depend on its absence cannot run concurrently.
    /// Rust runs unit tests as threads in one process, so this is a real race
    /// and not a theoretical one: without the lock, one test's `set_var` moved
    /// another test's snapshot out from under it.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Holds the env lock and clears the override on drop, so a panicking test
    /// cannot leave the variable set for whichever test runs next.
    struct EnvGuard(#[allow(dead_code)] std::sync::MutexGuard<'static, ()>);

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            std::env::remove_var(BACKGROUND_REVIEW_INPUT_ENV);
        }
    }

    fn lock_env() -> EnvGuard {
        let guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        std::env::remove_var(BACKGROUND_REVIEW_INPUT_ENV);
        EnvGuard(guard)
    }

    fn snapshot_root() -> (EnvGuard, tempfile::TempDir, PathBuf) {
        let guard = lock_env();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("workspace");
        std::fs::create_dir_all(&root).unwrap();
        (guard, dir, root)
    }

    fn record_window(store: &MemoryStore, stream: &str, window_id: &str, seq: u64) {
        let payload = json!({
            "contract": "adapt.detector-coverage.v2",
            "window_id": window_id,
            "session_id": SESSION,
            "task_id": TASK,
            "state": "ran",
        });
        let event = cortex_store::SessionEvent {
            schema_version: 1,
            session_id: stream.to_string(),
            seq,
            event_id: format!("adapt:window:{window_id}"),
            event_type: "adapt.detector_state".into(),
            payload,
            scope_id: SCOPE.into(),
            authority: "A0".into(),
            influence_class: "reference".into(),
            lifecycle: "active".into(),
            retention: "local_audit".into(),
            provenance: Vec::new(),
            content_hash: format!("sha256:{}", "a".repeat(64)),
            occurred_at_ms: 1_000 + seq,
            recorded_at_ms: 0,
        };
        AbsorbedStore::new(store.db().clone())
            .unwrap()
            .append_event(&event)
            .unwrap();
    }

    fn published(root: &Path) -> BackgroundReviewInputSnapshotV1 {
        let text = std::fs::read_to_string(input_path(root)).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    #[test]
    fn recorded_window_publishes_a_snapshot_the_daemon_reader_accepts() {
        let (_env, _dir, root) = snapshot_root();
        let store = MemoryStore::new();
        let stream = adapt_consumer_stream(SCOPE, SESSION, TASK);
        record_window(&store, &stream, "w1", 1);

        let outcome = publish_observation_window(
            &store, &root, SCOPE, SESSION, TASK, "w1", false, 5_000,
        )
        .unwrap()
        .expect("a recorded window publishes");
        assert_eq!(outcome.session_id, stream);
        assert_eq!((outcome.from_seq, outcome.to_seq), (1, 1));

        let snapshot = published(&root);
        assert_eq!(snapshot.schema_version, SNAPSHOT_SCHEMA_VERSION);
        assert_eq!(snapshot.activity.session_id, stream);
        assert_eq!(snapshot.activity.turn_id, "w1");
        assert_eq!(snapshot.activity.activity_units, 1);
        assert!(snapshot.activity.input_tokens.is_some_and(|value| value > 0));
        assert_eq!(snapshot.task_id.as_deref(), Some(TASK));
        assert_eq!(
            snapshot.job_kind,
            BackgroundReviewJobKindV1::CortexMemoryCandidateExtraction
        );
        snapshot.activity.validate().unwrap();
        snapshot.foreground_memory_state.validate().unwrap();
        // The published stream is the stream the events are actually in.
        assert_eq!(
            window_events(&store, &snapshot.activity.session_id, 1, 1)
                .unwrap()
                .len(),
            1
        );
    }

    /// The wire bytes must satisfy the daemon reader's own contract:
    /// camelCase, schemaVersion 1, `deny_unknown_fields`.
    #[test]
    fn snapshot_bytes_match_the_daemon_reader_contract() {
        let (_env, _dir, root) = snapshot_root();
        let store = MemoryStore::new();
        let stream = adapt_consumer_stream(SCOPE, SESSION, TASK);
        record_window(&store, &stream, "w1", 1);
        publish_observation_window(&store, &root, SCOPE, SESSION, TASK, "w1", false, 5_000)
            .unwrap()
            .unwrap();
        let raw: Value =
            serde_json::from_str(&std::fs::read_to_string(input_path(&root)).unwrap()).unwrap();
        for field in [
            "schemaVersion",
            "activity",
            "jobKind",
            "foregroundMemoryState",
            "taskId",
            "deadlineUnixMs",
        ] {
            assert!(raw.get(field).is_some(), "missing {field}");
        }
        assert_eq!(raw["schemaVersion"], 1);
        assert_eq!(raw["activity"]["schemaVersion"], 1);
        assert_eq!(raw["jobKind"], "cortex_memory_candidate_extraction");
    }

    #[test]
    fn no_activity_writes_no_snapshot() {
        let (_env, _dir, root) = snapshot_root();
        let store = MemoryStore::new();
        assert_eq!(
            publish_observation_window(&store, &root, SCOPE, SESSION, TASK, "w1", false, 5_000)
                .unwrap(),
            None
        );
        assert!(!input_path(&root).exists());
        assert!(!cursor_path(&root).exists());
    }

    #[test]
    fn cursor_advances_without_gaps_or_repeats_across_windows() {
        let (_env, _dir, root) = snapshot_root();
        let store = MemoryStore::new();
        let stream = adapt_consumer_stream(SCOPE, SESSION, TASK);

        record_window(&store, &stream, "w1", 1);
        let first = publish_observation_window(
            &store, &root, SCOPE, SESSION, TASK, "w1", false, 5_000,
        )
        .unwrap()
        .unwrap();
        assert_eq!((first.from_seq, first.to_seq), (1, 1));

        // Same window replayed with nothing newly appended: honest no-op.
        assert_eq!(
            publish_observation_window(&store, &root, SCOPE, SESSION, TASK, "w1", false, 6_000)
                .unwrap(),
            None
        );

        record_window(&store, &stream, "w2", 2);
        record_window(&store, &stream, "w3", 3);
        let second = publish_observation_window(
            &store, &root, SCOPE, SESSION, TASK, "w3", false, 7_000,
        )
        .unwrap()
        .unwrap();
        // Starts exactly where the previous window ended: no gap, no repeat.
        assert_eq!((second.from_seq, second.to_seq), (2, 3));
        assert_eq!(published(&root).activity.activity_units, 2);
    }

    /// A crash after the snapshot rename but before the cursor write must not
    /// lose the window: the next boundary republishes the identical window.
    #[test]
    fn crash_between_snapshot_and_cursor_write_republishes_the_same_window() {
        let (_env, _dir, root) = snapshot_root();
        let store = MemoryStore::new();
        let stream = adapt_consumer_stream(SCOPE, SESSION, TASK);
        record_window(&store, &stream, "w1", 1);
        publish_observation_window(&store, &root, SCOPE, SESSION, TASK, "w1", false, 5_000)
            .unwrap()
            .unwrap();
        let first = std::fs::read(input_path(&root)).unwrap();
        // Simulate the crash: the durable cursor never landed.
        std::fs::remove_file(cursor_path(&root)).unwrap();
        let again = publish_observation_window(
            &store, &root, SCOPE, SESSION, TASK, "w1", false, 5_000,
        )
        .unwrap()
        .expect("the unacknowledged window is republished");
        assert_eq!((again.from_seq, again.to_seq), (1, 1));
        assert_eq!(std::fs::read(input_path(&root)).unwrap(), first);
    }

    /// The rename leaves no partial file behind and no temp file in place.
    #[test]
    fn snapshot_write_is_atomic() {
        let (_env, _dir, root) = snapshot_root();
        let store = MemoryStore::new();
        let stream = adapt_consumer_stream(SCOPE, SESSION, TASK);
        record_window(&store, &stream, "w1", 1);
        publish_observation_window(&store, &root, SCOPE, SESSION, TASK, "w1", false, 5_000)
            .unwrap()
            .unwrap();
        let path = input_path(&root);
        let directory = path.parent().unwrap();
        let leftovers: Vec<_> = std::fs::read_dir(directory)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temp files left behind: {leftovers:?}");
        // Whatever a reader observes at the published path parses whole.
        serde_json::from_str::<BackgroundReviewInputSnapshotV1>(
            &std::fs::read_to_string(&path).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn env_override_is_honoured_for_a_workspace_root() {
        let (_env, dir, root) = snapshot_root();
        let target = dir.path().join("elsewhere/input.json");
        std::env::set_var(BACKGROUND_REVIEW_INPUT_ENV, &target);
        assert_eq!(input_path(&root), target);
        let store = MemoryStore::new();
        let stream = adapt_consumer_stream(SCOPE, SESSION, TASK);
        record_window(&store, &stream, "w1", 1);
        publish_observation_window(&store, &root, SCOPE, SESSION, TASK, "w1", false, 5_000)
            .unwrap()
            .unwrap();
        assert!(target.is_file());
    }

    #[test]
    fn installed_state_root_ignores_the_env_override() {
        let _env = lock_env();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Membrane/state");
        std::fs::create_dir_all(&root).unwrap();
        std::env::set_var(BACKGROUND_REVIEW_INPUT_ENV, dir.path().join("other.json"));
        assert_eq!(input_path(&root), root.join(DEFAULT_BACKGROUND_REVIEW_INPUT));
    }

    // ---- CTX-024 foreground coverage ----

    fn record_emission(store: &MemoryStore, session_id: &str, from: u64, to: u64, job: Option<&str>) {
        let mut emission = json!({
            "text": "an episodic observation",
            "kind": "episodic",
            "producer": "episodic",
            "epistemicClass": "reported",
            "sessionId": session_id,
            "fromSeq": from,
            "toSeq": to,
        });
        if let Some(job) = job {
            emission["jobId"] = json!(job);
        }
        let conn = store.db().lock_events();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS membrane_knowledge_proposal(
               proposal_id TEXT PRIMARY KEY,repository_id TEXT NOT NULL,scope_id TEXT NOT NULL,
               emission_json TEXT NOT NULL,emission_sha256 TEXT NOT NULL,
               state TEXT NOT NULL CHECK(state IN ('pending','approved','rejected')),
               created_at TEXT NOT NULL,decided_at TEXT,reviewer TEXT) STRICT;",
        )
        .unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO membrane_knowledge_proposal
             (proposal_id,repository_id,scope_id,emission_json,emission_sha256,state,created_at)
             VALUES(?1,'repo','repo',?2,'sha',?3,'2026-01-01T00:00:00Z')",
            rusqlite::params![
                format!("proposal-{from}-{to}-{}", job.unwrap_or("foreground")),
                serde_json::to_string(&emission).unwrap(),
                "pending",
            ],
        )
        .unwrap();
    }

    /// The gate must reach the same conclusion the published state implies:
    /// suppressed when foreground memory already covers the range, permitted
    /// when it does not. The gate is evaluated here, unmodified.
    fn gate_decision(
        session_id: &str,
        from_seq: u64,
        to_seq: u64,
        state: &BackgroundReviewForegroundMemoryStateV1,
    ) -> MemoryCandidateExtractionDecisionV1 {
        let cursor = EventCursor {
            session_id: session_id.to_string(),
            last_seq: from_seq.saturating_sub(1),
        };
        let events: Vec<cortex_core::SessionEvent> = (from_seq..=to_seq)
            .map(|seq| cortex_core::SessionEvent {
                schema_version: 1,
                session_id: session_id.to_string(),
                seq,
                event_id: format!("event-{seq}"),
                event_type: "adapt.detector_state".into(),
                payload: json!({"window_id": format!("w{seq}")}),
                scope_id: SCOPE.into(),
                authority: "A0".into(),
                influence_class: "reference".into(),
                lifecycle: "active".into(),
                retention: "local_audit".into(),
                provenance: Vec::new(),
                occurred_at_ms: 1_000 + seq,
                recorded_at_ms: 0,
                content_hash: format!("sha256:{}", "a".repeat(64)),
            })
            .collect();
        let core_state = match state {
            BackgroundReviewForegroundMemoryStateV1::Unavailable => {
                ForegroundMemoryStateV1::Unavailable
            }
            BackgroundReviewForegroundMemoryStateV1::AvailableNoEmission => {
                ForegroundMemoryStateV1::AvailableNoEmission
            }
            BackgroundReviewForegroundMemoryStateV1::AvailableEmission { range } => {
                ForegroundMemoryStateV1::AvailableEmission(ForegroundMemoryEmissionV1 {
                    emission_id: format!(
                        "background-signal-{}-{}",
                        range.start_seq, range.end_seq
                    ),
                    session_id: session_id.to_string(),
                    start_seq: range.start_seq,
                    end_seq: range.end_seq,
                })
            }
        };
        bound_memory_candidate_extraction_window_with_state(
            &cursor,
            &events,
            &core_state,
            MemoryCandidateExtractionLimitsV1 {
                max_events: 256,
                max_input_tokens: 4_096,
                max_duration_ms: 60_000,
                max_model_requests: 1,
            },
            true,
        )
        .unwrap()
    }

    #[test]
    fn foreground_coverage_suppresses_extraction_and_absence_permits_it() {
        let (_env, _dir, root) = snapshot_root();
        let store = MemoryStore::new();
        let stream = adapt_consumer_stream(SCOPE, SESSION, TASK);
        record_window(&store, &stream, "w1", 1);
        record_window(&store, &stream, "w2", 2);

        // Nothing durable covers the range: the state permits extraction and
        // the gate agrees.
        publish_observation_window(&store, &root, SCOPE, SESSION, TASK, "w2", false, 5_000)
            .unwrap()
            .unwrap();
        let permitted = published(&root);
        assert_eq!(
            permitted.foreground_memory_state,
            BackgroundReviewForegroundMemoryStateV1::AvailableNoEmission
        );
        assert!(matches!(
            gate_decision(&stream, 1, 2, &permitted.foreground_memory_state),
            MemoryCandidateExtractionDecisionV1::WindowBound { .. }
        ));

        // Foreground memory now covers the same range: the published state
        // reports it and the gate suppresses extraction.
        record_emission(&store, &stream, 1, 3, None);
        let covered = foreground_memory_state(&store.db().clone(), &stream, 1, 3);
        assert_eq!(
            covered,
            BackgroundReviewForegroundMemoryStateV1::AvailableEmission {
                range: BackgroundReviewEventRangeV1 {
                    start_seq: 1,
                    end_seq: 3
                }
            }
        );
        assert!(matches!(
            gate_decision(&stream, 1, 2, &covered),
            MemoryCandidateExtractionDecisionV1::Skipped {
                reason: MemoryCandidateExtractionSkipV1::ForegroundMemoryEmissionPresent
            }
        ));
    }

    #[test]
    fn background_produced_knowledge_is_not_reported_as_foreground_memory() {
        let store = MemoryStore::new();
        let stream = adapt_consumer_stream(SCOPE, SESSION, TASK);
        record_emission(&store, &stream, 1, 3, Some("background-job-1"));
        assert_eq!(
            foreground_memory_state(&store.db().clone(), &stream, 1, 3),
            BackgroundReviewForegroundMemoryStateV1::AvailableNoEmission
        );
        // It still counts as durably reviewed, so a restarted daemon does not
        // replay the window.
        assert_eq!(
            durable_reviewed_through_seq(&store.db().clone(), &stream),
            Some(2)
        );
    }

    #[test]
    fn a_window_for_another_session_does_not_cover_this_one() {
        let store = MemoryStore::new();
        let stream = adapt_consumer_stream(SCOPE, SESSION, TASK);
        record_emission(&store, "adapt:consumer:other", 1, 3, None);
        assert_eq!(
            foreground_memory_state(&store.db().clone(), &stream, 1, 3),
            BackgroundReviewForegroundMemoryStateV1::AvailableNoEmission
        );
    }

    /// Pins the duplicated detector identity: if `adapt_observations` ever
    /// changes the stream it writes, this fails rather than the producer
    /// silently publishing over an empty stream.
    #[test]
    fn derived_stream_matches_the_stream_adapt_observations_writes() {
        let store = MemoryStore::new();
        let unavailable = json!({"coverage":"unavailable","unavailableReason":"not_instrumented"});
        let complete = |value: Value| json!({"coverage":"complete","value":value});
        let observation = |id: &str, kind: &str, exit: Option<i32>| -> Value {
            let mut value = json!({
                "schemaVersion":1,"observationId":id,"sessionId":SESSION,
                "taskId":complete(json!(TASK)),"observedAtUnixMs":100,"model":"model",
                "provider":"provider","client":"test","observationKind":kind,
                "scope":complete(json!(SCOPE)),"callId":complete(json!("test-call")),
                "exitCode":exit.map(|e| complete(json!(e))).unwrap_or(unavailable.clone()),
                "provenanceReceipt":{"schemaVersion":1,"receiptId":format!("receipt-{id}"),
                    "source":"host","observedAtUnixMs":100,
                    "receiptDigest":format!("sha256:{}","a".repeat(64))}});
            for field in [
                "parentTaskId","agentId","agentRole","routePolicy","subjectId","tool","outcome",
                "durationMs","usage","toolCost","assetCost","repository","artifactRefs",
                "evidenceRefs","completion",
            ] {
                value[field] = unavailable.clone();
            }
            value
        };
        let request: crate::adapt_observations::AdaptObservationRequestV1 =
            serde_json::from_value(json!({
                "operation":"analyze","scope":SCOPE,"window_id":"w1","session_id":SESSION,
                "task_id":TASK,"expected_cursor":0,"required_call_ids":["test-call"],
                "observations":[
                    {"sequence":1,"observation":observation("e1","verification_result",Some(1))},
                    {"sequence":2,"observation":observation("e2","completion_claim_emitted",None)},
                ]
            }))
            .expect("analyze request shape");
        crate::adapt_observations::execute(&store, request).expect("analyze succeeds");
        let stream = adapt_consumer_stream(SCOPE, SESSION, TASK);
        let cursor = AbsorbedStore::new(store.db().clone())
            .unwrap()
            .cursor(&stream)
            .unwrap();
        assert_eq!(
            cursor.last_seq, 1,
            "adapt_observations writes the stream this producer derives"
        );
    }
}
